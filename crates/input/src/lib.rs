//! Manette -> actions de navigation. Windows.Gaming.Input + XInput (bouton guide, repli).

use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};
mod mouse;
mod xpad;

pub fn set_capture(on: bool) {
    xpad::set_capture(on);
}

pub use mouse::{set_chord, active as mouse_active, osk as osk_open, backspace, enter, send_text, set_active as set_mouse_active, set_osk};

use windows::core::{w, PCSTR};
use windows::Gaming::Input::{GameControllerSwitchPosition as Hat, Gamepad, GamepadButtons, RawGameController};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};

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
    /// Hors manette de navigation : le mode souris a changé (L3 + R3) / le clavier visuel est demandé (Triangle en mode souris).
    Mouse,
    Osk,
    /// L1/LB, R1/RB : changement rapide de section (Jeux/Bibliothèque/Boutique) sans passer par la barre du haut.
    TabPrev,
    TabNext,
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
// Mêmes valeurs que les bits LB/RB d'XInput (wButtons) : pas de conversion nécessaire pour la manette Xbox.
const L1: u16 = 0x0100;
const R1: u16 = 0x0200;
const DIRS: u16 = UP | DOWN | LEFT | RIGHT;

const MAP: [(u16, Nav); 10] = [
    (UP, Nav::Up),
    (DOWN, Nav::Down),
    (LEFT, Nav::Left),
    (RIGHT, Nav::Right),
    (A, Nav::Accept),
    (B, Nav::Back),
    (GUIDE, Nav::Guide),
    (START, Nav::Menu),
    (L1, Nav::TabPrev),
    (R1, Nav::TabNext),
];

const POLL: Duration = Duration::from_millis(10);
const REPEAT_DELAY: Duration = Duration::from_millis(260);
const REPEAT_RATE: Duration = Duration::from_millis(80);
const REPEAT_FAST: Duration = Duration::from_millis(48);
const STICK: f64 = 0.5; // seuil de déviation du stick, 0..1

type GetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// Lance le thread de lecture ; `on_nav` est appelé depuis ce thread.
pub fn spawn(on_nav: impl Fn(Nav) + Send + Sync + 'static) {
    let on_nav = std::sync::Arc::new(on_nav);
    mouse::spawn(on_nav.clone());
    xpad::spawn();
    thread::spawn(move || {
        let on_nav = move |n: Nav| {
            stamp();
            (*on_nav)(n)
        };
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() };
        let get_state = xinput_get_state_ex().unwrap_or(xinput_get_state);
        let mut xinput = XInput { get_state, connected: [true; 4], tick: 0 };
        register_hotkeys();
        if sony_present() {
            LAST_PAD.store(1, Ordering::Relaxed);
        }
        let mut prev = 0u16;
        let mut next_repeat = Instant::now();
        let mut hold_start = Instant::now();
        loop {
            while let Some(nav) = poll_hotkey() {
                on_nav(nav);
            }
            // Avec une manette Sony, sa copie virtuelle Xbox (ViGEm) n'est pas relue : une seule source, pas de double lecture.
            let sony = read_sony();
            let other = if xpad::present() { 0 } else { read_wgi() | xinput.read() };
            if sony != 0 {
                LAST_PAD.store(1, Ordering::Relaxed);
            } else if other != 0 {
                LAST_PAD.store(0, Ordering::Relaxed);
            }
            let mut cur = sony | other;
            // Mode souris (sans clavier visuel) : la manette pilote le curseur, seule la touche guide reste à Hearth.
            if mouse::active() && !mouse::osk() {
                cur &= GUIDE;
            }
            let pressed = cur & !prev;
            for (bit, nav) in MAP {
                if pressed & bit != 0 {
                    on_nav(nav);
                }
            }
            let now = Instant::now();
            // Un stick/croix qui reste bloqué dans une direction (lecture HID parasite, dérive) ne doit pas
            // faire défiler le menu indéfiniment : passé 6 s de maintien continu, la répétition s'arrête
            // jusqu'au relâchement (une vraie pression, elle, se relâche bien avant).
            let stuck = now.duration_since(hold_start) > Duration::from_secs(6);
            if pressed & DIRS != 0 {
                next_repeat = now + REPEAT_DELAY;
                hold_start = now;
            } else if cur & DIRS != 0 && now >= next_repeat && !stuck {
                for (bit, nav) in &MAP[..4] {
                    if cur & bit != 0 {
                        on_nav(*nav);
                    }
                }
                // Maintenu : la répétition accélère après 1,2 s (défilement rapide des longues listes).
                next_repeat = now + if now.duration_since(hold_start) > Duration::from_millis(1200) { REPEAT_FAST } else { REPEAT_RATE };
            }
            prev = cur;
            thread::sleep(POLL);
        }
    });
}

