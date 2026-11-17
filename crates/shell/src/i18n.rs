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
    ("Dossier parent", "Parent folder"),
    ("Choisissez le dossier puis l'exécutable (.exe) du jeu", "Pick the folder, then the game executable (.exe)"),
    ("Identifier le jeu", "Identify the game"),
    ("Recherche dans les boutiques :", "Searching the stores:"),
    ("Saisir un autre nom…", "Type another name…"),
    ("Recherche actuelle :", "Current search:"),
    ("Utiliser", "Use"),
    ("sans visuels", "without artwork"),
    ("Nom du jeu", "Game name"),
    ("Jeu ajouté", "Game added"),
    ("Bureau", "Desktop"),
    ("Téléchargements", "Downloads"),
    ("Documents", "Documents"),
    ("Résultats pour", "Results for"),
    ("Rechercher un jeu", "Search for a game"),
    ("Jamais", "Never"),
    ("À proximité", "Nearby"),
    ("Appairé", "Paired"),
    ("Valider sur un appareil appairé pour l'oublier ; sur un appareil trouvé pour l'appairer.", "Select a paired device to forget it, or a found device to pair it."),
    ("Rechercher des appareils", "Search for devices"),
    ("Recherche d'appareils Bluetooth…", "Searching for Bluetooth devices…"),
    ("Appairage de", "Pairing"),
    ("appairé", "paired"),
    ("échec de l'appairage", "pairing failed"),
    ("oublié", "forgotten"),
    ("Mode d'alimentation", "Power mode"),
    ("Mise en veille après", "Sleep after"),
    ("Extinction de l'écran après", "Turn off screen after"),
    ("Alimentation", "Power"),
    ("Notifications", "Notifications"),
    ("À propos", "About"),
    ("Afficher les bannières", "Show banners"),
    ("Manettes", "Controllers"),
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
        .inspect(|v| {
            ANIM.store(v["anim"].as_bool().unwrap_or(true), Ordering::Relaxed);
            TOASTS.store(v["toasts"].as_bool().unwrap_or(true), Ordering::Relaxed);
        })
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
