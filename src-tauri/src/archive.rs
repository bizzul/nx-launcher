//! The archive: one folder the player picks, where each game's files (base
//! game, updates, DLC) live in a folder of their own, `<name> [<title ID>]`,
//! as the packing script wants them: one game per folder, its packages side
//! by side (engine README, "Put your game in a folder"). A build takes that
//! folder as its source.
//!
//! This module reads what the folders hold; `importer.rs` brings files in.
//! A package (NSP, XCI, NSZ, XCZ) is told by the `[title ID][vN]` tags of its
//! name, which the importer adds when it had to read the package itself; a
//! NUP by its table of contents; a dump by its `main.npdm` and folder names.
//! Nothing here goes on the internet: titledb's list (`titledb.rs`) is
//! compared here with what the folders hold.

use crate::titledb::Titledb;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const PACKAGE_EXTS: [&str; 4] = ["nsp", "xci", "nsz", "xcz"];
/// What bsdtar (macOS's `tar`) opens.
pub const ARCHIVE_EXTS: [&str; 3] = ["zip", "rar", "7z"];
/// The folders of a dump (scripts/pack_dump.py).
pub const DUMP_DIRS: [&str; 5] = ["base", "updates", "dlc", "exefs", "romfs"];

// ── Title IDs ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Base,
    Update,
    Dlc,
}

/// Base game …000, update …800, DLC = base + 0x1000 + index (the engine's
/// `scan_nsps` groups packages the same way).
pub fn kind_of(tid: u64) -> Kind {
    match tid & 0xFFF {
        0 => Kind::Base,
        0x800 => Kind::Update,
        _ => Kind::Dlc,
    }
}

/// The base game's title ID of any of its titles.
pub fn app_id(tid: u64) -> u64 {
    match kind_of(tid) {
        Kind::Base => tid,
        Kind::Update => tid & !0xFFF,
        Kind::Dlc => (tid & !0xFFF).wrapping_sub(0x1000),
    }
}

pub fn hex(tid: u64) -> String {
    format!("{tid:016X}")
}

/// One title in a file: the base game, an update (by version) or a DLC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Content {
    pub title_id: u64,
    pub version: u32,
}

impl Content {
    pub fn kind(&self) -> Kind {
        kind_of(self.title_id)
    }
    pub fn app(&self) -> u64 {
        app_id(self.title_id)
    }
}

// ── Names ───────────────────────────────────────────────────────────────────

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// What the archive never looks at: hidden files (an import in progress,
/// AppleDouble `._` files), and what builds leave in a game's folder.
pub fn ignored(name: &str) -> bool {
    name.starts_with('.') || name.ends_with("_build") || name.ends_with(".app") || name.ends_with(".ipa")
}

/// The `[title ID]` and `[vN]` tags of a name, as dump tools write them:
/// `Super Mario Bros. Wonder [010015100B514800][v327680].nsp`.
pub fn name_tags(name: &str) -> (Vec<u64>, Option<u32>) {
    let (mut ids, mut version) = (Vec::new(), None);
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let tag = &after[..close];
        if tag.len() == 16 && tag.bytes().all(|b| b.is_ascii_hexdigit()) {
            ids.extend(u64::from_str_radix(tag, 16).ok());
        } else if let Some(v) = tag.strip_prefix(['v', 'V']) {
            version = v.parse().ok().or(version);
        }
        rest = &after[close + 1..];
    }
    (ids, version)
}

/// What a package holds by its name's tags; `None` without a title ID. In a
/// merged package (base game and update in one file) the version is the
/// update's.
pub fn package_contents(name: &str) -> Option<Vec<Content>> {
    let (ids, version) = name_tags(name);
    let single = ids.len() == 1;
    let contents: Vec<Content> = ids
        .into_iter()
        .map(|t| Content {
            title_id: t,
            version: if single || kind_of(t) != Kind::Base { version.unwrap_or(0) } else { 0 },
        })
        .collect();
    (!contents.is_empty()).then_some(contents)
}

/// The name an identified package gets in the archive: its own, with the
/// tags that let the archive (and the player) tell what it is.
pub fn tagged_name(name: &str, contents: &[Content]) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ids: String = contents.iter().map(|c| format!("[{}]", hex(c.title_id))).collect();
    let version = contents.iter().filter(|c| c.kind() != Kind::Base).map(|c| c.version).max().unwrap_or(0);
    format!("{} {ids}[v{version}].{}", stem.trim(), ext_of(path))
}

