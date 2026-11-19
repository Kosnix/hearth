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
    ("Ajouter une boutique", "Add a store"),
    ("à installer depuis le Rockstar Games Launcher", "install it from the Rockstar Games Launcher"),
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
    ("aujourd'hui", "today"),
    ("hier", "yesterday"),
    ("il y a", "ago:"),
    ("j", "d"),
    ("de jeu", "played"),
    ("dernière partie", "last played"),
    ("Comptes", "Accounts"),
    ("Connexion par QR code. Le client de la boutique s'installe tout seul quand il faut installer un jeu.", "Sign in with a QR code. The store client installs itself when a game needs installing."),
    ("Bibliothèque locale", "Local library"),
    ("Client non installé", "Client not installed"),
    ("Se connecter", "Sign in"),
    ("Installer le client", "Install the client"),
    ("Automatique (winget)", "Automatic (winget)"),
    ("Installation", "Installation"),
    ("Installation du client", "Installing the client"),
    ("client installé", "client installed"),
    ("échec de l'installation du client", "client installation failed"),
    ("Connexion", "Sign-in"),
    ("service indisponible", "service unavailable"),
    ("déconnecté", "signed out"),
    ("demandée dans le client", "requested in the client"),
    ("Scannez avec votre téléphone pour acheter", "Scan with your phone to buy"),
    ("Connectez-vous sur votre téléphone, puis saisissez le code d'autorisation affiché", "Sign in on your phone, then enter the authorization code shown"),
    ("Saisir le code", "Enter the code"),
    ("Code d'autorisation", "Authorization code"),
    ("Scannez avec l'application Steam sur votre téléphone puis validez", "Scan with the Steam app on your phone, then confirm"),
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
    ("Installer", "Install"),
    ("Jouer", "Play"),
    ("Reprendre", "Resume"),
    ("Modifier le nom", "Rename"),
    ("Changer le visuel", "Change artwork"),
    ("Cacher", "Hide"),
    ("Retirer de Hearth", "Remove from Hearth"),
    ("Désinstaller", "Uninstall"),
    ("Ajouter un jeu depuis ce PC", "Add a game from this PC"),
    ("Jeux cachés", "Hidden games"),
    ("Ajouter", "Add"),
    ("Jeu caché", "Game hidden"),
    ("Confirmer", "Confirm"),
    ("Jeu retiré", "Game removed"),
    ("Désinstallation lancée (à valider dans le client si besoin)", "Uninstall started (confirm in the client if asked)"),
    ("Désinstallez ce jeu depuis le client de sa boutique", "Uninstall this game from its store client"),
    ("Visuel", "Artwork"),
    ("Téléchargement du nouveau visuel…", "Downloading the new artwork…"),
    ("à valider dans la fenêtre du client", "confirm in the client window"),
    ("Choisissez un jeu pour le réafficher", "Pick a game to show it again"),
    ("Autorisation administrateur", "Administrator permission"),
    ("Activée", "Enabled"),
    ("Activer (une seule fois)", "Enable (one time only)"),
    ("Connectez-vous à Epic (Réglages > Comptes)", "Sign in to Epic (Settings > Accounts)"),
    ("Préparation de l'installation impossible", "Could not prepare installation"),
    ("Désinstallation en cours…", "Uninstalling…"),
    ("Téléchargements", "Downloads"),
    ("Déjà dans la file", "Already queued"),
    ("ajouté à la file", "added to the queue"),
    ("Vérification du code…", "Checking the code…"),
    ("Saisissez le code Steam Guard (ou validez sur votre téléphone)", "Enter the Steam Guard code (or approve on your phone)"),
    ("Téléchargement annulé", "Download cancelled"),
    ("Connectez-vous à Steam (Réglages > Comptes) pour gérer les mises à jour", "Sign in to Steam (Settings > Accounts) to manage updates"),
    ("Aucun jeu installé par Hearth à mettre à jour", "No Hearth-installed game to update"),
    ("Recherche des mises à jour…", "Checking for updates…"),
    ("Tous les jeux sont à jour", "All games are up to date"),
    ("mise(s) à jour disponible(s)", "update(s) available"),
    ("Rechercher les mises à jour", "Check for updates"),
    ("Tout mettre à jour", "Update all"),
    ("En attente", "Waiting"),
    ("Mise à jour disponible", "Update available"),
    ("Terminé", "Done"),
    ("Mo/s", "MB/s"),
    ("Jamais joué", "Never played"),
    ("Nouveau profil", "New profile"),
    ("Nom du profil", "Profile name"),
    ("Profils", "Profiles"),
    ("Profil créé", "Profile created"),
    ("Profil supprimé", "Profile deleted"),
    ("Ce profil ne peut pas être supprimé", "This profile cannot be deleted"),
    ("Supprimer le profil", "Delete profile"),
    ("Supprimer", "Delete"),
    ("Effets sonores", "Sound effects"),
    ("Musique d'ambiance", "Ambient music"),
    ("Étape 2 : identifiant Steam pour télécharger les jeux", "Step 2: Steam username to download games"),
    ("Préparation de Steam…", "Preparing Steam…"),
    ("Démarrage de Steam…", "Starting Steam…"),
    ("Connectez-vous dans la fenêtre Steam (une seule fois), puis relancez le jeu", "Sign in in the Steam window (one time only), then launch the game again"),
    ("restant", "left"),
    ("Vérifier / mettre à jour", "Check / update"),
    ("Jeu désinstallé", "Game uninstalled"),
    ("Désinstallation impossible", "Could not uninstall"),
    ("Mise à jour disponible", "Update available"),
    ("Mettre à jour maintenant", "Update now"),
    ("Plus tard", "Later"),
    ("Identifiant Steam", "Steam username"),
    ("Mot de passe Steam", "Steam password"),
    ("Code Steam Guard", "Steam Guard code"),
    ("Code Steam Guard de l'application Steam (vide : validation sur le téléphone)", "Steam Guard code from the Steam app (empty: approve on your phone)"),
    ("Connexion à Steam… (si besoin, validez sur votre téléphone, 1 minute)", "Signing in to Steam… (approve on your phone if asked, 1 minute)"),
    ("Code Steam Guard, ou Valider (vide) si approuvé sur le téléphone", "Steam Guard code, or Confirm (empty) if approved on your phone"),
    ("Connexion à Steam…", "Signing in to Steam…"),
    ("Connecté à Steam :", "Signed in to Steam:"),
    ("Saisissez le code Steam Guard", "Enter the Steam Guard code"),
    ("Connexion refusée", "Sign-in refused"),
    ("Déjà en cours", "Already in progress"),
    ("téléchargement en cours (plusieurs minutes)…", "downloading (several minutes)…"),
    ("installé", "installed"),
    ("échec", "failed"),
    ("Steam : installer sans le client", "Steam: install without the client"),
    ("Autorisation administrateur requise : acceptez la fenêtre Windows (une seule fois)", "Administrator permission needed: accept the Windows prompt (one time only)"),
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

pub fn set_sfx(on: bool) {
    SFX.store(on, Ordering::Relaxed);
    set_pref("sfx", if on { "1" } else { "0" });
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
