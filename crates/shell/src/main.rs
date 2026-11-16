#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

slint::include_modules!();

/// Code de sortie lu par le superviseur : ouvrir le bureau Windows.
const EXIT_DESKTOP: i32 = 10;

#[derive(serde::Deserialize)]
struct Game {
    name: String,
    exe: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cover: String,
}

fn main() -> Result<(), slint::PlatformError> {
    // set_fullscreen() avant run() est ignoré par winit : plein écran dès la création.
    std::env::set_var("SLINT_FULLSCREEN", "1");
    let games = load_games();
    let ui = App::new()?;

    let tiles: Vec<Tile> = games
        .iter()
        .map(|g| Tile {
            name: g.name.as_str().into(),
            cover: slint::Image::load_from_path(Path::new(&g.cover)).unwrap_or_default(),
        })
        .collect();
    ui.set_games(Rc::new(slint::VecModel::from(tiles)).into());

    ui.on_launch(move |i| {
        if let Some(g) = games.get(i as usize) {
            let dir = Path::new(&g.exe).parent().filter(|d| d.is_dir());
            let mut cmd = Command::new(&g.exe);
            cmd.args(&g.args);
            if let Some(d) = dir {
                cmd.current_dir(d);
            }
            let _ = cmd.spawn();
        }
    });
    ui.on_power(|a| match a {
        0 => unsafe {
            let _ = windows::Win32::System::Power::SetSuspendState(false, false, false);
        },
        1 => drop(Command::new("shutdown").args(["/r", "/t", "0"]).spawn()),
        2 => drop(Command::new("shutdown").args(["/s", "/t", "0"]).spawn()),
        _ => {}
    });
    ui.on_desktop(|| std::process::exit(EXIT_DESKTOP));

    let weak = ui.as_weak();
    input::spawn(move |nav| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                if nav == input::Nav::Guide {
                    bring_to_front();
                }
                ui.invoke_nav(nav as i32);
            }
        });
    });

    ui.run()
}

fn games_file() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    Path::new(&base).join("Hearth").join("games.json")
}

// ponytail: liste manuelle en attendant les connecteurs de boutiques (phase 5).
fn load_games() -> Vec<Game> {
    if let Ok(s) = std::fs::read_to_string(games_file()) {
        if let Ok(games) = serde_json::from_str(&s) {
            return games;
        }
    }
    ["notepad.exe", "calc.exe", "mspaint.exe"]
        .into_iter()
        .zip(["Bloc-notes", "Calculatrice", "Paint"])
        .map(|(exe, name)| Game { name: name.into(), exe: exe.into(), args: vec![], cover: String::new() })
        .collect()
}

/// Windows refuse SetForegroundWindow à un processus en arrière-plan, sauf s'il vient
/// d'injecter une entrée : on maintient Alt le temps de l'appel.
// ponytail: recherche par titre ; remplacée par le gestionnaire de fenêtres en phase 2.
fn bring_to_front() {
    use windows::core::w;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let Ok(hwnd) = FindWindowW(None, w!("Hearth")) else { return };
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, dwFlags: flags, ..Default::default() } },
        };
        SendInput(&[key(KEYBD_EVENT_FLAGS(0))], size_of::<INPUT>() as i32);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        SendInput(&[key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
    }
}
