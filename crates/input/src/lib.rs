//! Manette -> actions de navigation. Windows.Gaming.Input + XInput (bouton guide, repli).

use std::thread;
use std::time::{Duration, Instant};
use windows::core::{w, PCSTR};
use windows::Gaming::Input::{GameControllerSwitchPosition as Hat, Gamepad, GamepadButtons, RawGameController};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};

/// L'ordre (valeurs entières) est partagé avec l'UI Slint (`nav(int)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Guide,
    Menu,
}

// Masques au format XInput (wButtons).
const UP: u16 = 0x0001;
const DOWN: u16 = 0x0002;
const LEFT: u16 = 0x0004;
const RIGHT: u16 = 0x0008;
const START: u16 = 0x0010;
const GUIDE: u16 = 0x0400;
const A: u16 = 0x1000;
const B: u16 = 0x2000;
const DIRS: u16 = UP | DOWN | LEFT | RIGHT;

const MAP: [(u16, Nav); 8] = [
    (UP, Nav::Up),
    (DOWN, Nav::Down),
    (LEFT, Nav::Left),
    (RIGHT, Nav::Right),
    (A, Nav::Accept),
    (B, Nav::Back),
    (GUIDE, Nav::Guide),
    (START, Nav::Menu),
];

const POLL: Duration = Duration::from_millis(10);
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_RATE: Duration = Duration::from_millis(120);
const STICK: f64 = 0.5; // seuil de déviation du stick, 0..1

type GetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// Lance le thread de lecture ; `on_nav` est appelé depuis ce thread.
pub fn spawn(on_nav: impl Fn(Nav) + Send + 'static) {
    thread::spawn(move || {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() };
        let get_state = xinput_get_state_ex().unwrap_or(xinput_get_state);
        let mut xinput = XInput { get_state, connected: [true; 4], tick: 0 };
        let mut prev = 0u16;
        let mut next_repeat = Instant::now();
        loop {
            let cur = read_wgi() | read_sony() | xinput.read();
            let pressed = cur & !prev;
            for (bit, nav) in MAP {
                if pressed & bit != 0 {
                    on_nav(nav);
                }
            }
            let now = Instant::now();
            if pressed & DIRS != 0 {
                next_repeat = now + REPEAT_DELAY;
            } else if cur & DIRS != 0 && now >= next_repeat {
                for (bit, nav) in &MAP[..4] {
                    if cur & bit != 0 {
                        on_nav(*nav);
                    }
                }
                next_repeat = now + REPEAT_RATE;
            }
            prev = cur;
            thread::sleep(POLL);
        }
    });
}

/// XInputGetStateEx (ordinal 100, non documenté) expose le bouton guide.
fn xinput_get_state_ex() -> Option<GetState> {
    unsafe {
        let dll = LoadLibraryW(w!("xinput1_4.dll")).ok()?;
        let f = GetProcAddress(dll, PCSTR(100 as *const u8))?;
        Some(std::mem::transmute::<_, GetState>(f))
    }
}

unsafe extern "system" fn xinput_get_state(i: u32, s: *mut XINPUT_STATE) -> u32 {
    XInputGetState(i, s)
}

fn read_wgi() -> u16 {
    let Ok(pads) = Gamepad::Gamepads() else { return 0 };
    let mut bits = 0;
    for pad in pads {
        let Ok(r) = pad.GetCurrentReading() else { continue };
        let b = r.Buttons;
        for (flag, bit) in [
            (GamepadButtons::DPadUp, UP),
            (GamepadButtons::DPadDown, DOWN),
            (GamepadButtons::DPadLeft, LEFT),
            (GamepadButtons::DPadRight, RIGHT),
            (GamepadButtons::A, A),
            (GamepadButtons::B, B),
            (GamepadButtons::Menu, START),
        ] {
            if b.0 & flag.0 != 0 {
                bits |= bit;
            }
        }
        bits |= stick(r.LeftThumbstickX, r.LeftThumbstickY);
    }
    bits
}

const SONY: u16 = 0x054C;

/// DualShock 4 / DualSense (HID) : boutons 0 Carré, 1 Croix, 2 Rond, 9 Options, 12 PS ; chapeau 0 = croix ; axes 0/1 = stick gauche (0..1).
// ponytail: seul le schéma Sony est mappé ; autres manettes HID en phase 4.
fn read_sony() -> u16 {
    let Ok(raws) = RawGameController::RawGameControllers() else { return 0 };
    let mut bits = 0;
    for raw in raws {
        if raw.HardwareVendorId() != Ok(SONY) {
            continue;
        }
        let (Ok(nb), Ok(ns), Ok(na)) = (raw.ButtonCount(), raw.SwitchCount(), raw.AxisCount()) else { continue };
        let mut b = vec![false; nb as usize];
        let mut s = vec![Hat::Center; ns as usize];
        let mut a = vec![0.5; na as usize];
        if raw.GetCurrentReading(&mut b, &mut s, &mut a).is_err() {
            continue;
        }
        for (i, bit) in [(1, A), (2, B), (9, START), (12, GUIDE)] {
            if b.get(i) == Some(&true) {
                bits |= bit;
            }
        }
        if let Some(h) = s.first() {
            bits |= hat(*h);
        }
        if a.len() >= 2 {
            bits |= stick((a[0] - 0.5) * 2.0, (0.5 - a[1]) * 2.0);
        }
    }
    bits
}

fn hat(h: Hat) -> u16 {
    match h {
        Hat::Up => UP,
        Hat::UpRight => UP | RIGHT,
        Hat::Right => RIGHT,
        Hat::DownRight => DOWN | RIGHT,
        Hat::Down => DOWN,
        Hat::DownLeft => DOWN | LEFT,
        Hat::Left => LEFT,
        Hat::UpLeft => UP | LEFT,
        _ => 0,
    }
}

struct XInput {
    get_state: GetState,
    connected: [bool; 4],
    tick: u32,
}

impl XInput {
    fn read(&mut self) -> u16 {
        // Interroger un emplacement vide est lent : on ne le refait qu'une fois par seconde.
        self.tick = self.tick.wrapping_add(1);
        let retry = self.tick % 100 == 0;
        let mut bits = 0;
        for i in 0..4 {
            if !self.connected[i] && !retry {
                continue;
            }
            let mut st = XINPUT_STATE::default();
            self.connected[i] = unsafe { (self.get_state)(i as u32, &mut st) } == 0;
            if self.connected[i] {
                let g = st.Gamepad;
                bits |= g.wButtons.0 & (DIRS | START | GUIDE | A | B);
                bits |= stick(g.sThumbLX as f64 / 32767.0, g.sThumbLY as f64 / 32767.0);
            }
        }
        bits
    }
}

fn stick(x: f64, y: f64) -> u16 {
    let mut bits = 0;
    if y > STICK { bits |= UP }
    if y < -STICK { bits |= DOWN }
    if x < -STICK { bits |= LEFT }
    if x > STICK { bits |= RIGHT }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_thresholds() {
        assert_eq!(stick(0.0, 0.0), 0);
        assert_eq!(stick(0.9, 0.0), RIGHT);
        assert_eq!(stick(-0.9, 0.9), LEFT | UP);
        assert_eq!(stick(0.3, -0.3), 0);
    }
}
