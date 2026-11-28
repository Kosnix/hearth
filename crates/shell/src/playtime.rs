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

/// Écart heure locale − UTC, en secondes (fuseaux de −12 h à +12 h).
fn local_offset() -> i64 {
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    let (l, u) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let mins = |h: u16, m: u16| h as i64 * 60 + m as i64;
    let mut d = mins(l.wHour, l.wMinute) - mins(u.wHour, u.wMinute);
    if d > 720 {
        d -= 1440;
    } else if d < -720 {
        d += 1440;
    }
    d * 60
}

/// Jours de calendrier (heure locale) écoulés depuis `last` : une partie d'hier soir à 23 h est
/// « hier » le lendemain matin, pas « aujourd'hui » (24 h ne sont pas encore passées).
fn days_since(last: u64) -> u64 {
    let off = local_offset();
    let day = |t: u64| (t as i64 + off).div_euclid(86_400);
    (day(now()) - day(last)).max(0) as u64
}

/// Ajoute une session de jeu terminée.
pub fn record(key: &str, secs: u64) {
    let mut m = load();
    let e = m.entry(key.to_string()).or_default();
    e.secs += secs;
    e.last = now();
    if let Ok(s) = serde_json::to_string(&m) {
        crate::prefs::write_atomic(&file(), &s);
    }
}

/// « 12 h de jeu · dernière partie hier ».
pub fn label_in(map: &HashMap<String, Entry>, key: &str) -> String {
    let Some(e) = map.get(key).copied() else { return String::new() };
    if e.last == 0 {
        return String::new();
    }
    let played = if e.secs >= 3600 { format!("{} {}", e.secs / 3600, tr("h")) } else { format!("{} {}", (e.secs / 60).max(1), tr("min")) };
    let days = days_since(e.last);
    let when = match days {
        0 => tr("aujourd'hui"),
        1 => tr("hier"),
        n => tr("il y a {n} j").replace("{n}", &n.to_string()),
    };
    format!("{played} {} · {} {when}", tr("de jeu"), tr("dernière partie"))
}

/// Date de dernière partie (0 = jamais), pour trier la bibliothèque.
pub fn last(key: &str, map: &HashMap<String, Entry>) -> u64 {
    map.get(key).map_or(0, |e| e.last)
}

/// « 25h 24m » (ou « 12 min », « Jamais joué »).
pub fn played_in(map: &HashMap<String, Entry>, key: &str) -> String {
    match map.get(key).map(|e| e.secs) {
        Some(s) if s >= 3600 => format!("{} {} {:02} {}", s / 3600, tr("h"), (s % 3600) / 60, tr("min")),
        Some(s) if s > 0 => format!("{} {}", (s / 60).max(1), tr("min")),
        _ => tr("Jamais joué"),
    }
}

/// « aujourd'hui », « hier », « il y a 3 j ».
pub fn last_in(map: &HashMap<String, Entry>, key: &str) -> String {
    let Some(e) = map.get(key).filter(|e| e.last > 0) else { return "—".into() };
    match days_since(e.last) {
        0 => tr("aujourd'hui"),
        1 => tr("hier"),
        n => tr("il y a {n} j").replace("{n}", &n.to_string()),
    }
}
