//! Building a game with nxnative's packing script, as docs/frontends.md
//! ("Building: the NSP script") describes it: `--progress json` writes one
//! event per line on stdout; the exit code says how it ended; stderr holds
//! what is not an event (argument errors, a traceback).

use crate::engine::{child_path, Engine};
use crate::library::now;
use crate::AppState;
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const GIB: u64 = 1 << 30;
const MAX_MESSAGES: usize = 400;
const MAX_STDERR: usize = 60;

// ── What is being built ─────────────────────────────────────────────────────

/// What the build makes: a Mac app for the library, or an IPA to sideload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    #[default]
    Mac,
    Ios,
}

#[derive(Clone, Copy, Debug, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    #[serde(default)]
    pub target: Target,
    /// IPA only: `--romfs separate`, for a game too big to install: the
    /// script prints the command that copies its files to the device once.
    #[serde(default)]
    pub romfs_separate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// .nsp / .xci / .nsz / .xcz packages: need prod.keys.
    Nsp,
    /// A .nup container, or a folder holding one (Switch 2 games).
    Nup,
    /// A folder of the game's files: base/, updates/<version>/, dlc/ (or exefs/ + romfs/).
    Dump,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: PathBuf,
    pub kind: SourceKind,
    pub name: String,
    pub needs_keys: bool,
    pub size_bytes: u64,
    /// The packages found (at most a few), to show the player what was picked.
    pub files: Vec<String>,
    /// Where the script keeps `<title ID>_build/` and writes the app.
    pub build_dir: PathBuf,
    /// A `*_build` folder from an earlier build is there: a repeat build
    /// reuses it and needs far less time and space.
    pub previous_build: bool,
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Recognizes what the player picked, as the script will read it.
pub fn inspect(path: &Path) -> Result<SourceInfo, String> {
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (kind, files, size_bytes, build_dir) = if path.is_file() {
        if ext_of(path) != "nup" {
            return Err("Scegli una cartella di gioco o un file .nup".into());
        }
        let size = fs::metadata(path).map_or(0, |m| m.len());
        let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        (SourceKind::Nup, vec![file_name(path)], size, dir)
    } else if path.is_dir() {
        let entries: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| format!("Cartella illeggibile: {e}"))?
            .flatten()
            .map(|e| e.path())
            .collect();
        let with_ext = |exts: &[&str]| -> Vec<&PathBuf> {
            entries.iter().filter(|p| p.is_file() && exts.contains(&ext_of(p).as_str())).collect()
        };
        let nups = with_ext(&["nup"]);
        let packages = with_ext(&["nsp", "xci", "nsz", "xcz"]);
        let has_dir = |n: &str| path.join(n).is_dir();
        let size_of = |ps: &[&PathBuf]| ps.iter().map(|p| fs::metadata(p).map_or(0, |m| m.len())).sum();
        if !nups.is_empty() {
            (SourceKind::Nup, nups.iter().map(|p| file_name(p)).collect(), size_of(&nups), path.into())
        } else if !packages.is_empty() {
            let files = packages.iter().map(|p| file_name(p)).collect();
            (SourceKind::Nsp, files, size_of(&packages), path.into())
        } else if ["base", "updates", "exefs"].iter().any(|d| has_dir(d)) {
            (SourceKind::Dump, vec![], dir_size(path), path.into())
        } else {
            return Err(
                "Nessun gioco qui: servono file .nsp/.xci/.nsz/.xcz, un .nup, o una cartella dump (base/, updates/, exefs/)"
                    .into(),
            );
        }
    } else {
        return Err(format!("{} non esiste", path.display()));
    };
    let previous_build = fs::read_dir(&build_dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with("_build") && e.path().is_dir());
    Ok(SourceInfo {
        path: path.into(),
        needs_keys: kind == SourceKind::Nsp,
        files: files.into_iter().take(8).collect(),
        kind,
        name,
        size_bytes,
        build_dir,
        previous_build,
    })
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ── Before starting ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub source: SourceInfo,
    pub keys: Option<PathBuf>,
    pub free_bytes: u64,
    pub needed_bytes: u64,
    /// Problems that stop the build before it starts.
    pub blockers: Vec<String>,
    /// Problems the player may choose to ignore.
    pub warnings: Vec<String>,
}

