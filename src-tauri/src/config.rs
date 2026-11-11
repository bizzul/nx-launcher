//! The launcher's own settings, in `config.json` beside `library.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Where the player's prod.keys is. Only the path is kept: the keys are
    /// never copied, read or sent anywhere by the launcher; the build script
    /// reads the file itself (`-k`).
    #[serde(default)]
    pub prod_keys: Option<PathBuf>,
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
