mod build;
mod config;
mod engine;
mod library;
mod prefs;
mod runner;
mod settings;

use config::ConfigStore;
use library::{Game, Library};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

pub struct AppState {
    library: Mutex<Library>,
    /// Games this launcher started, by title ID.
    running: Mutex<HashMap<String, runner::Session>>,
    config: Mutex<ConfigStore>,
    /// The build in progress, or the last one until the player dismisses it.
    build: Mutex<Option<build::BuildRun>>,
}

/// A library entry as the UI sees it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    #[serde(flatten)]
    game: Game,
    /// When the running session started (Unix seconds); `None` when not running.
    running_since: Option<u64>,
}

impl AppState {
    fn view(&self, game: &Game) -> GameView {
        let running = self.running.lock().unwrap();
        GameView {
            game: game.clone(),
            running_since: running.get(&game.title_id).map(|s| s.started_at),
        }
    }

    fn game(&self, title_id: &str) -> Result<Game, String> {
        let lib = self.library.lock().unwrap();
        lib.get(title_id).cloned().ok_or_else(|| format!("{title_id} non è in libreria"))
    }

    fn building(&self) -> bool {
        self.build.lock().unwrap().as_ref().is_some_and(build::BuildRun::running)
    }
}

/// Adds one app packed by nxnative to the library (or refreshes its entry).
pub async fn import_app(state: &AppState, path: PathBuf) -> Result<Game, String> {
    let info = library::read_app(&path)?;
    let icons_dir = state.library.lock().unwrap().icons_dir.clone();
    let icon = match &info.icns {
        Some(icns) => library::cache_icon(icns, &icons_dir, &info.title_id).await,
        None => None,
    };
    let mut lib = state.library.lock().unwrap();
    let game = lib.upsert(info, path, icon);
    lib.save().map_err(|e| format!("Impossibile salvare la libreria: {e}"))?;
    Ok(game)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportReport {
    added: Vec<GameView>,
    errors: Vec<String>,
}

/// Checks that this Mac has everything nxnative needs to build and run games.
#[tauri::command]
async fn doctor() -> engine::DoctorReport {
    engine::doctor(&engine::Engine::dev_default()).await
}

#[tauri::command]
fn library_list(state: State<AppState>) -> Vec<GameView> {
    let games = state.library.lock().unwrap().games().to_vec();
    games.iter().map(|g| state.view(g)).collect()
}

/// Adds Mac apps packed by nxnative (from the file picker or dropped on the window).
#[tauri::command]
async fn library_import(state: State<'_, AppState>, paths: Vec<PathBuf>) -> Result<ImportReport, String> {
    let mut report = ImportReport { added: Vec::new(), errors: Vec::new() };
    for path in paths {
        match import_app(&state, path).await {
            Ok(game) => report.added.push(state.view(&game)),
            Err(e) => report.errors.push(e),
        }
    }
    Ok(report)
}

#[tauri::command]
fn library_remove(state: State<AppState>, title_id: String) -> Result<(), String> {
    if state.running.lock().unwrap().contains_key(&title_id) {
        return Err("Chiudi il gioco prima di toglierlo dalla libreria".into());
    }
    let mut lib = state.library.lock().unwrap();
    lib.remove(&title_id);
    lib.save().map_err(|e| e.to_string())
}

/// Starts a game. `overrides` hold settings for this start only: they go in
/// the launch environment, which wins over everything, and are not saved.
#[tauri::command]
async fn game_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    title_id: String,
    settings_window: bool,
    overrides: Option<BTreeMap<String, String>>,
) -> Result<GameView, String> {
    let game = state.game(&title_id)?;
    let env = settings::check_overrides(&game, &overrides.unwrap_or_default())?;
    runner::launch(&app, &game, settings_window, &env).await?;
    Ok(state.view(&game))
}

fn home(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().home_dir().map_err(|e| e.to_string())
}

#[tauri::command]
fn settings_get(app: AppHandle, state: State<AppState>, title_id: String) -> Result<settings::GameSettings, String> {
    settings::load(&state.game(&title_id)?, &home(&app)?)
}

/// Saves the player's choices in the game's preferences, where its own
/// settings window and a start from Finder read them too.
#[tauri::command]
fn settings_save(
    app: AppHandle,
    state: State<AppState>,
    title_id: String,
    choices: BTreeMap<String, String>,
) -> Result<settings::GameSettings, String> {
    settings::save(&state.game(&title_id)?, &home(&app)?, &choices)
}

#[tauri::command]
fn game_stop(state: State<AppState>, title_id: String) -> Result<(), String> {
    runner::stop(&state, &title_id)
}

#[tauri::command]
fn game_reveal(
    app: AppHandle,
    state: State<AppState>,
    title_id: String,
    target: runner::RevealTarget,
) -> Result<(), String> {
    runner::reveal(&app, &state.game(&title_id)?, target)
}

