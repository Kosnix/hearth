//! Réglages système supplémentaires : Bluetooth (appairage), réseau actuel, plans d'alimentation, infos matérielles.

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

/// Lecture du registre (chaînes), pour les infos matérielles.
mod reg {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, RRF_RT_REG_SZ};

    pub fn string(root: HKEY, key: &str, name: &str) -> Option<String> {
        let (k, n) = (HSTRING::from(key), HSTRING::from(name));
        let mut buf = vec![0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(root, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
        r.is_ok().then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    }
}

/// Lance une commande sans fenêtre et renvoie sa sortie (UTF-8 via chcp 65001).
fn command_output(cmd: &str) -> String {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("cmd")
        .raw_arg(format!("/c chcp 65001 >nul & {cmd}"))
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub mod bluetooth {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice};
    use windows::Devices::Enumeration::{DeviceInformation, DevicePairingKinds, DevicePairingResultStatus, DevicePairingRequestedEventArgs, DeviceUnpairingResultStatus};
    use windows::Foundation::TypedEventHandler;

    pub struct Dev {
        pub id: String,
        pub name: String,
        pub paired: bool,
        pub connected: bool,
    }

    /// Appareils Bluetooth appairés (`true`) ou détectés à proximité mais non appairés (`false`). Bloquant (secondes).
    pub fn devices(paired: bool) -> Vec<Dev> {
        let Ok(sel) = BluetoothDevice::GetDeviceSelectorFromPairingState(paired) else { return vec![] };
        let Some(list) = DeviceInformation::FindAllAsyncAqsFilter(&sel).ok().and_then(|a| a.join().ok()) else { return vec![] };
        let mut v: Vec<Dev> = list
            .into_iter()
            .filter_map(|d| {
                let name = d.Name().ok()?.to_string();
                if name.is_empty() {
                    return None;
                }
                let id = d.Id().ok()?;
                let connected = paired
                    && BluetoothDevice::FromIdAsync(&id)
                        .ok()
                        .and_then(|a| a.join().ok())
                        .and_then(|b| b.ConnectionStatus().ok())
                        .is_some_and(|s| s == BluetoothConnectionStatus::Connected);
                Some(Dev { id: id.to_string(), name, paired, connected })
            })
            .collect();
        v.sort_by_key(|d| (!d.connected, d.name.to_lowercase()));
        v
    }

    fn is_controller(name: &str) -> bool {
        let n = name.to_lowercase();
        ["wireless controller", "dualsense", "dualshock", "xbox", "gamepad", "controller"].iter().any(|k| n.contains(k))
    }

    /// Manettes (DualSense, DualShock, Xbox…) à proximité et non appairées : Bluetooth classique et basse consommation (Xbox).
    pub fn nearby_controllers() -> Vec<Dev> {
        use windows::Devices::Bluetooth::BluetoothLEDevice;
        let mut out: Vec<Dev> = devices(false).into_iter().filter(|d| is_controller(&d.name)).collect();
        if let Ok(sel) = BluetoothLEDevice::GetDeviceSelectorFromPairingState(false) {
            if let Some(list) = DeviceInformation::FindAllAsyncAqsFilter(&sel).ok().and_then(|a| a.join().ok()) {
                for d in list {
                    let (Ok(name), Ok(id)) = (d.Name(), d.Id()) else { continue };
                    let (name, id) = (name.to_string(), id.to_string());
                    if is_controller(&name) && !out.iter().any(|o| o.id == id) {
                        out.push(Dev { id, name, paired: false, connected: false });
                    }
                }
            }
        }
        out
    }

    /// Appairage « personnalisé » plutôt que `PairAsync()` nu : une manette (ou tout appareil « appuyer
    /// pour confirmer ») attend un accord explicite de l'appli via l'événement `PairingRequested`, que
    /// l'appairage simple ne fournit pas — sans ça, Windows attend une confirmation qui ne vient jamais
    /// et l'appairage échoue ou expire (c'est ce que fait l'appli Paramètres de Windows en coulisses).
    pub fn pair(id: &str) -> Result<(), String> {
        let info = DeviceInformation::CreateFromIdAsync(&id.into()).and_then(|a| a.join()).map_err(|e| e.to_string())?;
        let custom = info.Pairing().and_then(|p| p.Custom()).map_err(|e| e.to_string())?;
        custom
            .PairingRequested(&TypedEventHandler::new(|_, args: windows_core::Ref<'_, DevicePairingRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let _ = args.Accept();
                }
                Ok(())
            }))
            .map_err(|e| e.to_string())?;
        let r = custom.PairAsync(DevicePairingKinds::ConfirmOnly).and_then(|a| a.join()).map_err(|e| e.to_string())?;
        match r.Status() {
            Ok(DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired) => Ok(()),
            Ok(s) => Err(format!("{s:?}")),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn unpair(id: &str) -> Result<(), String> {
        let info = DeviceInformation::CreateFromIdAsync(&id.into()).and_then(|a| a.join()).map_err(|e| e.to_string())?;
        let r = info.Pairing().and_then(|p| p.UnpairAsync()).and_then(|a| a.join()).map_err(|e| e.to_string())?;
        match r.Status() {
            Ok(DeviceUnpairingResultStatus::Unpaired | DeviceUnpairingResultStatus::AlreadyUnpaired) => Ok(()),
            Ok(s) => Err(format!("{s:?}")),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub mod net {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::HostNameType;

    pub struct Link {
        pub name: String,
        pub wifi: bool,
        pub ip: String,
    }

    /// Connexion Internet active (Wi-Fi ou Ethernet) et adresse IPv4.
    pub fn current() -> Option<Link> {
        let p = NetworkInformation::GetInternetConnectionProfile().ok()?;
        let name = p.ProfileName().ok()?.to_string();
        let wifi = p.IsWlanConnectionProfile().unwrap_or(false);
        let adapter = p.NetworkAdapter().ok()?.NetworkAdapterId().ok()?;
        let ip = NetworkInformation::GetHostNames()
            .ok()?
            .into_iter()
            .find(|h| {
                h.Type().is_ok_and(|t| t == HostNameType::Ipv4)
                    && h.IPInformation().and_then(|i| i.NetworkAdapter()).and_then(|a| a.NetworkAdapterId()).is_ok_and(|id| id == adapter)
            })
            .and_then(|h| h.CanonicalName().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        Some(Link { name, wifi, ip })
    }
}

pub mod power {
    use super::command_output;

    pub struct Plan {
        pub guid: String,
        pub name: String,
        pub active: bool,
    }

    fn guid_in(s: &str) -> Option<String> {
        s.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')).find(|t| t.len() == 36 && t.matches('-').count() == 4).map(String::from)
    }

    pub fn plans() -> Vec<Plan> {
        command_output("powercfg /list")
            .lines()
            .filter_map(|l| {
                let guid = guid_in(l)?;
                let name = l.split_once('(')?.1.split_once(')')?.0.trim().to_string();
                Some(Plan { guid, name, active: l.trim_end().ends_with('*') })
            })
            .collect()
    }

    pub fn set_plan(guid: &str) {
        command_output(&format!("powercfg /setactive {guid}"));
    }

    /// Délai en minutes (0 = jamais) sur secteur : mise en veille (`sleep`) ou extinction de l'écran.
    pub fn timeout(sleep: bool) -> Option<u32> {
        let sub = if sleep { "SUB_SLEEP STANDBYIDLE" } else { "SUB_VIDEO VIDEOIDLE" };
        let out = command_output(&format!("powercfg /query SCHEME_CURRENT {sub}"));
        let hex: Vec<&str> = out.lines().filter_map(|l| l.split("0x").nth(1)).collect();
        // Avant-dernière valeur : secteur ; dernière : batterie.
        let v = hex.get(hex.len().checked_sub(2)?)?;
        Some(u32::from_str_radix(v.trim(), 16).ok()? / 60)
    }

    pub fn set_timeout(sleep: bool, minutes: u32) {
        let kind = if sleep { "standby-timeout-ac" } else { "monitor-timeout-ac" };
        command_output(&format!("powercfg /change {kind} {minutes}"));
    }
}

pub mod about {
    use super::{reg, HKEY_LOCAL_MACHINE};
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    /// Paires (libellé, valeur) : PC, Windows, processeur, mémoire.
    pub fn info() -> Vec<(&'static str, String)> {
        let win = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let get = |k: &str, n: &str| reg::string(HKEY_LOCAL_MACHINE, k, n).unwrap_or_default();
        let mut mem = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        let ram = unsafe { GlobalMemoryStatusEx(&mut mem) }.map(|_| format!("{} Go", (mem.ullTotalPhys + 500_000_000) / 1_000_000_000)).unwrap_or_default();
        vec![
            ("PC", std::env::var("COMPUTERNAME").unwrap_or_default()),
            // Le registre garde « Windows 10 » même sous Windows 11 (build 22000 et plus).
            ("Windows", {
                let build = get(win, "CurrentBuild");
                let name = get(win, "ProductName");
                let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { name.replace("Windows 10", "Windows 11") } else { name };
                format!("{name} {} (build {build})", get(win, "DisplayVersion"))
            }),
            ("Processeur", get(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").trim().to_string()),
            ("Mémoire", ram),
        ]
    }
}

pub mod install {
    use std::os::windows::process::CommandExt;

    /// Identifiant winget du client d'une boutique.
    pub fn client_id(store: &str) -> Option<&'static str> {
        match store {
            "Steam" => Some("Valve.Steam"),
            "Epic Games" => Some("EpicGames.EpicGamesLauncher"),
            "GOG" => Some("GOG.Galaxy"),
            _ => None,
        }
    }

    /// Installe un paquet via l'assistant administrateur (autorisation demandée une seule fois). Bloquant.
    pub fn winget(id: &str) -> Result<(), String> {
        if !crate::admin::enabled() {
            crate::admin::setup()?;
        }
        match crate::admin::request(&format!("winget {id}")) {
            // Assistant d'une ancienne version (liste d'opérations plus courte) : réinstallé une fois, puis nouvel essai.
            Err(e) if e.contains("refusée") => {
                crate::admin::setup()?;
                crate::admin::request(&format!("winget {id}"))
            }
            r => r,
        }
    }

    /// Installation directe (sans assistant) : Windows demande alors une confirmation à chaque fois.
    #[allow(dead_code)]
    pub fn winget_direct(id: &str) -> Result<(), String> {
        let out = std::process::Command::new("winget")
            .args(["install", "-e", "--id", id, "--silent", "--accept-package-agreements", "--accept-source-agreements"])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            let text = String::from_utf8_lossy(&out.stdout);
            Err(text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("échec").trim().to_string())
        }
    }
}
