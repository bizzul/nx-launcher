//! The nxnative engine as the launcher sees it: where it lives, and a
//! "doctor" that checks this Mac can build and run games with it.

use serde::Serialize;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Homebrew tools the build script calls.
const BREW_TOOLS: [&str; 3] = ["xcodegen", "glslang", "zstd"];

/// Where the nxnative checkout and its Python venv live.
pub struct Engine {
    pub root: PathBuf,
    pub python: PathBuf,
}

impl Engine {
    /// Development layout: `engine/` (git submodule) and `engine-venv/`
    /// beside `src-tauri/`. A packaged launcher would keep them in its data
    /// folder instead; every caller goes through here.
    pub fn dev_default() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has a parent folder")
            .to_path_buf();
        Self {
            root: project.join("engine"),
            python: project.join("engine-venv/bin/python"),
        }
    }

    /// The venv's pip, for the engine's Python requirements.
    pub fn pip(&self) -> PathBuf {
        self.python.with_file_name("pip")
    }

    /// The packing script. `NXL_BUILD_SCRIPT` replaces it, for development:
    /// `scripts/fake-build.py` plays a build's events without a game.
    pub fn build_script(&self) -> PathBuf {
        match std::env::var_os("NXL_BUILD_SCRIPT") {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => self.root.join("scripts/build_ipa_from_nsp.py"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    id: &'static str,
    label: &'static str,
    ok: bool,
    detail: String,
    /// A command that fixes the problem, shown only when `ok` is false.
    fix: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    checks: Vec<Check>,
    /// The PATH every child process gets, shown so problems are visible.
    child_path: String,
}

impl DoctorReport {
    pub fn failed_labels(&self) -> Vec<&'static str> {
        self.checks.iter().filter(|c| !c.ok).map(|c| c.label).collect()
    }
}

/// The PATH given to every child process: Python, xcodebuild, the game.
///
/// An app opened from Finder or the Dock does not get your shell's PATH:
/// launchd gives it a minimal one (`/usr/bin:/bin:/usr/sbin:/sbin`) or none.
/// Then the Homebrew tools in `/opt/homebrew/bin` cannot be found and the
/// build fails with "xcodegen not found". `pnpm tauri dev` hides the
/// problem, because the app inherits the terminal's PATH.
///
/// The result is ordered as a shell set up by `brew shellenv` orders it: the
/// inherited folders keep their order, Homebrew goes just before the first
/// system folder, and whatever is missing is added at the end. So a tool
/// resolves to the same file from Finder as from the terminal.
pub fn child_path(inherited: Option<&str>) -> String {
    let mut dirs: Vec<&str> = Vec::new();
    for dir in inherited.unwrap_or("").split(':') {
        if SYSTEM_DIRS.contains(&dir) {
            HOMEBREW_DIRS.iter().for_each(|h| push_unique(&mut dirs, h));
        }
        push_unique(&mut dirs, dir);
    }
    HOMEBREW_DIRS.iter().chain(&SYSTEM_DIRS).for_each(|d| push_unique(&mut dirs, d));
    dirs.join(":")
}

/// Apple silicon Homebrew (an Intel one lives in `/usr/local`, under Rosetta).
const HOMEBREW_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/opt/homebrew/sbin"];
/// What launchd gives an app opened from Finder or the Dock.
const SYSTEM_DIRS: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

fn push_unique<'a>(dirs: &mut Vec<&'a str>, dir: &'a str) {
    if !dir.is_empty() && !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

pub async fn doctor(engine: &Engine) -> DoctorReport {
    let path = child_path(std::env::var("PATH").ok().as_deref());
    let mut checks = Vec::new();

    // 1. The engine checkout and the commit it is pinned to.
    let script_ok = engine.build_script().is_file();
    let root = engine.root.to_string_lossy().into_owned();
    let (_, commit) = run(
        "git",
        &["-C", &root, "log", "-1", "--format=%h · %ad · %s", "--date=short"],
        &path,
    )
    .await;
    checks.push(Check {
        id: "engine",
        label: "Motore nxnative",
        ok: script_ok,
        detail: if script_ok { commit } else { format!("manca {}", engine.build_script().display()) },
        fix: (!script_ok).then(|| "git submodule update --init engine".into()),
    });

    // 2. Python from the venv, native arm64 (the script refuses Rosetta).
    let (py_ok, arch) = run(
        &engine.python,
        &["-c", "import platform; print(platform.machine())"],
        &path,
    )
    .await;
    let py_ok = py_ok && arch == "arm64";
    checks.push(Check {
        id: "python",
        label: "Python del motore (arm64)",
        ok: py_ok,
        detail: format!("{} → {arch}", engine.python.display()),
        fix: (!py_ok).then(|| {
            "python3 -m venv engine-venv && engine-venv/bin/pip install -r engine/.github/requirements/tools.txt".into()
        }),
    });

    // 3. The selected Xcode (its Metal compiler is what the next check holds to the lock).
    let (xcode_ok, xcode) = run("xcodebuild", &["-version"], &path).await;
    checks.push(Check {
        id: "xcode",
        label: "Xcode",
        ok: xcode_ok,
        detail: xcode,
        fix: (!xcode_ok).then(|| "sudo xcode-select -s /Applications/Xcode.app".into()),
    });

    // 4. The Metal compiler: present (the Metal Toolchain is a separate
    //    download) and the version the engine's shader chain is locked to.
    //    The requirement is read from the engine, so it follows its updates.
    let (metal_ok, metal) = run("xcrun", &["metal", "--version"], &path).await;
    let required = required_metal(&engine.root);
    let matches = required.as_ref().is_none_or(|r| *r == metal);
    checks.push(Check {
        id: "metal",
        label: "Compilatore Metal",
        ok: metal_ok && matches,
        detail: match (&required, matches) {
            (Some(r), false) => format!("{metal} · il motore richiede: {r}"),
            _ => metal,
        },
        fix: if !metal_ok {
            Some("xcodebuild -downloadComponent MetalToolchain".into())
        } else if !matches {
            Some("Installa l'Xcode con quel compilatore e selezionalo: sudo xcode-select -s /Applications/Xcode-<versione>.app".into())
        } else {
            None
        },
    });

    // 5. Homebrew tools, looked up in the PATH children will get.
    for tool in BREW_TOOLS {
        let found = find_in_path(tool, &path);
        checks.push(Check {
            id: tool,
            label: tool,
            ok: found.is_some(),
            fix: found.is_none().then(|| format!("brew install {tool}")),
            detail: found.map_or("non trovato nel PATH".into(), |p| p.display().to_string()),
        });
    }

    DoctorReport { checks, child_path: path }
}

/// The Metal compiler the shader chain is locked to: `metal_macosx` in
/// `dm/gfx/tools/dmsh_chain.lock`. Another one stops an NVN game's build.
pub fn required_metal(root: &Path) -> Option<String> {
    metal_in_lock(&std::fs::read_to_string(root.join("dm/gfx/tools/dmsh_chain.lock")).ok()?)
}

pub fn metal_in_lock(lock: &str) -> Option<String> {
    lock.lines().find_map(|l| l.strip_prefix("metal_macosx\t")).map(|v| v.trim().to_string())
}

/// The Metal compiler this Mac would use now (the selected Xcode's).
pub async fn installed_metal() -> Option<String> {
    let path = child_path(std::env::var("PATH").ok().as_deref());
    let (ok, v) = run("xcrun", &["metal", "--version"], &path).await;
    ok.then_some(v)
}

/// Whether Xcode's iOS platform is installed (a separate download in
/// Xcode → Settings → Components), which an IPA build needs.
pub async fn has_ios_sdk() -> bool {
    let path = child_path(std::env::var("PATH").ok().as_deref());
    run("xcrun", &["--sdk", "iphoneos", "--show-sdk-path"], &path).await.0
}

/// Runs a program with the launcher's PATH. Returns whether it succeeded,
/// and the first line it printed (stdout, else stderr, else the spawn error).
async fn run(program: impl AsRef<OsStr>, args: &[&str], path: &str) -> (bool, String) {
    match Command::new(program).args(args).env("PATH", path).output().await {
        Ok(out) => {
            let text = if out.stdout.is_empty() { &out.stderr } else { &out.stdout };
            let first = String::from_utf8_lossy(text).lines().next().unwrap_or("").trim().to_string();
            (out.status.success(), first)
        }
        Err(e) => (false, e.to_string()),
    }
}

fn find_in_path(tool: &str, path: &str) -> Option<PathBuf> {
    std::env::split_paths(path).map(|dir| dir.join(tool)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::child_path;

    fn has(path: &str, dir: &str) -> bool {
        path.split(':').any(|d| d == dir)
    }

    #[test]
    fn opened_from_finder_still_finds_homebrew() {
        // What launchd gives an app opened from Finder or the Dock.
        let path = child_path(Some("/usr/bin:/bin:/usr/sbin:/sbin"));
        assert!(has(&path, "/opt/homebrew/bin"), "{path}");
        assert!(has(&path, "/usr/bin"), "{path}");
    }

    #[test]
    fn no_path_at_all() {
        let path = child_path(None);
        assert!(has(&path, "/opt/homebrew/bin"), "{path}");
        assert!(has(&path, "/usr/bin") && has(&path, "/bin"), "{path}");
    }

    #[test]
    fn keeps_what_the_terminal_had() {
        let path = child_path(Some("/Users/me/.cargo/bin:/usr/bin:/bin"));
        assert!(has(&path, "/Users/me/.cargo/bin"), "{path}");
    }

    #[test]
    fn homebrew_comes_before_the_system() {
        // Finder: a `git` or `zstd` in /opt/homebrew must win over /usr/bin,
        // as it does in a terminal after `brew shellenv`.
        assert_eq!(
            child_path(Some("/usr/bin:/bin:/usr/sbin:/sbin")),
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
        // The user's own folders stay first.
        assert_eq!(
            child_path(Some("/Users/me/.cargo/bin:/usr/bin")),
            "/Users/me/.cargo/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn a_terminal_path_is_left_as_it_is() {
        // Already complete: nothing moves, nothing is repeated, empty entries go.
        let terminal = "/Users/me/.local/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
        assert_eq!(child_path(Some(terminal)), terminal);
        assert_eq!(child_path(Some(&format!("{terminal}::/usr/bin:"))), terminal);
    }
}
