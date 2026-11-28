//! Profils utilisateurs de Hearth : chacun a ses comptes de boutiques, sa liste de jeux, son temps de jeu et ses personnalisations.
//! Ce ne sont pas des comptes Windows : les jeux installés et les visuels sont partagés.


use serde::{Deserialize, Serialize};
use slint::Model;

use crate::i18n::tr;
use crate::{ProfileRow, Shell};

/// Code de sortie lu par le superviseur : relancer le shell (changement de profil).
pub const EXIT_RESTART: i32 = 11;

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub avatar: i32,
    /// Photo importée (fichier `avatar.png` du dossier du profil).
    #[serde(default)]
    pub photo: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Store {
    pub current: String,
    pub profiles: Vec<Profile>,
}

fn file() -> std::path::PathBuf {
    stores::base_dir().join("profiles.json")
}

pub fn load() -> Store {
    let s: Option<Store> = std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok());
    let mut s = s.unwrap_or_else(|| Store { current: "default".into(), profiles: vec![] });
    if s.profiles.is_empty() {
        s.profiles.push(Profile { id: "default".into(), name: std::env::var("USERNAME").unwrap_or_else(|_| "Joueur".into()), avatar: 3, photo: false });
        s.current = "default".into();
    }
    if !s.profiles.iter().any(|p| p.id == s.current) {
        s.current = s.profiles[0].id.clone();
    }
    s
}

pub fn save(s: &Store) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        crate::prefs::write_atomic(&file(), &t);
    }
}

fn dir_of(id: &str) -> std::path::PathBuf {
    if id == "default" {
        stores::base_dir()
    } else {
        stores::base_dir().join("profiles").join(id)
    }
}

/// À appeler au tout début de `main` (avant tout thread) : fixe le dossier du profil actif pour tout le processus.
pub fn activate() -> Store {
    let s = load();
    let dir = dir_of(&s.current);
    let _ = std::fs::create_dir_all(&dir);
    std::env::set_var("HEARTH_PROFILE_DIR", &dir);
    std::env::set_var("HEARTH_PROFILE_ID", &s.current);
    s
}

impl Shell {
    pub fn profile_current(&self) -> Profile {
        let s = load();
        s.profiles.iter().find(|p| p.id == s.current).cloned().unwrap_or_else(|| s.profiles[0].clone())
    }

    /// Nom et avatar dans la barre du haut.
    pub fn profile_refresh_top(&self) {
        let p = self.profile_current();
        self.ui.set_user(p.name.into());
        self.ui.set_user_avatar(p.avatar);
        self.ui.set_user_photo(photo_of(&p.id, p.photo));
    }

    pub fn profile_open(&self) {
        let s = load();
        let mut rows: Vec<ProfileRow> = s.profiles.iter().map(|p| ProfileRow { name: p.name.as_str().into(), avatar: p.avatar, photo: photo_of(&p.id, p.photo), active: p.id == s.current, add: false, played: total_played(&p.id).into() }).collect();
        rows.push(ProfileRow { name: tr("Nouveau profil").into(), avatar: 0, photo: Default::default(), active: false, add: true, played: Default::default() });
        self.profile_rows.set_vec(rows);
        let max = self.profile_rows.row_count().saturating_sub(1) as i32;
        if self.ui.get_pfsel() > max {
            self.ui.set_pfsel(max);
        }
    }

    pub fn profile_pick(&self, i: usize) {
        let s = load();
        if i >= s.profiles.len() {
            // Nouveau profil : nom au clavier virtuel, puis choix de l'avatar.
            self.kb_profile.set(true);
            self.settings.kb_open("");
            self.kb_refresh();
            self.ui.set_kbx(0);
            self.ui.set_kby(0);
            self.ui.set_kb_title(tr("Nom du profil").into());
            self.ui.set_zone(4);
            return;
        }
        let p = &s.profiles[i];
        if p.id == s.current {
            self.ui.set_zone(1);
            return;
        }
        // Hearth redémarre pour changer de profil : un jeu en cours continuerait, mais sans guide ni temps de jeu.
        if !self.running.borrow().is_empty() || self.ext_game.borrow().is_some() || self.ext_pending.get().is_some() {
            self.push_notif(&tr("Profils"), &tr("Fermez le jeu en cours avant de changer de profil"));
            return;
        }
        let mut s = s;
        s.current = s.profiles[i].id.clone();
        save(&s);
        // Le superviseur relance le shell : toutes les données chargées sont celles du nouveau profil.
        std::process::exit(EXIT_RESTART);
    }

