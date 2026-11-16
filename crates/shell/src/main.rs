#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use input::Nav;
use i18n::tr;
use slint::{ComponentHandle, Model, SharedString, VecModel};

mod i18n;
mod settings;

slint::include_modules!();

/// Code de sortie lu par le superviseur : ouvrir le bureau Windows.
const EXIT_DESKTOP: i32 = 10;
const MAIN_TITLE: &str = "Hearth";
const GUIDE_TITLE: &str = "Hearth Guide";
const GUIDE_WIDTH: f32 = 460.0; // px logiques
const MAX_NOTES: usize = 5;

#[derive(serde::Deserialize)]
struct Game {
    name: String,
    #[serde(default)]
    id: String,
    exe: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cover: String,
    #[serde(default)]
    anticheat: bool,
}

/// Jeu en cours, lancé depuis la tuile `tile`.
struct Running {
    tile: usize,
    session: wm::Session,
    hands_off: bool, // anti-triche ou plein écran sans bordure désactivé
    closing: bool,
}

struct Shell {
    ui: App,
    guide: GuideMenu,
    games: Vec<Game>,
    tiles: Rc<VecModel<Tile>>,
    notes: Rc<VecModel<SharedString>>,
    running: RefCell<Vec<Running>>,
    guide_tile: Cell<usize>,
    settings: Rc<settings::Settings>,
}

fn main() -> Result<(), slint::PlatformError> {
    // Le shell n'a pas de console : les paniques vont dans un fichier.
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(games_file().with_file_name("panic.log"), info.to_string());
    }));
    // set_fullscreen() avant run() est ignoré par winit : plein écran dès la création.
    std::env::set_var("SLINT_FULLSCREEN", "1");
    // ponytail: femtovg cesse de se redessiner avec ce design (constaté sur l'Optiplex) ; rendu logiciel.
    std::env::set_var("SLINT_BACKEND", "winit-software");
    let games = load_games();
    let tiles: Vec<Tile> = games
        .iter()
        .map(|g| Tile {
            name: g.name.as_str().into(),
            initial: g.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
            hue: hue_of(&g.name),
            cover: cover_of(g),
            running: false,
        })
        .collect();
    let shell = Rc::new(Shell {
        ui: App::new()?,
        guide: GuideMenu::new()?,
        games,
        tiles: Rc::new(VecModel::from(tiles)),
        notes: Rc::new(VecModel::default()),
        running: RefCell::new(Vec::new()),
        guide_tile: Cell::new(0),
        settings: settings::Settings::new(),
    });
    shell.ui.set_games(shell.tiles.clone().into());
    shell.guide.set_notes(shell.notes.clone().into());
    i18n::init();
    shell.refresh_lang();
    shell.ui.set_rows(shell.settings.rows.clone().into());
    shell.ui.set_user(std::env::var("USERNAME").unwrap_or_default().into());
    shell.set_clock();

    let s = shell.clone();
    shell.ui.on_open_cat(move|c| s.open_cat(c));
    let s = shell.clone();
    shell.ui.on_setting(move|c, r, d| s.setting(c, r, d));
    let s = shell.clone();
    shell.ui.on_kb_press(move |x, y| s.kb_press(x, y));

    let s = shell.clone();
    shell.ui.on_launch(move |i| s.launch(i as usize));
    shell.ui.on_power(|a| match a {
        0 => unsafe {
            let _ = windows::Win32::System::Power::SetSuspendState(false, false, false);
        },
        1 => drop(Command::new("shutdown").args(["/r", "/t", "0"]).spawn()),
        2 => drop(Command::new("shutdown").args(["/s", "/t", "0"]).spawn()),
        _ => {}
    });
    shell.ui.on_desktop(|| std::process::exit(EXIT_DESKTOP));
    let s = shell.clone();
    shell.ui.on_open_store(move |_| s.notify(tr("Bientôt disponible")));
    let s = shell.clone();
    shell.guide.on_action(move |item, delta| s.guide_action(item, delta));

    let s = shell.clone();
    wm::watch(move |h| {
        let Ok(running) = s.running.try_borrow() else { return };
        if running.iter().any(|r| !r.hands_off && r.session.owns(h)) && !wm::exclusive_fullscreen() {
            wm::make_borderless(h);
        }
    });

    let s = shell.clone();
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || s.tick());

    let s = shell.clone();
    let phase = Cell::new(0.0f32);
    let anim = slint::Timer::default();
    anim.start(slint::TimerMode::Repeated, Duration::from_millis(160), move || s.animate(&phase));

    // Le menu guide est créé une fois puis affiché/masqué en Win32, sans jamais prendre le focus
    // (un jeu plein écran qui perd le focus se réduit et clignote).
    shell.guide.show()?;
    // Les fenêtres natives n'existent pas forcément dès le premier tour : on réessaie.
    let setup = Rc::new(slint::Timer::default());
    let t = setup.clone();
    setup.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            wm::make_overlay(h);
            wm::hide(h);
            if let Some(m) = wm::find_window(MAIN_TITLE) {
                wm::focus(m);
                // Dev : un ancien shell élevé peut masquer le nôtre ; HEARTH_DEV_TOPMOST le recouvre.
                if std::env::var_os("HEARTH_DEV_TOPMOST").is_some() {
                    let (x, y, w, h) = wm::primary_monitor();
                    wm::show_overlay(m, x, y, w, h);
                }
            }
            t.stop();
        }
    });

    // Rc n'est pas Send : le thread manette retrouve le Shell via un thread_local du thread UI.
    SHELL.with(|s| *s.borrow_mut() = Some(shell.clone()));
    input::spawn(|nav| {
        let _ = slint::invoke_from_event_loop(move || {
            SHELL.with(|s| {
                if let Some(s) = s.borrow().as_ref() {
                    s.on_nav(nav);
                }
            });
        });
    });

    shell.ui.run()
}

