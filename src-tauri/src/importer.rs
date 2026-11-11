//! Bringing game files into the archive (`archive.rs`): from a folder, a
//! file, or a .zip / .rar / .7z, which bsdtar (macOS's own `/usr/bin/tar`)
//! opens. Each package, NUP or dump folder found goes to its game's folder,
//! made when the game is new; what the folder already holds is skipped.
//!
//! On one volume a copy is an APFS clone (instant, no extra space) and a move
//! is a rename; across volumes both copy through a hidden `.part` name, so a
//! stop halfway never leaves a file that looks whole. Archives are opened in
//! a hidden folder inside the archive, so what they hold is then moved, not
//! copied. The import runs on a thread of its own and reports to the UI as a
//! build does (`archive-import` events), with Cancel.

use crate::archive::{self, file_name, hex, Content, Format, Kind};
use crate::library::now;
use crate::AppState;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const GIB: u64 = 1 << 30;
const MAX_LOG: usize = 500;
/// An archive inside an archive is opened; one more level down is not.
const MAX_NESTING: u32 = 2;
/// Folders are searched this many levels down for game files.
const MAX_DEPTH: u32 = 6;
const CANCELLED: &str = "Importazione annullata";

// ── What the UI sees ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Info,
    Ok,
    Warn,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub running: bool,
    pub cancelled: bool,
    /// The player's files are moved into the archive, not copied.
    pub moving: bool,
    pub step: String,
    /// 0 to 1, or `None` when the work cannot be measured.
    pub frac: Option<f64>,
    pub log: Vec<Line>,
    /// The games that received files (title IDs).
    pub games: Vec<String>,
    pub added: usize,
    pub skipped: usize,
    pub failed: usize,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

/// The import in progress, or the last one until the player dismisses it.
pub struct ImportRun {
    pub snap: Snapshot,
    cancel: Arc<AtomicBool>,
    last_emit: Instant,
}

impl ImportRun {
    pub fn running(&self) -> bool {
        self.snap.running
    }
}

pub struct Request {
    pub paths: Vec<PathBuf>,
    pub root: PathBuf,
    pub moving: bool,
    /// prod.keys, for nx_extract to read packages whose name says nothing.
    pub keys: Option<PathBuf>,
    pub nx_extract: PathBuf,
    /// The library's names by title ID: a new game's folder takes its name.
    pub names: HashMap<u64, String>,
}

/// Where an import reports: the UI, or a test.
pub trait Report {
    fn step(&self, text: &str, frac: Option<f64>);
    fn log(&self, text: String, tone: Tone);
    fn cancelled(&self) -> bool;
    /// A file reached its game's folder (`added`), or was there already.
    fn placed(&self, app: u64, added: bool);
    fn failed(&self);
}

// ── Finding the game files ──────────────────────────────────────────────────

#[derive(Debug)]
struct Source {
    path: PathBuf,
    format: Format,
    /// Empty for a package whose name says nothing (nx_extract reads it).
    contents: Vec<Content>,
    /// A name for a new game's folder: the NUP's own, else the file's.
    name: Option<String>,
    /// The archive holds it already (opened from an archive, or loose in
    /// the archive's folder): moved, never copied.
    owned: bool,
    /// The player's archive (.zip, .rar, .7z) it came out of, by index.
    from: Option<usize>,
}

