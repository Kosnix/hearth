//! Menus contextuels : options d'un jeu (modifier, cacher, désinstaller) et ajout de jeux.

use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use slint::{SharedString, VecModel};

use crate::i18n::tr;
use crate::{prefs, scmd, Shell};

#[derive(Clone)]
pub enum Act {
    // Identifiant stable du jeu (`Game::id`), pas son index dans `self.games` : celui-ci peut changer
    // pendant qu'un menu reste ouvert (poll_installed() tourne toutes les 20 s en fond et peut
    // réordonner la liste si un autre jeu est installé/désinstallé par une boutique) — un index capturé
    // à l'ouverture du menu agirait alors sur le jeu qui s'y trouve maintenant, pas celui affiché.
    Launch(String),
    Rename(String),
    Reart(String),
    Hide(String),
    Uninstall(String),
    ConfirmUninstall(String),
    AddGame,
    ShowHidden,
    UpdateYes(String),
    DeleteProfile(usize),
    CancelDl(usize),
    SelfUpdate,
    ConnectStore(usize),
    ConfirmBtUnpair,
    OfferStore(Option<String>),
    OfferSort(i32),
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
            v.push((tr("Installer"), Act::Launch(g.id.clone())));
        } else {
            v.push((tr(if running { "Reprendre" } else { "Jouer" }), Act::Launch(g.id.clone())));
            v.push((tr("Modifier le nom"), Act::Rename(g.id.clone())));
            v.push((tr("Changer le visuel"), Act::Reart(g.id.clone())));
        }
        if self.is_scmd_game(&g.exe) && !running {
            v.push((tr("Vérifier / mettre à jour"), Act::UpdateYes(g.id.clone())));
        }
        v.push((tr(if g.hidden { "Afficher" } else { "Cacher" }), Act::Hide(g.id.clone())));
        if !g.install && !running {
            v.push((tr(if g.store.is_empty() { "Retirer de Hearth" } else { "Désinstaller" }), Act::Uninstall(g.id.clone())));
        }
        v.push((tr("Annuler"), Act::Cancel));
        self.menu_show(g.name, v);
    }

    /// Une mise à jour est disponible : on demande à l'utilisateur.
    pub fn update_menu(&self, gid: usize) {
        let Some(g) = self.games.borrow().get(gid).cloned() else { return };
        self.menu_show(
            format!("{} : {}", tr("Mise à jour disponible"), g.name),
            vec![(tr("Mettre à jour maintenant"), Act::UpdateYes(g.id)), (tr("Plus tard"), Act::Cancel)],
        );
    }

    /// Index courant d'un jeu par son identifiant stable (peut avoir bougé, ou disparu, depuis
    /// l'ouverture du menu).
    fn gid_of(&self, id: &str) -> Option<usize> {
        self.games.borrow().iter().position(|g| g.id == id)
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
        let Some(act) = self.menu_acts.borrow().get(i).cloned() else { return };
        let back = self.ui.get_menu_return();
        match act {
            Act::Cancel => self.ui.set_zone(back),
            Act::Launch(id) => {
                let Some(g) = self.gid_of(&id) else { return };
                self.ui.set_zone(0);
                self.launch(g);
            }
            Act::Rename(id) => {
                let Some(name) = self.games.borrow().iter().find(|g| g.id == id).map(|g| g.name.clone()) else { return };
                *self.rename_id.borrow_mut() = Some(id);
                self.settings.kb_open_text(&name);
                self.kb_refresh();
                self.ui.set_kbx(0);
                self.ui.set_kby(0);
                self.ui.set_kb_title(tr("Nom du jeu").into());
                self.ui.set_zone(4);
            }
            Act::Reart(id) => {
                let Some(name) = self.games.borrow().iter().find(|g| g.id == id).map(|g| g.name.clone()) else { return };
                *self.identify_target.borrow_mut() = Some(id);
                self.run_identify(name);
                self.ui.set_zone(6);
            }
            Act::Hide(id) => {
                let Some((name, now_hidden)) = self.games.borrow().iter().find(|g| g.id == id).map(|g| (g.name.clone(), !g.hidden)) else { return };
                self.set_hidden(&id, now_hidden);
                self.ui.set_zone(back);
                self.push_notif(&tr(if now_hidden { "Jeu caché" } else { "Jeu affiché" }), &name);
            }
            Act::Uninstall(id) => {
                let Some(name) = self.games.borrow().iter().find(|g| g.id == id).map(|g| g.name.clone()) else { return };
                self.menu_show(format!("{} {name} ?", tr("Désinstaller")), vec![(tr("Confirmer"), Act::ConfirmUninstall(id)), (tr("Annuler"), Act::Cancel)]);
            }
            Act::ConfirmUninstall(id) => {
                self.ui.set_zone(back);
                if let Some(g) = self.gid_of(&id) {
                    self.uninstall(g);
                }
            }
            Act::AddGame => self.add_open(),
            Act::ShowHidden => self.show_hidden(),
            Act::DeleteProfile(i) => {
                self.ui.set_zone(back);
                self.profile_delete(i);
            }
            Act::CancelDl(i) => {
                self.ui.set_zone(back);
                self.dl_cancel(i);
            }
            Act::ConnectStore(i) => {
                self.ui.set_zone(back);
                self.connect_store(i);
            }
            Act::SelfUpdate => {
                self.ui.set_zone(back);
                self.hearth_update_install();
            }
            Act::UpdateYes(id) => {
                self.ui.set_zone(back);
                self.steam_install(&id);
            }
            Act::OfferStore(f) => self.set_offer_store_filter(f),
            Act::OfferSort(s) => self.set_offer_sort(s),
            Act::ConfirmBtUnpair => {
                self.ui.set_zone(back);
                if let Some((id, name)) = self.bt_unpair_id.borrow_mut().take() {
                    self.bt_unpair_do(id, name);
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

    pub(crate) fn uninstall(&self, gid: usize) {
        let Some(g) = self.games.borrow().get(gid).cloned() else { return };
        if g.store.is_empty() {
            // Jeu ajouté à la main : on le retire seulement de Hearth, ses fichiers restent.
            self.games.borrow_mut().remove(gid);
            // Tout index de tuile capturé ailleurs (jeu encapsulé en cours, jeu de boutique suivi,
            // guide ouvert, splash) doit être décalé pareil, sinon il pointe sur le jeu suivant dans
            // la liste après la suppression.
            for r in self.running.borrow_mut().iter_mut() {
                if r.tile > gid {
                    r.tile -= 1;
                }
            }
            if let Some(e) = self.ext_game.borrow_mut().as_mut() {
                if e.tile > gid {
                    e.tile -= 1;
                }
            }
            if self.guide_tile.get() > gid {
                self.guide_tile.set(self.guide_tile.get() - 1);
            }
            if self.splash_tile.get().is_some_and(|t| t > gid) {
                self.splash_tile.set(self.splash_tile.get().map(|t| t - 1));
            }
            self.save_games();
            self.rebuild();
            self.push_notif(&tr("Jeu retiré"), &g.name);
            return;
        }
        if std::path::Path::new(&g.exe) == stores::epiccmd::exe() {
            let app = g.id.strip_prefix("epic-").unwrap_or_default().to_string();
            // Même précaution que pour SteamCMD : ne pas désinstaller pendant qu'un téléchargement
            // actif pour ce même jeu écrit encore dans son dossier.
            let had_active_dl = self.dl.borrow().iter().any(|d| d.appid == format!("epic:{app}") && matches!(d.state, scmd::DlState::Active));
            if had_active_dl {
                stores::epiccmd::cancel(self.dl_pid.load(std::sync::atomic::Ordering::SeqCst));
            }
            self.push_notif(&g.name, &tr("Désinstallation en cours…"));
            std::thread::spawn(move || {
                // Hors du thread UI : laisse le temps à l'annulation de libérer ses fichiers sans geler l'interface.
                if had_active_dl {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
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
            let appid = g.id.strip_prefix("steam-").map(String::from);
            // Un téléchargement/mise à jour de ce jeu encore actif écrirait pendant qu'on supprime ses
            // fichiers (dossier recréé en partie juste après, manifeste corrompu) : on l'arrête d'abord.
            let had_active_dl = appid.as_deref().is_some_and(|a| self.dl.borrow().iter().any(|d| d.appid == a && matches!(d.state, scmd::DlState::Active)));
            if had_active_dl {
                stores::steamcmd::cancel(self.dl_pid.load(std::sync::atomic::Ordering::SeqCst));
            }
            self.push_notif(&g.name, &tr("Désinstallation en cours…"));
            let name = g.name.clone();
            std::thread::spawn(move || {
                // Hors du thread UI : la suppression des fichiers et l'attente de libération ne doivent pas geler l'interface.
                if had_active_dl {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                let ok = appid.as_deref().is_some_and(stores::steamcmd::uninstall);
                let _ = slint::invoke_from_event_loop(move || {
                    crate::with_shell(|s| {
                        if let Some(a) = &appid {
                            // Sinon la mise à jour reste proposée (notification, écran Téléchargements) jusqu'à
                            // la prochaine recherche programmée (jusqu'à 6 h) pour un jeu qui n'est déjà plus installé.
                            s.updates.borrow_mut().retain(|u| u != a);
                            s.prompted.borrow_mut().remove(a);
                        }
                        s.push_notif(&name, &tr(if ok { "Jeu désinstallé" } else { "Désinstallation impossible" }));
                        s.poll_installed();
                        s.dl_refresh();
                    })
                });
            });
            return;
        }
        let started = match g.store.as_str() {
            "Steam" =>g.id.strip_prefix("steam-").is_some_and(|id| { system::open_uri(&format!("steam://uninstall/{id}")); true }),
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
                system::open_uri("com.epicgames.launcher://store/library");
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
