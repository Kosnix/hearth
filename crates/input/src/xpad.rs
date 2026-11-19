//! DualSense/DualShock 4 vue comme une manette Xbox 360 par les jeux (sans Steam Input) : manette virtuelle ViGEmBus.
//! Le bouton PS (guide) n'est pas transmis : il reste réservé à Hearth.

use std::thread;
use std::time::Duration;

use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use windows::Gaming::Input::{GameControllerSwitchPosition as Hat, RawGameController};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const SONY: u16 = 0x054C;

static PRESENT: AtomicBool = AtomicBool::new(false);

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
            match read().map(|p| if crate::mouse::active() { XGamepad::default() } else { p }) {
                Some(pad) => {
                    missing = None;
                    PRESENT.store(true, Ordering::Relaxed);
                    if !plugged {
                        plugged = target.plugin().is_ok() && target.wait_ready().is_ok();
                        last = XGamepad { thumb_lx: 1, ..Default::default() };
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
                        }
                    }
                    thread::sleep(Duration::from_millis(4));
                }
                None => {
                    // Une lecture ratée isolée ne débranche pas la manette virtuelle (sinon boutons bloqués dans le jeu).
                    let since = *missing.get_or_insert_with(Instant::now);
                    if since.elapsed() > Duration::from_millis(1500) {
                        PRESENT.store(false, Ordering::Relaxed);
                        if plugged {
                            let _ = target.unplug();
                            plugged = false;
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

/// État de la première manette Sony : boutons 0 Carré, 1 Croix, 2 Rond, 3 Triangle, 4 L1, 5 R1, 6 L2, 7 R2, 8 Create, 9 Options, 10 L3, 11 R3 ;
/// axes 0/1 stick gauche, 2/3 stick droit, 4/5 gâchettes (ordre du descripteur HID).
fn read() -> Option<XGamepad> {
    let raws = RawGameController::RawGameControllers().ok()?;
    let raw = raws.into_iter().find(|r| r.HardwareVendorId() == Ok(SONY))?;
    let (nb, ns, na) = (raw.ButtonCount().ok()?, raw.SwitchCount().ok()?, raw.AxisCount().ok()?);
    let mut b = vec![false; nb as usize];
    let mut s = vec![Hat::Center; ns as usize];
    let mut a = vec![0.5; na as usize];
    raw.GetCurrentReading(&mut b, &mut s, &mut a).ok()?;
    let on = |i: usize| b.get(i) == Some(&true);
    let mut raw_bits = 0u16;
    for (i, bit) in [(1, 0x1000), (2, 0x2000), (0, 0x4000), (3, 0x8000), (4, 0x0100), (5, 0x0200), (8, 0x0020), (9, 0x0010), (10, 0x0040), (11, 0x0080)] {
        if on(i) {
            raw_bits |= bit;
        }
    }
    raw_bits |= match s.first().copied().unwrap_or(Hat::Center) {
        Hat::Up => 0x1,
        Hat::UpRight => 0x1 | 0x8,
        Hat::Right => 0x8,
        Hat::DownRight => 0x2 | 0x8,
        Hat::Down => 0x2,
        Hat::DownLeft => 0x2 | 0x4,
        Hat::Left => 0x4,
        Hat::UpLeft => 0x1 | 0x4,
        _ => 0,
    };
    let (st, tr) = crate::mouse::split(&a);
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