pub fn run(r: &dyn Report, req: &Request) -> Result<(), String> {
    if !req.root.is_dir() {
        return Err(format!("La cartella archivio non c'è più: {}", req.root.display()));
    }
    let staging = req.root.join(format!(".nxl-import-{}", now()));
    let result = run_in(r, req, &staging);
    if staging.exists() {
        r.step("Pulisco i file temporanei…", None);
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn run_in(r: &dyn Report, req: &Request, staging: &Path) -> Result<(), String> {
    let mut sources = Vec::new();
    let mut walk = Walk { req, staging, opened: 0, archives: Vec::new(), spoiled: BTreeSet::new() };
    for path in &req.paths {
        if r.cancelled() {
            return Err(CANCELLED.into());
        }
        r.step(&format!("Cerco i file di gioco in «{}»…", file_name(path)), None);
        walk.walk(r, path, 0, 0, false, None, &mut sources)?;
    }
    identify(r, req, &mut sources);
    walk.spoiled.extend(sources.iter().filter(|s| s.contents.is_empty()).filter_map(|s| s.from));
    if sources.is_empty() && !r.cancelled() {
        r.log(
            "Nessun file di gioco trovato: servono NSP, XCI, NSZ, XCZ, NUP o una cartella dump (base/, updates/, dlc/)".into(),
            Tone::Warn,
        );
    }
    // Base games first: a new game's folder takes the base game's name.
    sources.sort_by_key(|s| (s.contents.iter().map(Content::kind).min(), file_name(&s.path)));
    let mut names = req.names.clone();
    for src in sources.iter().filter(|s| !s.contents.is_empty()) {
        if r.cancelled() {
            return Err(CANCELLED.into());
        }
        if let Err(e) = place(r, req, src, &mut names) {
            if r.cancelled() {
                return Err(CANCELLED.into());
            }
            r.log(e, Tone::Error);
            r.failed();
            walk.spoiled.extend(src.from);
        }
    }
    // An archive all of whose game files reached the archive is no longer
    // needed when the player moves files, or when it lay in the archive.
    let used: BTreeSet<usize> = sources.iter().filter_map(|s| s.from).collect();
    for (i, path) in walk.archives.iter().enumerate() {
        let consumed = used.contains(&i) && !walk.spoiled.contains(&i);
        if consumed && (req.moving || path.starts_with(&req.root)) && fs::remove_file(path).is_ok() {
            r.log(format!("«{}»: tutto importato, archivio eliminato", file_name(path)), Tone::Info);
        }
    }
    Ok(())
}

struct Walk<'a> {
    req: &'a Request,
    staging: &'a Path,
    /// Archives opened so far: each gets its own folder in `staging`.
    opened: u32,
    /// The player's archives opened (not those found inside one).
    archives: Vec<PathBuf>,
    /// The archives (by index) one of whose files could not be imported.
    spoiled: BTreeSet<usize>,
}

impl Walk<'_> {
    /// Adds the game files at `path` to `out`: a package, a NUP, a dump
    /// folder, what a folder holds, what an archive holds once opened.
    /// Problems with one file are logged; only Cancel stops the walk.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        r: &dyn Report,
        path: &Path,
        depth: u32,
        nesting: u32,
        owned: bool,
        from: Option<usize>,
        out: &mut Vec<Source>,
    ) -> Result<(), String> {
        if r.cancelled() {
            return Err(CANCELLED.into());
        }
        let name = file_name(path);
        let top = depth == 0 && nesting == 0;
        let Ok(meta) = fs::metadata(path) else {
            if top {
                r.log(format!("«{name}» non esiste"), Tone::Warn);
            }
            return Ok(());
        };
        let inside = path.starts_with(&self.req.root) && !path.starts_with(self.staging);
        let owned = owned || inside;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

        if meta.is_dir() {
            if name.ends_with(".app") {
                if top {
                    r.log(format!("«{name}» è un'app già pronta: per la libreria usa «Aggiungi gioco»"), Tone::Warn);
                }
                return Ok(());
            }
            if inside {
                // A game's folder (or the archive itself): already in place.
                r.log(format!("«{name}» è già nell'archivio"), Tone::Info);
                return Ok(());
            }
            if archive::is_dump(path) {
                match archive::read_dump(path) {
                    Ok(contents) => out.push(Source {
                        path: path.into(),
                        format: Format::Dump,
                        contents,
                        name: archive::clean_name(&name),
                        owned,
                        from,
                    }),
                    Err(e) => {
                        r.log(e, Tone::Error);
                        r.failed();
                        self.spoiled.extend(from);
                    }
                }
                return Ok(());
            }
            if depth >= MAX_DEPTH {
                return Ok(());
            }
            let mut children: Vec<PathBuf> = fs::read_dir(path)
                .map_err(|e| format!("«{name}»: {e}"))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| !archive::ignored(&file_name(p)))
                .collect();
            children.sort();
            let before = out.len();
            for child in children {
                self.walk(r, &child, depth + 1, nesting, owned, from, out)?;
            }
            if top && out.len() == before {
                r.log(format!("«{name}»: nessun file di gioco dentro"), Tone::Warn);
            }
            return Ok(());
        }

        let ext = archive::ext_of(path);
        if let Some(part) = volume(&name, path) {
            if part == 1 {
                r.log(
                    format!("«{name}»: archivio diviso in più parti, che tar non apre: estrailo con Keka o The Unarchiver e importa la cartella"),
                    Tone::Error,
                );
                r.failed();
                self.spoiled.extend(from);
            }
            return Ok(());
        }
        if archive::PACKAGE_EXTS.contains(&ext.as_str()) {
            out.push(Source {
                path: path.into(),
                format: Format::Package,
                contents: archive::package_contents(&name).unwrap_or_default(),
                name: archive::clean_name(&stem),
                owned,
                from,
            });
        } else if ext == "nup" {
            match archive::read_nup(path) {
                Ok(info) => out.push(Source {
                    path: path.into(),
                    format: Format::Nup,
                    contents: info.contents,
                    name: info.name.or_else(|| archive::clean_name(&stem)),
                    owned,
                    from,
                }),
                Err(e) => {
                    r.log(e, Tone::Error);
                    r.failed();
                    self.spoiled.extend(from);
                }
            }
        } else if archive::ARCHIVE_EXTS.contains(&ext.as_str()) {
            if nesting >= MAX_NESTING {
                r.log(format!("«{name}»: archivio dentro un archivio dentro un archivio, saltato"), Tone::Warn);
                return Ok(());
            }
            // What comes out of the player's archive is traced back to it;
            // an archive found inside one belongs to the outer one.
            let from = if path.starts_with(self.staging) {
                from
            } else {
                self.archives.push(path.to_path_buf());
                Some(self.archives.len() - 1)
            };
            self.opened += 1;
            let into = self.staging.join(self.opened.to_string());
            match extract(r, path, &into) {
                Ok(()) => {
                    if path.starts_with(self.staging) {
                        let _ = fs::remove_file(path); // an archive opened from an archive: no longer needed
                    }
                    let before = out.len();
                    self.walk(r, &into, 1, nesting + 1, true, from, out)?;
                    if out.len() == before {
                        r.log(format!("«{name}»: nessun file di gioco dentro"), Tone::Warn);
                    }
                }
                Err(_) if r.cancelled() => return Err(CANCELLED.into()),
                Err(e) => {
                    r.log(e, Tone::Error);
                    r.failed();
                    self.spoiled.extend(from);
                }
            }
        } else if top {
            r.log(format!("«{name}» non è un file di gioco"), Tone::Warn);
        }
        Ok(())
    }
}

