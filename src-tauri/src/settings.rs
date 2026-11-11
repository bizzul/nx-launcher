//! A game's settings (docs/frontends.md, "The settings list" and "Saved
//! choices"): the list the app carries in `Contents/Resources/dm/launcher.json`,
//! the player's saved choices in the app's preferences, and what an `env.txt`
//! fixes. The rules follow `dm/mac/launcher.c`, which reads the same data.

use crate::library::Game;
use crate::prefs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The schema this launcher draws. Schema 1 only grows by optional fields; a
/// new `kind` would be schema 2, which a reader of 1 cannot draw.
const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub env: String,
    #[serde(flatten)]
    pub kind: Kind,
    pub section: String,
    pub label: String,
    #[serde(default)]
    pub note: String,
    pub default: String,
    /// "basic": shown first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// "<VAR>=<value>": drawn only while the row that sets <VAR> has that value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
    /// Render scale only: the 3D views' size docked and handheld.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Base>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Kind {
    Choice { options: Vec<(String, String)> },
    Range { min: f64, max: f64, step: f64 },
    Toggle { on: String, off: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Base {
    pub docked: (u32, u32),
    pub handheld: (u32, u32),
}

impl Setting {
    /// A value this build offers, as the game's launcher checks a saved
    /// choice (`saved_accepted`): one of its values, or a number in its range
    /// written plainly (no leading space, no hex, not NaN).
    pub fn accepts(&self, v: &str) -> bool {
        match &self.kind {
            Kind::Choice { options } => options.iter().any(|(value, _)| value == v),
            Kind::Toggle { on, off } => v == on || v == off,
            Kind::Range { min, max, .. } => {
                !v.is_empty()
                    && !v.starts_with(char::is_whitespace)
                    && !v.contains(['x', 'X'])
                    && v.parse::<f64>().is_ok_and(|x| x >= *min && x <= *max)
            }
        }
    }

    /// Whether `v` is the tested default; for a range, "1" is "1.0".
    pub fn is_default(&self, v: &str) -> bool {
        match (&self.kind, v.parse::<f64>(), self.default.parse::<f64>()) {
            (Kind::Range { step, .. }, Ok(a), Ok(b)) => (a - b).abs() < step / 2.0,
            _ => v == self.default,
        }
    }
}

/// A setting an env file sets: the game's launcher uses that value whatever
/// the saved choice, so it is shown but cannot be changed here.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixed {
    pub value: String,
    /// "player": `~/Library/Application Support/nxnative/<title ID>/env.txt`;
    /// "app": the bundle's `dm/env.txt`, baked in at build time.
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    /// The list's schema; 0 when the app carries no list (start it as it is).
    pub schema: u32,
    /// Whether this launcher can draw the list.
    pub supported: bool,
    pub settings: Vec<Setting>,
    /// The saved choices the game will use (only those this build offers).
    pub saved: BTreeMap<String, String>,
    pub fixed: BTreeMap<String, Fixed>,
}

#[derive(Deserialize)]
struct LauncherJson {
    schema: Option<u32>,
    settings: Vec<serde_json::Value>,
}

fn pref_key(env: &str) -> String {
    format!("launcher.{env}")
}

/// Reads the list. A row this launcher cannot read is left out, not the list.
pub fn read_list(app: &Path) -> Result<Option<(u32, Vec<Setting>)>, String> {
    let file = app.join("Contents/Resources/dm/launcher.json");
    let bytes = match fs::read(&file) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("launcher.json illeggibile: {e}")),
    };
    let list: LauncherJson =
        serde_json::from_slice(&bytes).map_err(|e| format!("launcher.json non valido: {e}"))?;
    let rows = list
        .settings
        .into_iter()
        .filter_map(|row| match serde_json::from_value::<Setting>(row) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("launcher.json: row skipped ({e})");
                None
            }
        })
        .collect();
    // A list written before the field existed has none and is schema 1.
    Ok(Some((list.schema.unwrap_or(1), rows)))
}

/// `KEY=VALUE` lines, `#` comments, as the launcher's `env_file` reads them.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.trim_matches([' ', '\t', '\r']))
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.trim_end().is_empty())
        .map(|(k, v)| (k.trim_end_matches([' ', '\t']).to_string(), v.to_string()))
        .collect()
}

/// What the env files fix, in the launcher's order: the first that sets a
/// variable wins, the player's file before the app's own.
fn fixed_by_env_files(game: &Game, home: &Path) -> BTreeMap<String, Fixed> {
    let player = home.join(format!("Library/Application Support/nxnative/{}/env.txt", game.title_id));
    let app = game.app_path.join("Contents/Resources/dm/env.txt");
    let mut fixed = BTreeMap::new();
    for (path, source) in [(player, "player"), (app, "app")] {
        let Ok(text) = fs::read_to_string(path) else { continue };
        for (key, value) in parse_env_file(&text) {
            fixed.entry(key).or_insert(Fixed { value, source });
        }
    }
    fixed
}

