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
        write_atomic(&file(), &s);
    }
}

/// Écrit `path` en passant par un fichier temporaire + renommage (atomique sur le même volume NTFS) :
/// une coupure pendant l'écriture laisse l'ancien contenu intact au lieu d'un fichier tronqué/corrompu
/// que le prochain lancement ne saurait plus relire (perte silencieuse de tous les réglages).
pub fn write_atomic(path: &std::path::Path, data: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}
