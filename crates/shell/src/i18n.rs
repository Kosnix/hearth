//! Langues : l'interface Slint est traduite par @tr (dossier lang/) ; les textes composés en Rust
//! passent par `tr` (le français est la langue source).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const LANGS: [(&str, &str); 2] = [("fr", "Français"), ("en", "English")];

static LANG: AtomicUsize = AtomicUsize::new(0);
static ANIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

const EN: &[(&str, &str)] = &[
    ("Son", "Sound"),
    ("Affichage", "Display"),
    ("Manettes", "Controllers"),
    ("Langue", "Language"),
    ("Sortie audio", "Audio output"),
    ("Garder ce réglage", "Keep this setting"),
    ("Annuler", "Cancel"),
    ("Retour automatique à l'ancien mode dans", "Automatic revert to the previous mode in"),
    ("Mode actuel :", "Current mode:"),
    ("Résolution", "Resolution"),
    ("Fréquence", "Refresh rate"),
    ("Appliquer", "Apply"),
    ("Recherche des réseaux…", "Searching for networks…"),
    ("Connecté", "Connected"),
    ("Sécurisé", "Secured"),
    ("Lecture…", "Reading…"),
    ("Aucune manette détectée", "No controller detected"),
    ("Connectée", "Connected"),
    ("Activé", "On"),
    ("Coupé", "Off"),
    ("Indisponible", "Unavailable"),
    ("Connexion à", "Connecting to"),
    ("Connecté à", "Connected to"),
    ("Échec de la connexion à", "Could not connect to"),
    ("Mot de passe :", "Password:"),
    ("s'est fermé", "has closed"),
    ("Impossible de lancer", "Cannot launch"),
    ("Bientôt disponible", "Coming soon"),
    ("À la une", "Featured"),
    ("jeux installés", "installed games"),
    ("Non détecté", "Not detected"),
    ("Gratuit", "Free"),
    ("Pilotes", "Drivers"),
    ("pilote", "driver"),
    ("Pilotes Windows Update", "Windows Update drivers"),
    ("Détection du matériel…", "Detecting hardware…"),
    ("Fond animé", "Animated background"),
    ("Windows exige l'autorisation de localisation pour lister les réseaux Wi-Fi.", "Windows requires location permission to list Wi-Fi networks."),
    ("Mises à jour", "Updates"),
    ("Stockage", "Storage"),
    ("Recherche en cours…", "Searching…"),
    ("Rechercher des mises à jour", "Check for updates"),
    ("libres sur", "free of"),
    ("Go", "GB"),
    ("À jour", "Up to date"),
    ("disponible(s)", "available"),
    ("Non connecté", "Signed out"),
];

fn file() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("settings.json")
}

pub fn tr(fr: &str) -> String {
    if lang() == 1 {
        EN.iter().find(|(f, _)| *f == fr).map_or(fr, |(_, e)| e).into()
    } else {
        fr.into()
    }
}

pub fn lang() -> usize {
    LANG.load(Ordering::Relaxed)
}

/// Langue enregistrée, sinon celle de Windows (anglais si ce n'est pas le français).
pub fn init() {
    let saved = std::fs::read_to_string(file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim_start_matches('\u{feff}')).ok())
        .inspect(|v| ANIM.store(v["anim"].as_bool().unwrap_or(true), Ordering::Relaxed))
        .and_then(|v| v["lang"].as_str().map(String::from));
    let code = saved.unwrap_or_else(system_lang);
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
    let _ = std::fs::write(file(), format!("{{\"lang\":\"{}\",\"anim\":{}}}", LANGS[lang()].0, anim()));
}

/// Valeur textuelle d'une préférence de `settings.json` (ex. clé SteamGridDB).
pub fn pref(key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(std::fs::read_to_string(file()).ok()?.trim_start_matches('\u{feff}')).ok()?;
    v[key].as_str().map(String::from).filter(|s| !s.is_empty())
}