// ── Launcher settings ───────────────────────────────────────────────────────

#[tauri::command]
fn config_get(state: State<AppState>) -> config::Config {
    state.config.lock().unwrap().config.clone()
}

/// Remembers where prod.keys is (only the path), or forgets it with `None`.
#[tauri::command]
fn config_set_keys(state: State<AppState>, path: Option<PathBuf>) -> Result<config::Config, String> {
    if let Some(p) = &path {
        if !p.is_file() {
            return Err(format!("{} non è un file", p.display()));
        }
    }
    let mut store = state.config.lock().unwrap();
    store.config.prod_keys = path;
    store.save().map_err(|e| e.to_string())?;
    Ok(store.config.clone())
}

// ── Building ────────────────────────────────────────────────────────────────

/// What a build of `path` would be, and what stands in its way.
#[tauri::command]
async fn build_preflight(state: State<'_, AppState>, path: PathBuf) -> Result<build::Preflight, String> {
    let keys = state.config.lock().unwrap().config.prod_keys.clone();
    build::preflight(&engine::Engine::dev_default(), &path, keys).await
}

#[tauri::command]
async fn build_start(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> Result<build::Snapshot, String> {
    let keys = state.config.lock().unwrap().config.prod_keys.clone();
    build::start(&app, &engine::Engine::dev_default(), &path, keys).await
}

/// The current or last build, for a view that opens while it runs.
#[tauri::command]
fn build_status(state: State<AppState>) -> Option<build::Snapshot> {
    state.build.lock().unwrap().as_ref().map(|r| r.snap.clone())
}

#[tauri::command]
fn build_cancel(state: State<AppState>) -> Result<(), String> {
    build::cancel(&state)
}

/// Opens the current build's logs folder (`<title ID>_build/logs`), else
/// shows the game's folder: the path comes from the build, not the web view.
#[tauri::command]
fn build_reveal(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let (dir, source, id) = {
        let slot = state.build.lock().unwrap();
        let snap = &slot.as_ref().ok_or("Nessuna build")?.snap;
        (snap.source.build_dir.clone(), snap.source.path.clone(), snap.game.as_ref().map(|g| g.title_id.clone()))
    };
    let logs = id.map(|id| dir.join(format!("{id}_build/logs"))).filter(|p| p.is_dir());
    let result = match logs {
        Some(logs) => app.opener().open_path(logs.to_string_lossy(), None::<&str>),
        None => app.opener().reveal_item_in_dir(&source),
    };
    result.map_err(|e| e.to_string())
}

/// Forgets a finished build, so the view can start another.
#[tauri::command]
fn build_dismiss(state: State<AppState>) {
    let mut slot = state.build.lock().unwrap();
    if !slot.as_ref().is_some_and(build::BuildRun::running) {
        *slot = None;
    }
}

/// Closing or quitting while a build runs asks first: a first build takes
/// 30 to 60 minutes. Confirmed, the build is cancelled cleanly and the
/// launcher quits once the script has stopped.
fn confirm_quit_during_build(app: &AppHandle) {
    let handle = app.clone();
    let mut dialog = app
        .dialog()
        .message("Una build è in corso. Se esci viene annullata; la prossima riprenderà da dove si è fermata.")
        .title("Build in corso")
        .buttons(MessageDialogButtons::OkCancelCustom("Annulla la build ed esci".into(), "Continua la build".into()));
    // A sheet on the window: a free-standing alert can end up behind it.
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.show(move |quit| {
            if !quit {
                return;
            }
            let state = handle.state::<AppState>();
            if let Some(run) = state.build.lock().unwrap().as_mut() {
                run.quit_after = true;
            }
            if build::cancel(&state).is_err() {
                handle.exit(0); // it ended meanwhile
            }
        });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // ~/Library/Application Support/<identifier>/: library.json, config.json, icons/
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState {
                library: Mutex::new(Library::load(&data_dir)?),
                running: Mutex::new(HashMap::new()),
                config: Mutex::new(ConfigStore::load(&data_dir)),
                build: Mutex::new(None),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<AppState>().building() {
                    api.prevent_close();
                    confirm_quit_during_build(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            doctor,
            library_list,
            library_import,
            library_remove,
            game_launch,
            game_stop,
            game_reveal,
            settings_get,
            settings_save,
            config_get,
            config_set_keys,
            build_preflight,
            build_start,
            build_status,
            build_cancel,
            build_dismiss,
            build_reveal,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // ⌘Q and the last window closing ask too; our own exit (code set) goes through.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            if app.state::<AppState>().building() {
                api.prevent_exit();
                confirm_quit_during_build(app);
            }
        }
    });
}
