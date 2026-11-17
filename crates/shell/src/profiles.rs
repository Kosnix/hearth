//! Profils utilisateurs de Hearth : chacun a ses comptes de boutiques, sa liste de jeux, son temps de jeu et ses personnalisations.
//! Ce ne sont pas des comptes Windows : les jeux installés et les visuels sont partagés.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use slint::{Model, VecModel};

use crate::i18n::tr;
use crate::{ProfileRow, Shell};

/// Code de sortie lu par le superviseur : relancer le shell (changement de profil).
pub const EXIT_RESTART: i32 = 11;

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub avatar: i32,
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
        s.profiles.push(Profile { id: "default".into(), name: std::env::var("USERNAME").unwrap_or_else(|_| "Joueur".into()), avatar: 3 });
        s.current = "default".into();
    }
    if !s.profiles.iter().any(|p| p.id == s.current) {
        s.current = s.profiles[0].id.clone();
    }
    s
}

pub fn save(s: &Store) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(file(), t);
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
    }

    pub fn profile_open(&self) {
        let s = load();
        let mut rows: Vec<ProfileRow> = s.profiles.iter().map(|p| ProfileRow { name: p.name.as_str().into(), avatar: p.avatar, active: p.id == s.current, add: false }).collect();
        rows.push(ProfileRow { name: tr("Nouveau profil").into(), avatar: 0, active: false, add: true });
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
        self.ui.set_avsel(0);
        self.ui.set_zone(12);
    }

    pub fn avatar_pick(&self, a: i32) {
        let name = std::mem::take(&mut *self.new_profile.borrow_mut());
        let mut s = load();
        let id = format!("p{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let _ = std::fs::create_dir_all(dir_of(&id));
        s.profiles.push(Profile { id, name: name.clone(), avatar: a });
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
        save(&s);
        self.push_notif(&tr("Profils"), &format!("{} : {}", tr("Profil supprimé"), p.name));
        self.profile_open();
    }

    /// Options d'un profil (suppression).
    pub fn profile_menu(&self, i: usize) {
        let s = load();
        let Some(p) = s.profiles.get(i) else { return };
        if p.id == s.current || p.id == "default" {
            self.push_notif(&tr("Profils"), &tr("Ce profil ne peut pas être supprimé"));
            return;
        }
        self.menu_show(
            format!("{} {} ?", tr("Supprimer le profil"), p.name),
            vec![(tr("Supprimer"), crate::menus::Act::DeleteProfile(i)), (tr("Annuler"), crate::menus::Act::Cancel)],
        );
    }

    pub fn profile_model() -> Rc<VecModel<ProfileRow>> {
        Rc::new(VecModel::default())
    }
}
