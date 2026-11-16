#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use input::Nav;
use slint::{ComponentHandle, Model, SharedString, VecModel};

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
}

fn main() -> Result<(), slint::PlatformError> {
    // set_fullscreen() avant run() est ignoré par winit : plein écran dès la création.
    std::env::set_var("SLINT_FULLSCREEN", "1");
    let games = load_games();
    let tiles: Vec<Tile> = games
        .iter()
        .map(|g| Tile {
            name: g.name.as_str().into(),
            cover: slint::Image::load_from_path(Path::new(&g.cover)).unwrap_or_default(),
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
    });
    shell.ui.set_games(shell.tiles.clone().into());
    shell.guide.set_notes(shell.notes.clone().into());

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
            Err(e) => self.notify(format!("Impossible de lancer {} : {}", g.name, e.message())),
        }
    }

    fn on_nav(&self, nav: Nav) {
        if self.guide.window().is_visible() {
            if nav == Nav::Guide {
                self.guide_action(-1, 0);
            } else {
                self.guide.invoke_nav(nav as i32);
            }
            return;
        }
        if nav != Nav::Guide {
            self.ui.invoke_nav(nav as i32);
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
            None => self.home(),
        }
    }

    fn home(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            wm::focus(h);
        }
        self.ui.invoke_nav(Nav::Guide as i32);
    }

    fn open_guide(&self, tile: usize) {
        self.guide_tile.set(tile);
        let closing = self.running.borrow().iter().any(|r| r.tile == tile && r.closing);
        self.guide.set_game(self.games[tile].name.as_str().into());
        self.guide.set_closing(closing);
        self.guide.set_sel(0);
        self.guide.set_volume(system::audio::volume().map_or(-1, |v| v as i32));
        let scale = self.ui.window().scale_factor();
        let height = self.ui.window().size().height;
        self.guide.window().set_position(slint::PhysicalPosition::new(0, 0));
        self.guide.window().set_size(slint::PhysicalSize::new((GUIDE_WIDTH * scale) as u32, height));
        let _ = self.guide.show();
        slint::Timer::single_shot(Duration::from_millis(50), || {
            if let Some(h) = wm::find_window(GUIDE_TITLE) {
                wm::focus(h);
            }
        });
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
                let _ = self.guide.hide();
                if let Some(h) = game() {
                    wm::focus(h);
                }
            }
            (1, 0) => {
                let _ = self.guide.hide();
                if let Some(h) = game() {
                    wm::minimize(h);
                }
                self.home();
            }
            (2, 0) => {
                let _ = self.guide.hide();
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
        let ended: Vec<usize> = {
            let mut running = self.running.borrow_mut();
            let ended = running.iter().filter(|r| !r.session.alive()).map(|r| r.tile).collect();
            running.retain(|r| r.session.alive());
            ended
        };
        for &tile in &ended {
            self.set_running(tile, false);
            self.notify(format!("{} s'est fermé", self.games[tile].name));
        }
        if !ended.is_empty() {
            if ended.contains(&self.guide_tile.get()) {
                let _ = self.guide.hide();
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
        .map(|(exe, name)| Game { name: name.into(), exe: exe.into(), args: vec![], cover: String::new(), anticheat: false })
        .collect()
}
