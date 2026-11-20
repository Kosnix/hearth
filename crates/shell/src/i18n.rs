//! Langues : l'interface Slint est traduite par @tr (dossier lang/) ; les textes composés en Rust
//! passent par `tr` (le français est la langue source).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Les dix langues les plus parlées (noms dans leur propre langue). Le français est la langue source des textes.
pub const LANGS: [(&str, &str); 10] = [
    ("en", "English"),
    ("fr", "Français"),
    ("zh", "中文"),
    ("hi", "हिन्दी"),
    ("es", "Español"),
    ("ar", "العربية"),
    ("bn", "বাংলা"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("ur", "اردو"),
];

static LANG: AtomicUsize = AtomicUsize::new(0);
static ANIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);


fn file() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("settings.json")
}

/// Textes traduits (colonnes : français | en | zh | hi | es | ar | bn | pt | ru | ur), générés par build.rs depuis lang/strings-*.tsv.
const TSV: &str = include_str!(concat!(env!("OUT_DIR"), "/strings.tsv"));

fn table() -> &'static std::collections::HashMap<&'static str, Vec<&'static str>> {
    static T: std::sync::OnceLock<std::collections::HashMap<&'static str, Vec<&'static str>>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        TSV.lines()
            .filter_map(|l| {
                let c: Vec<&str> = l.trim_end_matches('\r').split('|').collect();
                (c.len() == 10).then(|| (c[0], c[1..].to_vec()))
            })
            .collect()
    })
}

/// Code de la langue courante (« en », « fr »…).
pub fn code() -> &'static str {
    LANGS[lang()].0
}

/// Séparateur décimal : la virgule en français, espagnol, portugais et russe.
pub fn decimal_sep() -> &'static str {
    if matches!(code(), "fr" | "es" | "pt" | "ru") { "," } else { "." }
}

pub fn tr(fr: &str) -> String {
    let c = code();
    if c == "fr" {
        return fr.into();
    }
    // Colonnes : en = 0, zh = 1, hi = 2, es = 3, ar = 4, bn = 5, pt = 6, ru = 7, ur = 8.
    let col = ["en", "zh", "hi", "es", "ar", "bn", "pt", "ru", "ur"].iter().position(|x| *x == c).unwrap_or(0);
    table().get(fr).and_then(|v| v.get(col)).copied().filter(|s| !s.is_empty()).unwrap_or(fr).into()
}

pub fn lang() -> usize {
    LANG.load(Ordering::Relaxed)
}

/// Langue enregistrée, sinon celle de Windows (anglais par défaut).
pub fn init() {
    let saved = std::fs::read_to_string(file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim_start_matches('\u{feff}')).ok())
        .inspect(|v| {
            ANIM.store(v["anim"].as_bool().unwrap_or(true), Ordering::Relaxed);
            TOASTS.store(v["toasts"].as_bool().unwrap_or(true), Ordering::Relaxed);
        })
        .and_then(|v| v["lang"].as_str().map(String::from));
    // Première exécution : l'assistant démarre toujours en anglais ; ensuite la langue choisie, sinon celle de Windows.
    let code = saved.unwrap_or_else(|| if crate::oobe::is_done() { system_lang() } else { "en".into() });
    set(LANGS.iter().position(|(c, _)| *c == code).unwrap_or(0), false);
}

fn system_lang() -> String {
    let mut buf = [0u16; 85];
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) } as usize;
    let name = String::from_utf16_lossy(&buf[..n.saturating_sub(1)]);
    name.split('-').next().unwrap_or("fr").to_lowercase()
}

pub fn set(i: usize, save: bool) {
    LANG.store(i, Ordering::Relaxed);
    let r = slint::select_bundled_translation(LANGS[i].0);
    let _ = std::fs::write(file().with_file_name("lang.log"), format!("{r:?}"));
    if save {
        persist();
    }
}

pub fn anim() -> bool {
    ANIM.load(Ordering::Relaxed)
}

pub fn set_anim(on: bool) {
    ANIM.store(on, Ordering::Relaxed);
    persist();
}

fn persist() {
    // Conserve les autres clés (ex. steamgriddb_key).
    let mut v: serde_json::Value = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_else(|| serde_json::json!({}));
    v["lang"] = LANGS[lang()].0.into();
    v["anim"] = anim().into();
    v["toasts"] = toasts().into();
    let _ = std::fs::write(file(), v.to_string());
}

/// Valeur textuelle d'une préférence de `settings.json` (ex. clé SteamGridDB).
pub fn pref(key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(std::fs::read_to_string(file()).ok()?.trim_start_matches('\u{feff}')).ok()?;
    v[key].as_str().map(String::from).filter(|s| !s.is_empty())
}

static TOASTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Bannières de notification activées ?
pub fn toasts() -> bool {
    TOASTS.load(Ordering::Relaxed)
}

pub fn set_toasts(on: bool) {
    TOASTS.store(on, Ordering::Relaxed);
    persist();
}

/// Enregistre une préférence textuelle dans `settings.json`.
pub fn set_pref(key: &str, value: &str) {
    let mut v: serde_json::Value = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_else(|| serde_json::json!({}));
    v[key] = value.into();
    let _ = std::fs::write(file(), v.to_string());
}

static SFX: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static MUSIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn sfx() -> bool {
    SFX.load(Ordering::Relaxed)
}

pub fn music() -> bool {
    MUSIC.load(Ordering::Relaxed)
}


pub fn set_music(on: bool) {
    MUSIC.store(on, Ordering::Relaxed);
    set_pref("music", if on { "1" } else { "0" });
}

/// Lit les préférences sonores au démarrage.
pub fn init_audio_prefs() {
    SFX.store(pref("sfx").as_deref() != Some("0"), Ordering::Relaxed);
    MUSIC.store(pref("music").as_deref() != Some("0"), Ordering::Relaxed);
}

static SFX_VOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(80);
static MUSIC_VOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(10);

pub fn sfx_vol() -> u32 {
    SFX_VOL.load(Ordering::Relaxed)
}

pub fn music_vol() -> u32 {
    MUSIC_VOL.load(Ordering::Relaxed)
}

/// Envoie les volumes (réglages) au moteur audio.
pub fn apply_volumes() {
    system::sfx::set_volumes(if sfx() { sfx_vol() } else { 0 }, music_vol());
}

pub fn set_sfx_vol(v: u32) {
    SFX_VOL.store(v.min(100), Ordering::Relaxed);
    set_pref("sfx_vol", &v.min(100).to_string());
    apply_volumes();
}

pub fn set_music_vol(v: u32) {
    MUSIC_VOL.store(v.min(100), Ordering::Relaxed);
    set_pref("music_vol", &v.min(100).to_string());
    apply_volumes();
}

/// Volumes enregistrés (à appeler après le démarrage du moteur audio).
pub fn init_volumes() {
    if let Some(v) = pref("sfx_vol").and_then(|s| s.parse::<u32>().ok()) {
        SFX_VOL.store(v.min(100), Ordering::Relaxed);
    }
    if let Some(v) = pref("music_vol").and_then(|s| s.parse::<u32>().ok()) {
        MUSIC_VOL.store(v.min(100), Ordering::Relaxed);
    }
    apply_volumes();
}