    /// Nom saisi : on passe au choix de l'avatar.
    pub fn profile_named(&self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.ui.set_zone(11);
            return;
        }
        *self.new_profile.borrow_mut() = name;
        // Création d'un profil : pas la modification d'un avatar existant laissée en suspens (Retour).
        *self.avatar_for.borrow_mut() = None;
        self.avatar_editor_open(3, Default::default());
        self.ui.set_zone(12);
    }

    pub fn avatar_pick(&self, a: i32) {
        if self.oobe_avatar.replace(false) && self.oobe_active() {
            self.oobe_avatar_set(a);
            return;
        }
        // Avatar d'un profil existant (menu Options d'un profil).
        if let Some(id) = self.avatar_for.borrow_mut().take() {
            let mut s = load();
            let photo = self.avatar_commit_photo(&id);
            if let Some(p) = s.profiles.iter_mut().find(|p| p.id == id) {
                p.avatar = a;
                p.photo = photo;
            }
            save(&s);
            self.profile_refresh_top();
            self.profile_open();
            self.ui.set_zone(11);
            return;
        }
        let name = std::mem::take(&mut *self.new_profile.borrow_mut());
        let mut s = load();
        let id = format!("p{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let _ = std::fs::create_dir_all(dir_of(&id));
        let photo = self.avatar_commit_photo(&id);
        s.profiles.push(Profile { id, name: name.clone(), avatar: a, photo });
        save(&s);
        self.push_notif(&tr("Profils"), &format!("{} : {name}", tr("Profil créé")));
        self.profile_open();
        self.ui.set_pfsel(self.profile_rows.row_count().saturating_sub(2) as i32);
        self.ui.set_zone(11);
    }

    pub fn profile_delete(&self, i: usize) {
        let mut s = load();
        if i >= s.profiles.len() || s.profiles[i].id == s.current || s.profiles[i].id == "default" {
            self.push_notif(&tr("Profils"), &tr("Ce profil ne peut pas être supprimé"));
            return;
        }
        let p = s.profiles.remove(i);
        let _ = std::fs::remove_dir_all(dir_of(&p.id));
        // Données rangées ailleurs pour ce profil : identifiant SteamCMD et session Legendary (Epic).
        crate::i18n::set_pref(&format!("steamcmd_user_{}", p.id), "");
        let _ = std::fs::remove_dir_all(stores::epiccmd::dir().join(format!("config-{}", p.id)));
        save(&s);
        self.push_notif(&tr("Profils"), &format!("{} : {}", tr("Profil supprimé"), p.name));
        self.profile_open();
    }

    /// Options d'un profil (suppression).
    pub fn profile_menu(&self, i: usize) {
        let s = load();
        let Some(p) = s.profiles.get(i) else { return };
        // Nom et avatar modifiables pour tout profil ; suppression seulement hors profil actif et par défaut.
        let mut v = vec![
            (tr("Modifier le nom"), crate::menus::Act::ProfileRename(p.id.clone())),
            (tr("Changer l'avatar"), crate::menus::Act::ProfileAvatar(p.id.clone())),
        ];
        if p.id != s.current && p.id != "default" {
            v.push((tr("Supprimer"), crate::menus::Act::DeleteProfile(i)));
        }
        v.push((tr("Annuler"), crate::menus::Act::Cancel));
        self.menu_show(p.name.clone(), v);
    }

    /// Nom d'un profil existant saisi au clavier virtuel.
    pub fn profile_rename_open(&self, id: &str) {
        let s = load();
        let Some(p) = s.profiles.iter().find(|p| p.id == id) else { return };
        *self.profile_edit.borrow_mut() = Some(id.to_string());
        self.settings.kb_open_text(&p.name);
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(tr("Nom du profil").into());
        self.ui.set_zone(4);
    }

    pub fn profile_renamed(&self, id: &str, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            let mut s = load();
            if let Some(p) = s.profiles.iter_mut().find(|p| p.id == id) {
                p.name = name.to_string();
            }
            save(&s);
            self.profile_refresh_top();
        }
        self.profile_open();
        self.ui.set_zone(11);
    }

    /// Avatar d'un profil existant : même éditeur que pour un nouveau profil.
    pub fn profile_avatar_open(&self, id: &str) {
        let s = load();
        let Some(p) = s.profiles.iter().find(|p| p.id == id) else { return };
        *self.avatar_for.borrow_mut() = Some(id.to_string());
        self.avatar_editor_open(p.avatar, photo_of(&p.id, p.photo));
    }

}

