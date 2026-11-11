//! Starting a game, following it until it exits, stopping it, and opening
//! the files nxnative keeps for it (docs/frontends.md, "Starting a game").

use crate::gamelog::{GameStatus, LogWatch};
use crate::library::{classify_exit, now, ExitInfo, Game, EXECUTABLE};
use crate::{AppState, GameView};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::process::Command;

/// A game this launcher started and that has not exited yet.
pub struct Session {
    pid: u32,
    started: Instant,
    pub started_at: u64,
    stop_requested: bool,
    /// What the game is doing, kept current from its log (follow_log).
    status: Arc<Mutex<GameStatus>>,
}

impl Session {
    pub fn status(&self) -> GameStatus {
        self.status.lock().unwrap().clone()
    }
}

/// `game-status`: what a running game is doing changed.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent<'a> {
    title_id: &'a str,
    status: &'a GameStatus,
}

/// Starts the app's own launcher, `Contents/MacOS/nxnative`, as our child, so
/// its exit code tells how the game ended. Without `settings_window` it starts
/// with `DM_LAUNCHER=0`: no settings window, the saved choices still apply.
pub async fn launch(
    app: &AppHandle,
    game: &Game,
    settings_window: bool,
    overrides: &[(String, String)],
) -> Result<u64, String> {
    let exe = game.app_path.join("Contents/MacOS").join(EXECUTABLE);
    if !exe.is_file() {
        return Err(format!(
            "L'app non è più in {}: spostata o cancellata? Aggiungila di nuovo.",
            game.app_path.display()
        ));
    }
    let state = app.state::<AppState>();
    let mut running = state.running.lock().unwrap();
    if running.contains_key(&game.title_id) {
        return Err(format!("{} è già in esecuzione", game.name));
    }

    let mut cmd = Command::new(&exe);
    // The launcher writes everything to ~/Library/Logs/nxnative/<title ID>.log.
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if !settings_window {
        cmd.env("DM_LAUNCHER", "0");
    }
    // Already checked against the game's settings list (settings::check_overrides).
    cmd.envs(overrides.iter().map(|(k, v)| (k, v)));
    // The log this session writes is made after this (the app's launcher starts a new one).
    let spawned = SystemTime::now();
    let mut child = cmd.spawn().map_err(|e| format!("Impossibile avviare {}: {e}", game.name))?;
    let pid = child.id().ok_or("Il gioco si è chiuso subito")?;

    let started_at = now();
    let status = Arc::new(Mutex::new(GameStatus::default()));
    running.insert(
        game.title_id.clone(),
        Session { pid, started: Instant::now(), started_at, stop_requested: false, status: status.clone() },
    );
    drop(running);

    let (app, title_id) = (app.clone(), game.title_id.clone());
    if let Ok(home) = app.path().home_dir() {
        let log = home.join(format!("Library/Logs/nxnative/{title_id}.log"));
        let watch = LogWatch::new(log, spawned);
        tauri::async_runtime::spawn(follow_log(app.clone(), title_id.clone(), watch, status));
    }
    tauri::async_runtime::spawn(async move {
        let status = child.wait().await;
        finish(&app, &title_id, status.ok());
    });
    Ok(started_at)
}

/// Reads the game's log once a second while the session lasts, and tells the
/// UI (`game-status`) when what it shows changes.
async fn follow_log(app: AppHandle, title_id: String, mut watch: LogWatch, shared: Arc<Mutex<GameStatus>>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let state = app.state::<AppState>();
        // Over, or another session of this game started since: stop.
        let ours = state.running.lock().unwrap().get(&title_id).is_some_and(|s| Arc::ptr_eq(&s.status, &shared));
        if !ours {
            return;
        }
        let status = match watch.poll(now()) {
            Ok(s) => s.clone(),
            Err(e) => {
                eprintln!("cannot read the log of {title_id}: {e}");
                continue;
            }
        };
        let changed = {
            let mut s = shared.lock().unwrap();
            let changed = *s != status;
            *s = status.clone();
            changed
        };
        if changed {
            let _ = app.emit("game-status", StatusEvent { title_id: &title_id, status: &status });
        }
    }
}

/// Records how the session ended and tells the UI (`game-exited`).
fn finish(app: &AppHandle, title_id: &str, status: Option<std::process::ExitStatus>) {
    let state = app.state::<AppState>();
    let Some(session) = state.running.lock().unwrap().remove(title_id) else { return };
    let (code, signal) = status.map_or((None, None), |s| (s.code(), s.signal()));
    let exit = ExitInfo {
        kind: classify_exit(code, signal, session.stop_requested),
        code,
        at: now(),
        seconds: session.started.elapsed().as_secs(),
    };

    let mut lib = state.library.lock().unwrap();
    let Some(game) = lib.get_mut(title_id) else { return };
    game.last_played = Some(session.started_at);
    game.play_seconds += exit.seconds;
    game.last_exit = Some(exit);
    let view = GameView { game: game.clone(), running_since: None, status: None };
    if let Err(e) = lib.save() {
        eprintln!("cannot save the library: {e}");
    }
    drop(lib);
    let _ = app.emit("game-exited", view);
}

/// Asks the game to quit: SIGTERM to its launcher, which forwards it to the
/// host. Like quitting from the Home prompt, unsaved progress is lost.
pub fn stop(state: &AppState, title_id: &str) -> Result<(), String> {
    let mut running = state.running.lock().unwrap();
    let session = running.get_mut(title_id).ok_or("Il gioco non è in esecuzione")?;
    session.stop_requested = true;
    kill(Pid::from_raw(session.pid as i32), Signal::SIGTERM)
        .map_err(|e| format!("Impossibile fermare il gioco: {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevealTarget {
    App,
    Log,
    Saves,
}

/// Shows the app in Finder, opens the last log (in Console), or the saves.
/// Paths are worked out here from the title ID, never taken from the web view.
pub fn reveal(app: &AppHandle, game: &Game, target: RevealTarget) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let id = &game.title_id;
    let opener = app.opener();
    let result = match target {
        RevealTarget::App => opener.reveal_item_in_dir(&game.app_path),
        RevealTarget::Log => {
            let log = home.join(format!("Library/Logs/nxnative/{id}.log"));
            must_exist(&log, "Nessun log: il gioco non è ancora stato avviato")?;
            opener.open_path(log.to_string_lossy(), None::<&str>)
        }
        RevealTarget::Saves => {
            let saves = home.join(format!("Library/Application Support/nxnative/{id}/save"));
            must_exist(&saves, "Nessun salvataggio: il gioco non è ancora stato avviato")?;
            opener.open_path(saves.to_string_lossy(), None::<&str>)
        }
    };
    result.map_err(|e| e.to_string())
}

fn must_exist(path: &PathBuf, message: &str) -> Result<(), String> {
    if path.exists() { Ok(()) } else { Err(message.into()) }
}
