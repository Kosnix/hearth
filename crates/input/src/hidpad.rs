//! Manettes Sony (DualSense, DualSense Edge, DualShock 4) lues directement en HID, en plus de
//! Windows.Gaming.Input. En Bluetooth, ces manettes démarrent en rapport « simplifié » (0x01, le seul
//! que décrit leur descripteur HID, donc le seul que Windows.Gaming.Input sache lire) ; beaucoup de
//! jeux (prise en charge native, SDL, Unity…) les passent en rapport « étendu » (0x31 DualSense,
//! 0x11 DualShock 4), et elles y restent jusqu'à leur extinction. Windows.Gaming.Input renvoie alors
//! le dernier état connu, sans erreur : la manette ne répondait plus dans Hearth en sortie de ces jeux.
//! Ici, les trois formats (USB, Bluetooth simplifié, Bluetooth étendu) sont décodés, d'après les
//! formats documentés par le pilote Linux `hid-playstation` et SDL (aucun code repris).
//!
//! Ouverture en mode partagé : Windows remet à chaque lecteur sa propre copie des rapports, ni les
//! jeux ni Windows.Gaming.Input n'en sont privés.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use hidapi::HidApi;

const SONY: u16 = 0x054C;
const DS4: [u16; 2] = [0x05C4, 0x09CC];
const DUALSENSE: [u16; 2] = [0x0CE6, 0x0DF2];

/// État d'une manette Sony, dans la numérotation de Windows.Gaming.Input pour rester interchangeable
/// avec elle : boutons 0 Carré, 1 Croix, 2 Rond, 3 Triangle, 4 L1, 5 R1, 6 L2, 7 R2, 8 Create/Share,
/// 9 Options, 10 L3, 11 R3, 12 PS, 13 pavé tactile ; croix directionnelle 0..7 (haut, puis sens
/// horaire), 8 au repos ; sticks 0..1 (0,5 au centre, Y croissant vers le bas) ; gâchettes 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SonyState {
    pub buttons: [bool; 14],
    pub hat: u8,
    pub sticks: [f64; 4],
    pub triggers: [f64; 2],
}

/// Manettes ouvertes et leur dernier état : (chemin HID, état, rapport étendu Bluetooth). Une entrée
/// existe tant que la lecture fonctionne, même si la manette n'envoie rien de neuf (l'état reste valable).
static STATES: Mutex<Vec<(String, Option<SonyState>, bool)>> = Mutex::new(Vec::new());

/// États des manettes Sony lues en HID, seulement si l'une d'elles est en rapport étendu Bluetooth
/// — le seul cas où Windows.Gaming.Input est muet. En USB ou en Bluetooth simplifié, il reste la
/// source (vide renvoyé), comme avant : aucun risque de régression sur ces modes qui fonctionnaient.
pub fn states() -> Vec<SonyState> {
    let l = STATES.lock().unwrap_or_else(|e| e.into_inner());
    if !l.iter().any(|(_, s, ext)| *ext && s.is_some()) {
        return vec![];
    }
    l.iter().filter_map(|(_, s, _)| *s).collect()
}

