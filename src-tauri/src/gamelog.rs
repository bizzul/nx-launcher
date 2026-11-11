//! What a running game is doing, read from its log as it grows:
//! ~/Library/Logs/nxnative/<title ID>.log, which the app's launcher starts
//! anew at each start (dm/mac/launcher.c renames the last one to .prev.log).
//!
//! The lines it looks for, from the engine's sources:
//! - `>>> nnMain`: the game's code runs (dm/host);
//! - `pipeline prewarm: this device's user list does not exist yet`: the first
//!   start on this Mac, which prepares every pipeline (nvn_mtl.m warm_start);
//! - `pipeline prepare: N of M lines done after …, the progress screen shows`
//!   and `pipeline prepare: M lines (…) in …`: the shader preparation behind
//!   the progress screen, before the first frame (warm_prepare);
//! - `pipeline prebuild: … (prepare step, before the first frame)` and
//!   `pipeline prebuild done:` / `pipeline prepare: every shader pair handed
//!   out`: the pipelines prepared next, behind the same screen (prepare_prebuild);
//! - `present: picture shape`: the first frame shown (written at the first
//!   present, then only when the picture's shape changes);
//! - `front end: window covered` / `front end: window visible` (front_mac.m);
//! - `[RED] shader <key> (<stage>) failed to load:` and `[RED] failed to load
//!   shader <key>_b:`: a shader the app does not carry, the same two forms as
//!   SHADER_MISSING in scripts/build_ipa_from_nsp.py, whose `--seen-shaders`
//!   build adds those keys.

use serde::Serialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// The app's launcher runs; the game's code has not started yet.
    #[default]
    Starting,
    /// The game's code runs, nothing on screen yet.
    Loading,
    /// The progress screen: shaders and pipelines prepared before the first frame.
    Preparing,
    /// A frame has been shown.
    Playing,
}

/// What the UI shows about a running game.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub phase: Phase,
    /// No pipeline list of this Mac yet: the first start, the long one.
    pub first_launch: bool,
    /// When the preparation started (Unix seconds, as this launcher saw it).
    pub prep_since: Option<u64>,
    /// Another window covers the game's: macOS slows it down meanwhile.
    pub window_covered: bool,
    /// Distinct shaders the game asked for that the app does not carry.
    pub missing_shaders: usize,
}

/// The log is no API: its wording can change with the engine. So a phase with
/// no sure end gives way to Playing after a while, rather than a clock that
/// runs on while the game plays. Starting took about 2.5 minutes on a first
/// start of Ocarina of Time. Preparing has its own end lines and no limit: a
/// first start can prepare for more than 10 minutes.
const STARTING_MAX: u64 = 300;
const LOADING_MAX: u64 = 60;

/// The status and what it is worked out from.
#[derive(Default)]
struct Fold {
    status: GameStatus,
    missing: HashSet<String>,
    shown: bool,
    /// When the current phase began (Unix seconds).
    phase_since: Option<u64>,
}

impl Fold {
    fn line(&mut self, line: &str, now: u64) {
        let before = self.status.phase;
        self.apply(line, now);
        if self.status.phase != before {
            self.phase_since = Some(now);
        }
    }

    /// Time passes with or without new lines (see STARTING_MAX).
    fn tick(&mut self, now: u64) {
        let since = *self.phase_since.get_or_insert(now);
        let limit = match self.status.phase {
            Phase::Starting => STARTING_MAX,
            Phase::Loading => LOADING_MAX,
            Phase::Preparing | Phase::Playing => return,
        };
        if now.saturating_sub(since) > limit {
            self.status.phase = Phase::Playing;
            self.phase_since = Some(now);
        }
    }