/// The part number of a split archive's volume: `x.part2.rar`, `x.r00`,
/// `x.z01`, `x.7z.001`, or the `x.rar` / `x.zip` such parts go with.
fn volume(name: &str, path: &Path) -> Option<u32> {
    let lower = name.to_lowercase();
    if let Some(stem) = lower.strip_suffix(".rar") {
        if let Some((_, n)) = stem.rsplit_once(".part") {
            return n.parse().ok();
        }
        return path.with_extension("r00").exists().then_some(1);
    }
    if lower.ends_with(".zip") {
        return path.with_extension("z01").exists().then_some(1);
    }
    let ext = archive::ext_of(path);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if ext.len() == 3 && (ext.starts_with('r') || ext.starts_with('z')) && digits(&ext[1..]) {
        return Some(2);
    }
    if ext.len() == 3 && digits(&ext) && [".7z.", ".zip.", ".rar."].iter().any(|s| lower.contains(s)) {
        return ext.parse().ok();
    }
    None
}

fn free_bytes(dir: &Path) -> u64 {
    nix::sys::statvfs::statvfs(dir)
        .map(|s| s.blocks_available() as u64 * s.fragment_size() as u64)
        .unwrap_or(0)
}

/// Opens an archive into `into` with bsdtar, which reads ZIP, RAR (4 and 5)
/// and 7z by their content, whatever the extension says.
fn extract(r: &dyn Report, archive: &Path, into: &Path) -> Result<(), String> {
    let name = file_name(archive);
    let size = fs::metadata(archive).map_err(|e| format!("«{name}»: {e}"))?.len();
    fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let free = free_bytes(into);
    if free < size + GIB {
        return Err(format!(
            "«{name}»: per estrarlo servono almeno {} GB liberi nell'archivio, ce ne sono {}",
            (size + GIB).div_ceil(GIB),
            free / GIB
        ));
    }
    let log = into.with_extension("log");
    let err = fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("/usr/bin/tar");
    cmd.arg("-xf").arg(archive).arg("-C").arg(into).stdin(Stdio::null()).stdout(Stdio::null()).stderr(err);
    let measure = || (size > 0).then(|| archive::dir_size(into) as f64 / size as f64);
    let ok = watch(r, cmd, &format!("Estraggo «{name}»…"), measure)?;
    let text = fs::read_to_string(&log).unwrap_or_default();
    let _ = fs::remove_file(&log);
    if ok {
        return Ok(());
    }
    let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").trim_start_matches("tar: ");
    let why = if text.contains("assphrase") || text.contains("encrypt") {
        "è protetto da password: estrailo tu e importa la cartella".to_string()
    } else if text.contains("nsupported") || text.contains("nrecognized") {
        format!("tar non lo apre ({last}): estrailo con Keka o The Unarchiver e importa la cartella")
    } else if last.is_empty() {
        "estrazione non riuscita".to_string()
    } else {
        format!("estrazione non riuscita ({last})")
    };
    Err(format!("«{name}»: {why}"))
}

/// Runs a tool, reporting `measure` (0 to 1) about every second; Cancel kills
/// it. `Ok(false)` when it ended with an error.
fn watch(r: &dyn Report, mut cmd: Command, label: &str, measure: impl Fn() -> Option<f64>) -> Result<bool, String> {
    let mut child = cmd.spawn().map_err(|e| format!("{label} {e}"))?;
    r.step(label, Some(0.0));
    let mut ticks = 0u32;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status.success());
        }
        if r.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELLED.into());
        }
        std::thread::sleep(Duration::from_millis(250));
        ticks += 1;
        if ticks % 4 == 0 {
            r.step(label, measure().map(|f| f.clamp(0.0, 0.99)));
        }
    }
}

// ── Packages whose name says nothing ────────────────────────────────────────

