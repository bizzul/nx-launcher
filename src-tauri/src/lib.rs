mod engine;
mod library;
mod runner;

use library::{Game, Library};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    library: Mutex<Library>,
    /// Games this launcher started, by title ID.
    running: Mutex<HashMap<String, runner::Session>>,
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
    let icons_dir = state.library.lock().unwrap().icons_dir.clone();
    let mut report = ImportReport { added: Vec::new(), errors: Vec::new() };
    for path in paths {
        let info = match library::read_app(&path) {
            Ok(info) => info,
            Err(e) => {
                report.errors.push(e);
                continue;
            }
        };
        let icon = match &info.icns {
            Some(icns) => library::cache_icon(icns, &icons_dir, &info.title_id).await,
            None => None,
        };
        let mut lib = state.library.lock().unwrap();
        let game = lib.upsert(info, path, icon);
        lib.save().map_err(|e| format!("Impossibile salvare la libreria: {e}"))?;
        drop(lib);
        report.added.push(state.view(&game));
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

#[tauri::command]
async fn game_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    title_id: String,
    settings_window: bool,
) -> Result<GameView, String> {
    let game = state.game(&title_id)?;
    runner::launch(&app, &game, settings_window).await?;
    Ok(state.view(&game))
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // ~/Library/Application Support/<identifier>/: library.json and icons/
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState {
                library: Mutex::new(Library::load(&data_dir)?),
                running: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            doctor,
            library_list,
            library_import,
            library_remove,
            game_launch,
            game_stop,
            game_reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