thread_local! {
    static SHELL: RefCell<Option<Rc<Shell>>> = const { RefCell::new(None) };
}

impl Shell {
    fn launch(&self, i: usize) {
        if let Some(h) = self.running.borrow().iter().find(|r| r.tile == i).and_then(|r| r.session.main_window()) {
            wm::focus(h);
            return;
        }
        let Some(g) = self.games.get(i) else { return };
        let exe = Path::new(&g.exe);
        let compat = wm::compat::lookup(exe);
        compat.apply_ini();
        let args: Vec<String> = g.args.iter().chain(&compat.args).cloned().collect();
        match wm::Session::launch(exe, &args) {
            Ok(session) => {
                let hands_off = g.anticheat || compat.anticheat || !compat.borderless;
                self.running.borrow_mut().push(Running { tile: i, session, hands_off, closing: false });
                self.set_running(i, true);
            }
            Err(e) => self.notify(format!("{} {} : {}", tr("Impossible de lancer"), g.name, e.message())),
        }
    }

    fn on_nav(&self, nav: Nav) {
        if wm::find_window(GUIDE_TITLE).is_some_and(wm::is_visible) {
            if nav == Nav::Guide {
                self.guide_action(-1, 0);
            } else {
                self.guide.invoke_nav(nav as i32);
            }
            return;
        }
        if nav != Nav::Guide {
            // La manette est lue même en arrière-plan : n'agir que si la console est visible
            // au premier plan, sinon on navigue à l'aveugle (jusqu'à « Veille »…).
            if wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
                self.ui.invoke_nav(nav as i32);
            }
            return;
        }
        let fg = wm::foreground();
        let target = self.running.borrow().iter().find(|r| r.session.owns(fg)).map(|r| (r.tile, r.hands_off));
        match target {
            // Jeu protégé ou plein écran exclusif : pas d'overlay, retour propre au shell.
            Some((_, true)) => {
                wm::minimize(fg);
                self.home();
            }
            Some(_) if wm::exclusive_fullscreen() => {
                wm::minimize(fg);
                self.home();
            }
            Some((tile, false)) => self.open_guide(tile),
            None => {
                // Depuis la console, avec un jeu en arrière-plan : son menu (le seul moyen de
                // fermer un jeu en plein écran exclusif, qui n'a pas d'overlay).
                let in_console = wm::find_window(MAIN_TITLE).is_some_and(|h| h == fg);
                let last = self.running.borrow().last().map(|r| r.tile);
                match last {
                    Some(tile) if in_console => self.open_guide(tile),
                    _ => self.home(),
                }
            }
        }
    }

    fn open_cat(&self, cat: i32) {
        let hint = self.settings.open(cat);
        self.ui.set_shint(hint.into());
        if settings::needs_fetch(cat) {
            self.spawn_fetch(cat);
        }
    }

    /// Lecture WinRT hors du thread UI, résultat renvoyé au thread UI.
    fn spawn_fetch(&self, cat: i32) {
        std::thread::spawn(move || {
            let f = settings::fetch(cat);
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.fetched(cat, f)));
        });
    }

    fn fetched(&self, cat: i32, f: settings::Fetched) {
        self.settings.apply(f);
        if self.ui.get_cat() == cat {
            self.open_cat_keep(cat);
        }
    }

    fn setting(&self, cat: i32, row: i32, delta: i32) {
        match self.settings.act(cat, row, delta) {
            settings::Outcome::DisplayChanged => self.display_changed(),
            settings::Outcome::Toggle { wifi, on } => {
                self.settings.start_loading();
                std::thread::spawn(move || {
                    if wifi {
                        system::radio::set_wifi(on);
                    } else {
                        system::radio_ext::set_bluetooth(on);
                    }
                    let f = settings::fetch(cat);
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.fetched(cat, f)));
                });
            }
            settings::Outcome::Password(ssid) => {
                self.settings.kb_open(&ssid);
                self.kb_refresh();
                self.ui.set_kbx(0);
                self.ui.set_kby(0);
                self.ui.set_kb_title(format!("{} {ssid}", tr("Mot de passe :")).into());
                self.ui.set_zone(4);
            }
            settings::Outcome::Connect(ssid) => self.connect(ssid, None),
            settings::Outcome::LangChanged => self.refresh_lang(),
            settings::Outcome::CheckUpdates(drivers) => {
                std::thread::spawn(move || {
                    let r = if drivers { system::updates::pending_drivers() } else { system::updates::pending() };
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            s.settings.set_update(drivers, r);
                            s.open_cat_keep(if drivers { 8 } else { 6 });
                        })
                    });
                });
            }
            settings::Outcome::None => {}
        }
        self.open_cat_keep(cat);
    }

    fn kb_refresh(&self) {
        self.ui.set_kb_text(self.settings.kb_text().into());
        let keys: Vec<SharedString> = self.settings.kb_keys().into_iter().map(Into::into).collect();
        self.ui.set_kb_keys(Rc::new(VecModel::from(keys)).into());
    }

    fn kb_press(&self, x: i32, y: i32) {
        match self.settings.kb_press(x, y) {
            settings::Kb::Continue => self.kb_refresh(),
            settings::Kb::Cancel => self.ui.set_zone(3),
            settings::Kb::Done(ssid, pw) => {
                self.ui.set_zone(3);
                self.connect(ssid, Some(pw));
            }
        }
    }

    fn connect(&self, ssid: String, password: Option<String>) {
        self.settings.set_message(format!("{} {ssid}…", tr("Connexion à")));
        self.open_cat_keep(2);
        std::thread::spawn(move || {
            let ok = system::wifi::connect(&ssid, password.as_deref());
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.settings.set_message(format!("{} {ssid}", tr(if ok { "Connecté à" } else { "Échec de la connexion à" })));
                    s.settings.start_loading();
                    s.open_cat_keep(2);
                    s.spawn_fetch(2);
                })
            });
        });
    }

    /// Rafraîchit les lignes sans recharger les données (garde les choix en cours).
    fn open_cat_keep(&self, cat: i32) {
        let hint = self.settings.refresh(cat);
        self.ui.set_shint(hint.into());
        let max = self.settings.row_count().saturating_sub(1) as i32;
        self.ui.set_srow(self.ui.get_srow().min(max));
    }

    fn display_changed(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            wm::fit_to_monitor(h);
        }
    }

    fn set_clock(&self) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        self.ui.set_clock(format!("{:02}:{:02}", t.wHour, t.wMinute).into());
    }

    fn home(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            // Un jeu qui a changé la résolution a pu rétrécir notre fenêtre : on la recale.
            wm::fit_to_monitor(h);
            wm::focus(h);
        }
        self.ui.invoke_nav(Nav::Guide as i32);
    }

    fn open_guide(&self, tile: usize) {
        self.guide_tile.set(tile);
        let closing = self.running.borrow().iter().any(|r| r.tile == tile && r.closing);
        self.guide.set_game(self.games[tile].name.as_str().into());
        self.guide.set_art(self.tiles.row_data(tile).map(|t| t.cover).unwrap_or_default());
        self.guide.set_closing(closing);
        self.guide.set_sel(0);
        self.guide.set_volume(system::audio::volume().map_or(-1, |v| v as i32));
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            let (x, y, _, height) = wm::primary_monitor();
            let width = (GUIDE_WIDTH * self.ui.window().scale_factor()) as i32;
            wm::show_overlay(h, x, y, width, height);
            self.guide.window().request_redraw();
        }
        // DDC/CI et WinRT sont lents : lecture en arrière-plan.
        let g = self.guide.as_weak();
        std::thread::spawn(move || {
            let b = system::display::brightness().map_or(-1, |v| v as i32);
            let w = system::radio::wifi_on().map_or(-1, |on| on as i32);
            let _ = g.upgrade_in_event_loop(move |g| {
                g.set_brightness(b);
                g.set_wifi(w);
            });
        });
    }

    fn guide_action(&self, item: i32, delta: i32) {
        let tile = self.guide_tile.get();
        let game = || self.running.borrow().iter().find(|r| r.tile == tile).and_then(|r| r.session.main_window());
        match (item, delta) {
            (-1, _) | (0, 0) => {
                self.hide_guide();
                if let Some(h) = game() {
                    wm::focus(h);
                }
            }
            (1, 0) => {
                self.hide_guide();
                if let Some(h) = game() {
                    wm::minimize(h);
                }
                self.home();
            }
            (2, 0) => {
                self.hide_guide();
                let mut running = self.running.borrow_mut();
                if let Some(r) = running.iter_mut().find(|r| r.tile == tile) {
                    if r.closing {
                        r.session.kill();
                    } else {
                        r.closing = true;
                        r.session.close();
                    }
                }
                drop(running);
                // Laisse le jeu afficher sa propre confirmation éventuelle.
                if let Some(h) = game() {
                    wm::focus(h);
                }
            }
            (3, d) if d != 0 => {
                let v = (self.guide.get_volume() + d * 5).clamp(0, 100);
                system::audio::set_volume(v as u32);
                self.guide.set_volume(system::audio::volume().map_or(-1, |v| v as i32));
            }
            (4, d) if d != 0 && self.guide.get_brightness() >= 0 => {
                let b = (self.guide.get_brightness() + d * 10).clamp(0, 100);
                self.guide.set_brightness(b);
                std::thread::spawn(move || system::display::set_brightness(b as u32));
            }
            (5, _) if self.guide.get_wifi() >= 0 => {
                let on = self.guide.get_wifi() == 0;
                self.guide.set_wifi(on as i32);
                std::thread::spawn(move || system::radio::set_wifi(on));
            }
            _ => {}
        }
    }

    /// Chaque seconde : jeux terminés, et ré-application du sans-bordure (certains jeux le retirent).
    fn tick(&self) {
        self.set_clock();
        if self.settings.tick() {
            self.display_changed();
            self.open_cat_keep(self.ui.get_cat());
        } else if self.settings.confirming() {
            self.open_cat_keep(self.ui.get_cat());
        }
        let ended: Vec<usize> = {
            let mut running = self.running.borrow_mut();
            let ended = running.iter().filter(|r| !r.session.alive()).map(|r| r.tile).collect();
            running.retain(|r| r.session.alive());
            ended
        };
        for &tile in &ended {
            self.set_running(tile, false);
            self.notify(format!("{} {}", self.games[tile].name, tr("s'est fermé")));
        }
        if !ended.is_empty() {
            if ended.contains(&self.guide_tile.get()) {
                self.hide_guide();
            }
            let fg = wm::foreground();
            if !self.running.borrow().iter().any(|r| r.session.owns(fg)) {
                self.home();
            }
        }
        let fg = wm::foreground();
        let fix = self.running.borrow().iter().any(|r| !r.hands_off && r.session.owns(fg));
        if fix && wm::is_main_window(fg) && !wm::exclusive_fullscreen() {
            wm::make_borderless(fg);
        }
    }

    fn hide_guide(&self) {
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            wm::hide(h);
        }
    }

    fn set_running(&self, tile: usize, on: bool) {
        if let Some(mut t) = self.tiles.row_data(tile) {
            t.running = on;
            self.tiles.set_row_data(tile, t);
        }
    }

    fn notify(&self, msg: String) {
        self.notes.insert(0, msg.into());
        if self.notes.row_count() > MAX_NOTES {
            self.notes.remove(MAX_NOTES);
        }
    }
}