/// A readable name from a file or folder name: no tags in brackets, no
/// extension, underscores as spaces. `None` when nothing is left.
pub fn clean_name(stem: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in stem.chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(if c == '_' { ' ' } else { c }),
            _ => {}
        }
    }
    let words = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = words.trim_matches(|c: char| c == '-' || c == '+' || c.is_whitespace());
    (!name.is_empty()).then(|| name.to_string())
}

/// `<name> [<title ID>]`, safe as a folder name.
pub fn folder_name(name: &str, app: u64) -> String {
    let safe: String = name
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c })
        .take(120)
        .collect();
    let safe = safe.trim().trim_start_matches('.').trim();
    if safe.is_empty() { format!("[{}]", hex(app)) } else { format!("{safe} [{}]", hex(app)) }
}

/// The base game's title ID a folder's name carries.
pub fn folder_app(name: &str) -> Option<u64> {
    name_tags(name).0.into_iter().find(|t| kind_of(*t) == Kind::Base)
}

// ── NUP containers (scripts/pack_nup.py, NUP SPEC v1) ───────────────────────

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

const NUP_SUPER: usize = 0x80;
const NUP_TOC: usize = 0x30;
const NUP_PKG: usize = 0x40;
const NUP_NACP: u16 = 2;
/// Package flags bit 0: an update in a container without a base game.
const NUP_STANDALONE: u8 = 1;

#[derive(Debug)]
pub struct NupInfo {
    pub contents: Vec<Content>,
    /// The game's name from the NACP a package carries (most do not).
    pub name: Option<String>,
    /// No base game, and an update the build takes as the whole game: a base
    /// record with no files, or every update marked `standalone_update`
    /// (scripts/pack_nup.py, `scan`).
    pub whole_update: bool,
}

/// What a NUP holds, from its table of contents: the base game, its updates
/// by version, its DLC by title ID. Only the header and the package records
/// are read; the files' hashes are the build's to check.
pub fn read_nup(path: &Path) -> Result<NupInfo, String> {
    let file = file_name(path);
    let bad = |why: &str| format!("«{file}»: {why}");
    let mut f = fs::File::open(path).map_err(|e| bad(&e.to_string()))?;
    let len = f.metadata().map_err(|e| bad(&e.to_string()))?.len();
    let mut sb = [0u8; NUP_SUPER];
    f.read_exact(&mut sb).map_err(|_| bad("non è un NUP (troppo corto)"))?;
    if &sb[..3] != b"NUP" {
        return Err(bad("non è un NUP"));
    }
    if sb[3] != 1 {
        return Err(bad(&format!("NUP di formato {}, si legge il formato 1", sb[3])));
    }
    let (toc_off, fsize, npkg) = (u64_at(&sb, 8), u64_at(&sb, 64), u32_at(&sb, 72) as u64);
    if fsize != len {
        return Err(bad(&format!("{len} byte, ma l'intestazione ne dice {fsize} (copiato o scaricato a metà?)")));
    }
    let toc_len = NUP_TOC as u64 + npkg * NUP_PKG as u64;
    if npkg == 0 || npkg > 4096 || toc_off.saturating_add(toc_len) > len {
        return Err(bad("indice del NUP non valido"));
    }
    let mut toc = vec![0u8; toc_len as usize];
    f.seek(SeekFrom::Start(toc_off)).and_then(|_| f.read_exact(&mut toc)).map_err(|e| bad(&e.to_string()))?;
    if &toc[..4] != b"TOC\x01" || u32_at(&toc, 4) as u64 != npkg {
        return Err(bad("indice del NUP non valido"));
    }

    let mut contents = Vec::new();
    let mut metas = Vec::new();
    let (mut base, mut empty_base, mut updates, mut standalone) = (false, false, 0, 0);
    for i in 0..npkg as usize {
        let r = &toc[NUP_TOC + i * NUP_PKG..][..NUP_PKG];
        let (tid, version, kind) = (u64_at(r, 0), u32_at(r, 20), r[24]);
        match kind {
            0 if tid == 0 => empty_base = true,
            0 => base = true,
            1 => {
                updates += 1;
                standalone += usize::from(r[25] & NUP_STANDALONE != 0);
            }
            _ => {}
        }
        let (meta_off, meta_size) = (u64_at(r, 32), u32_at(r, 40) as u64);
        match kind {
            // A base record with title ID 0 has no files: the update is the whole game.
            0 if tid != 0 => contents.push(Content { title_id: tid, version: 0 }),
            1 if tid != 0 => contents.push(Content { title_id: tid | 0x800, version }),
            2 if tid != 0 => contents.push(Content { title_id: tid, version }),
            _ => {} // DLC updates and system packages
        }
        if kind <= 1 && meta_size > 0 && meta_size <= 4 << 20 && meta_off.saturating_add(meta_size) <= len {
            metas.push((kind, meta_off, meta_size));
        }
    }
    if contents.is_empty() {
        return Err(bad("il NUP non dice di quale gioco è"));
    }
    // The base game's control data first, then the updates'.
    metas.sort();
    let name = metas.into_iter().find_map(|(_, off, size)| {
        let mut block = vec![0u8; size as usize];
        f.seek(SeekFrom::Start(off)).and_then(|_| f.read_exact(&mut block)).ok()?;
        nacp_name(tlv(&block, NUP_NACP)?)
    });
    let whole_update = !base && updates > 0 && (empty_base || standalone == updates);
    Ok(NupInfo { contents, name, whole_update })
}