/// Ctrl+Alt+Maj + F1..F8 (haut, bas, gauche, droite, valider, retour, guide, menu) = bouton de manette, quel que soit le focus : permet de piloter
/// et tester la console sans manette (scripts : SendKeys "^%+{UP}", "^%+g"…).
const HOTKEYS: [(VIRTUAL_KEY, Nav); 8] = [
    (VK_F1, Nav::Up),
    (VK_F2, Nav::Down),
    (VK_F3, Nav::Left),
    (VK_F4, Nav::Right),
    (VK_F5, Nav::Accept),
    (VK_F6, Nav::Back),
    (VK_F7, Nav::Guide),
    (VK_F8, Nav::Menu),
];

fn register_hotkeys() {
    // Test : HEARTH_DEV_TOPMOST décale les raccourcis sur F13..F20 (une autre instance peut garder F1..F8).
    let shift = if std::env::var_os("HEARTH_DEV_TOPMOST").is_some() { 12 } else { 0 };
    let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
    for (i, (vk, _)) in HOTKEYS.iter().enumerate() {
        // Rattachés à la file de messages de ce thread (pas de fenêtre).
        if let Err(e) = unsafe { RegisterHotKey(None, i as i32 + 1, mods, vk.0 as u32 + shift) } {
            let dir = std::env::var("LOCALAPPDATA").unwrap_or_default();
            let _ = std::fs::write(format!("{dir}\\Hearth\\hotkey-{i}.log"), format!("{e}"));
        }
    }
}

fn poll_hotkey() -> Option<Nav> {
    let mut msg = MSG::default();
    unsafe { PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }
        .as_bool()
        .then(|| HOTKEYS.get(msg.wParam.0.wrapping_sub(1)).map(|(_, n)| *n))
        .flatten()
}