/// Photo d'un profil (image vide s'il n'en a pas).
fn photo_of(id: &str, has: bool) -> slint::Image {
    if !has {
        return Default::default();
    }
    slint::Image::load_from_path(&dir_of(id).join("avatar.png")).unwrap_or_default()
}

impl Shell {
    /// Sélecteur de photo : l'avatar courant sert de point de départ.
    pub fn avatar_editor_open(&self, kind: i32, photo: slint::Image) {
        self.ui.set_avsel(kind.clamp(0, 95));
        self.ui.set_avrow(0);
        self.ui.set_av_photo(photo);
        self.photo_fresh.set(false);
        self.ui.set_zone(12);
    }

    /// « Importer une photo… » : boîte de dialogue Windows, image recadrée au carré.
    pub fn avatar_photo(&self) {
        let filter = format!("{}\0*.png;*.jpg;*.jpeg\0", tr("Images"));
        let Some(path) = crate::native::open_file(&tr("Choisir une photo"), &filter) else { return };
        let tmp = stores::base_dir().join("avatar-tmp.png");
        // Décodage et recadrage hors du fil de l'interface (une grande photo le figeait une seconde ou plus).
        std::thread::spawn(move || {
            let ok = crate::native::square_image(&path, &tmp);
            let _ = slint::invoke_from_event_loop(move || {
                crate::with_shell(|s| match slint::Image::load_from_path(&tmp).ok().filter(|_| ok) {
                    Some(img) => {
                        s.ui.set_av_photo(img);
                        s.photo_fresh.set(true);
                        s.ui.set_avrow(3);
                    }
                    None => s.push_notif(&tr("Profils"), &tr("Image illisible")),
                })
            });
        });
    }

    /// Photo retenue au moment de valider : copie la photo importée dans le dossier du profil. Renvoie vrai s'il y a une photo.
    pub(crate) fn avatar_commit_photo(&self, id: &str) -> bool {
        let has = self.ui.get_av_photo().size().width > 0;
        if has && self.photo_fresh.replace(false) {
            let _ = std::fs::create_dir_all(dir_of(id));
            let _ = std::fs::copy(stores::base_dir().join("avatar-tmp.png"), dir_of(id).join("avatar.png"));
        }
        has
    }

    /// Photo du profil courant (barre du haut).
    pub fn profile_photo_current(&self) -> slint::Image {
        let p = self.profile_current();
        photo_of(&p.id, p.photo)
    }
}

/// Temps de jeu total d'un profil (« 25 h », « 40 min »), vide s'il n'a jamais joué.
fn total_played(id: &str) -> String {
    let secs: u64 = std::fs::read_to_string(dir_of(id).join("playtime.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim_start_matches('\u{feff}')).ok())
        .and_then(|j| j.as_object().map(|m| m.values().filter_map(|e| e["secs"].as_u64()).sum()))
        .unwrap_or(0);
    match secs {
        0 => String::new(),
        s if s >= 3600 => format!("{} {} {}", s / 3600, tr("h"), tr("de jeu")),
        s => format!("{} {} {}", (s / 60).max(1), tr("min"), tr("de jeu")),
    }
}
