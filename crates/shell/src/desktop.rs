//! Version bureau : fenêtre Windows classique (souris et clavier) pour gérer la bibliothèque, ajouter des jeux locaux,
//! relier les boutiques et régler Hearth. Ouverte depuis Réglages > À propos, le raccourci « Hearth (bureau) » ou `hearth-shell --desktop`.

use std::rc::Rc;

use slint::{ComponentHandle, Model, SharedString, VecModel};

use crate::i18n::tr;
use crate::{settings, with_shell, DeskGame, DeskStore, DesktopWindow, Game, SettingRow, Shell, Theme};

const STORES: [&str; 3] = ["Steam", "Epic Games", "GOG"];

/// Une demande d'ouverture déposée par un second lancement (`hearth-shell --desktop`) : lue par le shell déjà en cours.
pub fn request_file() -> std::path::PathBuf {
    stores::base_dir().join("desktop.request")
}

/// Une console Hearth (autre processus) est-elle déjà ouverte ? (`wm::find_window` ne regarde que notre processus.)
pub fn console_running(title: &str) -> bool {
    native_window(title).is_some()
}

/// Fenêtre native de ce titre, visible ou non, de n'importe quel processus.
fn native_window(title: &str) -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    unsafe { FindWindowW(None, &HSTRING::from(title)) }.ok().filter(|h| !h.is_invalid())
}

/// Masque ou affiche la fenêtre bureau sans la détruire (`hide()` de Slint la détruirait : OLE échouerait à la recréation).
fn set_desktop_visible(on: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_RESTORE};
    if let Some(h) = native_window(DESK_TITLE) {
        unsafe {
            let _ = ShowWindow(h, if on { SW_RESTORE } else { SW_HIDE });
        }
        if on {
            wm::focus(h);
        }
    }
}

const DESK_TITLE: &str = "Hearth Bureau";

impl Shell {
    pub fn desktop_open(&self) {
        if self.desktop.borrow().is_none() {
            self.desktop_create(false);
        }
        if let Some(w) = self.desktop.borrow().as_ref() {
            let _ = w.show();
        }
        set_desktop_visible(true);
        // La fenêtre native peut n'exister qu'au tour suivant de la boucle : on la remet devant une fois créée.
        slint::Timer::single_shot(std::time::Duration::from_millis(300), || set_desktop_visible(true));
        self.desktop_refresh_all();
    }

    /// Crée la fenêtre bureau (normale, pas plein écran). Appelée au démarrage : winit initialise OLE à la création
    /// de la fenêtre native, ce qui échoue (RPC_E_CHANGED_MODE) si du COM multithread a été initialisé depuis. Elle est donc créée puis masquée.
    pub fn desktop_create(&self, hide: bool) {
        let full = std::env::var_os("SLINT_FULLSCREEN");
        std::env::remove_var("SLINT_FULLSCREEN");
        let Ok(w) = DesktopWindow::new() else { return };
        self.theme_window(&w);
        w.set_ui_font(self.ui.get_ui_font());
        w.set_games(self.desk_games.clone().into());
        w.set_srows(self.desk_rows.clone().into());
        w.set_stores(self.desk_stores.clone().into());
        w.on_play(|g| with_shell(|s| s.launch(g as usize)));
        w.on_install(|g| {
            with_shell(|s| {
                let game = s.games.borrow().get(g as usize).cloned();
                if let Some(game) = game {
                    s.install_owned(&game);
                }
            })
        });
        w.on_hide_game(|g| {
            with_shell(|s| {
                let now = s.games.borrow().get(g as usize).map(|x| (x.id.clone(), !x.hidden));
                if let Some((id, hide)) = now {
                    s.set_hidden(&id, hide);
                }
            })
        });
        w.on_remove_game(|g| {
            with_shell(|s| {
                let name = s.games.borrow().get(g as usize).map(|x| x.name.clone()).unwrap_or_default();
                if crate::native::confirm(&format!("{} {name} ?", tr("Désinstaller"))) {
                    s.uninstall(g as usize);
                }
            })
        });
        w.on_query_changed(|q| {
            with_shell(|s| {
                *s.desk_query.borrow_mut() = q.to_string();
                s.desktop_refresh_games();
            })
        });
        w.on_filter_changed(|f| {
            with_shell(|s| {
                s.desk_filter.set(f);
                s.desktop_refresh_games();
            })
        });
        w.on_add_local(|| with_shell(|s| s.desktop_add_local()));
        w.on_rescan(|| with_shell(|s| s.import_games()));
        w.on_open_console(|| {
            with_shell(|_| {
                set_desktop_visible(false);
                if let Some(h) = wm::find_window("Hearth") {
                    wm::focus(h);
                }
            })
        });
        w.on_cat_open(|c| with_shell(|s| s.desktop_open_cat(c)));
        w.on_setting(|c, r, d| {
            with_shell(|s| {
                s.setting(c, r, d);
                s.desktop_sync_rows();
            })
        });
        w.on_store_connect(|i| {
            with_shell(|s| {
                if let Some(store) = STORES.get(i as usize) {
                    s.begin_login(store.to_string());
                }
            })
        });
        w.on_store_logout(|i| {
            with_shell(|s| {
                if let Some(store) = STORES.get(i as usize) {
                    s.logout(store);
                    s.desktop_refresh_stores();
                }
            })
        });
        w.on_login_cancel(|| {
            with_shell(|s| {
                s.login_cancel();
                s.desktop_login_close();
            })
        });
        w.on_login_submit(|code| {
            with_shell(|s| {
                s.login_exchange(code.to_string());
                s.desktop_login_close();
            })
        });
        let cats: Vec<SharedString> = settings::cats().into_iter().map(Into::into).collect();
        w.set_cats(Rc::new(VecModel::from(cats)).into());
        let _ = w.show();
        if let Some(f) = full {
            std::env::set_var("SLINT_FULLSCREEN", f);
        }
        // Jamais `hide()` : la fenêtre native serait détruite puis recréée (même panique OLE). On la
        // réduit, y compris quand c'est l'utilisateur qui clique sur la croix de fermeture native —
        // sinon cette fermeture-là échappe à `set_desktop_visible` et détruit la fenêtre pour de vrai :
        // `self.desktop` resterait `Some` d'un handle mort, et `desktop_open()` ne recréerait plus rien.
        w.window().on_close_requested(|| {
            set_desktop_visible(false);
            slint::CloseRequestResponse::KeepWindowShown
        });
        if hide {
            set_desktop_visible(false);
        }
        *self.desktop.borrow_mut() = Some(w);
    }

