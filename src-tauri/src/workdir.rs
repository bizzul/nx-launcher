//! A game's build folder, `<title ID>_build` beside its game files: the
//! packing script's cache, which makes a repeat build take a fraction of the
//! first (docs/frontends.md). The Mac app needs none of it: its RomFS,
//! modules, shaders, pipeline archive and DLC are APFS clones or copies of
//! the build's files (docs/development.md, "the app is written as
//! `<app>.partial`"). So the folder can go, and the next build starts over.
//!
//! Freeing it keeps `profile/`: the provisional profile of a game the engine
//! has none for, a few files the next build reuses instead of surveying the
//! game again.

use crate::library::Game;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// What freeing keeps.
const KEEP: &str = "profile";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildFolder {
    pub path: PathBuf,
    /// What its files take on disk, counted one by one.
    pub size_bytes: u64,
    /// About what deleting it gives back. On the app's APFS volume the app's
    /// files are clones of the build's: their blocks stay taken while the
    /// app is there, so only the rest comes back.
    pub frees_bytes: u64,
    /// The app shares blocks with it (same APFS volume).
    pub shared: bool,
}

/// Where a game's build folder may be: beside the files it was built from,
/// else beside its app (the script writes the app next to the game files
/// when no `-o` is given, as the launcher builds).
pub fn candidates(game: &Game) -> Vec<PathBuf> {
    let name = format!("{}_build", game.title_id);
    let mut dirs = Vec::new();
    if let Some(src) = &game.source_path {
        // A .nup file's build folder is beside it; a folder's is inside it.
        dirs.extend(if src.is_dir() { Some(src.clone()) } else { src.parent().map(Path::to_path_buf) });
    }
    dirs.extend(game.app_path.parent().map(Path::to_path_buf));
    let mut out: Vec<PathBuf> = Vec::new();
    for p in dirs.into_iter().map(|d| d.join(&name)) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The game's build folder, when there is one (a real folder, not a link).
pub fn find(game: &Game) -> Option<PathBuf> {
    candidates(game).into_iter().find(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
}

/// Disk blocks under `path`, links not followed.
fn usage(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else { return 0 };
    if meta.is_dir() {
        fs::read_dir(path).into_iter().flatten().flatten().map(|e| usage(&e.path())).sum()
    } else {
        meta.blocks() * 512
    }
}

fn is_apfs(dir: &Path) -> bool {
    nix::sys::statfs::statfs(dir).is_ok_and(|s| s.filesystem_type_name() == "apfs")
}

fn same_volume(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

/// The folder, its size and about what deleting it frees.
pub fn info(game: &Game) -> Option<BuildFolder> {
    let path = find(game)?;
    let kept = usage(&path.join(KEEP));
    let size_bytes = usage(&path);
    let app = &game.app_path;
    let shared = app.is_dir() && same_volume(&path, app) && is_apfs(&path);
    // The app is (almost) all clones of the build's files: those blocks stay.
    let frees_bytes = size_bytes.saturating_sub(kept).saturating_sub(if shared { usage(app) } else { 0 });
    Some(BuildFolder { path, size_bytes, frees_bytes, shared })
}

/// Deletes what the build folder holds, but `profile/`; the folder itself
/// goes too when there is no profile to keep.
pub fn free(path: &Path) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Only ever a build folder: the path comes from `find`, this is a guard.
    if !meta.is_dir() || !name.ends_with("_build") {
        return Err(format!("{} non è una cartella di build", path.display()));
    }
    if !path.join(KEEP).is_dir() {
        return fs::remove_dir_all(path).map_err(|e| format!("Cartella di build non cancellata del tutto: {e}"));
    }
    let mut failed = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| e.to_string())?.flatten() {
        if entry.file_name() == KEEP {
            continue;
        }
        let p = entry.path();
        // remove_dir_all does not follow links: a link inside goes, not what it points to.
        let gone = match entry.file_type() {
            Ok(t) if t.is_dir() => fs::remove_dir_all(&p),
            _ => fs::remove_file(&p),
        };
        if let Err(e) = gone {
            failed.push(format!("{}: {e}", entry.file_name().to_string_lossy()));
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("Cartella di build non cancellata del tutto ({})", failed.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(dir: &Path, source: Option<PathBuf>) -> Game {
        Game {
            title_id: "0400486024DEC000".into(),
            name: "Ocarina".into(),
            version: "1.0".into(),
            bundle_id: "org.nxnative.0400486024dec000".into(),
            app_path: dir.join("Ocarina.app"),
            icon_path: None,
            added_at: 0,
            last_played: None,
            play_seconds: 0,
            last_exit: None,
            built_with: None,
            source_path: source,
            build_options: None,
            cover_path: None,
            cover_source: None,
            cover_checked: None,
        }
    }

    #[test]
    fn finds_the_folder_beside_a_nup_or_inside_a_game_folder() {
        let t = tempfile::tempdir().unwrap();
        let nup = t.path().join("Ocarina [0400486024dec000][v131072].nup");
        fs::write(&nup, "x").unwrap();
        let g = game(t.path(), Some(nup));
        assert_eq!(candidates(&g), [t.path().join("0400486024DEC000_build")]);
        assert_eq!(find(&g), None);
        fs::create_dir_all(t.path().join("0400486024DEC000_build")).unwrap();
        assert_eq!(find(&g), Some(t.path().join("0400486024DEC000_build")));

        let folder = t.path().join("Archivio/Ocarina [0400486024DEC000]");
        fs::create_dir_all(&folder).unwrap();
        let g = game(&folder, Some(folder.clone()));
        assert_eq!(candidates(&g), [folder.join("0400486024DEC000_build")]);
        // Built elsewhere, no source known: beside the app.
        let g = game(t.path(), None);
        assert_eq!(find(&g), Some(t.path().join("0400486024DEC000_build")));
    }

    #[test]
    fn frees_all_but_the_profile() {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().join("0400486024DEC000_build");
        fs::create_dir_all(b.join("extract/v0")).unwrap();
        fs::create_dir_all(b.join("profile")).unwrap();
        fs::write(b.join("extract/v0/RomFS.img"), vec![0u8; 100_000]).unwrap();
        fs::write(b.join("profile/settings.tsv"), "kept").unwrap();
        fs::write(b.join("tools.log"), "x").unwrap();
        // A link inside goes; what it points to stays.
        let outside = t.path().join("outside.txt");
        fs::write(&outside, "mine").unwrap();
        std::os::unix::fs::symlink(&outside, b.join("link")).unwrap();

        let g = game(t.path(), None);
        let i = info(&g).unwrap();
        assert!(i.size_bytes >= 100_000 && i.frees_bytes >= 100_000 && !i.shared, "{i:?}");
        free(&b).unwrap();
        let left: Vec<_> = fs::read_dir(&b).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["profile"]);
        assert!(outside.exists());

        // No profile: the whole folder goes.
        fs::remove_dir_all(b.join("profile")).unwrap();
        fs::write(b.join("x"), "x").unwrap();
        free(&b).unwrap();
        assert!(!b.exists());
        assert!(free(&outside).is_err());
    }

    #[test]
    fn an_app_of_clones_leaves_only_the_rest_to_free() {
        // The app shares the build's blocks on one APFS volume (the temp
        // folder is on the system volume): only what the app lacks counts.
        let t = tempfile::tempdir().unwrap();
        if !is_apfs(t.path()) {
            return;
        }
        let b = t.path().join("0400486024DEC000_build");
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("RomFS.img"), vec![1u8; 1 << 20]).unwrap();
        fs::write(b.join("cache.db"), vec![2u8; 1 << 18]).unwrap();
        let app = t.path().join("Ocarina.app/Contents/Resources");
        fs::create_dir_all(&app).unwrap();
        let cloned = std::process::Command::new("/bin/cp").arg("-c").arg(b.join("RomFS.img")).arg(app.join("romfs.img")).status();
        assert!(cloned.unwrap().success());
        let i = info(&game(t.path(), None)).unwrap();
        assert!(i.shared);
        assert!(i.frees_bytes >= 1 << 18 && i.frees_bytes < 1 << 20, "{i:?}");
    }
}
