//! Temps de jeu et dernière partie, par jeu (`%LOCALAPPDATA%\Hearth\playtime.json`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::i18n::tr;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Entry {
    pub secs: u64,
    pub last: u64,
}

fn file() -> PathBuf {
    stores::profile_dir().join("playtime.json")
}

pub fn load() -> HashMap<String, Entry> {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_default()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Ajoute une session de jeu terminée.
pub fn record(key: &str, secs: u64) {
    let mut m = load();
    let e = m.entry(key.to_string()).or_default();
    e.secs += secs;
    e.last = now();
    if let Ok(s) = serde_json::to_string(&m) {
        let _ = std::fs::write(file(), s);
    }
}

/// « 12 h de jeu · dernière partie hier ».
pub fn label_in(map: &HashMap<String, Entry>, key: &str) -> String {
    let Some(e) = map.get(key).copied() else { return String::new() };
    if e.last == 0 {
        return String::new();
    }
    let played = if e.secs >= 3600 { format!("{} h", e.secs / 3600) } else { format!("{} min", (e.secs / 60).max(1)) };
    let days = now().saturating_sub(e.last) / 86_400;
    let when = match days {
        0 => tr("aujourd'hui"),
        1 => tr("hier"),
        n => tr("il y a {n} j").replace("{n}", &n.to_string()),
    };
    format!("{played} {} · {} {when}", tr("de jeu"), tr("dernière partie"))
}

/// Date de dernière partie (0 = jamais), pour trier la bibliothèque.
#[allow(dead_code)]
pub fn last(key: &str, map: &HashMap<String, Entry>) -> u64 {
    map.get(key).map_or(0, |e| e.last)
}

/// « 25h 24m » (ou « 12 min », « Jamais joué »).
pub fn played_in(map: &HashMap<String, Entry>, key: &str) -> String {
    match map.get(key).map(|e| e.secs) {
        Some(s) if s >= 3600 => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
        Some(s) if s > 0 => format!("{} min", (s / 60).max(1)),
        _ => tr("Jamais joué"),
    }
}

/// « aujourd'hui », « hier », « il y a 3 j ».
pub fn last_in(map: &HashMap<String, Entry>, key: &str) -> String {
    let Some(e) = map.get(key).filter(|e| e.last > 0) else { return "—".into() };
    match now().saturating_sub(e.last) / 86_400 {
        0 => tr("aujourd'hui"),
        1 => tr("hier"),
        n => tr("il y a {n} j").replace("{n}", &n.to_string()),
    }
}