/// One record of a package's meta block: type, reserved, length, payload.
fn tlv(block: &[u8], want: u16) -> Option<&[u8]> {
    let mut p = 0;
    while p + 8 <= block.len() {
        let (t, n) = (u16_at(block, p), u32_at(block, p + 4) as usize);
        p += 8;
        let payload = block.get(p..p.checked_add(n)?)?;
        if t == want {
            return Some(payload);
        }
        p += n;
    }
    None
}

/// The first name a NACP gives (16 languages of 0x300 bytes, the name in the
/// first 0x200; American English comes first).
fn nacp_name(nacp: &[u8]) -> Option<String> {
    (0..16).find_map(|i| {
        let entry = nacp.get(i * 0x300..i * 0x300 + 0x200)?;
        let end = entry.iter().position(|b| *b == 0).unwrap_or(entry.len());
        let name = std::str::from_utf8(&entry[..end]).ok()?.trim();
        (!name.is_empty()).then(|| name.to_string())
    })
}

// ── Dump folders (scripts/pack_dump.py) ─────────────────────────────────────

/// A folder laid out as a dump: `base/`, `updates/`, `dlc/`, or `exefs/` with
/// `romfs/` directly in it (one version, as `nup extract` writes it).
pub fn is_dump(dir: &Path) -> bool {
    ["base", "updates", "dlc"].iter().any(|d| dir.join(d).is_dir())
        || (dir.join("exefs").is_dir() && dir.join("romfs").is_dir())
}

/// The program ID in an NPDM's ACI0 section (the game's title ID).
pub fn program_id(npdm: &[u8]) -> Option<u64> {
    if npdm.len() < 0x80 || &npdm[..4] != b"META" {
        return None;
    }
    let (off, size) = (u32_at(npdm, 0x70) as usize, u32_at(npdm, 0x74));
    let aci = npdm.get(off..off.checked_add(0x18)?)?;
    (&aci[..4] == b"ACI0" && size >= 0x18).then(|| u64_at(aci, 0x10))
}

/// The visible subfolders of `dir`, sorted by name.
pub fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, p)| !n.starts_with('.') && p.is_dir())
        .collect();
    out.sort();
    out
}

/// The update folders of a dump, by version (`updates/458752/`).
pub fn dump_updates(dir: &Path) -> Vec<(u32, PathBuf)> {
    subdirs(&dir.join("updates")).into_iter().filter_map(|(n, p)| Some((n.parse().ok()?, p))).collect()
}

/// The DLC folders of a dump, by title ID (`dlc/040074a01bf13001/`).
pub fn dump_dlc(dir: &Path) -> Vec<(u64, PathBuf)> {
    subdirs(&dir.join("dlc"))
        .into_iter()
        .filter(|(n, _)| n.len() == 16)
        .filter_map(|(n, p)| Some((u64::from_str_radix(&n, 16).ok()?, p)))
        .collect()
}

/// An update folder the build takes as the whole game: its code and a RomFS
/// (pack_dump.py's `is_version`), the patched RomFS a dump tool writes.
pub fn dump_whole_update(dir: &Path) -> bool {
    dump_updates(dir).iter().any(|(_, p)| p.join("exefs/main.npdm").is_file() && p.join("romfs").is_dir())
}