/// Recherche des manettes toutes les 2 s, un fil de lecture par manette ouverte.
pub fn spawn() {
    thread::spawn(|| {
        let Ok(mut api) = HidApi::new() else { return };
        loop {
            let _ = api.refresh_devices();
            let found: Vec<(String, u16)> = api
                .device_list()
                .filter(|d| d.vendor_id() == SONY && (DS4.contains(&d.product_id()) || DUALSENSE.contains(&d.product_id())))
                .filter(|d| d.usage_page() == 0x01 && d.usage() == 0x05)
                .map(|d| (d.path().to_string_lossy().into_owned(), d.product_id()))
                .collect();
            for (path, product) in found {
                if STATES.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(p, _, _)| *p == path) {
                    continue;
                }
                let Ok(cpath) = std::ffi::CString::new(path.clone()) else { continue };
                let Ok(dev) = api.open_path(&cpath) else { continue };
                STATES.lock().unwrap_or_else(|e| e.into_inner()).push((path.clone(), None, false));
                thread::spawn(move || {
                    let mut buf = [0u8; 128];
                    loop {
                        match dev.read_timeout(&mut buf, 250) {
                            Ok(0) => {}
                            Ok(n) => {
                                if let Some(s) = parse(product, &buf[..n]) {
                                    let extended = matches!(buf[0], 0x31 | 0x11);
                                    if let Some(e) = STATES.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().find(|(p, _, _)| *p == path) {
                                        e.1 = Some(s);
                                        e.2 = extended;
                                    }
                                }
                            }
                            // Manette débranchée / éteinte : on l'oublie, la recherche la rouvrira.
                            Err(_) => break,
                        }
                    }
                    STATES.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _, _)| *p != path);
                });
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Décode un rapport d'entrée (premier octet = numéro de rapport, tel que le rend hidapi).
pub fn parse(product: u16, r: &[u8]) -> Option<SonyState> {
    let ds5 = DUALSENSE.contains(&product);
    // (début des sticks, gâchette gauche, droite, premier octet de boutons)
    let (st, l2, r2, b) = match (r.first()?, r.len()) {
        // USB : rapport complet de 64 octets.
        (0x01, n) if n >= 64 => {
            if ds5 {
                (1, 5, 6, 8)
            } else {
                (1, 8, 9, 5)
            }
        }
        // Bluetooth, mode simplifié (DualSense comme DualShock 4).
        (0x01, n) if n >= 10 => (1, 8, 9, 5),
        // Bluetooth, mode étendu : un (DualSense) ou deux (DualShock 4) octets d'en-tête de plus.
        (0x31, n) if ds5 && n >= 12 => (2, 6, 7, 9),
        (0x11, n) if !ds5 && n >= 12 => (3, 10, 11, 7),
        _ => return None,
    };
    let (b0, b1, b2) = (r[b], r[b + 1], r[b + 2]);
    let bit = |v: u8, m: u8| v & m != 0;
    let buttons = [
        bit(b0, 0x10),
        bit(b0, 0x20),
        bit(b0, 0x40),
        bit(b0, 0x80),
        bit(b1, 0x01),
        bit(b1, 0x02),
        bit(b1, 0x04),
        bit(b1, 0x08),
        bit(b1, 0x10),
        bit(b1, 0x20),
        bit(b1, 0x40),
        bit(b1, 0x80),
        bit(b2, 0x01),
        bit(b2, 0x02),
    ];
    let f = |i: usize| r[i] as f64 / 255.0;
    Some(SonyState { buttons, hat: (b0 & 0x0F).min(8), sticks: [f(st), f(st + 1), f(st + 2), f(st + 3)], triggers: [f(l2), f(r2)] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: u8, len: usize) -> Vec<u8> {
        let mut r = vec![0u8; len];
        r[0] = id;
        r
    }

    #[test]
    fn dualsense_bluetooth_extended() {
        // Croix + chapeau à droite, Options, PS ; stick gauche poussé à gauche ; L2 à fond.
        let mut r = report(0x31, 78);
        r[2] = 0;
        r[3] = 128;
        r[4] = 128;
        r[5] = 128;
        r[6] = 255;
        r[9] = 0x20 | 2;
        r[10] = 0x20;
        r[11] = 0x01;
        let s = parse(0x0CE6, &r).unwrap();
        assert!(s.buttons[1] && s.buttons[9] && s.buttons[12] && !s.buttons[2]);
        assert_eq!(s.hat, 2);
        assert!(s.sticks[0] < 0.01 && (s.sticks[1] - 0.5).abs() < 0.01);
        assert!(s.triggers[0] > 0.99);
    }

    #[test]
    fn dualsense_usb_and_simple_bluetooth() {
        let mut usb = report(0x01, 64);
        usb[8] = 0x40 | 8; // Rond, chapeau au repos
        let s = parse(0x0CE6, &usb).unwrap();
        assert!(s.buttons[2] && s.hat == 8);
        let mut simple = report(0x01, 10);
        simple[5] = 0x20 | 4; // Croix, chapeau en bas
        simple[6] = 0x01; // L1
        let s = parse(0x0CE6, &simple).unwrap();
        assert!(s.buttons[1] && s.buttons[4] && s.hat == 4);
    }

    #[test]
    fn dualshock4_bluetooth_extended() {
        let mut r = report(0x11, 78);
        r[7] = 0x20 | 6; // Croix, chapeau à gauche
        r[9] = 0x01; // PS
        let s = parse(0x09CC, &r).unwrap();
        assert!(s.buttons[1] && s.buttons[12] && s.hat == 6);
    }

    #[test]
    fn unknown_reports_ignored() {
        assert!(parse(0x0CE6, &report(0x05, 64)).is_none());
        assert!(parse(0x0CE6, &report(0x31, 5)).is_none());
    }
}
