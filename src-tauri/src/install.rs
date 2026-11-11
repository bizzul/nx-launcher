//! Installing the engine in the launcher's data folder (`Engine::in_data`):
//! nxnative's checkout and its Python venv, as PLAN.md 3.2 sets them up by
//! hand. The installed app needs this once; then the engine follows main
//! with the updater (updater.rs, and `autoupdate.rs` at every start).
//!
//! The checkout comes from the project's submodule when this Mac has it (a
//! local clone: its objects are hard links, nothing is downloaded), else
//! from GitHub. Either way it starts at the commit the launcher was built
//! with (`PIN`), and its `origin` is nxnative's GitHub repository. It is
//! made under `engine.partial` and renamed when whole, so a stopped install
//! never looks done. A venv cannot be moved (its scripts hold its path), so
//! it is made in place and removed when making it fails.

use crate::engine::{child_path, Engine, PIN, UPSTREAM};
use crate::library::now;
use crate::AppState;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

const MAX_LOG: usize = 200;

/// Where a Python 3.12+ for Apple silicon usually is: Homebrew's, then
/// python.org's installer (which links /usr/local/bin). macOS's own
/// /usr/bin/python3 is too old.
const PYTHONS: [&str; 4] = [
    "/opt/homebrew/bin/python3",
    "/usr/local/bin/python3",
    "/Library/Frameworks/Python.framework/Versions/Current/bin/python3",
    "/opt/homebrew/opt/python@3.13/bin/python3.13",
];

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub running: bool,
    pub step: String,
    /// 0 to 1, or `None` when the work cannot be counted.
    pub frac: Option<f64>,
    pub log: VecDeque<String>,
    pub error: Option<String>,
    pub finished_at: Option<u64>,
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Snapshot)) {
    let state = app.state::<AppState>();
    let snap = {
        let mut slot = state.install.lock().unwrap();
        let snap = slot.get_or_insert_with(Snapshot::default);
        f(snap);
        snap.clone()
    };
    let _ = app.emit("engine-install", snap);
}

fn step(app: &AppHandle, text: &str, frac: Option<f64>) {
    update(app, |s| {
        s.step = text.into();
        s.frac = frac;
    });
}

fn log(app: &AppHandle, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    update(app, |s| {
        if s.log.len() == MAX_LOG {
            s.log.pop_front();
        }
        s.log.push_back(line.into());
    });
}

/// Starts the install in the background; progress comes as `engine-install`
/// events, and `library-changed` at the end (the rebuild hints follow the engine).
pub fn start(app: &AppHandle) -> Result<Snapshot, String> {
    let state = app.state::<AppState>();
    if !state.engine.managed {
        return Err("Questo launcher usa il motore della cartella del progetto: si aggiorna da lì".into());
    }
    {
        let mut slot = state.install.lock().unwrap();
        if slot.as_ref().is_some_and(|s| s.running) {
            return Err("L'installazione del motore è già in corso".into());
        }
        *slot = Some(Snapshot { running: true, step: "Preparo l'installazione…".into(), ..Default::default() });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let engine = app.state::<AppState>().engine.clone();
        let result = run(&app, &engine).await;
        update(&app, |s| {
            s.running = false;
            s.frac = None;
            s.finished_at = Some(now());
            match &result {
                Ok(()) => s.step = "Motore installato".into(),
                Err(e) => {
                    s.step.clear();
                    s.error = Some(e.clone());
                }
            }
        });
        let _ = app.emit("library-changed", ());
    });
    Ok(app_snapshot(&state))
}

fn app_snapshot(state: &AppState) -> Snapshot {
    state.install.lock().unwrap().clone().unwrap_or_default()
}

