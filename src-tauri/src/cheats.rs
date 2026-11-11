//! Cheats before the game starts (docs/frontends.md, "Cheats"): the same
//! files the game's Cheats panel reads and writes, in the game's cheats
//! folder. The host reads them at the start (or on ⌘R), so changes here
//! apply from the next start.

use crate::library::Game;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// Atmosphère refuses a cheat file of 64 KiB or more.
const MAX_FILE: u64 = 64 * 1024;

/// `<DM_ROOT>/cheats`: the host's default for a Mac app.
pub fn folder(home: &Path, title_id: &str) -> PathBuf {
    home.join(format!("Library/Application Support/nxnative/{title_id}/cheats"))
}

/// The build the app runs: the first 16 hex digits of main's build ID, as
/// the rewriter wrote it into `main.fixups.tsv` (`# BUILD_ID<TAB>…`).
pub fn app_build_id(app: &Path) -> Option<String> {
    let text = fs::read_to_string(app.join("Contents/Resources/dm/main.fixups.tsv")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("# BUILD_ID\t"))
        .map(|id| id.trim().chars().take(16).collect::<String>().to_uppercase())
        .filter(|id| id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_build_id(stem: &str) -> bool {
    stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// `{Name}`: always on.
    Master,
    /// `[Name]` with code words: a switch.
    Cheat,
    /// An entry without code words: a heading or a credits line.
    Label,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The name as `<build ID>.enabled` writes it: "Name", "Name #2", …
    pub key: String,
    pub name: String,
    pub kind: EntryKind,
    /// EdiZon's `[--SectionStart:X--]` … `[--SectionEnd:X--]`.
    pub section: Option<String>,
}

/// Reads the entries of an Atmosphère cheat file: `[Name]` and `{Name}`
/// headers, then 8-hex-digit code words. Only names and kinds are needed
/// here; the host's engine checks the code itself.
pub fn parse(text: &str) -> Vec<Entry> {
    struct Raw {
        name: String,
        master: bool,
        words: usize,
        section: Option<String>,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut section: Option<String> = None;
    for line in text.lines().map(str::trim) {
        let header = match line.chars().next() {
            Some('[') => line.find(']').map(|end| (&line[1..end], false, &line[end + 1..])),
            Some('{') => line.find('}').map(|end| (&line[1..end], true, &line[end + 1..])),
            _ => None,
        };
        let rest = match header {
            Some((name, master, rest)) => {
                let name = name.trim();
                if let Some(s) = name.strip_prefix("--SectionStart:").and_then(|s| s.strip_suffix("--")) {
                    section = Some(s.to_string());
                } else if name.starts_with("--SectionEnd:") {
                    section = None;
                }
                raw.push(Raw { name: name.to_string(), master, words: 0, section: section.clone() });
                rest
            }
            None => line,
        };
        // Code words on the header's own line belong to the entry, as in dmnt.
        let hex: usize = rest.split_whitespace().filter(|t| t.chars().all(|c| c.is_ascii_hexdigit())).map(|t| t.len()).sum();
        if let Some(last) = raw.last_mut() {
            last.words += hex / 8;
        }
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    raw.into_iter()
        .map(|r| {
            let n = seen.entry(r.name.clone()).or_insert(0);
            *n += 1;
            let key = if *n == 1 { r.name.clone() } else { format!("{} #{}", r.name, n) };
            let kind = match (r.master, r.words) {
                (_, 0) => EntryKind::Label,
                (true, _) => EntryKind::Master,
                (false, _) => EntryKind::Cheat,
            };
            let section = r.section.filter(|_| !r.name.starts_with("--Section"));
            Entry { key, name: r.name, kind, section }
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheatFile {
    pub build_id: String,
    /// The build this app runs: the file the game will load.
    pub current: bool,
    pub entries: Vec<Entry>,
    /// Keys of the cheats that are on (`<build ID>.enabled`).
    pub enabled: BTreeSet<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheatsView {
    pub folder: PathBuf,
    pub build_id: Option<String>,
    pub files: Vec<CheatFile>,
    /// The master switch: off while `.cheats-off-<title ID>` exists.
    pub master_on: bool,
    /// The last run crashed with cheats on: the host starts with all of
    /// them off until the player restores them in the game's panel.
    pub safe_mode: bool,
}

fn read_enabled(path: &Path) -> BTreeSet<String> {
    fs::read_to_string(path)
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// The files of the flat layout (`<build ID>.txt`); the Atmosphère tree under
/// the folder is read by the host too, and listed here when present.
fn cheat_files(dir: &Path, title_id: &str) -> Vec<PathBuf> {
    let tree = |root: &Path| root.join(format!("contents/{title_id}/cheats"));
    let dirs = [dir.to_path_buf(), tree(&dir.join("atmosphere")), tree(dir)];
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"))
                && p.file_stem().is_some_and(|s| is_build_id(&s.to_string_lossy()))
        })
        .collect();
    out.sort();
    out
}

pub fn view(game: &Game, home: &Path) -> CheatsView {
    let dir = folder(home, &game.title_id);
    let build_id = app_build_id(&game.app_path);
    let files = cheat_files(&dir, &game.title_id)
        .into_iter()
        .map(|path| {
            let bid = path.file_stem().unwrap().to_string_lossy().to_uppercase();
            let (entries, error) = match fs::metadata(&path) {
                Ok(m) if m.len() >= MAX_FILE => (vec![], Some("File di 64 KiB o più: Atmosphère non lo carica".into())),
                _ => match fs::read(&path) {
                    Ok(b) => (parse(&String::from_utf8_lossy(&b)), None),
                    Err(e) => (vec![], Some(e.to_string())),
                },
            };
            CheatFile {
                current: build_id.as_deref() == Some(bid.as_str()),
                enabled: read_enabled(&path.with_extension("enabled")),
                build_id: bid,
                entries,
                error,
            }
        })
        .collect();
    CheatsView {
        master_on: !dir.join(format!(".cheats-off-{}", game.title_id)).exists(),
        safe_mode: dir.join(format!(".cheats-running-{}", game.title_id)).exists(),
        folder: dir,
        build_id,
        files,
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp-nxl");
    fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, path)).map_err(|e| e.to_string())
}

/// Writes which cheats are on for one build: only keys of `[Name]` cheats
/// of that file (masters are always on, labels never), in file order.
pub fn set_enabled(game: &Game, home: &Path, build_id: &str, keys: &BTreeSet<String>) -> Result<(), String> {
    if !is_build_id(build_id) {
        return Err(format!("{build_id} non è un build ID"));
    }
    let dir = folder(home, &game.title_id);
    let file = cheat_files(&dir, &game.title_id)
        .into_iter()
        .find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(build_id)))
        .ok_or("File di cheat non trovato")?;
    let text = String::from_utf8_lossy(&fs::read(&file).map_err(|e| e.to_string())?).into_owned();
    let lines: String = parse(&text)
        .into_iter()
        .filter(|e| e.kind == EntryKind::Cheat && keys.contains(&e.key))
        .map(|e| e.key + "\n")
        .collect();
    write_atomic(&file.with_extension("enabled"), &lines)
}

/// The master switch: `.cheats-off-<title ID>` stops every cheat, toggles kept.
pub fn set_master(game: &Game, home: &Path, on: bool) -> Result<(), String> {
    let dir = folder(home, &game.title_id);
    let flag = dir.join(format!(".cheats-off-{}", game.title_id));
    if on {
        match fs::remove_file(&flag) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    } else {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(&flag, "").map_err(|e| e.to_string())
    }
}

/// Copies a cheat file into the folder. A build-ID name keeps its name; any
/// other becomes this app's build (as the game's panel does after asking).
/// A file already there is kept as `.bak`.
pub fn import(game: &Game, home: &Path, src: &Path) -> Result<String, String> {
    if !src.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt")) {
        return Err("Scegli un file .txt di cheat (formato Atmosphère)".into());
    }
    if fs::metadata(src).map_err(|e| e.to_string())?.len() >= MAX_FILE {
        return Err("File di 64 KiB o più: Atmosphère non lo carica".into());
    }
    let stem = src.file_stem().unwrap_or_default().to_string_lossy().to_uppercase();
    let build = if is_build_id(&stem) {
        stem
    } else {
        app_build_id(&game.app_path)
            .ok_or("Il nome del file non è un build ID e il build ID di quest'app non si legge")?
    };
    let dir = folder(home, &game.title_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{build}.txt"));
    if dest.exists() {
        fs::rename(&dest, dest.with_extension("txt.bak")).map_err(|e| e.to_string())?;
    }
    fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "[Credits: someone]\n\
        {Master Code}\n580F0000 01234567\n\
        [--SectionStart:Stats--]\n\
        [Max HP]\n04000000 00123456 0000270F\n\
        [Max HP]\n04000000 00123460 0000270F\n\
        [--SectionEnd:Stats--]\n\
        [Moon Jump] 80000040\n20000000\n";

    #[test]
    fn parses_kinds_sections_and_duplicates() {
        let e = parse(FILE);
        let got: Vec<_> = e.iter().map(|e| (e.key.as_str(), e.kind.clone(), e.section.as_deref())).collect();
        assert_eq!(
            got,
            [
                ("Credits: someone", EntryKind::Label, None),
                ("Master Code", EntryKind::Master, None),
                ("--SectionStart:Stats--", EntryKind::Label, None),
                ("Max HP", EntryKind::Cheat, Some("Stats")),
                ("Max HP #2", EntryKind::Cheat, Some("Stats")),
                ("--SectionEnd:Stats--", EntryKind::Label, None),
                ("Moon Jump", EntryKind::Cheat, None),
            ]
        );
    }

    #[test]
    fn reads_the_build_id_from_the_fixups_header() {
        let dir = tempfile::tempdir().unwrap();
        let dm = dir.path().join("Contents/Resources/dm");
        fs::create_dir_all(&dm).unwrap();
        fs::write(dm.join("main.fixups.tsv"), "# PATCH_OFF\t0x1000\n# BUILD_ID\tdf154e1d08e396460000000000000000\n").unwrap();
        assert_eq!(app_build_id(dir.path()).as_deref(), Some("DF154E1D08E39646"));
    }
}
