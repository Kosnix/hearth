//! Mode souris à la manette PlayStation (connexion à un lanceur, navigateur…) : L3 + R3 l'active ou le quitte.
//! Stick gauche = curseur, Croix = clic, Rond = clic droit, stick droit = défilement, Triangle = clavier visuel.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::Nav;

static MOUSE: AtomicBool = AtomicBool::new(false);
static OSK: AtomicBool = AtomicBool::new(false);
static CHORD: AtomicBool = AtomicBool::new(true);

/// L3 + R3 n'active le mode souris que si aucun jeu ne tourne (les jeux utilisent ces deux clics : sprint, mêlée…).
pub fn set_chord(on: bool) {
    CHORD.store(on, Ordering::Relaxed);
}

pub fn active() -> bool {
    MOUSE.load(Ordering::Relaxed)
}

pub fn set_active(on: bool) {
    MOUSE.store(on, Ordering::Relaxed);
}

/// Le clavier visuel est ouvert : la manette navigue dedans au lieu de piloter la souris.
pub fn osk() -> bool {
    OSK.load(Ordering::Relaxed)
}

pub fn set_osk(on: bool) {
    OSK.store(on, Ordering::Relaxed);
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
}

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) {
    send(&[INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, ..Default::default() } } }]);
}

/// Texte tapé dans la fenêtre active (caractères Unicode : accents et symboles compris).
pub fn send_text(text: &str) {
    for u in text.encode_utf16() {
        let k = |flags| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wScan: u, dwFlags: KEYEVENTF_UNICODE | flags, ..Default::default() } } };
        send(&[k(KEYBD_EVENT_FLAGS(0)), k(KEYEVENTF_KEYUP)]);
    }
}

fn key(vk: VIRTUAL_KEY) {
    let k = |flags| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } } };
    send(&[k(KEYBD_EVENT_FLAGS(0)), k(KEYEVENTF_KEYUP)]);
}

fn read() -> Option<crate::hidpad::SonyState> {
    crate::sony_states().into_iter().next()
}

/// Stick -1..1 avec zone morte, courbe progressive (précis au centre, rapide au bout).
fn curve(v: f64) -> f64 {
    let v = (v - 0.5) * 2.0;
    let m = v.abs();
    if m < 0.16 {
        0.0
    } else {
        v.signum() * ((m - 0.16) / 0.84).powf(2.0)
    }
}

/// `on_event` reçoit `Nav::Mouse` (mode changé) et `Nav::Osk` (demande du clavier visuel).
pub fn spawn(on_event: Arc<dyn Fn(Nav) + Send + Sync>) {
    thread::spawn(move || {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() };
        let (mut chord, mut left, mut right, mut tri) = (false, false, false, false);
        let (mut fx, mut fy, mut wheel) = (0.0f64, 0.0f64, 0.0f64);
        loop {
            let Some(r) = read() else {
                thread::sleep(Duration::from_millis(500));
                continue;
            };
            let on = |i: usize| r.buttons[i];
            let both = on(10) && on(11);
            if both && !chord && (CHORD.load(Ordering::Relaxed) || active()) {
                let now = !active();
                set_active(now);
                if !now {
                    set_osk(false);
                }
                on_event(Nav::Mouse);
            }
            chord = both;
            if active() && !osk() {
                let st = r.sticks;
                let (x, y) = (curve(st[0]), curve(st[1]));
                // Vitesse proportionnelle à la définition (référence 1920 px) : sinon le curseur rampait en 4K.
                let speed = 9.0 * (unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) }.max(1920) as f64 / 1920.0);
                fx += x * speed;
                fy += y * speed;
                let (dx, dy) = (fx as i32, fy as i32);
                if dx != 0 || dy != 0 {
                    mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                    fx -= dx as f64;
                    fy -= dy as f64;
                }
                wheel += -curve(st[3]) * 18.0;
                if wheel.abs() >= 1.0 {
                    mouse(0, 0, wheel as i32, MOUSEEVENTF_WHEEL);
                    wheel -= wheel.trunc();
                }
                for (held, down, up, cur) in [(&mut left, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, on(1)), (&mut right, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, on(2))] {
                    if cur != *held {
                        mouse(0, 0, 0, if cur { down } else { up });
                        *held = cur;
                    }
                }
                if on(3) && !tri {
                    on_event(Nav::Osk);
                }
                tri = on(3);
            } else {
                // Mode quitté ou clavier ouvert : boutons relâchés, pas de clic fantôme.
                if left {
                    mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
                    left = false;
                }
                if right {
                    mouse(0, 0, 0, MOUSEEVENTF_RIGHTUP);
                    right = false;
                }
                tri = on(3);
            }
            thread::sleep(Duration::from_millis(if active() { 6 } else { 30 }));
        }
    });
}

pub fn backspace() {
    key(VK_BACK);
}

pub fn enter() {
    key(VK_RETURN);
}

static KINDS: std::sync::Mutex<Vec<bool>> = std::sync::Mutex::new(Vec::new());

/// Sépare les axes d'une manette Sony sans dépendre de leur ordre : repos à 0 = gâchette, repos à 0,5 = stick
/// (sticks dans l'ordre gauche X, Y, droit X, Y ; gâchettes gauche, droite). Classement fait à la première lecture.
pub fn split(a: &[f64]) -> ([f64; 4], [f64; 2]) {
    let mut k = KINDS.lock().unwrap_or_else(|e| e.into_inner());
    if k.len() != a.len() {
        *k = a.iter().map(|v| *v < 0.1).collect();
    }
    let (mut s, mut t) = ([0.5; 4], [0.0; 2]);
    let (mut si, mut ti) = (0, 0);
    for (i, v) in a.iter().enumerate() {
        if k[i] {
            if ti < 2 {
                t[ti] = *v;
                ti += 1;
            }
        } else if si < 4 {
            s[si] = *v;
            si += 1;
        }
    }
    (s, t)
}