async fn run(app: &AppHandle, engine: &Engine) -> Result<(), String> {
    let path = child_path(std::env::var("PATH").ok().as_deref());
    if !engine.root.join("scripts/build_ipa_from_nsp.py").is_file() {
        clone(app, engine, &path).await?;
    } else {
        log(app, &format!("Motore già presente in {}", engine.root.display()));
    }
    let (python, zstd_builtin) = match engine.python.is_file() {
        true => (engine.python.clone(), python_info(&engine.python, &path).await.is_some_and(|i| i.zstd)),
        false => {
            step(app, "Cerco Python 3.12 o più recente…", None);
            let (base, info) = find_python(&path).await.ok_or(
                "Serve Python 3.12 o più recente per Apple silicon: installalo da python.org o con «brew install python», poi riprova",
            )?;
            log(app, &format!("Python: {}", base.display()));
            step(app, "Creo l'ambiente Python del motore…", None);
            let venv = engine.venv();
            if let Err(e) = stream(app, Command::new(&base).arg("-m").arg("venv").arg(&venv), &path).await {
                let _ = fs::remove_dir_all(&venv);
                return Err(format!("Ambiente Python non creato: {e}"));
            }
            (engine.python.clone(), info.zstd)
        }
    };
    step(app, "Installo le dipendenze Python del motore…", None);
    let pip = |args: &[&str]| {
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "pip", "install", "--disable-pip-version-check"]).args(args);
        cmd
    };
    let req = engine.root.join(".github/requirements/tools.txt");
    stream(app, pip(&["-r"]).arg(&req), &path).await.map_err(|e| format!("Dipendenze Python non installate: {e}"))?;
    // The build script needs zstd: built into Python from 3.14, backports.zstd before.
    if !zstd_builtin {
        stream(app, &mut pip(&["backports.zstd"]), &path).await.map_err(|e| format!("backports.zstd non installato: {e}"))?;
    }
    Ok(())
}

/// The checkout: from this Mac's project when it has one, else GitHub.
async fn clone(app: &AppHandle, engine: &Engine, path: &str) -> Result<(), String> {
    let parent = engine.root.parent().ok_or("Cartella del motore non valida")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join("engine.partial");
    let _ = fs::remove_dir_all(&tmp);
    let project = Engine::dev_default().root;
    let local = project != engine.root && git(&project, &["rev-parse", "HEAD"], path).await.is_ok();
    let (source, label) = if local {
        (project.to_string_lossy().into_owned(), "Copio il motore dalla cartella del progetto…")
    } else {
        (UPSTREAM.to_string(), "Scarico il motore da GitHub (circa 220 MB)…")
    };
    let tmp_s = tmp.to_string_lossy().into_owned();
    with_progress(app, &["clone", "--no-checkout", "--progress", &source, &tmp_s], None, label, path).await?;
    if local {
        git(&tmp, &["remote", "set-url", "origin", UPSTREAM], path).await?;
    }
    // The commit the launcher was built with, else the source's HEAD (main on GitHub).
    let mut target = "HEAD".to_string();
    if let Some(pin) = PIN {
        if git(&tmp, &["cat-file", "-e", &format!("{pin}^{{commit}}")], path).await.is_ok() {
            target = pin.to_string();
        }
    }
    with_progress(app, &["checkout", "--progress", "--detach", &target], Some(&tmp), "Preparo i file del motore…", path)
        .await?;
    let _ = fs::remove_dir_all(&engine.root); // an earlier install left without its script
    fs::rename(&tmp, &engine.root).map_err(|e| format!("{}: {e}", engine.root.display()))?;
    let short = git(&engine.root, &["rev-parse", "--short", "HEAD"], path).await.unwrap_or_default();
    log(app, &format!("Motore alla versione {short}"));
    Ok(())
}

async fn git(root: &Path, args: &[&str], path: &str) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).env("PATH", path).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The percentage of a git progress line: `Receiving objects:  45% (…)`.
pub fn progress_of(line: &str) -> Option<f64> {
    let (_, rest) = line.split_once(": ")?;
    let pct = rest.trim_start().split('%').next()?.trim();
    pct.parse::<f64>().ok().filter(|p| (0.0..=100.0).contains(p)).map(|p| p / 100.0)
}

