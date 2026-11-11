//! The launcher's own settings, in `config.json` beside `library.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Where the player's prod.keys is. Only the path is kept: the keys are
    /// never copied, read or sent anywhere by the launcher; the build script
    /// reads the file itself (`-k`).
    #[serde(default)]
    pub prod_keys: Option<PathBuf>,
    /// The folder every game's save syncs through (docs/frontends.md, "The
    /// cloud saves folder"): written as `cloudSaves.folder` into each game's
    /// preferences, so a start from Finder uses it too.
    #[serde(default)]
    pub cloud_saves_folder: Option<PathBuf>,
    /// Pass the Home quit prompt and the players prompt in Italian
    /// (`DM_HOME_QUIT_TEXT`, `DM_CTRL_PROMPT_TEXT`) when starting a game.
    #[serde(default = "yes")]
    pub italian_prompts: bool,
    /// The engine commit before the last update, for "Torna alla precedente".
    #[serde(default)]
    pub previous_engine: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self { prod_keys: None, cloud_saves_folder: None, italian_prompts: true, previous_engine: None }
    }
}

pub struct ConfigStore {
    file: PathBuf,
    pub config: Config,
}

impl ConfigStore {
    pub fn load(data_dir: &Path) -> Self {
        let file = data_dir.join("config.json");
        let config = fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { file, config }
    }

    pub fn save(&self) -> io::Result<()> {
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.config)?)?;
        fs::rename(&tmp, &self.file)
    }
}

/// The host's prompts in Italian, one text per line in the order
/// docs/frontends.md gives ("Starting a game"); `{min}`, `{max}` and
/// `{joined}` are filled in by the host.
pub fn italian_prompts(game_name: &str) -> [(String, String); 2] {
    [
        (
            "DM_HOME_QUIT_TEXT".into(),
            [
                "Tieni premuto per uscire",
                &format!("Uscire da {game_name}?"),
                "I progressi non salvati andranno persi.",
                "Esci",
                "Annulla",
            ]
            .join("\n"),
        ),
        (
            "DM_CTRL_PROMPT_TEXT".into(),
            [
                "Servono {min} giocatori",
                "Premi un pulsante su ogni controller per unirti.",
                "Giocatori: {joined} di {min}",
                "Fatto",
                "Annulla",
            ]
            .join("\n"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_config_gets_the_new_defaults() {
        let c: Config = serde_json::from_str(r#"{"prodKeys": null}"#).unwrap();
        assert!(c.italian_prompts && c.cloud_saves_folder.is_none());
    }

    #[test]
    fn prompts_have_five_lines_each() {
        for (_, text) in italian_prompts("Kart") {
            assert_eq!(text.lines().count(), 5);
        }
        assert!(italian_prompts("Kart")[0].1.contains("Uscire da Kart?"));
    }
}
