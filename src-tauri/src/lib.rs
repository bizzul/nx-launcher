mod build;
mod cheats;
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
    let game = {
        let mut lib = state.library.lock().unwrap();
        let game = lib.upsert(info, path, icon);
        lib.save().map_err(|e| format!("Impossibile salvare la libreria: {e}"))?;
        game
    };
    // A new game joins the library's cloud saves folder, if there is one.
    if let Some(folder) = state.config.lock().unwrap().config.cloud_saves_folder.clone() {
        write_cloud_folder(&game, Some(&folder));
    }
    Ok(game)
}

/// `cloudSaves.folder` in the game's preferences, which its launcher reads
/// at every start (docs/frontends.md, "The cloud saves folder").
fn write_cloud_folder(game: &Game, folder: Option<&std::path::Path>) -> bool {
    let value = folder.map(|f| f.to_string_lossy().into_owned());
    prefs::set_string(&game.bundle_id, "cloudSaves.folder", value.as_deref());
    prefs::sync(&game.bundle_id)
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
    let mut env = settings::check_overrides(&game, &overrides.unwrap_or_default())?;
    if state.config.lock().unwrap().config.italian_prompts {
        env.extend(config::italian_prompts(&game.name));
    }
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

/// One cloud saves folder for the whole library: written into every game's
/// preferences (and into games added later); `None` turns cloud saves off.
/// "icloud" picks iCloud Drive › nxnative Saves, created when missing.
#[tauri::command]
fn config_set_cloud_folder(
    app: AppHandle,
    state: State<AppState>,
    path: Option<String>,
) -> Result<config::Config, String> {
    let folder = match path.as_deref() {
        None | Some("") => None,
        Some("icloud") => {
            let drive = home(&app)?.join("Library/Mobile Documents/com~apple~CloudDocs");
            if !drive.is_dir() {
                return Err("iCloud Drive non è attivo su questo Mac".into());
            }
            let f = drive.join("nxnative Saves");
            std::fs::create_dir_all(&f).map_err(|e| e.to_string())?;
            Some(f)
        }
        Some(p) => {
            let f = PathBuf::from(p);
            if !f.is_dir() {
                return Err(format!("{} non è una cartella", f.display()));
            }
            Some(f)
        }
    };
    let games = state.library.lock().unwrap().games().to_vec();
    let failed: Vec<_> = games.iter().filter(|g| !write_cloud_folder(g, folder.as_deref())).map(|g| g.name.clone()).collect();
    let mut store = state.config.lock().unwrap();
    store.config.cloud_saves_folder = folder;
    store.save().map_err(|e| e.to_string())?;
    if !failed.is_empty() {
        return Err(format!("Preferenze non salvate per: {}", failed.join(", ")));
    }
    Ok(store.config.clone())
}

#[tauri::command]
fn config_set_italian_prompts(state: State<AppState>, on: bool) -> Result<config::Config, String> {
    let mut store = state.config.lock().unwrap();
    store.config.italian_prompts = on;
    store.save().map_err(|e| e.to_string())?;
    Ok(store.config.clone())
}

// ── Cheats ──────────────────────────────────────────────────────────────────

#[tauri::command]
fn cheats_get(app: AppHandle, state: State<AppState>, title_id: String) -> Result<cheats::CheatsView, String> {
    Ok(cheats::view(&state.game(&title_id)?, &home(&app)?))
}

#[tauri::command]
fn cheats_set(
    app: AppHandle,
    state: State<AppState>,
    title_id: String,
    build_id: String,
    enabled: std::collections::BTreeSet<String>,
) -> Result<cheats::CheatsView, String> {
    let (game, home) = (state.game(&title_id)?, home(&app)?);
    cheats::set_enabled(&game, &home, &build_id, &enabled)?;
    Ok(cheats::view(&game, &home))
}

#[tauri::command]
fn cheats_master(app: AppHandle, state: State<AppState>, title_id: String, on: bool) -> Result<cheats::CheatsView, String> {
    let (game, home) = (state.game(&title_id)?, home(&app)?);
    cheats::set_master(&game, &home, on)?;
    Ok(cheats::view(&game, &home))
}

#[tauri::command]
fn cheats_import(app: AppHandle, state: State<AppState>, title_id: String, path: PathBuf) -> Result<cheats::CheatsView, String> {
    let (game, home) = (state.game(&title_id)?, home(&app)?);
    cheats::import(&game, &home, &path)?;
    Ok(cheats::view(&game, &home))
}

/// Opens the game's cheats folder, creating it as the game's menu does.
#[tauri::command]
fn cheats_reveal(app: AppHandle, state: State<AppState>, title_id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = cheats::folder(&home(&app)?, &state.game(&title_id)?.title_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

// ── Building ────────────────────────────────────────────────────────────────

/// What a build of `path` would be, and what stands in its way.
#[tauri::command]
async fn build_preflight(
    state: State<'_, AppState>,
    path: PathBuf,
    options: Option<build::Options>,
) -> Result<build::Preflight, String> {
    let keys = state.config.lock().unwrap().config.prod_keys.clone();
    build::preflight(&engine::Engine::dev_default(), &path, keys, options.unwrap_or_default()).await
}

#[tauri::command]
async fn build_start(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    options: Option<build::Options>,
) -> Result<build::Snapshot, String> {
    let keys = state.config.lock().unwrap().config.prod_keys.clone();
    build::start(&app, &engine::Engine::dev_default(), &path, keys, options.unwrap_or_default()).await
}

/// Shows what the last build wrote (the IPA, or the Mac app) in Finder.
#[tauri::command]
fn build_reveal_output(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let out = {
        let slot = state.build.lock().unwrap();
        slot.as_ref().and_then(|r| r.snap.game.as_ref()).and_then(|g| g.out.clone())
    };
    let out = out.filter(|o| o.exists()).ok_or("Nessun file creato dall'ultima build")?;
    app.opener().reveal_item_in_dir(&out).map_err(|e| e.to_string())
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
            config_set_cloud_folder,
            config_set_italian_prompts,
            cheats_get,
            cheats_set,
            cheats_master,
            cheats_import,
            cheats_reveal,
            build_reveal_output,
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