/// The README's rule: about twice the game's size plus 10 GB, for a first
/// build. A repeat build reuses its folder.
pub fn needed_bytes(source: &SourceInfo) -> u64 {
    if source.previous_build { 10 * GIB } else { source.size_bytes * 2 + 10 * GIB }
}

fn free_bytes(dir: &Path) -> u64 {
    nix::sys::statvfs::statvfs(dir)
        .map(|s| s.blocks_available() as u64 * s.fragment_size() as u64)
        .unwrap_or(0)
}

pub async fn preflight(engine: &Engine, path: &Path, keys: Option<PathBuf>, opts: Options) -> Result<Preflight, String> {
    let source = inspect(path)?;
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    let doctor = crate::engine::doctor(engine).await;
    let missing: Vec<_> = doctor.failed_labels();
    if !missing.is_empty() {
        blockers.push(format!("Ambiente incompleto ({}): vedi la scheda Ambiente", missing.join(", ")));
    }
    if opts.target == Target::Ios && !crate::engine::has_ios_sdk().await {
        blockers.push("Per l'IPA serve la piattaforma iOS di Xcode: Xcode → Settings → Components".into());
    }
    if opts.target == Target::Ios && source.kind == SourceKind::Nup {
        warnings.push("I giochi Switch 2 sono testati solo su Mac: su iPhone e iPad potrebbero non partire".into());
    }
    let keys = keys.filter(|k| k.is_file());
    if source.needs_keys && keys.is_none() {
        blockers.push("I file NSP/XCI/NSZ richiedono prod.keys: sceglilo nella riga «prod.keys»".into());
    }
    let (free, needed) = (free_bytes(&source.build_dir), needed_bytes(&source));
    if free < needed {
        warnings.push(format!(
            "Spazio libero {} GB, ne servono circa {} GB per la prima build",
            free / GIB,
            needed / GIB
        ));
    }
    Ok(Preflight { source, keys, free_bytes: free, needed_bytes: needed, blockers, warnings })
}

// ── The build as the UI sees it ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StageStatus {
    Running,
    Ok,
    /// Finished without what it makes (a pipeline archive that failed).
    Warn,
    Failed,
    /// Interrupted by Cancel: not an error, the next build resumes it.
    Stopped,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub k: u64,
    pub name: String,
    pub status: StageStatus,
    pub summary: String,
    pub elapsed: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: Option<f64>,
    pub total: Option<f64>,
    pub unit: String,
    /// The total is an estimate.
    pub est: bool,
    pub note: String,
    /// 0 to 0.99, or None when the work cannot be counted.
    pub frac: Option<f64>,
    pub eta: Option<f64>,
    pub elapsed: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub t: f64,
    pub text: String,
    pub red: bool,
}

/// The `game` event: what the app will be.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltGame {
    pub title_id: String,
    pub name: String,
    pub version: String,
    pub out: Option<PathBuf>,
    /// Our copy of the icon (the script may delete its own before the end).
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub source: SourceInfo,
    pub options: Options,
    /// The engine commit the build runs, recorded on the game it makes.
    pub engine_commit: Option<String>,
    pub status: Status,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub stages: Vec<Stage>,
    /// `n` of the latest stage event; it can grow during the build.
    pub total_stages: u64,
    pub step: String,
    pub progress: Option<Progress>,
    pub game: Option<BuiltGame>,
    pub messages: VecDeque<Message>,
    pub stderr: VecDeque<String>,
    /// The title ID added to the library when the build succeeded.
    pub imported: Option<String>,
}

impl Snapshot {
    fn new(source: SourceInfo, options: Options) -> Self {
        Self {
            source,
            options,
            engine_commit: None,
            status: Status::Running,
            started_at: now(),
            finished_at: None,
            error: None,
            stages: vec![],
            total_stages: 0,
            step: String::new(),
            progress: None,
            game: None,
            messages: VecDeque::new(),
            stderr: VecDeque::new(),
            imported: None,
        }
    }