/// What a dump holds. The game is the program ID in a `main.npdm` (the base
/// game's or an update's); a dump of DLC only says it by the DLC's IDs.
pub fn read_dump(dir: &Path) -> Result<Vec<Content>, String> {
    let updates = dump_updates(dir);
    let dlc = dump_dlc(dir);
    let flat = dir.join("exefs").is_dir() && dir.join("romfs").is_dir();
    let npdms = [dir.join("base/exefs/main.npdm"), dir.join("exefs/main.npdm")]
        .into_iter()
        .chain(updates.iter().map(|(_, p)| p.join("exefs/main.npdm")));
    let app = npdms
        .filter_map(|p| fs::read(p).ok())
        .find_map(|b| program_id(&b))
        // A program of a multi-program game counts as its application.
        .map(|id| id & !0xFFF)
        .or_else(|| dlc.first().map(|(id, _)| app_id(*id)))
        .ok_or_else(|| format!("«{}»: nessun main.npdm, non si capisce quale gioco è", file_name(dir)))?;
    let mut out = Vec::new();
    if dir.join("base").is_dir() || flat {
        out.push(Content { title_id: app, version: 0 });
    }
    out.extend(updates.iter().map(|(v, _)| Content { title_id: app | 0x800, version: *v }));
    out.extend(dlc.iter().map(|(id, _)| Content { title_id: *id, version: 0 }));
    if out.is_empty() {
        return Err(format!("«{}»: cartella dump vuota", file_name(dir)));
    }
    Ok(out)
}

pub fn dir_size(dir: &Path) -> u64 {
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

// ── A game's folder as the UI sees it ───────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// NSP, XCI, NSZ or XCZ: need prod.keys.
    Package,
    /// One NUP container (Switch 2).
    Nup,
    /// A dump folder: base/, updates/, dlc/.
    Dump,
}

/// One title in a game's folder, and the file it is in.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub kind: Kind,
    pub title_id: String,
    pub version: u32,
    /// The file it is in; empty for the dump in the folder itself.
    pub file: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveGame {
    pub folder: PathBuf,
    /// The base game's title ID; empty when no file says it.
    pub title_id: String,
    pub name: String,
    pub size_bytes: u64,
    /// What a build of the folder reads, as the script picks it: packages
    /// over a NUP over a dump.
    pub format: Option<Format>,
    pub pieces: Vec<Piece>,
    pub has_base: bool,
    /// No base game, but an update that holds the whole game (its code and
    /// its complete RomFS): the build takes it as the game (engine README,
    /// "An update's folder alone will do").
    pub standalone: bool,
    /// The newest update in the folder.
    pub update: Option<u32>,
    pub dlc: Vec<String>,
    /// titledb lists the game (the rest below is only said when it does).
    pub known: bool,
    pub latest_update: Option<u32>,
    /// titledb knows a newer update than the folder's.
    pub update_missing: bool,
    pub missing_dlc: Vec<String>,
    /// What stands in a build's way, or what the player should know.
    pub issues: Vec<String>,
}

impl ArchiveGame {
    /// Compares the folder with what titledb lists for the game.
    pub fn check(&mut self, db: &Titledb) {
        let Ok(app) = u64::from_str_radix(&self.title_id, 16) else { return };
        let Some(known) = db.get(app) else { return };
        self.known = true;
        self.latest_update = known.latest_update;
        self.update_missing = known.latest_update.is_some_and(|l| self.update.is_none_or(|u| u < l));
        self.missing_dlc = known.dlc.iter().map(|d| hex(*d)).filter(|d| !self.dlc.contains(d)).collect();
    }
}