/// Reads the packages whose name carries no title ID with nx_extract, as the
/// build does (`--contents`: their content metas, which need prod.keys).
fn identify(r: &dyn Report, req: &Request, sources: &mut [Source]) {
    let unnamed: Vec<PathBuf> = sources.iter().filter(|s| s.contents.is_empty()).map(|s| s.path.clone()).collect();
    if unnamed.is_empty() || r.cancelled() {
        return;
    }
    let why = match &req.keys {
        None => Some("imposta prod.keys in Impostazioni e importalo di nuovo".to_string()),
        Some(_) if !req.nx_extract.is_file() => Some("manca nx_extract del motore".to_string()),
        Some(keys) => {
            r.step("Leggo i pacchetti che non hanno il title ID nel nome…", None);
            match read_contents(&req.nx_extract, keys, &unnamed) {
                Ok(found) => {
                    for s in sources.iter_mut() {
                        if let Some(cs) = found.get(&s.path) {
                            s.contents = cs.clone();
                        }
                    }
                    Some("nx_extract non lo legge: file danneggiato?".to_string())
                }
                Err(e) => Some(e),
            }
        }
    };
    for s in sources.iter().filter(|s| s.contents.is_empty()) {
        let why = why.as_deref().unwrap_or_default();
        r.log(format!("«{}»: dal nome non si capisce quale gioco è ({why})", file_name(&s.path)), Tone::Error);
        r.failed();
    }
}

/// `nx_extract <keys> --contents <package>...`: for each package a `package
/// <index> <name>` line, then one `content <type> <title ID> <version>` per
/// content meta (dm/tools/nx_extract/Program.cs). Exit 5: some could not be
/// read; 6: the keys cannot open them.
fn read_contents(nx: &Path, keys: &Path, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<Content>>, String> {
    let out = Command::new(nx)
        .arg(keys)
        .arg("--contents")
        .args(paths)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("nx_extract non parte: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let red = || {
        stdout.lines().chain(String::from_utf8_lossy(&out.stderr).lines()).find_map(|l| l.strip_prefix("[RED] ").map(String::from))
    };
    match out.status.code() {
        Some(0 | 5) => {}
        Some(6) => return Err(format!("prod.keys non apre questi file: {}", red().unwrap_or_default())),
        code => return Err(format!("nx_extract si è fermato (codice {code:?}) {}", red().unwrap_or_default())),
    }
    Ok(parse_contents(&stdout, paths))
}

fn parse_contents(stdout: &str, paths: &[PathBuf]) -> HashMap<PathBuf, Vec<Content>> {
    let mut found: HashMap<PathBuf, Vec<Content>> = HashMap::new();
    let mut current = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("package ") {
            current = rest.split(' ').next().and_then(|i| i.parse::<usize>().ok()).and_then(|i| paths.get(i));
        } else if let (Some(rest), Some(p)) = (line.strip_prefix("content "), current) {
            let cols: Vec<&str> = rest.split(' ').collect();
            if let [kind, tid, version] = cols[..] {
                if !matches!(kind, "Application" | "Patch" | "AddOnContent") {
                    continue; // a delta update, or something else no build takes
                }
                if let (Ok(title_id), Ok(version)) = (u64::from_str_radix(tid, 16), version.parse()) {
                    found.entry(p.clone()).or_default().push(Content { title_id, version });
                }
            }
        }
    }
    found
}

// ── Putting a file in its game's folder ─────────────────────────────────────

/// "gioco base, update v131072, 3 DLC"
fn describe(cs: &[Content]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if cs.iter().any(|c| c.kind() == Kind::Base) {
        parts.push("gioco base".into());
    }
    parts.extend(cs.iter().filter(|c| c.kind() == Kind::Update).map(|c| format!("update v{}", c.version)));
    let dlc: Vec<&Content> = cs.iter().filter(|c| c.kind() == Kind::Dlc).collect();
    match dlc.len() {
        0 => {}
        1 => parts.push(format!("DLC {}", hex(dlc[0].title_id))),
        n => parts.push(format!("{n} DLC")),
    }
    parts.join(", ")
}

/// What a game's folder holds already.
fn held(folder: &Path) -> BTreeSet<Content> {
    archive::scan_game(folder)
        .map(|g| {
            g.pieces
                .iter()
                .filter_map(|p| Some(Content { title_id: u64::from_str_radix(&p.title_id, 16).ok()?, version: p.version }))
                .collect()
        })
        .unwrap_or_default()
}

/// `path`, or `name (2).ext`, `name (3).ext`… when it is taken.
fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.into();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| path.with_file_name(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap()
}

