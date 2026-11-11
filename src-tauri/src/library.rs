//! The game library: the Mac apps nxnative packed (`--platform mac-app`) that
//! the player added, kept in `library.json` in the launcher's data folder.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The launcher every nxnative Mac app runs (`dm/mac/pack_app.py`, `EXE`).
pub const EXECUTABLE: &str = "nxnative";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// `DMTitleID`, upper case: the key of everything nxnative keeps per game
    /// (Application Support folder, log name, cheats).
    pub title_id: String,
    pub name: String,
    pub version: String,
    /// `CFBundleIdentifier`: the preferences domain of the saved choices.
    pub bundle_id: String,
    pub app_path: PathBuf,
    /// Our PNG copy of the app's icon; `None` for a game packed without one.
    pub icon_path: Option<PathBuf>,
    pub added_at: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
    #[serde(default)]
    pub play_seconds: u64,
    #[serde(default)]
    pub last_exit: Option<ExitInfo>,
    /// The engine commit this launcher built the app with; `None` for an app
    /// built elsewhere (its engine version is unknown).
    #[serde(default)]
    pub built_with: Option<String>,
    /// The game files the launcher built it from, for "Ricompila".
    #[serde(default)]
    pub source_path: Option<PathBuf>,
}

/// How the last session ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitInfo {
    pub kind: ExitKind,
    /// The launcher's exit code; `None` when it was itself killed by a signal.
    pub code: Option<i32>,
    pub at: u64,
    pub seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ExitKind {
    /// ⌘Q, the Home prompt, closing the window, or Quit in the settings window.
    Quit,
    /// The player pressed Stop in this launcher.
    Stopped,
    /// 70: macOS stopped running the game's GPU work.
    GpuRevoked,
    /// 1: the launcher stopped before the game (data missing, no title ID…).
    LauncherError,
    /// The host died on a signal: the launcher returns 128 + signal (134 abort).
    Crashed { signal: i32 },
    Other,
}

/// Turns how the app's process ended into what the player is told.
/// `code` is the exit code, `signal` the signal that killed the process itself.
pub fn classify_exit(code: Option<i32>, signal: Option<i32>, stop_requested: bool) -> ExitKind {
    if stop_requested {
        return ExitKind::Stopped;
    }
    match (code, signal) {
        (Some(0), _) => ExitKind::Quit,
        (Some(70), _) => ExitKind::GpuRevoked,
        (Some(1), _) => ExitKind::LauncherError,
        (Some(c), _) if (129..=159).contains(&c) => ExitKind::Crashed { signal: c - 128 },
        (None, Some(s)) => ExitKind::Crashed { signal: s },
        _ => ExitKind::Other,
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The Info.plist keys the launcher reads (`dm/mac/pack_app.py`, `info_plist`).
#[derive(Deserialize)]
struct InfoPlist {
    #[serde(rename = "DMTitleID")]
    title_id: Option<String>,
    #[serde(rename = "CFBundleDisplayName")]
    display_name: Option<String>,
    #[serde(rename = "CFBundleName")]
    name: Option<String>,
    #[serde(rename = "CFBundleShortVersionString")]
    version: Option<String>,
    #[serde(rename = "CFBundleIdentifier")]
    bundle_id: Option<String>,
    #[serde(rename = "CFBundleIconFile")]
    icon_file: Option<String>,
}

/// What an nxnative Mac app says about itself.
#[derive(Debug, PartialEq)]
pub struct AppInfo {
    pub title_id: String,
    pub name: String,
    pub version: String,
    pub bundle_id: String,
    /// The `.icns` in the bundle, when the game was packed with an icon.
    pub icns: Option<PathBuf>,
}

/// Reads an app packed by nxnative. Errors are for the player, in Italian.
pub fn read_app(app: &Path) -> Result<AppInfo, String> {
    let file = app.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !file.ends_with(".app") || !app.is_dir() {
        return Err(format!("«{file}» non è un'app per Mac"));
    }
    let info: InfoPlist = plist::from_file(app.join("Contents/Info.plist"))
        .map_err(|e| format!("«{file}»: Info.plist illeggibile ({e})"))?;
    let title_id = info
        .title_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("«{file}» non è un gioco di nxnative (manca DMTitleID)"))?
        .to_uppercase();
    if !app.join("Contents/MacOS").join(EXECUTABLE).is_file() {
        return Err(format!("«{file}»: manca Contents/MacOS/{EXECUTABLE}, ricompilalo con nxnative"));
    }
    let icns = info.icon_file.map(|f| {
        let f = if f.ends_with(".icns") { f } else { format!("{f}.icns") };
        app.join("Contents/Resources").join(f)
    });
    Ok(AppInfo {
        name: info.display_name.or(info.name).unwrap_or_else(|| title_id.clone()),
        version: info.version.unwrap_or_default(),
        bundle_id: info.bundle_id.unwrap_or_else(|| format!("org.nxnative.{}", title_id.to_lowercase())),
        icns: icns.filter(|p| p.is_file()),
        title_id,
    })
}

/// Converts the app's `.icns` into a 256-pixel PNG of our own, which the web
/// view can show (it cannot draw `.icns`). A failure only costs the icon.
pub async fn cache_icon(icns: &Path, icons_dir: &Path, title_id: &str) -> Option<PathBuf> {
    let out = icons_dir.join(format!("{title_id}.png"));
    let status = tokio::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "--resampleHeightWidthMax", "256"])
        .arg(icns)
        .arg("--out")
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok()?;
    (status.success() && out.is_file()).then_some(out)
}