    fn push_message(&mut self, t: f64, msg: &str) {
        let (red, text) = match msg.strip_prefix("[RED] ") {
            Some(rest) => (true, rest),
            None => (false, msg),
        };
        if self.messages.len() == MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(Message { t, text: text.into(), red });
    }

    /// The last error the script reported, for the failure banner.
    fn last_red(&self) -> Option<String> {
        self.messages.iter().rev().find(|m| m.red).map(|m| m.text.clone())
    }
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

/// Folds one stdout line into the snapshot. Returns the icon path of a
/// `game` event, which the caller copies. Unknown events are ignored, as a
/// newer script may send more; a line that is not JSON becomes a message.
pub fn fold(b: &mut Snapshot, line: &str) -> Option<PathBuf> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        if !line.trim().is_empty() {
            b.push_message(now() as f64, line);
        }
        return None;
    };
    let t = v["t"].as_f64().unwrap_or(now() as f64);
    let k = v["k"].as_u64().unwrap_or(0);
    match v["ev"].as_str().unwrap_or_default() {
        "stage" => {
            b.total_stages = v["n"].as_u64().unwrap_or(b.total_stages);
            b.stages.push(Stage { k, name: s(&v, "name"), status: StageStatus::Running, summary: String::new(), elapsed: None });
            b.step = s(&v, "step");
            b.progress = None;
        }
        "step" => {
            b.step = s(&v, "step");
            b.progress = None;
        }
        "progress" => {
            b.step = s(&v, "step");
            b.progress = Some(Progress {
                done: v["done"].as_f64(),
                total: v["total"].as_f64(),
                unit: s(&v, "unit"),
                est: v["est"].as_bool().unwrap_or(false),
                note: s(&v, "note"),
                frac: v["frac"].as_f64(),
                eta: v["eta"].as_f64(),
                elapsed: v["elapsed"].as_f64().unwrap_or(0.0),
            });
        }
        "stage_end" => {
            if let Some(st) = b.stages.iter_mut().rev().find(|st| st.k == k) {
                st.status = match (v["ok"].as_bool().unwrap_or(false), v["warn"].as_bool().unwrap_or(false)) {
                    (false, _) => StageStatus::Failed,
                    (true, true) => StageStatus::Warn,
                    (true, false) => StageStatus::Ok,
                };
                st.summary = s(&v, "summary");
                st.elapsed = v["elapsed"].as_f64();
            }
            b.step.clear();
            b.progress = None;
        }
        "shaders_stale" => {
            let why = if s(&v, "reason") == "chain" { "nuova catena di traduzione" } else { "profilo cambiato" };
            let count = v["count"].as_u64().unwrap_or(0);
            b.push_message(t, &format!("Shader da ricostruire ({why}): {count} librerie"));
        }
        "game" => {
            b.game = Some(BuiltGame {
                title_id: s(&v, "title_id").to_uppercase(),
                name: s(&v, "name"),
                version: s(&v, "version"),
                out: v["out"].as_str().map(PathBuf::from),
                icon: None,
            });
            return v["icon"].as_str().map(PathBuf::from);
        }
        "say" => b.push_message(t, &s(&v, "msg")),
        _ => {}
    }
    None
}

/// The outcome by exit code (docs/frontends.md): 0 finished, 1 stopped on an
/// error, 2 invalid arguments (only stderr says why), 130 interrupted.
fn outcome(b: &Snapshot, status: Option<ExitStatus>, cancel_requested: bool) -> (Status, Option<String>) {
    let code = status.and_then(|s| s.code());
    let signal = status.and_then(|s| s.signal());
    match (code, signal) {
        // Finished before the cancel reached it: the app is there, keep it.
        (Some(0), _) => (Status::Succeeded, None),
        _ if cancel_requested => (Status::Cancelled, None),
        (Some(130), _) => (Status::Cancelled, None),
        (Some(1), _) => (Status::Failed, Some(b.last_red().unwrap_or("La build si è fermata su un errore".into()))),
        (Some(2), _) => {
            let why = b.stderr.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
            (Status::Failed, Some(format!("Argomenti non validi per lo script: {why}")))
        }
        (Some(c), _) => (Status::Failed, Some(format!("Lo script è uscito con codice {c}"))),
        (None, Some(sig)) => (Status::Failed, Some(format!("Lo script è stato interrotto (segnale {sig})"))),
        (None, None) => (Status::Failed, Some("Impossibile attendere lo script".into())),
    }
}

