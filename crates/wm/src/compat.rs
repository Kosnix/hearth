//! Réglages par jeu : base embarquée (compat.json), indexée par nom d'exécutable.

use std::collections::HashMap;
use std::path::Path;

#[derive(serde::Deserialize)]
#[serde(default)]
pub struct Compat {
    /// Arguments ajoutés au lancement (ex. `-windowed`).
    pub args: Vec<String>,
    /// Forcer le plein écran fenêtré sans bordure.
    pub borderless: bool,
    /// Anti-triche : aucune modification de fenêtre ni overlay.
    pub anticheat: bool,
    /// Modifications de fichiers de config `clé=valeur` avant lancement.
    pub ini: Vec<IniEdit>,
}

impl Default for Compat {
    fn default() -> Self {
        Compat { args: vec![], borderless: true, anticheat: false, ini: vec![] }
    }
}

#[derive(serde::Deserialize)]
pub struct IniEdit {
    /// Chemin, variables d'environnement `%VAR%` autorisées.
    pub file: String,
    pub key: String,
    pub value: String,
}

/// Dossiers posés par les anti-triches courants à côté de l'exécutable.
const ANTICHEAT_DIRS: [&str; 3] = ["EasyAntiCheat", "EasyAntiCheat_EOS", "BattlEye"];

pub fn lookup(exe: &Path) -> Compat {
    let name = exe.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let db: HashMap<String, Compat> = serde_json::from_str(include_str!("../compat.json")).unwrap_or_default();
    let mut c = db.into_iter().find(|(k, _)| k.to_lowercase() == name).map(|(_, v)| v).unwrap_or_default();
    if let Some(dir) = exe.parent() {
        if ANTICHEAT_DIRS.iter().any(|d| dir.join(d).is_dir()) {
            c.anticheat = true;
        }
    }
    c
}

impl Compat {
    pub fn apply_ini(&self) {
        for e in &self.ini {
            let path = expand_env(&e.file);
            if let Ok(text) = std::fs::read_to_string(&path) {
                let _ = std::fs::write(&path, set_key(&text, &e.key, &e.value));
            }
        }
    }
}

fn set_key(text: &str, key: &str, value: &str) -> String {
    let key_l = key.to_lowercase();
    let mut out: Vec<String> = text
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, _)) if k.trim().to_lowercase() == key_l => format!("{}={value}", k.trim_end()),
            _ => l.to_string(),
        })
        .collect();
    if text.ends_with('\n') {
        out.push(String::new());
    }
    // Fins de ligne d'origine conservées (fichiers Windows en CRLF).
    out.join(if text.contains("\r\n") { "\r\n" } else { "\n" })
}

fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut parts = s.split('%');
    out.push_str(parts.next().unwrap_or_default());
    loop {
        match (parts.next(), parts.next()) {
            (Some(var), Some(rest)) => {
                out.push_str(&std::env::var(var).unwrap_or_else(|_| format!("%{var}%")));
                out.push_str(rest);
            }
            // `%` isolé (nombre impair dans la chaîne) : pas de paire à substituer, on le restitue
            // tel quel plutôt que de perdre silencieusement le reste de la chaîne.
            (Some(last), None) => {
                out.push('%');
                out.push_str(last);
                break;
            }
            _ => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_edit() {
        let t = "[Video]\nFullscreen = 1\nWidth=1920\n";
        assert_eq!(set_key(t, "fullscreen", "0"), "[Video]\nFullscreen=0\nWidth=1920\n");
        assert_eq!(set_key("[V]\r\nFullscreen=1\r\n", "fullscreen", "0"), "[V]\r\nFullscreen=0\r\n");
    }

    #[test]
    fn env() {
        std::env::set_var("HEARTH_T", "X");
        assert_eq!(expand_env(r"%HEARTH_T%\a.ini"), r"X\a.ini");
        assert_eq!(expand_env("plain"), "plain");
    }

    #[test]
    fn db_parses() {
        let c = lookup(Path::new(r"C:\x\r5apex.exe"));
        assert!(c.anticheat);
        assert!(lookup(Path::new("inconnu.exe")).borderless);
    }
}
