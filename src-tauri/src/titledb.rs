//! What titledb (github.com/blawar/titledb) knows of each game's updates and
//! DLC, from its `versions.txt`: one line per title, `id|rightsId|version`.
//!
//! The whole list (about 3 MB) is downloaded, at most once a day, so the
//! request says nothing about which games the player has: the comparison
//! with the archive happens here. titledb knows few Switch 2 games: one it
//! does not know is "unknown", never "complete".

use crate::archive::{app_id, kind_of, Kind};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::process::Command;

const URL: &str = "https://raw.githubusercontent.com/blawar/titledb/master/versions.txt";
const USER_AGENT: &str = "NX-Launcher (update check)";
/// The list is about 3 MB; a far larger answer is not the list.
const MAX_BYTES: &str = "50000000";
/// The list is downloaded again when it is older than this.
pub const MAX_AGE_SECS: u64 = 24 * 3600;
const FILE: &str = "titledb-versions.txt";

/// What titledb lists for one game (by its base game's title ID).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Known {
    pub base: bool,
    /// The newest update's version (65536 per update: 327680 is the fifth).
    pub latest_update: Option<u32>,
    pub dlc: BTreeSet<u64>,
}

pub struct Titledb {
    /// When the list was downloaded (Unix seconds).
    pub fetched_at: u64,
    games: HashMap<u64, Known>,
}

impl Titledb {
    #[cfg(test)]
    pub fn from_text(text: &str) -> Self {
        Self { fetched_at: 0, games: parse(text) }
    }

    pub fn get(&self, app: u64) -> Option<&Known> {
        self.games.get(&app)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.games.len()
    }
}

/// Groups the list's titles by game: a base game, its update (…800) and its
/// DLC (base + 0x1000 + index), as the packing script groups packages.
pub fn parse(text: &str) -> HashMap<u64, Known> {
    let mut games: HashMap<u64, Known> = HashMap::new();
    for line in text.lines() {
        let mut cols = line.split('|');
        let (Some(id), _, version) = (cols.next(), cols.next(), cols.next()) else { continue };
        if id.len() != 16 {
            continue; // the header, or a damaged line
        }
        let Ok(tid) = u64::from_str_radix(id, 16) else { continue };
        let version = version.and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
        let known = games.entry(app_id(tid)).or_default();
        match kind_of(tid) {
            Kind::Base => known.base = true,
            Kind::Update => known.latest_update = known.latest_update.max(Some(version)),
            Kind::Dlc => {
                known.dlc.insert(tid);
            }
        }
    }
    games
}

fn cache_file(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

fn modified(path: &Path) -> Option<u64> {
    let t = fs::metadata(path).ok()?.modified().ok()?;
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// When the cached list was downloaded; `None` when there is none.
pub fn cached_at(data_dir: &Path) -> Option<u64> {
    modified(&cache_file(data_dir))
}

/// Reads the cached list; `None` when there is none or it is not the list.
pub fn load(data_dir: &Path) -> Option<Titledb> {
    let file = cache_file(data_dir);
    let text = fs::read_to_string(&file).ok()?;
    if !text.starts_with("id|") {
        return None;
    }
    Some(Titledb { fetched_at: modified(&file)?, games: parse(&text) })
}

/// Downloads the list over the cached one (written aside, then renamed, so a
/// failed download keeps the old list).
pub async fn download(data_dir: &Path) -> Result<(), String> {
    let file = cache_file(data_dir);
    let tmp = file.with_extension("txt.part");
    let out = Command::new("/usr/bin/curl")
        .args(["-sSfL", "-m", "60", "--max-filesize", MAX_BYTES, "-A", USER_AGENT, "-o"])
        .arg(&tmp)
        .arg(URL)
        .output()
        .await
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("elenco di titledb non scaricato: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let head = fs::read(&tmp).map(|b| b.starts_with(b"id|")).unwrap_or(false);
    if !head {
        let _ = fs::remove_file(&tmp);
        return Err("titledb ha risposto con qualcosa che non è il suo elenco".into());
    }
    fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "id|rightsId|version\n\
        010015100B514000|00000000000000000000000000000000|0\n\
        010015100B514800|00000000000000000000000000000000|327680\n\
        0100000000010000|01000000000100000000000000000003|0\n\
        0100000000010800|00000000000000000000000000000000|393216\n\
        0100000000011001|01000000000110010000000000000000|0\n\
        0100000000011002|01000000000110020000000000000000|65536\n\
        broken line\n";

    #[test]
    fn groups_updates_and_dlc_by_game() {
        let games = parse(SAMPLE);
        let wonder = &games[&0x010015100B514000];
        assert!(wonder.base);
        assert_eq!(wonder.latest_update, Some(327680));
        assert!(wonder.dlc.is_empty());
        let odyssey = &games[&0x0100000000010000];
        assert_eq!(odyssey.latest_update, Some(393216));
        assert_eq!(odyssey.dlc.iter().copied().collect::<Vec<_>>(), [0x0100000000011001, 0x0100000000011002]);
        assert_eq!(games.len(), 2);
    }

    #[test]
    fn a_game_with_no_update_has_none() {
        let games = parse("id|rightsId|version\n0400C3F00006E000|0|0\n");
        assert_eq!(games[&0x0400C3F00006E000].latest_update, None);
    }

    #[test]
    fn caches_only_the_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
        fs::write(dir.path().join(FILE), "<html>rate limited</html>").unwrap();
        assert!(load(dir.path()).is_none());
        fs::write(dir.path().join(FILE), SAMPLE).unwrap();
        let db = load(dir.path()).unwrap();
        assert_eq!(db.len(), 2);
        assert!(cached_at(dir.path()).is_some());
    }

    /// Against the real list: `cargo test titledb -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_the_real_list() {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(download(dir.path())).unwrap();
        let db = load(dir.path()).unwrap();
        assert!(db.len() > 10_000);
        assert!(db.get(0x010015100B514000).is_some_and(|k| k.latest_update.is_some()));
    }
}
