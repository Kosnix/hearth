//! Flux « Ajouter un jeu » : sélecteur de fichiers, identification dans les boutiques, ajout à la bibliothèque.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{Model, VecModel};

use crate::i18n::tr;
use crate::{addgame, fetch_all, name_hash, with_shell, Game, PickItem, Shell};

pub enum PickMode {
    Root,
    Dir(PathBuf),
    Identify,
    Hidden,
}

fn item(label: &str, sub: &str, glyph: &str) -> PickItem {
    PickItem { label: label.into(), sub: sub.into(), glyph: glyph.into(), art: Default::default() }
}

impl Shell {
    pub fn save_games(&self) {
        let games = self.games.borrow();
        let manual: Vec<&Game> = games.iter().filter(|g| g.store.is_empty() && !g.id.starts_with("demo-")).collect();
        if let Ok(s) = serde_json::to_string_pretty(&manual) {
            let _ = std::fs::write(crate::games_file(), s);
        }
    }

    fn pick_show(&self, title: String, hint: String, items: Vec<PickItem>) {
        self.ui.set_pick_title(title.into());
        self.ui.set_pick_hint(hint.into());
        self.ui.set_psel(0);
        self.ui.set_pick_items(Rc::new(VecModel::from(items)).into());
    }

    pub fn add_open(&self) {
        self.show_browse(None);
        self.ui.set_zone(6);
    }

    fn show_browse(&self, dir: Option<PathBuf>) {
        let entries = match &dir {
            None => addgame::roots(),
            Some(d) => addgame::list_dir(d),
        };
        let mut items: Vec<PickItem> = vec![];
        if dir.is_some() {
            items.push(item(&tr("Dossier parent"), "", "\u{E74A}"));
        }
        items.extend(entries.iter().map(|e| item(&tr(&e.label), &e.sub, e.glyph)));
        let hint = match &dir {
            None => tr("Choisissez le dossier puis l'exécutable (.exe) du jeu"),
            Some(d) => d.display().to_string(),
        };
        *self.pick_mode.borrow_mut() = match dir {
            None => PickMode::Root,
            Some(d) => PickMode::Dir(d),
        };
        *self.pick_entries.borrow_mut() = entries;
        self.pick_show(tr("Ajouter un jeu"), hint, items);
    }

    /// Liste des jeux cachés : en choisir un le réaffiche.
    pub fn pick_hidden(&self, hidden: Vec<(String, String, String)>) {
        let items: Vec<PickItem> = hidden.iter().map(|(_, name, store)| item(name, if store.is_empty() { "" } else { store }, "\u{E7B3}")).collect();
        *self.pick_ids.borrow_mut() = hidden.into_iter().map(|(id, _, _)| id).collect();
        *self.pick_mode.borrow_mut() = PickMode::Hidden;
        self.pick_show(tr("Jeux cachés"), tr("Choisissez un jeu pour le réafficher"), items);
        self.ui.set_zone(6);
    }

    pub fn pick(&self, i: usize) {
        let mode = match &*self.pick_mode.borrow() {
            PickMode::Root => 0,
            PickMode::Dir(_) => 1,
            PickMode::Identify => 2,
            PickMode::Hidden => 3,
        };
        if mode == 3 {
            let Some(id) = self.pick_ids.borrow().get(i).cloned() else { return };
            self.set_hidden(&id, false);
            let left: Vec<(String, String, String)> = self.games.borrow().iter().filter(|g| g.hidden).map(|g| (g.id.clone(), g.name.clone(), g.store.clone())).collect();
            if left.is_empty() {
                self.ui.set_zone(0);
            } else {
                self.pick_hidden(left);
            }
            return;
        }
        if mode == 2 {
            self.identify_pick(i);
            return;
        }
        let has_parent = mode == 1;
        if has_parent && i == 0 {
            self.pick_back();
            return;
        }
        let idx = i - has_parent as usize;
        let Some((path, is_dir)) = self.pick_entries.borrow().get(idx).map(|e| (e.path.clone(), e.is_dir)) else { return };
        if is_dir {
            self.show_browse(Some(path));
        } else {
            self.start_identify(path);
        }
    }

    pub fn pick_back(&self) {
        // None : fermer ; Some(None) : racine ; Some(Some(dossier)) : dossier parent.
        let target: Option<Option<PathBuf>> = match &*self.pick_mode.borrow() {
            PickMode::Root => None,
            PickMode::Dir(d) => Some(d.parent().map(Path::to_path_buf).filter(|p| p.parent().is_some())),
            PickMode::Identify if self.identify_target.borrow().is_some() => None,
            PickMode::Identify => Some(self.identify_exe.borrow().as_ref().and_then(|e| e.parent().map(Path::to_path_buf))),
            PickMode::Hidden => None,
        };
        *self.identify_target.borrow_mut() = None;
        match target {
            None => self.ui.set_zone(0),
            Some(Some(p)) => self.show_browse(Some(p)),
            Some(None) => self.show_browse(None),
        }
    }

