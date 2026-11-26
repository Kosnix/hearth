//! DualSense/DualShock 4 vue comme une manette Xbox 360 par les jeux (sans Steam Input) : manette virtuelle ViGEmBus.
//! Le bouton PS (guide) n'est pas transmis : il reste réservé à Hearth.

use std::thread;
use std::time::Duration;

use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static PRESENT: AtomicBool = AtomicBool::new(false);
/// Emplacement XInput (0..3) occupé par la manette virtuelle, 255 si aucune : seul celui-là est ignoré
/// par la navigation (avant, toutes les manettes Xbox l'étaient dès qu'une manette Sony était branchée).
static SLOT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);

pub fn virtual_slot() -> Option<u32> {
    let s = SLOT.load(Ordering::Relaxed);
    (s < 4).then_some(s as u32)
}

/// Une manette Sony est branchée (sa version virtuelle Xbox n'est alors pas relue par Hearth : pas de double lecture).
pub fn present() -> bool {
    PRESENT.load(Ordering::Relaxed)
}

pub fn spawn() {
    thread::spawn(|| {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() };
        // Le pilote peut être installé après le démarrage : on réessaie.
        let client = loop {
            match Client::connect() {
                Ok(c) => break c,
                Err(_) => thread::sleep(Duration::from_secs(20)),
            }
        };
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        let mut plugged = false;
        let mut last = XGamepad::default();
        let mut missing: Option<Instant> = None;
        loop {
            match read().map(|p| if captured() { XGamepad::default() } else { p }) {
                Some(pad) => {
                    missing = None;
                    PRESENT.store(true, Ordering::Relaxed);
                    if !plugged {
                        plugged = target.plugin().is_ok() && target.wait_ready().is_ok();
                        last = XGamepad { thumb_lx: 1, ..Default::default() };
                        SLOT.store(target.get_user_index().ok().filter(|&i| i < 4).map_or(255, |i| i as u8), Ordering::Relaxed);
                    }
                    if plugged && (pad.buttons.raw != last.buttons.raw || pad.left_trigger != last.left_trigger || pad.right_trigger != last.right_trigger
                        || (pad.thumb_lx, pad.thumb_ly, pad.thumb_rx, pad.thumb_ry) != (last.thumb_lx, last.thumb_ly, last.thumb_rx, last.thumb_ry))
                    {
                        // Échec d'envoi : on ne mémorise pas l'état (renvoyé au tour suivant), manette virtuelle rebranchée.
                        if target.update(&pad).is_ok() {
                            last = pad;
                        } else {
                            let _ = target.unplug();
                            plugged = false;
                            SLOT.store(255, Ordering::Relaxed);
                        }
                    }
                    thread::sleep(Duration::from_millis(8));
                }
                None => {
                    // Une lecture ratée isolée ne débranche pas la manette virtuelle (sinon boutons bloqués dans le jeu).
                    let since = *missing.get_or_insert_with(Instant::now);
                    if since.elapsed() > Duration::from_millis(1500) {
                        PRESENT.store(false, Ordering::Relaxed);
                        if plugged {
                            let _ = target.unplug();
                            plugged = false;
                            SLOT.store(255, Ordering::Relaxed);
                        }
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    });
}

fn axis(v: f64) -> i16 {
    ((v - 0.5) * 2.0 * 32767.0).clamp(-32767.0, 32767.0) as i16
}

/// État de la manette Sony transmise aux jeux : boutons 0 Carré, 1 Croix, 2 Rond, 3 Triangle, 4 L1, 5 R1,
/// 6 L2, 7 R2, 8 Create, 9 Options, 10 L3, 11 R3 (voir `hidpad::SonyState`).
fn read() -> Option<XGamepad> {
    // Plusieurs manettes Sony branchées et une principale choisie (Réglages > Manettes, identifiée par
    // Windows.Gaming.Input) : celle-là. Sinon la première, lue en HID direct si possible.
    let raws = crate::sony_pads();
    let primary = crate::primary_pad().filter(|_| raws.len() > 1).and_then(|id| raws.iter().find(|r| r.NonRoamableId().ok().is_some_and(|n| n == id)).cloned());
    let s = match primary {
        Some(raw) => crate::wgi_state(&raw)?,
        None => crate::sony_states().into_iter().next()?,
    };
    let on = |i: usize| s.buttons[i];
    let mut raw_bits = 0u16;
    for (i, bit) in [(1, 0x1000), (2, 0x2000), (0, 0x4000), (3, 0x8000), (4, 0x0100), (5, 0x0200), (8, 0x0020), (9, 0x0010), (10, 0x0040), (11, 0x0080)] {
        if on(i) {
            raw_bits |= bit;
        }
    }
    raw_bits |= crate::hat_bits(s.hat);
    let (st, tr) = (s.sticks, s.triggers);
    let (lt, rt) = (if on(6) { 255 } else { (tr[0].clamp(0.0, 1.0) * 255.0) as u8 }, if on(7) { 255 } else { (tr[1].clamp(0.0, 1.0) * 255.0) as u8 });
    Some(XGamepad {
        buttons: XButtons { raw: raw_bits },
        left_trigger: lt,
        right_trigger: rt,
        thumb_lx: axis(st[0]),
        thumb_ly: -axis(st[1]),
        thumb_rx: axis(st[2]),
        thumb_ry: -axis(st[3]),
    })
}

static CAPTURE: AtomicBool = AtomicBool::new(false);

/// Un overlay de Hearth (menu guide…) est ouvert : la manette lui appartient, le jeu ne reçoit rien.
pub fn set_capture(on: bool) {
    CAPTURE.store(on, Ordering::Relaxed);
}

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
}

/// La manette ne va aux jeux que si ni un overlay, ni la console elle-même (au premier plan) n'en ont l'usage.
fn captured() -> bool {
    if CAPTURE.load(Ordering::Relaxed) || crate::mouse::active() {
        return true;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        pid == GetCurrentProcessId()
    }
}