/// Runs git, reading its progress (lines ended by `\r`) into the step's bar.
async fn with_progress(app: &AppHandle, args: &[&str], dir: Option<&Path>, label: &str, path: &str) -> Result<(), String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let mut child = cmd
        .args(args)
        .env("PATH", path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git non si avvia: {e}"))?;
    step(app, label, Some(0.0));
    let mut stderr = child.stderr.take().expect("piped stderr");
    let (mut buf, mut pending, mut last) = ([0u8; 4096], String::new(), Instant::now());
    let mut tail = Vec::new();
    loop {
        let n = stderr.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(i) = pending.find(['\r', '\n']) {
            let line: String = pending.drain(..=i).collect();
            let line = line.trim();
            match progress_of(line) {
                Some(f) if last.elapsed() >= Duration::from_millis(200) => {
                    step(app, &format!("{label} {}", line.split(':').next().unwrap_or("")), Some(f.min(0.99)));
                    last = Instant::now();
                }
                Some(_) => {}
                None if !line.is_empty() => tail.push(line.to_string()),
                None => {}
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        let why = tail.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" ");
        Err(format!("git {} non riuscito: {why}", args[0]))
    }
}

/// Runs a command, its output lines going to the install's log.
async fn stream(app: &AppHandle, cmd: &mut Command, path: &str) -> Result<(), String> {
    let mut child = cmd
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let (out, err) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    let app2 = app.clone();
    let errs = tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(err).lines();
        let mut last = Vec::new();
        while let Ok(Some(l)) = lines.next_line().await {
            log(&app2, &l);
            last.push(l);
        }
        last
    });
    let mut lines = BufReader::new(out).lines();
    while let Ok(Some(l)) = lines.next_line().await {
        log(app, &l);
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let last = errs.await.unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(last.into_iter().rev().find(|l| !l.trim().is_empty()).unwrap_or_else(|| format!("codice {:?}", status.code())))
    }
}

pub struct PythonInfo {
    /// zstd is built in (3.14+), so backports.zstd is not needed.
    pub zstd: bool,
}

/// A Python the engine can use: 3.12 or later, running natively on arm64.
async fn python_info(python: &Path, path: &str) -> Option<PythonInfo> {
    let out = Command::new(python)
        .args(["-c", "import sys, platform; print(int(sys.version_info >= (3, 12)), platform.machine(), int(sys.version_info >= (3, 14)))"])
        .env("PATH", path)
        .output()
        .await
        .ok()?;
    parse_python(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_python(line: &str) -> Option<PythonInfo> {
    let f: Vec<&str> = line.split_whitespace().collect();
    (f.len() == 3 && f[0] == "1" && f[1] == "arm64").then(|| PythonInfo { zstd: f[2] == "1" })
}

async fn find_python(path: &str) -> Option<(PathBuf, PythonInfo)> {
    let on_path = std::env::split_paths(path).map(|d| d.join("python3")).filter(|p| p.is_file());
    for p in PYTHONS.iter().map(PathBuf::from).chain(on_path) {
        if p.is_file() && !p.starts_with("/usr/bin") {
            if let Some(info) = python_info(&p, path).await {
                return Some((p, info));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_git_progress() {
        assert_eq!(progress_of("Receiving objects:  45% (1234/2742), 12.00 MiB | 5.00 MiB/s"), Some(0.45));
        assert_eq!(progress_of("Updating files: 100% (31415/31415), done."), Some(1.0));
        assert_eq!(progress_of("Cloning into 'engine.partial'..."), None);
        assert_eq!(progress_of("remote: Enumerating objects: 5, done."), None);
    }

    #[test]
    fn takes_only_a_recent_native_python() {
        assert!(parse_python("1 arm64 0\n").is_some_and(|p| !p.zstd));
        assert!(parse_python("1 arm64 1").is_some_and(|p| p.zstd));
        assert!(parse_python("1 x86_64 0").is_none()); // Rosetta
        assert!(parse_python("0 arm64 0").is_none()); // 3.9
        assert!(parse_python("").is_none());
    }

    #[test]
    fn the_launcher_knows_its_engine_commit() {
        // build.rs reads the submodule's HEAD when there is one.
        let root = Engine::dev_default().root;
        if root.join(".git").exists() {
            assert!(PIN.is_some_and(|p| p.len() == 40), "{PIN:?}");
        }
    }
}