/// Reads one game's folder; `None` when nothing in it is a game file.
pub fn scan_game(dir: &Path) -> Option<ArchiveGame> {
    let folder = file_name(dir);
    let mut pieces: Vec<(Content, String)> = Vec::new();
    let mut issues = Vec::new();
    let (mut size, mut packages, mut nups) = (0, 0, 0);
    let mut nup_name = None;
    let mut whole_update = false;
    let dump = is_dump(dir);
    if dump {
        whole_update |= dump_whole_update(dir);
        match read_dump(dir) {
            Ok(cs) => pieces.extend(cs.into_iter().map(|c| (c, String::new()))),
            Err(e) => issues.push(e),
        }
        size += DUMP_DIRS.iter().map(|d| dir_size(&dir.join(d))).sum::<u64>();
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = file_name(&path);
        if ignored(&name) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else { continue };
        if meta.is_dir() {
            if !DUMP_DIRS.contains(&name.as_str()) && (is_dump(&path) || has_game_files(&path)) {
                issues.push(format!("«{name}»: la build non guarda nelle sottocartelle"));
            }
            continue;
        }
        let ext = ext_of(&path);
        if PACKAGE_EXTS.contains(&ext.as_str()) {
            packages += 1;
            size += meta.len();
            match package_contents(&name) {
                Some(cs) => pieces.extend(cs.into_iter().map(|c| (c, name.clone()))),
                None => issues.push(format!(
                    "«{name}»: dal nome non si capisce cosa contiene; importalo di nuovo con prod.keys impostato"
                )),
            }
        } else if ext == "nup" {
            nups += 1;
            size += meta.len();
            match read_nup(&path) {
                Ok(info) => {
                    nup_name = nup_name.or(info.name);
                    whole_update |= info.whole_update;
                    pieces.extend(info.contents.into_iter().map(|c| (c, name.clone())));
                }
                Err(e) => issues.push(e),
            }
        } else if ARCHIVE_EXTS.contains(&ext.as_str()) {
            issues.push(format!("«{name}»: ancora compresso, importalo con «Importa file…»"));
        }
    }
    if pieces.is_empty() && issues.is_empty() {
        return None;
    }

    // The game: the folder's tag, else the one most files belong to.
    let mut count: BTreeMap<u64, usize> = BTreeMap::new();
    for (c, _) in &pieces {
        *count.entry(c.app()).or_default() += 1;
    }
    let app = folder_app(&folder).or_else(|| count.iter().max_by_key(|(_, n)| **n).map(|(a, _)| *a));
    for other in count.keys().filter(|a| Some(**a) != app) {
        issues.push(format!("Ci sono file di un altro gioco ({}): la build ne vuole uno per cartella", hex(*other)));
    }
    if packages > 0 && (nups > 0 || dump) {
        issues.push("Ci sono NSP/XCI insieme a un NUP o a un dump: la build usa solo gli NSP/XCI".into());
    } else if nups > 1 {
        issues.push("Più di un NUP: la build ne vuole uno solo per cartella (uniscili con «nup pack»)".into());
    } else if nups == 1 && dump {
        issues.push("Un NUP e una cartella dump insieme: la build usa il NUP".into());
    }
    let format = if packages > 0 {
        Some(Format::Package)
    } else if nups > 0 {
        Some(Format::Nup)
    } else if dump {
        Some(Format::Dump)
    } else {
        None
    };

    let mine: Vec<&(Content, String)> = pieces.iter().filter(|(c, _)| Some(c.app()) == app).collect();
    let has_base = mine.iter().any(|(c, _)| c.kind() == Kind::Base);
    let update = mine.iter().filter(|(c, _)| c.kind() == Kind::Update).map(|(c, _)| c.version).max();
    let dlc: BTreeSet<String> = mine.iter().filter(|(c, _)| c.kind() == Kind::Dlc).map(|(c, _)| hex(c.title_id)).collect();
    let mut sorted = pieces.clone();
    sorted.sort_by_key(|(c, f)| (c.kind(), c.title_id, c.version, f.clone()));
    sorted.dedup_by_key(|(c, _)| *c);
    let title_id = app.map(hex).unwrap_or_default();
    let name = clean_name(&folder).or(nup_name).unwrap_or_else(|| title_id.clone());
    Some(ArchiveGame {
        folder: dir.to_path_buf(),
        title_id,
        name,
        size_bytes: size,
        format,
        pieces: sorted
            .into_iter()
            .map(|(c, file)| Piece { kind: c.kind(), title_id: hex(c.title_id), version: c.version, file })
            .collect(),
        has_base,
        standalone: !has_base && whole_update,
        update,
        dlc: dlc.into_iter().collect(),
        known: false,
        latest_update: None,
        update_missing: false,
        missing_dlc: Vec::new(),
        issues,
    })
}

/// Whether a folder holds packages or a NUP directly.
fn has_game_files(dir: &Path) -> bool {
    fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let ext = ext_of(&e.path());
        ext == "nup" || PACKAGE_EXTS.contains(&ext.as_str())
    })
}

pub struct Scan {
    pub games: Vec<ArchiveGame>,
    /// Game files lying in the archive itself, outside a game's folder.
    pub loose: Vec<PathBuf>,
}

/// Reads every game's folder in the archive.
pub fn scan(root: &Path) -> Result<Scan, String> {
    let entries = fs::read_dir(root).map_err(|e| format!("Cartella archivio illeggibile ({}): {e}", root.display()))?;
    let (mut games, mut loose) = (Vec::new(), Vec::new());
    for path in entries.flatten().map(|e| e.path()) {
        let name = file_name(&path);
        if ignored(&name) {
            continue;
        }
        if path.is_dir() {
            games.extend(scan_game(&path));
        } else {
            let ext = ext_of(&path);
            if ext == "nup" || PACKAGE_EXTS.contains(&ext.as_str()) || ARCHIVE_EXTS.contains(&ext.as_str()) {
                loose.push(path);
            }
        }
    }
    games.sort_by_key(|g| g.name.to_lowercase());
    loose.sort();
    Ok(Scan { games, loose })
}