// ── Running it ──────────────────────────────────────────────────────────────

/// The build in progress, or the last one until the player dismisses it.
pub struct BuildRun {
    pub snap: Snapshot,
    /// The script's process group: Cancel signals all of it, as Ctrl-C would.
    pgid: Option<i32>,
    dirty: bool,
    cancel_requested: bool,
    /// Quit the launcher once the build has stopped (the player chose
    /// "cancel and quit" while it was running).
    pub quit_after: bool,
}

impl BuildRun {
    pub fn running(&self) -> bool {
        self.snap.status == Status::Running
    }
}

pub fn emit(app: &AppHandle, snap: &Snapshot) {
    let _ = app.emit("build-updated", snap);
}

pub async fn start(
    app: &AppHandle,
    engine: &Engine,
    path: &Path,
    keys: Option<PathBuf>,
    opts: Options,
) -> Result<Snapshot, String> {
    let state = app.state::<AppState>();
    if state.build.lock().unwrap().as_ref().is_some_and(BuildRun::running) {
        return Err("Una build è già in corso".into());
    }
    let pre = preflight(engine, path, keys, opts).await?;
    if let Some(b) = pre.blockers.first() {
        return Err(b.clone());
    }

    let platform = match opts.target {
        Target::Mac => "mac-app",
        Target::Ios => "ios",
    };
    let mut cmd = Command::new(&engine.python);
    cmd.arg(engine.build_script())
        .arg(&pre.source.path)
        .args(["--platform", platform, "--progress", "json"]);
    if opts.target == Target::Ios && opts.romfs_separate {
        cmd.args(["--romfs", "separate"]);
    }
    if let (true, Some(keys)) = (pre.source.needs_keys, &pre.keys) {
        cmd.arg("-k").arg(keys);
    }
    cmd.current_dir(&engine.root)
        .env("PATH", child_path(std::env::var("PATH").ok().as_deref()))
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group, so Cancel reaches the tools it runs too.
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("Impossibile avviare lo script: {e}"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let mut snap = Snapshot::new(pre.source, opts);
    snap.engine_commit = crate::updater::head(&engine.root).await.ok();
    *state.build.lock().unwrap() = Some(BuildRun {
        snap: snap.clone(),
        pgid: child.id().map(|p| p as i32),
        dirty: false,
        cancel_requested: false,
        quit_after: false,
    });

    let icons_dir = state.library.lock().unwrap().icons_dir.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        let out_task = {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let st = app.state::<AppState>();
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut slot = st.build.lock().unwrap();
                    let Some(run) = slot.as_mut() else { break };
                    if let Some(icon) = fold(&mut run.snap, &line) {
                        // Copied now: an IPA build deletes its extraction at the end.
                        let id = run.snap.game.as_ref().map(|g| g.title_id.clone()).unwrap_or_default();
                        let ext = icon.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or("jpg".into());
                        let copy = icons_dir.join(format!("build-{id}.{ext}"));
                        if fs::copy(&icon, &copy).is_ok() {
                            if let Some(g) = run.snap.game.as_mut() {
                                g.icon = Some(copy);
                            }
                        }
                    }
                    run.dirty = true;
                }
            })
        };
        let err_task = {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let st = app.state::<AppState>();
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(run) = st.build.lock().unwrap().as_mut() {
                        if run.snap.stderr.len() == MAX_STDERR {
                            run.snap.stderr.pop_front();
                        }
                        run.snap.stderr.push_back(line);
                        run.dirty = true;
                    }
                }
            })
        };
        // At most ~5 updates a second reach the UI, however fast lines come.
        let ticker = {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let st = app.state::<AppState>();
                loop {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let snap = {
                        let mut slot = st.build.lock().unwrap();
                        match slot.as_mut() {
                            Some(run) if run.running() => run.dirty.then(|| {
                                run.dirty = false;
                                run.snap.clone()
                            }),
                            _ => break,
                        }
                    };
                    if let Some(snap) = snap {
                        emit(&app, &snap);
                    }
                }
            })
        };

        let status = child.wait().await.ok();
        let _ = out_task.await;
        let _ = err_task.await;

        // Decide, then add the app to the library outside the lock.
        let (mut result, out, engine_commit, source) = {
            let slot = st.build.lock().unwrap();
            let run = slot.as_ref().expect("build slot");
            let out = run.snap.game.as_ref().and_then(|g| g.out.clone());
            let s = &run.snap;
            (outcome(s, status, run.cancel_requested), out, s.engine_commit.clone(), s.source.path.clone())
        };
        let mut imported = None;
        if result.0 == Status::Succeeded && opts.target == Target::Ios {
            // An IPA is sideloaded to the device: it is not a library entry.
            if !out.as_ref().is_some_and(|o| o.exists()) {
                result = (Status::Failed, Some("Lo script ha finito ma l'IPA non c'è".into()));
            }
        } else if result.0 == Status::Succeeded {
            match out {
                Some(out) => match crate::import_app(&st, out).await {
                    Ok(game) => {
                        // Remember what built it, for the rebuild hints and "Ricompila".
                        let mut lib = st.library.lock().unwrap();
                        if let Some(g) = lib.get_mut(&game.title_id) {
                            g.built_with = engine_commit.clone();
                            g.source_path = Some(source.clone());
                        }
                        let _ = lib.save();
                        imported = Some(game.title_id)
                    }
                    Err(e) => result = (Status::Failed, Some(format!("App creata, ma non aggiunta alla libreria: {e}"))),
                },
                None => result = (Status::Failed, Some("Lo script non ha detto dove ha scritto l'app".into())),
            }
        }

        let (snap, quit) = {
            let mut slot = st.build.lock().unwrap();
            let run = slot.as_mut().expect("build slot");
            run.snap.status = result.0;
            run.snap.error = result.1;
            run.snap.imported = imported;
            run.snap.finished_at = Some(now());
            run.snap.progress = None;
            let cut = if run.snap.status == Status::Cancelled { StageStatus::Stopped } else { StageStatus::Failed };
            for stage in run.snap.stages.iter_mut().filter(|s| s.status == StageStatus::Running) {
                stage.status = cut.clone();
            }
            run.pgid = None;
            (run.snap.clone(), run.quit_after)
        };
        let _ = ticker.await;
        emit(&app, &snap);
        if snap.imported.is_some() {
            let _ = app.emit("library-changed", ());
        }
        if quit {
            app.exit(0);
        }
    });
    Ok(snap)
}