    fn start_identify(&self, exe: PathBuf) {
        let title = addgame::guess_title(&exe);
        *self.identify_exe.borrow_mut() = Some(exe);
        self.run_identify(title);
    }

    /// Cherche le jeu dans les boutiques pour récupérer son nom exact et ses visuels.
    pub fn run_identify(&self, title: String) {
        *self.pick_mode.borrow_mut() = PickMode::Identify;
        let gen = self.pick_gen.get() + 1;
        self.pick_gen.set(gen);
        self.pick_offers.borrow_mut().clear();
        self.pick_show(
            tr("Identifier le jeu"),
            format!("{} « {title} »", tr("Recherche dans les boutiques :")),
            vec![item(&tr("Recherche en cours…"), "", "\u{E721}")],
        );
        std::thread::spawn(move || {
            let mut results = stores::search(&title);
            results.truncate(8);
            let paths = fetch_all(&results);
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.identify_ready(gen, title, results, paths)));
        });
    }

    fn identify_ready(&self, gen: u32, title: String, results: Vec<stores::Offer>, paths: Vec<Option<PathBuf>>) {
        if self.pick_gen.get() != gen {
            return;
        }
        let mut items = vec![item(&tr("Saisir un autre nom…"), &format!("{} {title}", tr("Recherche actuelle :")), "\u{E70F}")];
        for (o, p) in results.iter().zip(&paths) {
            let mut it = item(&o.title, &format!("{} · {}", o.store, o.price), "\u{E7FC}");
            it.art = p.as_ref().and_then(|p| slint::Image::load_from_path(p).ok()).unwrap_or_default();
            items.push(it);
        }
        items.push(item(&format!("{} « {title} » {}", tr("Utiliser"), tr("sans visuels")), "", "\u{E8FB}"));
        *self.pick_offers.borrow_mut() = results;
        *self.guess_title.borrow_mut() = title;
        let (t, h) = (self.ui.get_pick_title().to_string(), self.ui.get_pick_hint().to_string());
        self.pick_show(t, h, items);
    }

    fn identify_pick(&self, i: usize) {
        let n = self.pick_offers.borrow().len();
        if i == 0 {
            self.kb_identify.set(true);
            self.settings.kb_open("");
            self.kb_refresh();
            self.ui.set_kbx(0);
            self.ui.set_kby(0);
            self.ui.set_kb_title(tr("Nom du jeu").into());
            self.ui.set_zone(4);
        } else if i <= n {
            let offer = self.pick_offers.borrow().get(i - 1).cloned();
            if self.identify_target.borrow().is_some() {
                self.finish_reart(offer);
            } else {
                self.finish_add(offer);
            }
        } else if self.identify_target.borrow().is_some() {
            self.finish_reart(None);
        } else {
            self.finish_add(None);
        }
    }

    /// Remplace les visuels d'un jeu existant par ceux de l'offre choisie.
    fn finish_reart(&self, offer: Option<stores::Offer>) {
        let Some(id) = self.identify_target.borrow_mut().take() else { return };
        self.ui.set_zone(0);
        let Some(o) = offer else { return };
        self.push_notif(&tr("Visuel"), &tr("Téléchargement du nouveau visuel…"));
        std::thread::spawn(move || {
            stores::clear_art(&id);
            if stores::save_art(&id, &o) {
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
            }
        });
    }

    /// Ajoute le jeu (nom et visuels de l'offre choisie) et ferme le sélecteur.
    fn finish_add(&self, offer: Option<stores::Offer>) {
        let Some(exe) = self.identify_exe.borrow().clone() else { return };
        let name = offer.as_ref().map(|o| o.title.clone()).unwrap_or_else(|| self.guess_title.borrow().clone());
        let id = format!("custom-{:08x}", name_hash(&exe.to_string_lossy()));
        let exists = self.games.borrow().iter().any(|g| g.id == id);
        if !exists {
            let game = Game { name: name.clone(), id: id.clone(), exe: exe.to_string_lossy().into_owned(), ..Default::default() };
            self.games.borrow_mut().push(game);
            self.save_games();
        } else {
            self.set_hidden(&id, false);
        }
        self.rebuild();
        if let Some(p) = self.tiles.iter().position(|t| self.games.borrow().get(t.gid as usize).is_some_and(|g| g.id == id)) {
            self.ui.set_sel(p as i32);
        }
        self.ui.set_tab(0);
        self.ui.set_top(0);
        self.ui.set_zone(0);
        self.push_notif(&tr("Jeu ajouté"), &name);
        if let Some(o) = offer {
            std::thread::spawn(move || {
                if stores::save_art(&id, &o) {
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
                }
            });
        }
    }
}
