//! Lecture/écriture HID brute du DualSense (hidapi), en complément de Windows.Gaming.Input : batterie
//! fiable y compris en Bluetooth (où `RawGameController::TryGetBatteryReport` ment souvent) et couleur
//! du bandeau lumineux. Offsets du protocole tirés du pilote noyau Linux `hid-playstation.c`
//! (GPL-2.0-or-later, documentation du format uniquement, aucun code repris) :
//! - Entrée USB (report 0x01, 64 o) et Bluetooth (report 0x31, 78 o) : octet de statut batterie au
//!   même offset 53 dans les deux cas (bits 0-3 capacité, bits 4-7 état de charge) — contrairement à
//!   ce qu'on supposait au départ (en-tête Bluetooth décalant de 2 octets), vérifié en comparant au
//!   pourcentage réel (manette branchée, 100 % affiché par l'appli PlayStation Accessories) : l'octet
//!   lu jusqu'ici (55) était stable mais faux (toujours « 0x10 », jamais corrélé à la vraie charge),
//!   l'octet 53 colle exactement (nibble haut 0x2 = pleine/chargée).
//! - Sortie USB (report 0x02, 63 o) / Bluetooth (report 0x31, 78 o, CRC32 final amorcé par l'octet
//!   0xA2) : bandeau lumineux activé par le bit 2 de `valid_flag1`, couleur aux octets 45/46/47 (USB)
//!   ou 47/48/49 (Bluetooth).

use std::sync::{Mutex, OnceLock};

use hidapi::HidApi;

const VENDOR: u16 = 0x054C;
const PRODUCT: u16 = 0x0CE6;

/// `HidApi::new()` réénumère tous les périphériques HID du système (SetupAPI) : coûteux à refaire à
/// chaque appel alors que `battery()` est interrogée en continu toutes les 10 s. On garde une seule
/// instance, rafraîchie (beaucoup plus légère qu'une recréation complète) à chaque usage.
fn with_api<T>(f: impl FnOnce(&HidApi) -> Option<T>) -> Option<T> {
    static API: OnceLock<Mutex<Option<HidApi>>> = OnceLock::new();
    let mut guard = API.get_or_init(|| Mutex::new(None)).lock().ok()?;
    if guard.is_none() {
        *guard = HidApi::new().ok();
    }
    let api = guard.as_mut()?;
    let _ = api.refresh_devices();
    f(api)
}

fn open() -> Option<hidapi::HidDevice> {
    with_api(|api| {
        let path = api.device_list().find(|d| d.vendor_id() == VENDOR && d.product_id() == PRODUCT && d.usage_page() == 0x01 && d.usage() == 0x05)?.path().to_owned();
        api.open_path(&path).ok()
    })
}

/// Pourcentage (0-100) et indicateur de charge, si le décodage est plausible (bits de charge connus) :
/// mieux vaut garder la valeur de Windows.Gaming.Input que d'en afficher une inventée.
pub fn battery() -> Option<(u32, bool)> {
    let dev = open()?;
    let mut buf = [0u8; 78];
    let n = dev.read_timeout(&mut buf, 200).ok()?;
    let status = match (n, buf[0]) {
        (64, 0x01) | (78, 0x31) => buf[53],
        _ => return None,
    };
    let capacity = (status & 0x0F) as u32;
    let charging = (status >> 4) & 0x0F;
    if !matches!(charging, 0x0 | 0x1 | 0x2 | 0xA | 0xB | 0xF) {
        return None;
    }
    let pct = if charging == 0x2 { 100 } else { (capacity * 10 + 5).min(100) };
    // 0x1 (charge active) et 0x2 (pleine, câble toujours branché) sont tous deux « en charge » à
    // l'écran : à 100 % la manette ne tire plus de courant mais reste sur le câble, l'utilisateur
    // s'attend à le voir plutôt qu'à retomber sur l'alerte batterie basse par défaut.
    Some((pct, matches!(charging, 0x1 | 0x2)))
}

/// Couleur du bandeau lumineux (0-255 par canal). Au mieux : sans effet si la manette ne répond pas.
pub fn set_lightbar(r: u8, g: u8, b: u8) {
    let Some(dev) = open() else { return };
    let mut probe = [0u8; 78];
    let bt = matches!(dev.read_timeout(&mut probe, 50), Ok(78));
    if bt {
        let mut out = [0u8; 78];
        out[0] = 0x31;
        out[2] = 0x10; // tag
        out[4] = 0x04; // valid_flag1 : LIGHTBAR_CONTROL_ENABLE
        (out[47], out[48], out[49]) = (r, g, b);
        let mut seeded = Vec::with_capacity(75);
        seeded.push(0xA2);
        seeded.extend_from_slice(&out[..74]);
        out[74..78].copy_from_slice(&crc32(&seeded).to_le_bytes());
        let _ = dev.write(&out);
    } else {
        let mut out = [0u8; 63];
        out[0] = 0x02;
        out[2] = 0x04; // valid_flag1 : LIGHTBAR_CONTROL_ENABLE
        (out[45], out[46], out[47]) = (r, g, b);
        let _ = dev.write(&out);
    }
}

/// CRC-32 standard (IEEE 802.3, comme `zlib::crc32`) : celui attendu par le DualSense en Bluetooth.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