fn place(r: &dyn Report, req: &Request, src: &Source, names: &mut HashMap<u64, String>) -> Result<(), String> {
    let file = file_name(&src.path);
    let apps: BTreeSet<u64> = src.contents.iter().map(Content::app).collect();
    if apps.len() > 1 {
        let ids: Vec<String> = apps.iter().map(|a| hex(*a)).collect();
        return Err(format!("«{file}» contiene più giochi ({}): la build ne vuole uno per cartella", ids.join(", ")));
    }
    let app = *apps.first().ok_or_else(|| format!("«{file}»: vuoto"))?;
    let folder = match archive::folder_for(&req.root, app) {
        Some(f) => f,
        None => {
            let name = names.get(&app).cloned().or_else(|| src.name.clone()).unwrap_or_else(|| hex(app));
            let f = req.root.join(archive::folder_name(&name, app));
            fs::create_dir_all(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            names.insert(app, name);
            r.log(format!("Nuova cartella «{}»", file_name(&f)), Tone::Info);
            f
        }
    };
    let moving = req.moving || src.owned;
    if src.format == Format::Dump {
        return merge_dump(r, &src.path, &folder, app, moving);
    }
    let what = describe(&src.contents);
    let have = held(&folder);
    if src.path.parent() == Some(folder.as_path()) || src.contents.iter().all(|c| have.contains(c)) {
        r.log(format!("«{file}»: {what} già in archivio"), Tone::Info);
        r.placed(app, false);
        return Ok(());
    }
    // A package read by nx_extract gets the tags, so the archive needs no keys to tell it again.
    let name = if src.format == Format::Package && archive::package_contents(&file).is_none() {
        archive::tagged_name(&file, &src.contents)
    } else {
        file.clone()
    };
    transfer(r, &src.path, &unique(&folder.join(name)), moving)?;
    r.log(format!("«{file}» → {}: {what}", file_name(&folder)), Tone::Ok);
    r.placed(app, true);
    Ok(())
}

/// A dump's parts into the game's folder, each where the build looks for it:
/// `base/`, `updates/<version>/`, `dlc/<title ID>/`. `exefs/` and `romfs/`
/// directly in the dump are the base game. A part the folder has is skipped.
fn merge_dump(r: &dyn Report, dir: &Path, folder: &Path, app: u64, moving: bool) -> Result<(), String> {
    let mut parts: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    if dir.join("base").is_dir() {
        parts.push((dir.join("base"), folder.join("base"), "gioco base".into()));
    } else if dir.join("exefs").is_dir() && dir.join("romfs").is_dir() {
        parts.push((dir.join("exefs"), folder.join("base/exefs"), "gioco base (codice)".into()));
        parts.push((dir.join("romfs"), folder.join("base/romfs"), "gioco base (dati)".into()));
    }
    for (v, p) in archive::dump_updates(dir) {
        parts.push((p, folder.join(format!("updates/{v}")), format!("update v{v}")));
    }
    for (id, p) in archive::dump_dlc(dir) {
        let name = file_name(&p);
        parts.push((p, folder.join("dlc").join(name), format!("DLC {}", hex(id))));
    }
    let name = file_name(dir);
    for (from, to, what) in parts {
        if r.cancelled() {
            return Err(CANCELLED.into());
        }
        if to.exists() {
            r.log(format!("«{name}»: {what} già in archivio"), Tone::Info);
            r.placed(app, false);
            continue;
        }
        let parent = to.parent().expect("a part has a parent");
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        transfer(r, &from, &to, moving)?;
        r.log(format!("«{name}» → {}: {what}", file_name(folder)), Tone::Ok);
        r.placed(app, true);
    }
    Ok(())
}

fn same_volume(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

fn remove_any(p: &Path) {
    if p.is_dir() {
        let _ = fs::remove_dir_all(p);
    } else {
        let _ = fs::remove_file(p);
    }
}

/// Moves or copies a file or folder to `to`. The copy goes through a hidden
/// `.part` name, renamed when whole; a move deletes the original only then.
fn transfer(r: &dyn Report, from: &Path, to: &Path, moving: bool) -> Result<(), String> {
    let name = file_name(from);
    if moving {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            // Another volume: copy, then delete.
            Err(e) if e.raw_os_error() == Some(nix::libc::EXDEV) => {}
            Err(e) => return Err(format!("«{name}»: {e}")),
        }
    }
    let parent = to.parent().expect("a destination has a parent");
    let part = parent.join(format!(".{}.part", file_name(to)));
    remove_any(&part);
    let dir = from.is_dir();
    let size = if dir { archive::dir_size(from) } else { fs::metadata(from).map_or(0, |m| m.len()) };
    let same = same_volume(from, parent);
    if !same {
        let free = free_bytes(parent);
        if free < size + GIB {
            return Err(format!(
                "«{name}»: servono {} GB liberi nell'archivio, ce ne sono {}",
                (size + GIB).div_ceil(GIB),
                free / GIB
            ));
        }
    }
    let label = format!("{} «{name}»…", if moving { "Sposto" } else { "Copio" });
    let result = if dir {
        // -c: APFS clones on the same volume, which take no space.
        let mut cmd = Command::new("/bin/cp");
        cmd.arg(if same { "-Rc" } else { "-R" }).arg(from).arg(&part).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let measure = || (size > 0).then(|| archive::dir_size(&part) as f64 / size as f64);
        match watch(r, cmd, &label, measure) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!("«{name}»: copia non riuscita")),
            Err(e) => Err(e),
        }
    } else if same && clone_file(from, &part) {
        Ok(())
    } else {
        copy_file(r, from, &part, &label, size)
    };
    if let Err(e) = result {
        remove_any(&part);
        return Err(e);
    }
    fs::rename(&part, to).map_err(|e| format!("«{name}»: {e}"))?;
    if moving {
        remove_any(from);
    }
    Ok(())
}