fn games_file() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    Path::new(&base).join("Hearth").join("games.json")
}

// ponytail: liste manuelle en attendant les connecteurs de boutiques (phase 5).
fn load_games() -> Vec<Game> {
    if let Ok(s) = std::fs::read_to_string(games_file()) {
        // Notepad et PowerShell ajoutent souvent un BOM, que serde_json refuse.
        if let Ok(games) = serde_json::from_str(s.trim_start_matches('\u{feff}')) {
            return games;
        }
    }
    ["notepad.exe", "calc.exe", "mspaint.exe"]
        .into_iter()
        .zip(["Bloc-notes", "Calculatrice", "Paint"])
        .map(|(exe, name)| Game { name: name.into(), id: String::new(), exe: exe.into(), args: vec![], cover: String::new(), anticheat: false })
        .collect()
}

fn with_shell(f: impl FnOnce(&Shell)) {
    SHELL.with(|s| {
        if let Some(s) = s.borrow().as_ref() {
            f(s);
        }
    });
}

/// Visuel d'une tuile : `cover` du jeu, sinon assets\<id>\cover.* (cf. crate `stores`).
fn cover_of(g: &Game) -> slint::Image {
    let path = if g.cover.is_empty() { stores::asset(&g.id, "cover").unwrap_or_default() } else { PathBuf::from(&g.cover) };
    slint::Image::load_from_path(&path).unwrap_or_default()
}