/// XInputGetStateEx (ordinal 100, non documenté) expose le bouton guide.
fn xinput_get_state_ex() -> Option<GetState> {
    unsafe {
        let dll = LoadLibraryW(w!("xinput1_4.dll")).ok()?;
        let f = GetProcAddress(dll, PCSTR(100 as *const u8))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(f))
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
            (GamepadButtons::LeftShoulder, L1),
            (GamepadButtons::RightShoulder, R1),
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

/// DualShock 4 / DualSense (HID) : boutons 0 Carré, 1 Croix, 2 Rond, 9 Options, 12 PS ; chapeau 0 = croix ;
/// axes classés par `mouse::split` (voir ce commentaire : leur ordre n'est pas garanti).
// ponytail: seul le schéma Sony est mappé ; autres manettes HID en phase 4.
fn read_sony() -> u16 {
    let raws = sony_pads();
    let mut bits = 0;
    for raw in raws {
        let (Ok(nb), Ok(ns), Ok(na)) = (raw.ButtonCount(), raw.SwitchCount(), raw.AxisCount()) else { continue };
        let mut b = vec![false; nb as usize];
        let mut s = vec![Hat::Center; ns as usize];
        let mut a = vec![0.5; na as usize];
        if raw.GetCurrentReading(&mut b, &mut s, &mut a).is_err() {
            sony_invalidate(&raw);
            continue;
        }
        for (i, bit) in [(1, A), (2, B), (9, START), (12, GUIDE), (4, L1), (5, R1)] {
            if b.get(i) == Some(&true) {
                bits |= bit;
            }
        }
        if let Some(h) = s.first() {
            bits |= hat(*h);
        }
        // Stick gauche : jamais a[0]/a[1] en dur (voir `mouse::split`), une gâchette au repos (0) prise pour
        // un stick dévié au maximum sinon -> navigation figée dans une direction, insensible au vrai stick.
        let (st, _) = mouse::split(&a);
        bits |= stick((st[0] - 0.5) * 2.0, (0.5 - st[1]) * 2.0);
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
        let retry = self.tick.is_multiple_of(100);
        let mut bits = 0;
        for i in 0..4 {
            if !self.connected[i] && !retry {
                continue;
            }
            let mut st = XINPUT_STATE::default();
            self.connected[i] = unsafe { (self.get_state)(i as u32, &mut st) } == 0;
            if self.connected[i] {
                let g = st.Gamepad;
                bits |= g.wButtons.0 & (DIRS | START | GUIDE | A | B | L1 | R1);
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

/// Manette Sony choisie comme « principale » (Réglages > Manettes) : celle transmise aux jeux quand
/// plusieurs sont branchées. `None` : la première trouvée, peu importe laquelle.
static PRIMARY_PAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_primary_pad(id: Option<String>) {
    *PRIMARY_PAD.lock().unwrap_or_else(|e| e.into_inner()) = id;
}

pub(crate) fn primary_pad() -> Option<String> {
    PRIMARY_PAD.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Dernière manette utilisée (pour afficher les bons pictogrammes) : 0 Xbox/générique, 1 PlayStation.
static LAST_PAD: AtomicU8 = AtomicU8::new(0);

pub fn last_pad() -> u8 {
    LAST_PAD.load(Ordering::Relaxed)
}

fn sony_present() -> bool {
    RawGameController::RawGameControllers().is_ok_and(|l| l.into_iter().any(|r| r.HardwareVendorId() == Ok(SONY)))
}

static LAST_INPUT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Instant du dernier ordre de navigation (mesure de latence : voir HEARTH_PERF dans le shell).
pub fn last_input() -> Option<Instant> {
    *LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn take_last_input() -> Option<Instant> {
    LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn stamp() {
    *LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// Manettes Sony : liste tenue à jour en direct par les événements WinRT Ajout/Retrait (pas de
/// ré-énumération périodique) — une ré-énumération ponctuelle tombant pile pendant une micro-coupure
/// Bluetooth (manette faible en charge) pouvait renvoyer une liste vide ou un objet resté lié à une
/// session HID périmée (lecture figée, cf. `stuck` plus haut).
static SONY_LIST: std::sync::Mutex<Vec<RawGameController>> = std::sync::Mutex::new(Vec::new());
static SONY_WATCH: std::sync::Once = std::sync::Once::new();

fn refresh_sony_list() {
    let fresh = RawGameController::RawGameControllers()
        .map(|l| l.into_iter().filter(|r| r.HardwareVendorId() == Ok(SONY)).collect())
        .unwrap_or_default();
    *SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()) = fresh;
}

fn sony_watch_start() {
    SONY_WATCH.call_once(|| {
        refresh_sony_list();
        let added = windows::Foundation::EventHandler::new(|_, r: windows::core::Ref<'_, RawGameController>| {
            if let Some(r) = r.as_ref() {
                if r.HardwareVendorId() == Ok(SONY) {
                    let mut l = SONY_LIST.lock().unwrap_or_else(|e| e.into_inner());
                    if !l.contains(r) {
                        l.push(r.clone());
                    }
                }
            }
            Ok(())
        });
        let removed = windows::Foundation::EventHandler::new(|_, r: windows::core::Ref<'_, RawGameController>| {
            if let Some(r) = r.as_ref() {
                SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != r);
            }
            Ok(())
        });
        let _ = RawGameController::RawGameControllerAdded(&added);
        let _ = RawGameController::RawGameControllerRemoved(&removed);
    });
}

pub(crate) fn sony_pads() -> Vec<RawGameController> {
    sony_watch_start();
    SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Force une relecture complète des manettes (nouvel appel WinRT, pas la liste mémorisée) : à utiliser
/// en sortant d'un jeu, au cas où celui-ci aurait changé le mode de rapport HID d'une manette Sony
/// (Steam Input, DirectInput…) sans déclencher d'événement Ajout/Retrait.
pub fn resync_pads() {
    sony_watch_start();
    refresh_sony_list();
}

/// Une lecture a échoué sur cet objet précis (session HID périmée) : on le retire, un vrai événement
/// Ajout/Retrait ou `resync_pads()` le restaurera proprement.
pub(crate) fn sony_invalidate(raw: &RawGameController) {
    SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != raw);
}