/// An APFS clone: instant, and no space until one of the two changes.
fn clone_file(from: &Path, to: &Path) -> bool {
    Command::new("/bin/cp")
        .arg("-c")
        .arg(from)
        .arg(to)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn copy_file(r: &dyn Report, from: &Path, to: &Path, label: &str, size: u64) -> Result<(), String> {
    let name = file_name(from);
    let io = |e: std::io::Error| format!("«{name}»: {e}");
    let mut src = fs::File::open(from).map_err(io)?;
    let mut dst = fs::File::create(to).map_err(io)?;
    let mut buf = vec![0u8; 8 << 20];
    let (mut done, mut last) = (0u64, Instant::now());
    r.step(label, Some(0.0));
    loop {
        if r.cancelled() {
            return Err(CANCELLED.into());
        }
        let n = src.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(io)?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            r.step(label, (size > 0).then(|| (done as f64 / size as f64).min(0.99)));
            last = Instant::now();
        }
    }
    dst.flush().map_err(io)
}

// ── Running it for the UI ───────────────────────────────────────────────────

struct AppReport {
    app: AppHandle,
    cancel: Arc<AtomicBool>,
}

impl AppReport {
    fn with<T>(&self, f: impl FnOnce(&mut ImportRun) -> T) -> Option<T> {
        let state = self.app.state::<AppState>();
        let mut slot = state.import.lock().unwrap();
        slot.as_mut().map(f)
    }

    /// At most four updates a second reach the UI, except the ones forced.
    fn emit(&self, force: bool) {
        let snap = self
            .with(|run| {
                if !force && run.last_emit.elapsed() < Duration::from_millis(250) {
                    return None;
                }
                run.last_emit = Instant::now();
                Some(run.snap.clone())
            })
            .flatten();
        if let Some(snap) = snap {
            let _ = self.app.emit("archive-import", snap);
        }
    }
}

impl Report for AppReport {
    fn step(&self, text: &str, frac: Option<f64>) {
        self.with(|run| {
            run.snap.step = text.into();
            run.snap.frac = frac;
        });
        self.emit(false);
    }

    fn log(&self, text: String, tone: Tone) {
        self.with(|run| {
            if run.snap.log.len() == MAX_LOG {
                run.snap.log.remove(0);
            }
            run.snap.log.push(Line { text, tone });
        });
        self.emit(true);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn placed(&self, app: u64, added: bool) {
        self.with(|run| {
            if !added {
                run.snap.skipped += 1;
                return;
            }
            run.snap.added += 1;
            let id = hex(app);
            if !run.snap.games.contains(&id) {
                run.snap.games.push(id);
            }
        });
    }

    fn failed(&self) {
        self.with(|run| run.snap.failed += 1);
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

pub fn start(app: &AppHandle, req: Request) -> Result<Snapshot, String> {
    let state = app.state::<AppState>();
    let cancel = Arc::new(AtomicBool::new(false));
    let snap = Snapshot {
        running: true,
        cancelled: false,
        moving: req.moving,
        step: "Cerco i file di gioco…".into(),
        frac: None,
        log: Vec::new(),
        games: Vec::new(),
        added: 0,
        skipped: 0,
        failed: 0,
        started_at: now(),
        finished_at: None,
    };
    {
        let mut slot = state.import.lock().unwrap();
        if slot.as_ref().is_some_and(ImportRun::running) {
            return Err("C'è già un'importazione in corso".into());
        }
        *slot = Some(ImportRun { snap: snap.clone(), cancel: cancel.clone(), last_emit: Instant::now() });
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let report = AppReport { app: app.clone(), cancel };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&report, &req)))
            .unwrap_or_else(|_| Err("Errore interno durante l'importazione".into()));
        let cancelled = report.cancelled();
        if let (Err(e), false) = (&result, cancelled) {
            report.log(e.clone(), Tone::Error);
        }
        let summary = report.with(|run| {
            let s = &mut run.snap;
            s.running = false;
            s.cancelled = cancelled;
            s.step.clear();
            s.frac = None;
            s.finished_at = Some(now());
            let counts = format!(
                "{}, {}, {}",
                plural(s.added, "aggiunto", "aggiunti"),
                plural(s.skipped, "già presente", "già presenti"),
                plural(s.failed, "errore", "errori")
            );
            if cancelled { format!("Annullata: {counts}") } else { format!("Fatto: {counts}") }
        });
        if let Some(summary) = summary {
            let tone = if cancelled { Tone::Warn } else { Tone::Info };
            report.log(summary, tone);
        }
        report.emit(true);
        let _ = app.emit("archive-changed", ());
    });
    Ok(snap)
}

