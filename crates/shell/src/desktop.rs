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
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    unsafe { FindWindowW(None, &HSTRING::from(title)) }.is_ok_and(|h| !h.is_invalid())
}

impl Shell {
    pub fn desktop_open(&self) {
        if self.desktop.borrow().is_none() {
            self.desktop_create(false);
        }
        if let Some(w) = self.desktop.borrow().as_ref() {
            let _ = w.show();
            w.window().set_minimized(false);
            if let Some(h) = wm::find_window("Hearth Bureau") {
                wm::focus(h);
            }
        }
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
                let id = s.games.borrow().get(g as usize).map(|x| x.id.clone());
                if let Some(id) = id {
                    s.set_hidden(&id, true);
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
        w.on_rescan(|| {
            with_shell(|s| {
                s.poll_installed();
                s.desktop_message(&tr("Recherche en cours…"));
            })
        });
        w.on_open_console(|| {
            with_shell(|s| {
                if let Some(w) = s.desktop.borrow().as_ref() {
                    w.window().set_minimized(true);
                }
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
        if hide {
            // Jamais `hide()` : la fenêtre native serait détruite puis recréée (même panique OLE). On la réduit.
            w.window().set_minimized(true);
            let weak = w.as_weak();
            w.window().on_close_requested(move || {
                if let Some(w) = weak.upgrade() {
                    w.window().set_minimized(true);
                }
                slint::CloseRequestResponse::KeepWindowShown
            });
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
            if g.hidden || (f == 1 && g.install) || (f == 2 && !g.install) || (!q.is_empty() && !g.name.to_lowercase().contains(&q)) {
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
        let rows: Vec<SettingRow> = (0..self.settings.rows.row_count()).filter_map(|i| self.settings.rows.row_data(i)).map(|mut r| {
            r.label = tr(r.label.as_str()).into();
            r
        }).collect();
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

    /// « Ajouter un jeu local… » : choix de l'exécutable, titre deviné, visuels cherchés dans les boutiques.
    fn desktop_add_local(&self) {
        let Some(exe) = crate::native::open_file(&tr("Exécutable du jeu"), &format!("{} (*.exe)\0*.exe\0", tr("Applications"))) else { return };
        let title = crate::addgame::guess_title(&exe);
        let exe2 = exe.clone();
        let t2 = title.clone();
        std::thread::spawn(move || {
            let offer = stores::search(&t2).into_iter().next();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.add_local_game(exe2, offer)));
        });
        self.desktop_message(&format!("{} « {title} »…", tr("Recherche dans les boutiques :")));
    }

    fn add_local_game(&self, exe: std::path::PathBuf, offer: Option<stores::Offer>) {
        let name = offer.as_ref().map(|o| o.title.clone()).unwrap_or_else(|| crate::addgame::guess_title(&exe));
        let id = format!("custom-{:08x}", crate::name_hash(&exe.to_string_lossy()));
        let exists = self.games.borrow().iter().any(|g| g.id == id);
        if !exists {
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
