//! Mode souris à la manette PlayStation (connexion à un lanceur, navigateur…) : L3 + R3 l'active ou le quitte.
//! Stick gauche = curseur, Croix = clic, Rond = clic droit, stick droit = défilement, Triangle = clavier visuel.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows::Gaming::Input::{GameControllerSwitchPosition as Hat, RawGameController};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::Nav;

static MOUSE: AtomicBool = AtomicBool::new(false);
static OSK: AtomicBool = AtomicBool::new(false);

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

struct Raw {
    b: Vec<bool>,
    a: Vec<f64>,
}

fn read() -> Option<Raw> {
    let raw = RawGameController::RawGameControllers().ok()?.into_iter().find(|r| r.HardwareVendorId() == Ok(0x054C))?;
    let (nb, ns, na) = (raw.ButtonCount().ok()?, raw.SwitchCount().ok()?, raw.AxisCount().ok()?);
    let mut b = vec![false; nb as usize];
    let mut s = vec![Hat::Center; ns as usize];
    let mut a = vec![0.5; na as usize];
    raw.GetCurrentReading(&mut b, &mut s, &mut a).ok()?;
    Some(Raw { b, a })
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
            let on = |i: usize| r.b.get(i) == Some(&true);
            let both = on(10) && on(11);
            if both && !chord {
                let now = !active();
                set_active(now);
                if !now {
                    set_osk(false);
                }
                on_event(Nav::Mouse);
            }
            chord = both;
            if active() && !osk() {
                let (x, y) = (curve(r.a.first().copied().unwrap_or(0.5)), curve(r.a.get(1).copied().unwrap_or(0.5)));
                fx += x * 9.0;
                fy += y * 9.0;
                let (dx, dy) = (fx as i32, fy as i32);
                if dx != 0 || dy != 0 {
                    mouse(dx, dy, 0, MOUSEEVENTF_MOVE);
                    fx -= dx as f64;
                    fy -= dy as f64;
                }
                wheel += -curve(r.a.get(3).copied().unwrap_or(0.5)) * 18.0;
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
            thread::sleep(Duration::from_millis(6));
        }
    });
}

pub fn backspace() {
    key(VK_BACK);
}

pub fn enter() {
    key(VK_RETURN);
}