/// Cancel: SIGINT to the script's whole process group, as Ctrl-C in a
/// terminal does. The script stops cleanly (exit 130) and the next build
/// reuses what this one finished.
pub fn cancel(state: &AppState) -> Result<(), String> {
    let mut slot = state.build.lock().unwrap();
    let run = slot.as_mut().filter(|r| r.running()).ok_or("Nessuna build in corso")?;
    run.cancel_requested = true;
    let pgid = run.pgid.ok_or("Nessuna build in corso")?;
    killpg(Pid::from_raw(pgid), Signal::SIGINT).map_err(|e| format!("Impossibile annullare: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot::new(
            SourceInfo {
            path: "/g/Game".into(),
            kind: SourceKind::Nup,
            name: "Game".into(),
            needs_keys: false,
            size_bytes: 0,
            files: vec![],
            build_dir: "/g".into(),
            previous_build: false,
            },
            Options::default(),
        )
    }

    #[test]
    fn folds_a_build_stream() {
        let mut b = snap();
        let lines = [
            r#"{"ev":"stage","k":1,"n":5,"name":"rewrite","step":"main","t":1}"#,
            r#"{"ev":"progress","k":1,"name":"rewrite","step":"main","done":3,"total":10,"unit":"files","est":false,"note":"","frac":0.3,"eta":12.5,"elapsed":4.0,"t":2}"#,
            r#"{"ev":"stage_end","k":1,"name":"rewrite","ok":true,"summary":"3 modules","elapsed":5.0,"t":3}"#,
            r#"{"ev":"game","title_id":"0400c3f00006e000","name":"Kart","version":"1.2","bundle_id":"org.nxnative.x","out":"/g/Kart.app","icon":"/g/x_build/icon.jpg","t":4}"#,
            r#"{"ev":"stage","k":2,"n":6,"name":"Pipeline archive","step":"","t":5}"#,
            r#"{"ev":"stage_end","k":2,"name":"Pipeline archive","ok":true,"warn":true,"summary":"","elapsed":1,"t":6}"#,
            r#"{"ev":"say","msg":"[RED] something broke","t":7}"#,
            r#"{"ev":"from_the_future","t":8}"#,
            "plain text line",
        ];
        let icons: Vec<_> = lines.iter().filter_map(|l| fold(&mut b, l)).collect();
        assert_eq!(icons, [PathBuf::from("/g/x_build/icon.jpg")]);
        assert_eq!(b.total_stages, 6);
        assert_eq!(b.stages[0].status, StageStatus::Ok);
        assert_eq!(b.stages[0].summary, "3 modules");
        assert_eq!(b.stages[1].status, StageStatus::Warn);
        assert!(b.progress.is_none());
        let g = b.game.as_ref().unwrap();
        assert_eq!((g.title_id.as_str(), g.out.as_deref()), ("0400C3F00006E000", Some(Path::new("/g/Kart.app"))));
        assert_eq!(b.last_red().as_deref(), Some("something broke"));
        assert_eq!(b.messages.back().unwrap().text, "plain text line");
    }

    #[test]
    fn progress_with_an_uncountable_total() {
        let mut b = snap();
        fold(&mut b, r#"{"ev":"stage","k":1,"n":1,"name":"Mac shaders","step":"","t":1}"#);
        fold(&mut b, r#"{"ev":"progress","k":1,"name":"Mac shaders","step":"compile","done":null,"total":null,"unit":"","est":false,"note":"","frac":null,"eta":null,"elapsed":1,"t":2}"#);
        let p = b.progress.as_ref().unwrap();
        assert_eq!((p.frac, p.done), (None, None));
        assert_eq!(b.step, "compile");
    }

    #[test]
    fn exit_codes() {
        let mut b = snap();
        let st = |c: i32| Some(ExitStatus::from_raw(c << 8));
        assert_eq!(outcome(&b, st(0), false).0, Status::Succeeded);
        assert_eq!(outcome(&b, st(130), true).0, Status::Cancelled);
        b.push_message(1.0, "[RED] prod.keys (-k) not given");
        assert_eq!(outcome(&b, st(1), false), (Status::Failed, Some("prod.keys (-k) not given".into())));
        b.stderr.push_back("error: argument --platform: invalid choice".into());
        assert!(outcome(&b, st(2), false).1.unwrap().contains("invalid choice"));
    }

    #[test]
    fn recognizes_sources() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::create_dir_all(d.join("Nsp")).unwrap();
        fs::write(d.join("Nsp/Game [0100][v0].nsp"), vec![0; 100]).unwrap();
        fs::create_dir_all(d.join("Dump/base/exefs")).unwrap();
        fs::write(d.join("Dump/base/exefs/main"), vec![0; 50]).unwrap();
        fs::write(d.join("Kart.nup"), vec![0; 10]).unwrap();
        fs::create_dir_all(d.join("Empty")).unwrap();

        let nsp = inspect(&d.join("Nsp")).unwrap();
        assert_eq!((nsp.kind, nsp.needs_keys, nsp.size_bytes), (SourceKind::Nsp, true, 100));
        let dump = inspect(&d.join("Dump")).unwrap();
        assert_eq!((dump.kind, dump.needs_keys, dump.size_bytes), (SourceKind::Dump, false, 50));
        let nup = inspect(&d.join("Kart.nup")).unwrap();
        assert_eq!((nup.kind, nup.name.as_str(), nup.build_dir.as_path()), (SourceKind::Nup, "Kart", d));
        assert!(inspect(&d.join("Empty")).is_err());
        assert_eq!(needed_bytes(&nsp), 200 + 10 * GIB);
    }
}