pub struct Library {
    file: PathBuf,
    pub icons_dir: PathBuf,
    games: Vec<Game>,
}

impl Library {
    /// Loads `library.json` from `data_dir`. A file that cannot be read is
    /// set aside (never overwritten) and the library starts empty.
    pub fn load(data_dir: &Path) -> io::Result<Self> {
        let icons_dir = data_dir.join("icons");
        fs::create_dir_all(&icons_dir)?;
        let file = data_dir.join("library.json");
        let games = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                let aside = data_dir.join(format!("library.broken-{}.json", now()));
                eprintln!("library.json unreadable ({e}); moved to {}", aside.display());
                let _ = fs::rename(&file, &aside);
                Vec::new()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { file, icons_dir, games })
    }

    /// Writes a temporary file and renames it over `library.json`, so a crash
    /// mid-write never leaves half a library.
    pub fn save(&self) -> io::Result<()> {
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.games)?)?;
        fs::rename(&tmp, &self.file)
    }

    pub fn games(&self) -> &[Game] {
        &self.games
    }

    pub fn get(&self, title_id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.title_id == title_id)
    }

    pub fn get_mut(&mut self, title_id: &str) -> Option<&mut Game> {
        self.games.iter_mut().find(|g| g.title_id == title_id)
    }

    /// Adds an app, or refreshes the entry of the same game (a rebuild, or
    /// the app moved): its play time and history are kept.
    pub fn upsert(&mut self, info: AppInfo, app_path: PathBuf, icon_path: Option<PathBuf>) -> Game {
        let game = match self.get_mut(&info.title_id) {
            Some(g) => {
                g.name = info.name;
                g.version = info.version;
                g.bundle_id = info.bundle_id;
                g.app_path = app_path;
                g.icon_path = icon_path;
                // An app added by hand may have been rebuilt elsewhere: its
                // engine version is unknown again (a build here sets it after).
                g.built_with = None;
                g.clone()
            }
            None => {
                let g = Game {
                    title_id: info.title_id,
                    name: info.name,
                    version: info.version,
                    bundle_id: info.bundle_id,
                    app_path,
                    icon_path,
                    added_at: now(),
                    last_played: None,
                    play_seconds: 0,
                    last_exit: None,
                    built_with: None,
                    source_path: None,
                };
                self.games.push(g.clone());
                g
            }
        };
        self.games.sort_by_key(|g| g.name.to_lowercase());
        game
    }

    /// Forgets a game. Its app, saves and log stay where they are.
    pub fn remove(&mut self, title_id: &str) -> Option<Game> {
        let i = self.games.iter().position(|g| g.title_id == title_id)?;
        let g = self.games.remove(i);
        if let Some(icon) = &g.icon_path {
            let _ = fs::remove_file(icon);
        }
        Some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for an app packed by nxnative, as `scripts/make-fake-game.sh` makes.
    fn fake_app(dir: &Path, plist_xml: &str) -> PathBuf {
        let app = dir.join("Fake Game.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/MacOS/nxnative"), "#!/bin/sh\n").unwrap();
        fs::write(app.join("Contents/Info.plist"), plist_xml).unwrap();
        app
    }

    fn plist(keys: &str) -> String {
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>{keys}</dict></plist>"#)
    }

    #[test]
    fn reads_an_nxnative_app() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_app(dir.path(), &plist(
            "<key>DMTitleID</key><string>0100fa4e00001000</string>\
             <key>CFBundleName</key><string>Fake</string>\
             <key>CFBundleDisplayName</key><string>Fake Game</string>\
             <key>CFBundleShortVersionString</key><string>1.2.0</string>",
        ));
        let info = read_app(&app).unwrap();
        assert_eq!(info.title_id, "0100FA4E00001000");
        assert_eq!(info.name, "Fake Game");
        assert_eq!(info.version, "1.2.0");
        assert_eq!(info.bundle_id, "org.nxnative.0100fa4e00001000");
        assert_eq!(info.icns, None);
    }

    #[test]
    fn refuses_an_app_that_is_not_a_game() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_app(dir.path(), &plist("<key>CFBundleName</key><string>Notes</string>"));
        assert!(read_app(&app).unwrap_err().contains("DMTitleID"));
        assert!(read_app(dir.path()).is_err());
    }

    #[test]
    fn library_round_trip_keeps_play_time_on_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let info = || AppInfo {
            title_id: "0100FA4E00001000".into(),
            name: "Fake".into(),
            version: "1.0".into(),
            bundle_id: "org.nxnative.0100fa4e00001000".into(),
            icns: None,
        };
        let mut lib = Library::load(dir.path()).unwrap();
        lib.upsert(info(), "/a/Fake.app".into(), None);
        lib.get_mut("0100FA4E00001000").unwrap().play_seconds = 600;
        lib.save().unwrap();

        let mut lib = Library::load(dir.path()).unwrap();
        let g = lib.upsert(info(), "/b/Fake.app".into(), None);
        assert_eq!(lib.games().len(), 1);
        assert_eq!(g.play_seconds, 600);
        assert_eq!(g.app_path, PathBuf::from("/b/Fake.app"));
    }

    #[test]
    fn a_broken_library_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("library.json"), "{ not json").unwrap();
        let lib = Library::load(dir.path()).unwrap();
        assert!(lib.games().is_empty());
        let aside = fs::read_dir(dir.path()).unwrap().flatten().any(|e| {
            e.file_name().to_string_lossy().starts_with("library.broken-")
        });
        assert!(aside);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(classify_exit(Some(0), None, false), ExitKind::Quit);
        assert_eq!(classify_exit(Some(70), None, false), ExitKind::GpuRevoked);
        assert_eq!(classify_exit(Some(1), None, false), ExitKind::LauncherError);
        assert_eq!(classify_exit(Some(134), None, false), ExitKind::Crashed { signal: 6 });
        assert_eq!(classify_exit(None, Some(9), false), ExitKind::Crashed { signal: 9 });
        assert_eq!(classify_exit(Some(143), None, true), ExitKind::Stopped);
        assert_eq!(classify_exit(Some(3), None, false), ExitKind::Other);
    }
}
