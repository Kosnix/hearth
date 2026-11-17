//! Menus contextuels : options d'un jeu (modifier, cacher, désinstaller) et ajout de jeux.

use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use slint::{SharedString, VecModel};

use crate::i18n::tr;
use crate::{prefs, Shell};

#[derive(Clone, Copy)]
pub enum Act {
    Launch(usize),
    Rename(usize),
    Reart(usize),
    Hide(usize),
    Uninstall(usize),
    ConfirmUninstall(usize),
    AddGame,
    ShowHidden,
    UpdateYes(usize),
    DeleteProfile(usize),
    Cancel,
}

impl Shell {
    pub fn menu_show(&self, title: String, items: Vec<(String, Act)>) {
        let from = self.ui.get_zone();
        self.ui.set_menu_title(title.into());
        let labels: Vec<SharedString> = items.iter().map(|(l, _)| l.as_str().into()).collect();
        self.ui.set_menu_items(Rc::new(VecModel::from(labels)).into());
        self.ui.set_msel(0);
        if from != 9 {
            self.ui.set_menu_return(from);
        }
        *self.menu_acts.borrow_mut() = items.into_iter().map(|(_, a)| a).collect();
        self.ui.set_zone(9);
    }

    /// Options d'un jeu (bouton Options, clic droit).
    pub fn game_menu(&self, gid: usize) {
        let Some(g) = self.games.borrow().get(gid).cloned() else { return };
        let running = self.running.borrow().iter().any(|r| r.tile == gid);
        let mut v: Vec<(String, Act)> = vec![];
        if g.install {
            v.push((tr("Installer"), Act::Launch(gid)));
        } else {
            v.push((tr(if running { "Reprendre" } else { "Jouer" }), Act::Launch(gid)));
            v.push((tr("Modifier le nom"), Act::Rename(gid)));
            v.push((tr("Changer le visuel"), Act::Reart(gid)));
        }
        if self.is_scmd_game(&g.exe) && !running {
            v.push((tr("Vérifier / mettre à jour"), Act::UpdateYes(gid)));
        }
        v.push((tr("Cacher"), Act::Hide(gid)));
        if !g.install && !running {
            v.push((tr(if g.store.is_empty() { "Retirer de Hearth" } else { "Désinstaller" }), Act::Uninstall(gid)));
        }
        v.push((tr("Annuler"), Act::Cancel));
        self.menu_show(g.name, v);
    }

    /// Une mise à jour est disponible : on demande à l'utilisateur.
    pub fn update_menu(&self, gid: usize) {
        let Some(name) = self.games.borrow().get(gid).map(|g| g.name.clone()) else { return };
        self.menu_show(
            format!("{} : {name}", tr("Mise à jour disponible")),
            vec![(tr("Mettre à jour maintenant"), Act::UpdateYes(gid)), (tr("Plus tard"), Act::Cancel)],
        );
    }

    /// Menu du bouton « + » : ajouter un jeu, retrouver les jeux cachés.
    pub fn add_menu(&self) {
        let hidden = self.games.borrow().iter().filter(|g| g.hidden).count();
        let mut v = vec![(tr("Ajouter un jeu depuis ce PC"), Act::AddGame)];
        if hidden > 0 {
            v.push((format!("{} ({hidden})", tr("Jeux cachés")), Act::ShowHidden));
        }
        v.push((tr("Annuler"), Act::Cancel));
        self.menu_show(tr("Ajouter"), v);
    }

    pub fn menu_pick(&self, i: usize) {
        let Some(act) = self.menu_acts.borrow().get(i).copied() else { return };
        let back = self.ui.get_menu_return();
        match act {
            Act::Cancel => self.ui.set_zone(back),
            Act::Launch(g) => {
                self.ui.set_zone(0);
                self.launch(g);
            }
            Act::Rename(g) => {
                let Some((id, name)) = self.games.borrow().get(g).map(|g| (g.id.clone(), g.name.clone())) else { return };
                *self.rename_id.borrow_mut() = Some(id);
                self.settings.kb_open_text(&name);
                self.kb_refresh();
                self.ui.set_kbx(0);
                self.ui.set_kby(0);
                self.ui.set_kb_title(tr("Nom du jeu").into());
                self.ui.set_zone(4);
            }
            Act::Reart(g) => {
                let Some((id, name)) = self.games.borrow().get(g).map(|g| (g.id.clone(), g.name.clone())) else { return };
                *self.identify_target.borrow_mut() = Some(id);
                self.run_identify(name);
                self.ui.set_zone(6);
            }
            Act::Hide(g) => {
                let Some((id, name)) = self.games.borrow().get(g).map(|g| (g.id.clone(), g.name.clone())) else { return };
                self.set_hidden(&id, true);
                self.ui.set_zone(back);
                self.push_notif(&tr("Jeu caché"), &name);
            }
            Act::Uninstall(g) => {
                let Some(name) = self.games.borrow().get(g).map(|g| g.name.clone()) else { return };
                self.menu_show(format!("{} {name} ?", tr("Désinstaller")), vec![(tr("Confirmer"), Act::ConfirmUninstall(g)), (tr("Annuler"), Act::Cancel)]);
            }
            Act::ConfirmUninstall(g) => {
                self.ui.set_zone(back);
                self.uninstall(g);
            }
            Act::AddGame => self.add_open(),
            Act::ShowHidden => self.show_hidden(),
            Act::DeleteProfile(i) => {
                self.ui.set_zone(back);
                self.profile_delete(i);
            }
            Act::UpdateYes(g) => {
                self.ui.set_zone(back);
                if let Some(id) = self.games.borrow().get(g).map(|g| g.id.clone()) {
                    self.steam_install(&id);
                }
            }
        }
    }