    fn apply(&mut self, line: &str, now: u64) {
        let s = &mut self.status;
        // The host's own lines start with "[dm]": the game's code is loading.
        if s.phase == Phase::Starting && (line.contains(">>> nnMain") || line.starts_with("[dm] ")) {
            s.phase = Phase::Loading;
        }
        if line.contains("pipeline prewarm: this device's user list does not exist yet") {
            s.first_launch = true;
        } else if (line.contains("pipeline prepare:") && line.contains("lines done after"))
            || (line.contains("pipeline prebuild:") && line.contains("(prepare step, before the first frame)"))
        {
            if !self.shown {
                s.phase = Phase::Preparing;
                s.prep_since.get_or_insert(now);
            }
        } else if (line.contains("pipeline prepare:") && line.contains(" lines ("))
            || line.contains("pipeline prepare: every shader pair handed out")
            || line.contains("pipeline prebuild done:")
        {
            // The pipelines can follow the shaders: back to Preparing if they do.
            if s.phase == Phase::Preparing {
                s.phase = Phase::Loading;
                s.prep_since = None;
            }
        } else if line.contains("present: picture shape") || line.contains("[longframe] frame") {
            self.shown = true;
            s.phase = Phase::Playing;
            s.prep_since = None;
        } else if line.contains("front end: window covered") {
            s.window_covered = true;
        } else if line.contains("front end: window visible") {
            s.window_covered = false;
        } else if let Some(key) = missing_shader(line)
            && self.missing.insert(key.to_owned())
        {
            s.missing_shaders = self.missing.len();
        }
    }
}

/// The key of a shader the app lacks, when the line reports one.
fn missing_shader(line: &str) -> Option<&str> {
    let rest = &line[line.find("[RED] ")? + 6..];
    let key = if let Some(r) = rest.strip_prefix("failed to load shader ") {
        // [RED] failed to load shader <key>_b:
        r.get(16..)?.starts_with("_b:").then(|| &r[..16])?
    } else {
        // [RED] shader <key> (<stage>) failed to load:
        let r = rest.strip_prefix("shader ")?;
        let stage = r.get(16..)?.strip_prefix(" (")?;
        stage[stage.find(')')?..].starts_with(") failed to load:").then(|| &r[..16])?
    };
    key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')).then_some(key)
}

/// Follows a game's log from the start of one session, a little at a time.
pub struct LogWatch {
    path: PathBuf,
    /// The log must have been made after this: until the app's launcher
    /// starts its new one, the file there is the last session's.
    not_before: SystemTime,
    /// The file being read (its inode) and how far.
    file: Option<(u64, u64)>,
    /// The end of a line not written in full yet.
    partial: Vec<u8>,
    fold: Fold,
}

/// At most this much is read per call; the rest waits for the next one.
const CHUNK: u64 = 4 << 20;

impl LogWatch {
    pub fn new(path: PathBuf, not_before: SystemTime) -> Self {
        LogWatch { path, not_before, file: None, partial: Vec::new(), fold: Fold::default() }
    }

    pub fn status(&self) -> &GameStatus {
        &self.fold.status
    }

    /// Reads what the game wrote since the last call. `now`: Unix seconds.
    pub fn poll(&mut self, now: u64) -> io::Result<&GameStatus> {
        self.read(now)?;
        self.fold.tick(now);
        Ok(self.status())
    }