pub fn cancel(state: &AppState) -> Result<(), String> {
    let slot = state.import.lock().unwrap();
    let run = slot.as_ref().filter(|r| r.running()).ok_or("Nessuna importazione in corso")?;
    run.cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::tests::{fake_dump, fake_nup};
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Recorder {
        lines: RefCell<Vec<(String, Tone)>>,
        added: Cell<usize>,
        skipped: Cell<usize>,
        failed: Cell<usize>,
    }

    impl Report for Recorder {
        fn step(&self, _: &str, _: Option<f64>) {}
        fn log(&self, text: String, tone: Tone) {
            self.lines.borrow_mut().push((text, tone));
        }
        fn cancelled(&self) -> bool {
            false
        }
        fn placed(&self, _: u64, added: bool) {
            let c = if added { &self.added } else { &self.skipped };
            c.set(c.get() + 1);
        }
        fn failed(&self) {
            self.failed.set(self.failed.get() + 1);
        }
    }

    impl Recorder {
        fn text(&self) -> String {
            self.lines.borrow().iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("\n")
        }
    }

    fn request(root: &Path, paths: Vec<PathBuf>, moving: bool) -> Request {
        Request {
            paths,
            root: root.into(),
            moving,
            keys: None,
            nx_extract: "/nonexistent/nx_extract".into(),
            names: HashMap::new(),
        }
    }

    /// Packs `dir`'s content into an archive with bsdtar (the format from the suffix).
    fn pack(dir: &Path, out: &Path) {
        let ok = Command::new("/usr/bin/tar").arg("-a").arg("-cf").arg(out).arg("-C").arg(dir).arg(".").status().unwrap();
        assert!(ok.success());
    }

    #[test]
    fn imports_packages_archives_and_dumps() {
        let t = tempfile::tempdir().unwrap();
        let (root, inbox) = (t.path().join("Archivio"), t.path().join("Download"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(inbox.join("Wonder")).unwrap();
        fs::write(inbox.join("Wonder/Super Mario Bros. Wonder [010015100B514000][v0].nsp"), vec![7; 1000]).unwrap();
        fs::write(inbox.join("Wonder/notes.txt"), "mine").unwrap();
        // An update in a ZIP, a Switch 2 game's NUP in a 7z, a dump in a ZIP.
        let z = t.path().join("z");
        fs::create_dir_all(&z).unwrap();
        fs::write(z.join("Wonder [010015100B514800][v131072].nsp"), vec![8; 500]).unwrap();
        pack(&z, &inbox.join("update.zip"));
        let n = t.path().join("n");
        fs::create_dir_all(&n).unwrap();
        fake_nup(&n.join("kart.nup"), &[(0, 0x0400C3F00006E000, 0), (1, 0x0400C3F00006E000, 65536)], Some("Kart World"));
        pack(&n, &inbox.join("kart.7z"));
        let d = t.path().join("d/Banana");
        fake_dump(&d, 0x040074A01BF12000, &[458752], &[0x040074A01BF13001]);
        pack(&t.path().join("d"), &inbox.join("banana.zip"));

        let r = Recorder::default();
        run(&r, &request(&root, vec![inbox.clone()], false)).unwrap();
        assert_eq!((r.added.get(), r.failed.get()), (6, 0), "{}", r.text());

        let scan = archive::scan(&root).unwrap();
        let names: Vec<&str> = scan.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Banana", "Kart World", "Super Mario Bros. Wonder"]);
        let wonder = &scan.games[2];
        assert_eq!((wonder.has_base, wonder.update), (true, Some(131072)));
        assert!(wonder.folder.ends_with("Super Mario Bros. Wonder [010015100B514000]"));
        let banana = &scan.games[0];
        assert_eq!((banana.format, banana.update, banana.dlc.len()), (Some(Format::Dump), Some(458752), 1));
        assert!(scan.games.iter().all(|g| g.issues.is_empty()));
        // Copied, not moved (the archives too); the staging folder is gone.
        assert!(inbox.join("Wonder/Super Mario Bros. Wonder [010015100B514000][v0].nsp").exists());
        assert!(inbox.join("update.zip").exists() && inbox.join("kart.7z").exists());
        assert!(fs::read_dir(&root).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".nxl")));

        // Again: everything is there already.
        let r = Recorder::default();
        run(&r, &request(&root, vec![inbox.clone()], false)).unwrap();
        assert_eq!((r.added.get(), r.skipped.get(), r.failed.get()), (0, 6, 0), "{}", r.text());
    }

    #[test]
    fn moves_and_tidies_loose_files() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Archivio");
        fs::create_dir_all(&root).unwrap();
        let loose = root.join("Odyssey [0100000000010800][v393216].nsp");
        fs::write(&loose, vec![1; 10]).unwrap();
        let elsewhere = t.path().join("Odyssey [0100000000010000][v0].nsp");
        fs::write(&elsewhere, vec![1; 20]).unwrap();
        let r = Recorder::default();
        // Loose files are moved even when copying: the archive already owns them.
        run(&r, &request(&root, vec![loose.clone(), elsewhere.clone()], false)).unwrap();
        assert_eq!(r.added.get(), 2, "{}", r.text());
        assert!(!loose.exists() && elsewhere.exists());
        let r = Recorder::default();
        let other = t.path().join("Odyssey DLC [0100000000011001][v0].nsp");
        fs::write(&other, vec![1; 5]).unwrap();
        run(&r, &request(&root, vec![other.clone()], true)).unwrap();
        assert!(!other.exists());
        let game = &archive::scan(&root).unwrap().games[0];
        assert_eq!((game.name.as_str(), game.dlc.len()), ("Odyssey", 1));
    }

    #[test]
    fn an_emptied_archive_goes_only_when_moving_or_loose() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Archivio");
        fs::create_dir_all(&root).unwrap();
        let zip = |name: &str, file: &str, at: &Path| {
            let d = t.path().join(format!("src-{name}"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join(file), "x").unwrap();
            pack(&d, &at.join(name));
            at.join(name)
        };
        let good = zip("good.zip", "Odyssey [0100000000010000][v0].nsp", t.path());
        let mixed = zip("mixed.zip", "mystery.nsp", t.path());
        let loose = zip("loose.zip", "Odyssey [0100000000010800][v65536].nsp", &root);
        let r = Recorder::default();
        run(&r, &request(&root, vec![good.clone(), mixed.clone(), loose.clone()], true)).unwrap();
        let text = r.text();
        // All its files imported: gone. One file failed: kept, to try again.
        assert!(!good.exists() && mixed.exists() && !loose.exists(), "{text}");
        assert!(text.contains("«good.zip»: tutto importato"), "{text}");

        // Copying, a zip outside the archive stays where it is.
        let kept = zip("kept.zip", "Odyssey DLC [0100000000011001][v0].nsp", t.path());
        run(&Recorder::default(), &request(&root, vec![kept.clone()], false)).unwrap();
        assert!(kept.exists());
        assert_eq!(archive::scan(&root).unwrap().games[0].dlc.len(), 1);
    }

    #[test]
    fn says_what_it_cannot_take() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Archivio");
        fs::create_dir_all(&root).unwrap();
        let mystery = t.path().join("mystery.nsp");
        fs::write(&mystery, "").unwrap();
        let split = t.path().join("game.part1.rar");
        fs::write(&split, "").unwrap();
        let broken = t.path().join("broken.zip");
        fs::write(&broken, "not a zip").unwrap();
        let note = t.path().join("note.pdf");
        fs::write(&note, "").unwrap();
        let r = Recorder::default();
        run(&r, &request(&root, vec![mystery, split, broken, note], false)).unwrap();
        let text = r.text();
        assert_eq!(r.failed.get(), 3, "{text}");
        assert!(text.contains("prod.keys") && text.contains("più parti") && text.contains("broken.zip"), "{text}");
        assert!(text.contains("non è un file di gioco"), "{text}");
        assert!(archive::scan(&root).unwrap().games.is_empty());
    }

    #[test]
    fn split_archives() {
        let t = tempfile::tempdir().unwrap();
        let p = |n: &str| t.path().join(n);
        assert_eq!(volume("g.part1.rar", &p("g.part1.rar")), Some(1));
        assert_eq!(volume("g.part02.rar", &p("g.part02.rar")), Some(2));
        assert_eq!(volume("Counterpart.rar", &p("Counterpart.rar")), None);
        assert_eq!(volume("g.7z.001", &p("g.7z.001")), Some(1));
        assert_eq!(volume("g.r00", &p("g.r00")), Some(2));
        fs::write(p("old.r00"), "").unwrap();
        assert_eq!(volume("old.rar", &p("old.rar")), Some(1));
        assert_eq!(volume("game.zip", &p("game.zip")), None);
    }

    #[test]
    fn reads_nx_extract_contents() {
        let paths = [PathBuf::from("/a.nsp"), PathBuf::from("/b.xci")];
        let out = "package 0 a.nsp\ncontent Patch 010015100b514800 327680\n\
                   package 1 b.xci\ncontent Application 0100000000010000 0\ncontent AddOnContent 0100000000011001 0\ncontent Delta 0100000000010800 1\n";
        let found = parse_contents(out, &paths);
        assert_eq!(found[&paths[0]], [Content { title_id: 0x010015100B514800, version: 327680 }]);
        assert_eq!(found[&paths[1]].len(), 2);
        assert_eq!(describe(&found[&paths[1]]), "gioco base, DLC 0100000000011001");
    }
}