pub fn load(game: &Game, home: &Path) -> Result<GameSettings, String> {
    let Some((schema, settings)) = read_list(&game.app_path)? else {
        return Ok(GameSettings {
            schema: 0,
            supported: true,
            settings: vec![],
            saved: BTreeMap::new(),
            fixed: BTreeMap::new(),
        });
    };
    let saved = settings
        .iter()
        .filter_map(|s| {
            let v = prefs::get_string(&game.bundle_id, &pref_key(&s.env))?;
            s.accepts(&v).then(|| (s.env.clone(), v))
        })
        .collect();
    let mut fixed = fixed_by_env_files(game, home);
    fixed.retain(|env, _| settings.iter().any(|s| &s.env == env));
    Ok(GameSettings { supported: schema == SCHEMA, schema, settings, saved, fixed })
}

/// Saves the player's choices: a value at its tested default removes the key,
/// so a later build's defaults reach the player. Settings not in `choices`,
/// and those an env file fixes, are left as they are.
pub fn save(game: &Game, home: &Path, choices: &BTreeMap<String, String>) -> Result<GameSettings, String> {
    let current = load(game, home)?;
    if !current.supported {
        return Err(format!("Formato delle impostazioni non supportato (schema {})", current.schema));
    }
    for s in &current.settings {
        let Some(v) = choices.get(&s.env) else { continue };
        if current.fixed.contains_key(&s.env) {
            continue;
        }
        if !s.accepts(v) {
            return Err(format!("«{}»: valore non valido ({v})", s.label));
        }
        let value = (!s.is_default(v)).then_some(v.as_str());
        prefs::set_string(&game.bundle_id, &pref_key(&s.env), value);
    }
    if !prefs::sync(&game.bundle_id) {
        return Err("macOS non ha salvato le preferenze del gioco".into());
    }
    load(game, home)
}

/// Overrides for one start, checked against the list: only the game's own
/// settings, with values this build offers. Nothing else reaches the
/// environment, so the web view cannot set, say, DYLD_INSERT_LIBRARIES.
pub fn check_overrides(
    game: &Game,
    overrides: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, String> {
    if overrides.is_empty() {
        return Ok(vec![]);
    }
    let Some((_, settings)) = read_list(&game.app_path)? else {
        return Err("Questo gioco non ha impostazioni".into());
    };
    overrides
        .iter()
        .map(|(env, v)| match settings.iter().find(|s| &s.env == env) {
            Some(s) if s.accepts(v) => Ok((env.clone(), v.clone())),
            Some(s) => Err(format!("«{}»: valore non valido ({v})", s.label)),
            None => Err(format!("{env} non è un'impostazione di questo gioco")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(json: &str) -> Setting {
        serde_json::from_str(json).unwrap()
    }

    fn fps() -> Setting {
        row(r#"{"env":"DM_FPS_TARGET","kind":"choice","section":"Game","label":"Frame rate","note":"",
               "default":"60","options":[["30","30 fps"],["60","60 fps"]],"level":"basic"}"#)
    }

    fn gain() -> Setting {
        row(r#"{"env":"DM_RUMBLE_GAIN","kind":"range","section":"Controls","label":"Rumble strength","note":"",
               "default":"1.0","min":0.0,"max":3.0,"step":0.1,"needs":"DM_RUMBLE=1"}"#)
    }

    #[test]
    fn reads_the_three_kinds() {
        assert!(matches!(fps().kind, Kind::Choice { ref options } if options.len() == 2));
        assert!(matches!(gain().kind, Kind::Range { max, .. } if max == 3.0));
        let hold = row(r#"{"env":"DM_FPS_HOLD","kind":"toggle","section":"Performance","label":"Hold",
                          "note":"","default":"on","on":"on","off":"off","future_field":42}"#);
        assert!(matches!(hold.kind, Kind::Toggle { ref on, .. } if on == "on"));
    }

    #[test]
    fn accepts_what_the_launcher_accepts() {
        assert!(fps().accepts("30") && !fps().accepts("45"));
        let g = gain();
        assert!(g.accepts("1.5") && g.accepts("0") && g.accepts("3"));
        for bad in ["", " 1", "0x1", "nan", "3.5", "-0.1", "abc"] {
            assert!(!g.accepts(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_range_default_matches_however_it_is_written() {
        assert!(gain().is_default("1") && gain().is_default("1.00"));
        assert!(!gain().is_default("1.1"));
        assert!(fps().is_default("60") && !fps().is_default("30"));
    }

    #[test]
    fn env_files_as_the_launcher_reads_them() {
        let text = "# comment\n  DM_FPS_TARGET = 30\r\nDM_MOTION=0\n=nokey\nnoequals\n\tDM_X=a=b \n";
        assert_eq!(
            parse_env_file(text),
            [("DM_FPS_TARGET", " 30"), ("DM_MOTION", "0"), ("DM_X", "a=b")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
        );
    }
}