/// The archive's folder for a game: the one whose name carries its title ID,
/// else one whose files are that game's (a folder the player made by hand).
pub fn folder_for(root: &Path, app: u64) -> Option<PathBuf> {
    let dirs: Vec<(String, PathBuf)> = subdirs(root).into_iter().filter(|(n, _)| !ignored(n)).collect();
    if let Some((_, p)) = dirs.iter().find(|(n, _)| folder_app(n) == Some(app)) {
        return Some(p.clone());
    }
    let want = hex(app);
    dirs.into_iter()
        .filter(|(n, _)| folder_app(n).is_none())
        .find(|(_, p)| scan_game(p).is_some_and(|g| g.title_id == want))
        .map(|(_, p)| p)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn title_ids() {
        assert_eq!(app_id(0x010015100B514800), 0x010015100B514000);
        assert_eq!(app_id(0x0100000000011001), 0x0100000000010000);
        assert_eq!(app_id(0x040074A01BF13001), 0x040074A01BF12000);
        assert_eq!(kind_of(0x0400C3F00006E000), Kind::Base);
        assert_eq!(hex(0x0400c3f00006e000), "0400C3F00006E000");
    }

    #[test]
    fn reads_name_tags() {
        let (ids, v) = name_tags("Super Mario Bros. Wonder [010015100B514800][v327680].nsp");
        assert_eq!((ids, v), (vec![0x010015100B514800], Some(327680)));
        let (ids, v) = name_tags("[site] - Zelda [0400486024dec000][v131072].nup");
        assert_eq!((ids, v), (vec![0x0400486024DEC000], Some(131072)));
        assert_eq!(name_tags("My Game.nsp"), (vec![], None));
        assert_eq!(name_tags("broken [0100"), (vec![], None));
        let merged = package_contents("Game [0100000000010000][0100000000010800][v65536].xci").unwrap();
        assert_eq!(merged[0], Content { title_id: 0x0100000000010000, version: 0 });
        assert_eq!(merged[1], Content { title_id: 0x0100000000010800, version: 65536 });
    }

    #[test]
    fn names() {
        assert_eq!(clean_name("[site] - The Legend of Zelda [0400486024dec000][v131072]").as_deref(), Some("The Legend of Zelda"));
        assert_eq!(clean_name("Mario_Kart (EU) [v0]").as_deref(), Some("Mario Kart"));
        assert_eq!(clean_name("[0100000000010000]"), None);
        assert_eq!(folder_name("Zelda: Echoes/Wisdom", 0x01008CF01BAAC000), "Zelda- Echoes-Wisdom [01008CF01BAAC000]");
        assert_eq!(folder_name("", 0x01008CF01BAAC000), "[01008CF01BAAC000]");
        assert_eq!(folder_app("Zelda [01008CF01BAAC000]"), Some(0x01008CF01BAAC000));
        let c = [Content { title_id: 0x010015100B514800, version: 327680 }];
        assert_eq!(tagged_name("wonder update.nsp", &c), "wonder update [010015100B514800][v327680].nsp");
    }

    /// A NUP with the given packages (kind, title ID, version) and, on the
    /// first, a NACP naming the game: only the parts `read_nup` reads.
    pub fn fake_nup(path: &Path, packages: &[(u8, u64, u32)], name: Option<&str>) {
        let toc_off = NUP_SUPER;
        let toc_len = NUP_TOC + packages.len() * NUP_PKG;
        let meta_off = toc_off + toc_len;
        let mut nacp = vec![0u8; 0x4000];
        if let Some(n) = name {
            nacp[..n.len()].copy_from_slice(n.as_bytes());
        }
        let mut meta = Vec::new();
        meta.extend(NUP_NACP.to_le_bytes());
        meta.extend(0u16.to_le_bytes());
        meta.extend((nacp.len() as u32).to_le_bytes());
        meta.extend(&nacp);
        let total = meta_off + if name.is_some() { meta.len() } else { 0 };
        let mut b = vec![0u8; total];
        b[..4].copy_from_slice(b"NUP\x01");
        b[8..16].copy_from_slice(&(toc_off as u64).to_le_bytes());
        b[16..24].copy_from_slice(&(toc_len as u64).to_le_bytes());
        b[64..72].copy_from_slice(&(total as u64).to_le_bytes());
        b[72..76].copy_from_slice(&(packages.len() as u32).to_le_bytes());
        b[toc_off..toc_off + 4].copy_from_slice(b"TOC\x01");
        b[toc_off + 4..toc_off + 8].copy_from_slice(&(packages.len() as u32).to_le_bytes());
        for (i, (kind, tid, version)) in packages.iter().enumerate() {
            let r = toc_off + NUP_TOC + i * NUP_PKG;
            b[r..r + 8].copy_from_slice(&tid.to_le_bytes());
            b[r + 20..r + 24].copy_from_slice(&version.to_le_bytes());
            b[r + 24] = *kind;
            if i == 0 && name.is_some() {
                b[r + 32..r + 40].copy_from_slice(&(meta_off as u64).to_le_bytes());
                b[r + 40..r + 44].copy_from_slice(&(meta.len() as u32).to_le_bytes());
            }
        }
        if name.is_some() {
            b[meta_off..].copy_from_slice(&meta);
        }
        fs::write(path, b).unwrap();
    }

    /// A main.npdm with the program ID where `program_id` reads it.
    pub fn fake_npdm(program: u64) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[..4].copy_from_slice(b"META");
        b[0x70..0x74].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x74..0x78].copy_from_slice(&0x40u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"ACI0");
        b[0x90..0x98].copy_from_slice(&program.to_le_bytes());
        b
    }

    pub fn fake_dump(dir: &Path, app: u64, updates: &[u32], dlc: &[u64]) {
        fs::create_dir_all(dir.join("base/exefs")).unwrap();
        fs::create_dir_all(dir.join("base/romfs")).unwrap();
        fs::write(dir.join("base/exefs/main.npdm"), fake_npdm(app)).unwrap();
        fs::write(dir.join("base/romfs/data.bin"), vec![1; 64]).unwrap();
        for v in updates {
            fs::create_dir_all(dir.join(format!("updates/{v}/exefs"))).unwrap();
            fs::create_dir_all(dir.join(format!("updates/{v}/romfs"))).unwrap();
            fs::write(dir.join(format!("updates/{v}/exefs/main.npdm")), fake_npdm(app)).unwrap();
        }
        for d in dlc {
            fs::create_dir_all(dir.join(format!("dlc/{d:016x}/0/romfs"))).unwrap();
        }
    }

    #[test]
    fn reads_a_nup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.nup");
        fake_nup(&p, &[(0, 0x0400C3F00006E000, 0), (1, 0x0400C3F00006E800, 131072), (2, 0x0400C3F00006F001, 0)], Some("Kart World"));
        let info = read_nup(&p).unwrap();
        assert_eq!(info.name.as_deref(), Some("Kart World"));
        let kinds: Vec<_> = info.contents.iter().map(|c| (c.kind(), c.app(), c.version)).collect();
        assert_eq!(
            kinds,
            [(Kind::Base, 0x0400C3F00006E000, 0), (Kind::Update, 0x0400C3F00006E000, 131072), (Kind::Dlc, 0x0400C3F00006E000, 0)]
        );
        assert!(!info.whole_update);
        // A container with no base game: the update is the whole game.
        fake_nup(&p, &[(0, 0, 0), (1, 0x0400C3F00006E000, 655360)], None);
        let info = read_nup(&p).unwrap();
        assert_eq!(info.contents, [Content { title_id: 0x0400C3F00006E800, version: 655360 }]);
        assert!(info.whole_update);
        // An update alone, not marked standalone: it needs its base elsewhere.
        fake_nup(&p, &[(1, 0x0400C3F00006E000, 65536)], None);
        assert!(!read_nup(&p).unwrap().whole_update);
        // Cut short: the header's size says so.
        let bytes = fs::read(&p).unwrap();
        fs::write(&p, &bytes[..bytes.len() - 1]).unwrap();
        assert!(read_nup(&p).unwrap_err().contains("a metà"));
        fs::write(&p, b"not a nup at all").unwrap();
        assert!(read_nup(&p).is_err());
    }

    #[test]
    fn reads_a_dump() {
        let dir = tempfile::tempdir().unwrap();
        fake_dump(dir.path(), 0x040074A01BF12000, &[458752], &[0x040074A01BF13001]);
        let cs = read_dump(dir.path()).unwrap();
        assert_eq!(cs.len(), 3);
        assert_eq!(cs[1], Content { title_id: 0x040074A01BF12800, version: 458752 });
        assert_eq!(cs[2].kind(), Kind::Dlc);
        assert!(is_dump(dir.path()));
    }

    #[test]
    fn an_update_dump_alone_is_a_whole_game() {
        // As a dump tool writes Mario Kart World's 1.8.0: updates/917504/ with
        // its code and its patched RomFS, and no base/.
        let root = tempfile::tempdir().unwrap();
        let g = root.path().join("mkw 1.8.0 update");
        fs::create_dir_all(g.join("updates/917504/exefs")).unwrap();
        fs::create_dir_all(g.join("updates/917504/romfs/Movie")).unwrap();
        fs::write(g.join("updates/917504/exefs/main.npdm"), fake_npdm(0x0400C3F00006E000)).unwrap();
        fs::write(g.join("updates/917504/romfs/Movie/StaffRoll_00.webm"), "x").unwrap();
        let game = scan_game(&g).unwrap();
        assert_eq!((game.title_id.as_str(), game.format), ("0400C3F00006E000", Some(Format::Dump)));
        assert_eq!((game.has_base, game.standalone, game.update), (false, true, Some(917504)));
        assert!(game.issues.is_empty(), "{:?}", game.issues);
        // titledb lists Mario Kart World with no update: nothing is missing.
        let mut game = game;
        game.check(&Titledb::from_text("id|rightsId|version\n0400C3F00006E000|0|\n"));
        assert!(game.known && !game.update_missing && game.latest_update.is_none());

        // Code without a RomFS is no whole game: it needs its base.
        fs::remove_dir_all(g.join("updates/917504/romfs")).unwrap();
        assert!(!scan_game(&g).is_some_and(|g| g.standalone));
        // An update NSP is a patch, never the whole game.
        let n = root.path().join("Wonder");
        fs::create_dir_all(&n).unwrap();
        fs::write(n.join("Wonder [010015100B514800][v327680].nsp"), "x").unwrap();
        let game = scan_game(&n).unwrap();
        assert!(!game.has_base && !game.standalone);
    }

    /// Reads a real game folder, read-only, against the launcher's cached
    /// titledb list: `NXL_SCAN=<folder> cargo test real_folder -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_folder() {
        let Some(dir) = std::env::var_os("NXL_SCAN") else { return };
        let mut game = scan_game(Path::new(&dir)).expect("no game files there");
        let data = std::env::var_os("HOME").map(PathBuf::from).unwrap().join("Library/Application Support/dev.nxlauncher.desktop");
        if let Some(db) = crate::titledb::load(&data) {
            game.check(&db);
        }
        println!("{}", serde_json::to_string_pretty(&game).unwrap());
    }

    #[test]
    fn scans_a_game_folder() {
        let root = tempfile::tempdir().unwrap();
        let g = root.path().join("Wonder [010015100B514000]");
        fs::create_dir_all(&g).unwrap();
        fs::write(g.join("Wonder [010015100B514000][v0].nsp"), vec![0; 100]).unwrap();
        fs::write(g.join("Wonder [010015100B514800][v65536].nsp"), vec![0; 10]).unwrap();
        fs::write(g.join("Wonder [010015100B514800][v131072].nsz"), vec![0; 10]).unwrap();
        fs::write(g.join("readme.txt"), "hi").unwrap();
        fs::create_dir_all(g.join("010015100B514000_build")).unwrap();
        fs::create_dir_all(g.join("Wonder.app")).unwrap();
        fs::write(root.path().join("stray.nsp"), "").unwrap();

        let scan = scan(root.path()).unwrap();
        assert_eq!(scan.loose.len(), 1);
        let game = &scan.games[0];
        assert_eq!((game.title_id.as_str(), game.name.as_str()), ("010015100B514000", "Wonder"));
        assert_eq!((game.has_base, game.update, game.size_bytes), (true, Some(131072), 120));
        assert_eq!(game.format, Some(Format::Package));
        assert!(game.issues.is_empty(), "{:?}", game.issues);
        assert_eq!(folder_for(root.path(), 0x010015100B514000), Some(g.clone()));

        let db = Titledb::from_text(
            "id|rightsId|version\n010015100B514800|0|196608\n010015100B515001|0|0\n010015100B515002|0|0\n",
        );
        let mut game = game.clone();
        game.check(&db);
        assert!(game.known && game.update_missing);
        assert_eq!(game.missing_dlc, ["010015100B515001", "010015100B515002"]);
    }

    #[test]
    fn flags_what_a_build_cannot_take() {
        let root = tempfile::tempdir().unwrap();
        let g = root.path().join("Mixed");
        fs::create_dir_all(&g).unwrap();
        fake_nup(&g.join("a.nup"), &[(0, 0x0400C3F00006E000, 0)], None);
        fake_nup(&g.join("b.nup"), &[(1, 0x0400C3F00006E000, 65536)], None);
        fs::write(g.join("other [0100000000010000][v0].nsp"), "").unwrap();
        fs::write(g.join("mystery.xci"), "").unwrap();
        let game = scan_game(&g).unwrap();
        // The folder has no tag: the game most files belong to wins.
        assert_eq!(game.title_id, "0400C3F00006E000");
        let all = game.issues.join("\n");
        assert!(all.contains("mystery.xci") && all.contains("0100000000010000") && all.contains("solo gli NSP"), "{all}");
        assert_eq!(folder_for(root.path(), 0x0400C3F00006E000), Some(g));
    }
}