    fn theme_window(&self, w: &DesktopWindow) {
        let p = crate::theme::palette();
        let c = |v: [u8; 3]| slint::Color::from_rgb_u8(v[0], v[1], v[2]);
        let t = w.global::<Theme>();
        t.set_accent(c(p.accent));
        t.set_on_accent(c(p.on_accent));
        t.set_bg(c(p.bg));
        t.set_bg_settings(c(p.bg));
        t.set_band(c(p.band));
        t.set_tile(c(p.tile));
        t.set_bg_guide(c(p.guide));
        t.set_deep(c(p.deep));
        t.set_panel(slint::Color::from_argb_u8(0xf2, p.band[0], p.band[1], p.band[2]));
    }

    pub fn desktop_theme_changed(&self) {
        if let Some(w) = self.desktop.borrow().as_ref() {
            self.theme_window(w);
        }
    }

    pub fn desktop_refresh_all(&self) {
        self.desktop_refresh_games();
        self.desktop_refresh_stores();
        let cat = self.desktop.borrow().as_ref().map_or(0, |w| w.get_cat());
        self.desktop_open_cat(cat);
    }

    fn desktop_message(&self, m: &str) {
        if let Some(w) = self.desktop.borrow().as_ref() {
            w.set_message(m.into());
        }
    }

    /// Liste des jeux (filtre « tous / installés / à installer » et recherche).
    pub fn desktop_refresh_games(&self) {
        if self.desktop.borrow().is_none() {
            return;
        }
        let q = self.desk_query.borrow().to_lowercase();
        let f = self.desk_filter.get();
        let mut rows: Vec<(bool, String, DeskGame)> = vec![];
        for (i, g) in self.games.borrow().iter().enumerate() {
            // Un jeu caché reste listé (comme dans Bibliothèque en mode console) : sinon, caché depuis
            // le Bureau, plus aucun moyen de le réafficher depuis le Bureau.
            if (f == 1 && g.install) || (f == 2 && !g.install) || (!q.is_empty() && !g.name.to_lowercase().contains(&q)) {
                continue;
            }
            let running = self.running.borrow().iter().any(|r| r.tile == i);
            let cover = if g.install { self.lib_cover.borrow().get(&g.id).cloned().unwrap_or_default() } else { self.art_of(g).0 };
            let status = if running {
                tr("En cours")
            } else if g.install {
                tr("À installer")
            } else {
                tr("Installé")
            };
            rows.push((
                g.install,
                g.name.to_lowercase(),
                DeskGame {
                    gid: i as i32,
                    name: g.name.as_str().into(),
                    store: if g.store.is_empty() { tr("Local") } else { g.store.clone() }.into(),
                    status: status.into(),
                    installed: !g.install,
                    running,
                    hidden: g.hidden,
                    cover,
                    initial: g.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
                },
            ));
        }
        // Installés d'abord, puis par nom.
        rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        self.desk_games.set_vec(rows.into_iter().map(|r| r.2).collect::<Vec<_>>());
    }

    pub fn desktop_refresh_stores(&self) {
        if self.desktop.borrow().is_none() {
            return;
        }
        let st = self.store_status.borrow().clone();
        let rows: Vec<DeskStore> = STORES
            .iter()
            .map(|n| {
                let session = stores::account::session(n);
                let detected = st.iter().any(|(s, d, _)| s == n && *d);
                let (status, connected, can) = match (*n, session) {
                    (_, Some(a)) => (format!("{} · {}", tr("Connecté"), a.name), true, true),
                    ("GOG", None) => (if detected { tr("Bibliothèque locale") } else { tr("Client non installé") }, false, false),
                    (_, None) => (tr("Non connecté"), false, true),
                };
                DeskStore { name: (*n).into(), status: status.into(), connected, can_connect: can }
            })
            .collect();
        self.desk_stores.set_vec(rows);
    }

