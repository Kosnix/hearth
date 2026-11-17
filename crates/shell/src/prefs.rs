//! Personnalisation des jeux (nom modifié, jeu caché) : `%LOCALAPPDATA%\Hearth\overrides.json`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Override {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

fn file() -> PathBuf {
    stores::profile_dir().join("overrides.json")
}

pub fn load() -> HashMap<String, Override> {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_default()
}

pub fn save(map: &HashMap<String, Override>) {
    if let Ok(s) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(file(), s);
    }
}