    fn read(&mut self, now: u64) -> io::Result<()> {
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let made = meta.created().or_else(|_| meta.modified())?;
        if made < self.not_before {
            return Ok(());
        }
        let ino = meta.ino();
        let offset = match self.file {
            Some((i, off)) if i == ino && off <= meta.len() => off,
            // A new file (or one cut short): a new run, read from its start.
            _ => {
                self.partial.clear();
                self.fold = Fold::default();
                0
            }
        };
        let mut f = File::open(&self.path)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::new();
        let read = f.take(CHUNK).read_to_end(&mut buf)? as u64;
        self.file = Some((ino, offset + read));

        self.partial.extend_from_slice(&buf);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return Ok(());
        };
        let complete: Vec<u8> = self.partial.drain(..=end).collect();
        for line in complete.split(|&b| b == b'\n') {
            self.fold.line(&String::from_utf8_lossy(line), now);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    // Lines from a real first start (The Legend of Zelda: Ocarina of Time, 0400486024DEC000).
    const NNMAIN: &str = "[dm] >>> nnMain";
    const FIRST: &str = "[dm] [nvn-mtl] pipeline prewarm: this device's user list does not exist yet: /Users/x/Library/Application Support/nxnative/0400486024DEC000/pso_user.tsv";
    const PREP: &str = "[dm] [nvn-mtl] pipeline prepare: 783 of 14748 lines done after 0.5 s, the progress screen shows";
    const PREP_DONE: &str = "[dm] [nvn-mtl] pipeline prepare: 14748 lines (9546 shaders, 2418 render pipelines, 287 compute pipelines) in 110.02 s on 10 workers before the first frame, progress screen shown; bases 1880 compiled, 255 from the archive, 272 reused; 2407 specializations";
    const PREBUILD: &str = "[dm] [nvn-mtl] pipeline prebuild: 412 shader pairs of /x/shader_pairs.tsv, 10 workers (prepare step, before the first frame)";
    const PREBUILD_BG: &str = "[dm] [nvn-mtl] pipeline prebuild: 412 shader pairs of /x/shader_pairs.tsv, 10 workers (background)";
    const PREBUILD_DONE: &str = "[dm] [nvn-mtl] pipeline prebuild done: 412 pairs, 400 bases built, 12 already archived, 0 left to the draws, 0 failed, 31.2 s";
    const SHOWN: &str = "[dm] [nvn-mtl] present: picture shape 1920x1080 (1.778:1) for the front end's layout";
    const COVERED: &str = "[dm] front end: window covered (its display link fires about once a second: frame times measure nothing)";
    const VISIBLE: &str = "[dm] front end: window visible";
    const MISSING_B: &str = "[dm] [nvn-mtl] [RED] failed to load shader f588d206e9bcae62_b: cannot open /x/msl/b-5ee7c6407a57/f588d206e9bcae62_b.metallib";
    const MISSING_STAGE: &str = "[dm] [nvn-mtl] [RED] shader 607bd409ec4ee6e4 (pixel) failed to load: build the shaders with dm/gfx/tools/dmsh_build.py first";

    fn fold(lines: &[(&str, u64)]) -> GameStatus {
        let mut f = Fold::default();
        for (l, t) in lines {
            f.line(l, *t);
        }
        f.status
    }

    #[test]
    fn a_first_start_goes_through_every_phase() {
        let mut f = Fold::default();
        assert_eq!(f.status.phase, Phase::Starting);
        f.line("launcher: The Legend of Zelda: Ocarina of Time (0400486024DEC000), host /x/dmhost", 1);
        assert_eq!(f.status.phase, Phase::Starting);
        f.line(NNMAIN, 2);
        assert_eq!(f.status.phase, Phase::Loading);
        f.line(FIRST, 3);
        f.line(PREP, 4);
        assert_eq!((f.status.phase, f.status.prep_since, f.status.first_launch), (Phase::Preparing, Some(4), true));
        f.line(PREP_DONE, 114);
        assert_eq!((f.status.phase, f.status.prep_since), (Phase::Loading, None));
        f.line(SHOWN, 115);
        assert_eq!(f.status.phase, Phase::Playing);
    }

    #[test]
    fn the_pipelines_after_the_shaders_keep_the_progress_screen() {
        let s = fold(&[(NNMAIN, 1), (PREP, 2), (PREP_DONE, 50), (PREBUILD, 50)]);
        assert_eq!((s.phase, s.prep_since), (Phase::Preparing, Some(50)));
        let s = fold(&[(NNMAIN, 1), (PREBUILD, 2), (PREBUILD_DONE, 40)]);
        assert_eq!(s.phase, Phase::Loading);
        // A prebuild in the background while playing is no progress screen.
        let s = fold(&[(NNMAIN, 1), (SHOWN, 2), (PREBUILD_BG, 3), (PREBUILD, 4)]);
        assert_eq!((s.phase, s.prep_since), (Phase::Playing, None));
    }

    #[test]
    fn any_host_line_means_the_code_is_loading() {
        let s = fold(&[("[dm] build: commit 8fb5ffeb1f09, shader chain 5ee7c6407a57, profile 0400486024DEC000", 1)]);
        assert_eq!(s.phase, Phase::Loading);
    }

    #[test]
    fn a_phase_with_no_sure_end_gives_way_to_playing() {
        let mut f = Fold::default();
        f.tick(100);
        f.tick(100 + STARTING_MAX);
        assert_eq!(f.status.phase, Phase::Starting);
        f.tick(101 + STARTING_MAX);
        assert_eq!(f.status.phase, Phase::Playing);

        let mut f = Fold::default();
        f.tick(5);
        f.line(NNMAIN, 10);   // the clock starts again with the new phase
        f.tick(10 + LOADING_MAX);
        assert_eq!(f.status.phase, Phase::Loading);
        f.tick(11 + LOADING_MAX);
        assert_eq!(f.status.phase, Phase::Playing);

        // The preparation has its own end lines, and no limit.
        let mut f = Fold::default();
        f.line(PREP, 10);
        f.tick(10 + 3600);
        assert_eq!(f.status.phase, Phase::Preparing);
    }

    #[test]
    fn a_later_start_is_not_the_first() {
        let s = fold(&[(NNMAIN, 1), (PREP, 2)]);
        assert!(!s.first_launch);
    }

    #[test]
    fn the_window_state_follows_the_last_line() {
        assert!(fold(&[(VISIBLE, 1), (COVERED, 2)]).window_covered);
        assert!(!fold(&[(COVERED, 1), (VISIBLE, 2)]).window_covered);
    }

    #[test]
    fn missing_shaders_are_counted_once_each() {
        let s = fold(&[(MISSING_B, 1), (MISSING_B, 2), (MISSING_STAGE, 3)]);
        assert_eq!(s.missing_shaders, 2);
        assert_eq!(missing_shader(MISSING_B), Some("f588d206e9bcae62"));
        assert_eq!(missing_shader(MISSING_STAGE), Some("607bd409ec4ee6e4"));
        // Other red lines, and lookalikes, are not missing shaders.
        for l in [
            "[dm] [RED] failed to load the readable-tile version 0123456789abcdef_b",
            "[dm] failed to load shader f588d206e9bcae62_b: cannot open (no [RED])",
            "[dm] [nvn-mtl] [RED] failed to load shader F588D206E9BCAE62_b: cannot open",
            "[dm] [nvn-mtl] [RED] failed to load shader f588d206_b: cannot open",
            "[dm] [nvn-mtl] [RED] shader 607bd409ec4ee6e4 (pixel) compiled late",
        ] {
            assert_eq!(missing_shader(l), None, "{l}");
        }
    }

    #[test]
    fn the_watch_reads_only_this_session_and_only_whole_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("0400486024DEC000.log");
        std::fs::write(&path, format!("{NNMAIN}\n{SHOWN}\n")).unwrap();   // the last session's
        std::thread::sleep(Duration::from_millis(20));
        let mut w = LogWatch::new(path.clone(), SystemTime::now());
        assert_eq!(w.poll(1).unwrap().phase, Phase::Starting);

        // The app's launcher: the last log becomes .prev.log, a new one starts.
        std::fs::rename(&path, dir.path().join("0400486024DEC000.prev.log")).unwrap();
        let mut f = File::create(&path).unwrap();
        write!(f, "{NNMAIN}\n{FIRST}\n{PREP}").unwrap();   // the last line not finished yet
        f.flush().unwrap();
        let s = w.poll(5).unwrap().clone();
        assert_eq!((s.phase, s.first_launch), (Phase::Loading, true));

        writeln!(f).unwrap();
        writeln!(f, "{MISSING_B}").unwrap();
        f.flush().unwrap();
        let s = w.poll(6).unwrap().clone();
        assert_eq!((s.phase, s.prep_since, s.missing_shaders), (Phase::Preparing, Some(6), 1));

        writeln!(f, "{PREP_DONE}\n{SHOWN}\n{COVERED}").unwrap();
        f.flush().unwrap();
        let s = w.poll(120).unwrap().clone();
        assert_eq!((s.phase, s.window_covered, s.missing_shaders), (Phase::Playing, true, 1));
        assert_eq!(w.poll(121).unwrap().phase, Phase::Playing);   // nothing new: unchanged
    }

    /// A real log, read whole: NXL_TEST_LOG=<path> cargo test real_log -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_log() {
        let path = PathBuf::from(std::env::var("NXL_TEST_LOG").expect("NXL_TEST_LOG"));
        let mut w = LogWatch::new(path, SystemTime::UNIX_EPOCH);
        while w.file.is_none_or(|(_, off)| off < std::fs::metadata(&w.path).unwrap().len()) {
            w.poll(0).unwrap();
        }
        println!("{:?}", w.status());
    }

    #[test]
    fn no_log_yet_is_starting() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = LogWatch::new(dir.path().join("none.log"), SystemTime::now());
        assert_eq!(*w.poll(1).unwrap(), GameStatus::default());
    }
}