    pub fn desktop_open_cat(&self, cat: i32) {
        self.ui.set_cat(cat);
        let hint = self.settings.open(cat);
        if settings::needs_fetch(cat) {
            self.spawn_fetch(cat);
        }
        self.desktop_sync_rows_with(&hint);
    }

    pub fn desktop_sync_rows(&self) {
        let cat = self.ui.get_cat();
        let hint = self.settings.refresh(cat);
        self.desktop_sync_rows_with(&hint);
    }

    /// Copie les lignes des Réglages vers la fenêtre bureau (appelé aussi à chaque rafraîchissement d'une catégorie).
    pub fn desktop_sync_rows_with(&self, hint: &str) {
        let Some(w) = self.desktop.borrow().as_ref().map(|w| w.as_weak()) else { return };
        // Libellés déjà traduits par les Réglages (`row`).
        let rows: Vec<SettingRow> = (0..self.settings.rows.row_count()).filter_map(|i| self.settings.rows.row_data(i)).collect();
        self.desk_rows.set_vec(rows);
        if let Some(w) = w.upgrade() {
            w.set_shint(hint.into());
        }
    }

    /// QR code de connexion affiché dans la fenêtre bureau (en plus de l'écran de la console).
    pub fn desktop_show_login(&self, title: &str, qr: slint::Image, hint: &str, needs_code: bool) {
        if let Some(w) = self.desktop.borrow().as_ref() {
            w.set_qr_title(title.into());
            w.set_qr(qr);
            w.set_qr_hint(hint.into());
            w.set_qr_needs_code(needs_code);
            w.set_qr_code("".into());
            w.set_login_open(true);
        }
    }

    pub fn desktop_login_close(&self) {
        if let Some(w) = self.desktop.borrow().as_ref() {
            w.set_login_open(false);
        }
        self.desktop_refresh_stores();
    }

    /// Importe les jeux déjà présents : relit les boutiques (Steam, Epic, GOG) et ajoute les jeux « non-Steam » de Steam.
    pub fn import_games(&self) {
        self.poll_installed();
        let mut added: Vec<(String, String)> = vec![];
        for f in stores::steam_shortcuts() {
            let id = format!("custom-{:08x}", crate::name_hash(&f.exe.to_string_lossy()));
            if self.games.borrow().iter().any(|g| g.id == id) {
                continue;
            }
            self.drop_demos();
            self.games.borrow_mut().push(Game { name: f.title.clone(), id: id.clone(), exe: f.exe.to_string_lossy().into_owned(), args: f.args, ..Default::default() });
            added.push((id, f.title));
        }
        if !added.is_empty() {
            self.save_games();
            self.rebuild();
        }
        let n = added.len();
        let msg = if n == 0 { tr("Aucun nouveau jeu") } else { format!("{n} {}", tr("jeux ajoutés")) };
        self.push_notif(&tr("Import terminé"), &msg);
        self.desktop_message(&msg);
        // Visuels : identification par SteamGridDB, un jeu après l'autre.
        std::thread::spawn(move || {
            for (id, title) in added {
                if let Some(o) = crate::addgame::identify(&title).into_iter().next() {
                    if stores::save_art(&id, &o) {
                        let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
                    }
                }
            }
        });
    }

    /// « Ajouter un jeu local… » : choix de l'exécutable, titre deviné, visuels cherchés dans les boutiques.
    fn desktop_add_local(&self) {
        let Some(exe) = crate::native::open_file(&tr("Exécutable du jeu"), &format!("{} (*.exe)\0*.exe\0", tr("Applications"))) else { return };
        let title = crate::addgame::guess_title(&exe);
        let exe2 = exe.clone();
        let t2 = title.clone();
        std::thread::spawn(move || {
            let offer = crate::addgame::identify(&t2).into_iter().next();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.add_local_game(exe2, offer)));
        });
        self.desktop_message(&format!("{} « {title} »…", tr("Recherche dans les boutiques :")));
    }

    fn add_local_game(&self, exe: std::path::PathBuf, offer: Option<stores::Offer>) {
        let name = offer.as_ref().map(|o| o.title.clone()).unwrap_or_else(|| crate::addgame::guess_title(&exe));
        let id = format!("custom-{:08x}", crate::name_hash(&exe.to_string_lossy()));
        let exists = self.games.borrow().iter().any(|g| g.id == id);
        if !exists {
            self.drop_demos();
            self.games.borrow_mut().push(Game { name: name.clone(), id: id.clone(), exe: exe.to_string_lossy().into_owned(), ..Default::default() });
            self.save_games();
        } else {
            self.set_hidden(&id, false);
        }
        self.rebuild();
        self.push_notif(&tr("Jeu ajouté"), &name);
        self.desktop_message(&format!("{} : {name}", tr("Jeu local ajouté")));
        if let Some(o) = offer {
            std::thread::spawn(move || {
                if stores::save_art(&id, &o) {
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
                }
            });
        }
    }
}
