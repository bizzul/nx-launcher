//! Updates, checked a few seconds after every start and then once a day:
//!
//! - **The launcher itself**, with Tauri's updater plugin: `latest.json` on
//!   the latest GitHub release of bizzul/nx-launcher says the version; the
//!   package is checked against the public key in tauri.conf.json (signed by
//!   scripts/release.sh with the private key, which never leaves this Mac),
//!   installed over the app, and runs from the next start or "Riavvia".
//!   Only the installed app (a release build) does it on its own;
//!   `NXL_UPDATE_URL` points any build at another `latest.json`, to try it.
//! - **The managed engine** (`Engine::in_data`): moved to nxnative's main
//!   with updater.rs, unless a commit says it breaks something for front
//!   ends (`Front-ends: break`) or it needs another Metal compiler: those are
//!   only announced, for the Ambiente page. Never while a build or a game
//!   runs. A project's engine is left to its commits in the project.

use crate::AppState;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(8);
const EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// What the engine's automatic update did, for the UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUpdate {
    /// Applied; else only announced (`reason` says why).
    pub applied: bool,
    pub short: String,
    pub reason: Option<String>,
    /// The Python requirements were reinstalled.
    pub pip: bool,
}

/// The updater, at the release endpoint or at `NXL_UPDATE_URL`.
fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    if let Ok(url) = std::env::var("NXL_UPDATE_URL") {
        let url = url.parse().map_err(|e| format!("NXL_UPDATE_URL: {e}"))?;
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())
}

/// Whether this build updates itself without being asked.
pub fn automatic() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("NXL_UPDATE_URL").is_some()
}

pub async fn check(app: &AppHandle) -> Result<Option<AppUpdate>, String> {
    let update = updater(app)?.check().await.map_err(|e| format!("Controllo degli aggiornamenti non riuscito: {e}"))?;
    Ok(update.map(|u| AppUpdate { version: u.version.clone(), notes: u.body.clone(), date: u.date.map(|d| d.to_string()) }))
}

/// Downloads the update, checks its signature and installs it over the app;
/// `None` when there is none. It runs from the next start (or `restart`).
pub async fn install(app: &AppHandle) -> Result<Option<String>, String> {
    let Some(update) = updater(app)?.check().await.map_err(|e| format!("Controllo degli aggiornamenti non riuscito: {e}"))?
    else {
        return Ok(None);
    };
    let progress = app.clone();
    let mut done = 0u64;
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let _ = progress.emit("app-update-progress", (done, total));
            },
            || {},
        )
        .await
        .map_err(|e| format!("Aggiornamento non installato: {e}"))?;
    let version = update.version.clone();
    *app.state::<AppState>().app_update_ready.lock().unwrap() = Some(version.clone());
    let _ = app.emit("app-update-ready", version.clone());
    Ok(Some(version))
}

/// Something a restart would cut short: a build, an import, an engine install.
pub fn busy(state: &AppState) -> Option<&'static str> {
    if state.building() {
        return Some("una build è in corso");
    }
    if state.import.lock().unwrap().as_ref().is_some_and(crate::importer::ImportRun::running) {
        return Some("un'importazione nell'archivio è in corso");
    }
    if state.install.lock().unwrap().as_ref().is_some_and(|s| s.running) {
        return Some("l'installazione del motore è in corso");
    }
    None
}

/// The managed engine to nxnative's main, when nothing stands in the way.
pub async fn engine(app: &AppHandle) -> Option<EngineUpdate> {
    let state = app.state::<AppState>();
    let engine = state.engine.clone();
    if !engine.managed || !engine.installed() || !state.config.lock().unwrap().config.engine_auto_update {
        return None;
    }
    // Offline, or GitHub slow: the next check tries again.
    let info = crate::updater::check(&engine).await.ok()?;
    if info.up_to_date || info.diverged {
        return None;
    }
    let reason = if let Some(n) = info.notes.iter().find(|n| n.kind == "break") {
        Some(format!("cambia qualcosa per il launcher ({}): guardalo in Ambiente prima di aggiornare", n.text))
    } else if let Some(metal) = &info.metal_needed {
        Some(format!("richiede un altro compilatore Metal ({metal}): installa l'Xcode giusto, poi aggiorna in Ambiente"))
    } else {
        None
    };
    if reason.is_some() {
        let note = EngineUpdate { applied: false, short: info.to_short, reason, pip: false };
        let _ = app.emit("engine-update", note.clone());
        return Some(note);
    }
    if busy(&state).is_some() || !state.running.lock().unwrap().is_empty() {
        return None; // never under a build or a game: the next check
    }
    match crate::updater::apply(&engine, "origin/main").await {
        Ok(applied) => {
            {
                let mut store = state.config.lock().unwrap();
                store.config.previous_engine = Some(applied.previous.clone());
                let _ = store.save();
            }
            let note = EngineUpdate { applied: true, short: applied.info.short, reason: None, pip: applied.pip.is_some() };
            let _ = app.emit("engine-update", note.clone());
            let _ = app.emit("library-changed", ()); // the rebuild hints follow the engine
            Some(note)
        }
        Err(e) => {
            eprintln!("engine auto-update: {e}");
            None
        }
    }
}

/// Starts the checks: a few seconds after the start, then once a day.
pub fn schedule(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let auto_app = app.state::<AppState>().config.lock().unwrap().config.app_auto_update;
            if automatic() && auto_app && app.state::<AppState>().app_update_ready.lock().unwrap().is_none() {
                if let Err(e) = install(&app).await {
                    eprintln!("app auto-update: {e}");
                }
            }
            engine(&app).await;
            tokio::time::sleep(EVERY).await;
        }
    });
}