    fn set_override(&self, id: &str, f: impl FnOnce(&mut prefs::Override)) {
        let mut m = self.overrides.borrow_mut();
        f(m.entry(id.to_string()).or_default());
        prefs::save(&m);
    }

    pub fn set_hidden(&self, id: &str, hidden: bool) {
        self.set_override(id, |o| o.hidden = hidden);
        if let Some(g) = self.games.borrow_mut().iter_mut().find(|g| g.id == id) {
            g.hidden = hidden;
        }
        self.rebuild();
    }

    pub fn rename_game(&self, id: &str, name: &str) {
        if name.is_empty() {
            return;
        }
        self.set_override(id, |o| o.name = Some(name.to_string()));
        if let Some(g) = self.games.borrow_mut().iter_mut().find(|g| g.id == id) {
            g.name = name.to_string();
        }
        self.rebuild();
    }

    fn uninstall(&self, gid: usize) {
        let Some(g) = self.games.borrow().get(gid).cloned() else { return };
        if g.store.is_empty() {
            // Jeu ajouté à la main : on le retire seulement de Hearth, ses fichiers restent.
            self.games.borrow_mut().remove(gid);
            for r in self.running.borrow_mut().iter_mut() {
                if r.tile > gid {
                    r.tile -= 1;
                }
            }
            self.save_games();
            self.rebuild();
            self.push_notif(&tr("Jeu retiré"), &g.name);
            return;
        }
        if std::path::Path::new(&g.exe) == stores::epiccmd::exe() {
            let app = g.id.strip_prefix("epic-").unwrap_or_default().to_string();
            self.push_notif(&g.name, &tr("Désinstallation en cours…"));
            std::thread::spawn(move || {
                let ok = stores::epiccmd::uninstall(&app);
                let _ = slint::invoke_from_event_loop(move || {
                    crate::with_shell(|s| {
                        s.push_notif("Epic", &tr(if ok { "Jeu désinstallé" } else { "Désinstallation impossible" }));
                        s.poll_installed();
                    })
                });
            });
            return;
        }
        if self.is_scmd_game(&g.exe) {
            let ok = g.id.strip_prefix("steam-").is_some_and(stores::steamcmd::uninstall);
            self.push_notif(&g.name, &tr(if ok { "Jeu désinstallé" } else { "Désinstallation impossible" }));
            self.poll_installed();
            return;
        }
        let started = match g.store.as_str() {
            "Steam" =>g.id.strip_prefix("steam-").is_some_and(|id| Command::new("cmd").args(["/c", "start", "", &format!("steam://uninstall/{id}")]).spawn().is_ok()),
            "GOG" => {
                let exe = Path::new(&g.exe);
                let found = exe.ancestors().skip(1).take(3).find_map(|d| {
                    std::fs::read_dir(d).ok()?.flatten().map(|e| e.path()).find(|p| {
                        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                        n.starts_with("unins") && n.ends_with(".exe")
                    })
                });
                found.is_some_and(|u| Command::new(u).arg("/SILENT").spawn().is_ok())
            }
            _ => {
                let _ = Command::new("cmd").args(["/c", "start", "", "com.epicgames.launcher://store/library"]).spawn();
                false
            }
        };
        let msg = if started { tr("Désinstallation lancée (à valider dans le client si besoin)") } else { tr("Désinstallez ce jeu depuis le client de sa boutique") };
        self.push_notif(&g.name, &msg);
    }

    fn show_hidden(&self) {
        let hidden: Vec<(String, String, String)> = self.games.borrow().iter().filter(|g| g.hidden).map(|g| (g.id.clone(), g.name.clone(), g.store.clone())).collect();
        if hidden.is_empty() {
            self.ui.set_zone(0);
            return;
        }
        self.pick_hidden(hidden);
    }
}