impl Shell {
    /// Textes calculés en Rust après un changement de langue (les @tr de Slint suivent seuls).
    fn refresh_lang(&self) {
        let cats: Vec<SharedString> = settings::cats().into_iter().map(Into::into).collect();
        self.ui.set_cats(Rc::new(VecModel::from(cats)).into());
        let stores: Vec<StoreEntry> = stores::all()
            .iter()
            .map(|s| StoreEntry { name: s.name().into(), status: tr(if s.status() == stores::Status::Soon { "Bientôt disponible" } else { "Non connecté" }).into() })
            .collect();
        self.ui.set_stores(Rc::new(VecModel::from(stores)).into());
        // Pas avant la création de la fenêtre : COM multithread sur ce thread ferait échouer winit.
        if self.ui.get_zone() == 3 {
            self.open_cat_keep(self.ui.get_cat());
        }
    }

    /// Fond animé (arrêté quand la console n'est pas devant : économise le CPU en jeu) et pictogrammes.
    fn animate(&self, phase: &Cell<f32>) {
        let pad = input::last_pad() as i32;
        if self.ui.global::<Pad>().get_kind() != pad {
            self.ui.global::<Pad>().set_kind(pad);
            self.guide.global::<Pad>().set_kind(pad);
        }
        if i18n::anim() && self.ui.get_zone() < 3 && wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
            phase.set((phase.get() + 0.8) % 3600.0);
            self.ui.set_phase(phase.get());
        }
    }
}

/// Teinte stable par jeu (bleus/violets/turquoises, en harmonie avec le thème) pour les tuiles sans visuel.
fn hue_of(name: &str) -> slint::Color {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let hue = 190.0 + (h % 120) as f32; // 190..310°
    let (s, v) = (0.55f32, 0.85f32);
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 {
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    slint::Color::from_rgb_u8(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}
