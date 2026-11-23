//! Réglages système rapides. `display` et `radio` bloquent (DDC/CI, WinRT async) :
//! à appeler hors du thread de l'UI.

pub mod audio {
    #![allow(non_snake_case)]
    use std::ffi::c_void;
    use windows::core::{Result, GUID, HRESULT, PCWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

    fn endpoint() -> Result<IAudioEndpointVolume> {
        unsafe {
            // Déjà initialisé (STA) sur le thread de l'UI : l'erreur est sans conséquence.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            en.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
        }
    }

    /// Volume de la sortie par défaut, 0..=100.
    pub fn volume() -> Option<u32> {
        let v = unsafe { endpoint().ok()?.GetMasterVolumeLevelScalar().ok()? };
        Some((v * 100.0).round() as u32)
    }

    pub fn set_volume(pct: u32) {
        if let Ok(e) = endpoint() {
            unsafe {
                let _ = e.SetMasterVolumeLevelScalar(pct.min(100) as f32 / 100.0, std::ptr::null());
            }
        }
    }

    pub struct Device {
        pub id: String,
        pub name: String,
        pub default: bool,
    }

    // Interface COM non documentée mais stable (Windows 7 -> 11), utilisée par tous les outils de bascule audio.
    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: windows::core::IUnknown {
        fn GetMixFormat(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, a: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut i64) -> HRESULT;
        fn GetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, id: PCWSTR, role: i32) -> HRESULT;
        fn SetEndpointVisibility(&self, a: PCWSTR, b: i32) -> HRESULT;
    }

    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    fn enumerator() -> Result<IMMDeviceEnumerator> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }
    }

    /// Sorties audio actives.
    pub fn devices() -> Vec<Device> {
        let Ok(en) = enumerator() else { return vec![] };
        unsafe {
            let default = en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| d.GetId().ok()).and_then(|p| p.to_string().ok());
            let Ok(list) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return vec![] };
            (0..list.GetCount().unwrap_or(0))
                .filter_map(|i| {
                    let d = list.Item(i).ok()?;
                    let id = d.GetId().ok()?.to_string().ok()?;
                    let name = d.OpenPropertyStore(STGM_READ).ok()?.GetValue(&PKEY_Device_FriendlyName).ok()?;
                    let name = PropVariantToStringAlloc(&name).ok()?.to_string().ok()?;
                    Some(Device { default: default.as_deref() == Some(id.as_str()), id, name })
                })
                .collect()
        }
    }

    pub fn set_default(id: &str) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(cfg) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else { return };
            let wid: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
            for role in [0, 1, 2] {
                let _ = cfg.SetDefaultEndpoint(PCWSTR(wid.as_ptr()), role);
            }
        }
    }
}

pub mod display {
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Foundation::{HANDLE, POINT};
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::*;

    // ponytail: écran principal en DDC/CI uniquement ; les dalles de portables (WMI) arrivent en phase 7.
    fn with_monitor<T>(f: impl Fn(HANDLE) -> Option<T>) -> Option<T> {
        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut n = 0;
            GetNumberOfPhysicalMonitorsFromHMONITOR(mon, &mut n).ok()?;
            let mut pms = vec![PHYSICAL_MONITOR::default(); n as usize];
            GetPhysicalMonitorsFromHMONITOR(mon, &mut pms).ok()?;
            let r = pms.first().and_then(|p| f(p.hPhysicalMonitor));
            let _ = DestroyPhysicalMonitors(&pms);
            r
        }
    }

    fn range(h: HANDLE) -> Option<(u32, u32, u32)> {
        let (mut min, mut cur, mut max) = (0, 0, 0);
        (unsafe { GetMonitorBrightness(h, &mut min, &mut cur, &mut max) } != 0 && max > min).then_some((min, cur, max))
    }

    /// Luminosité 0..=100, `None` si l'écran ne gère pas DDC/CI.
    pub fn brightness() -> Option<u32> {
        with_monitor(|h| range(h).map(|(min, cur, max)| (cur - min) * 100 / (max - min)))
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct Mode {
        pub w: u32,
        pub h: u32,
        pub hz: u32,
    }

    fn devmode(which: ENUM_DISPLAY_SETTINGS_MODE) -> Option<Mode> {
        let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR::null(), which, &mut dm) }.as_bool().then_some(Mode {
            w: dm.dmPelsWidth,
            h: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency,
        })
    }

    pub fn current_mode() -> Option<Mode> {
        devmode(ENUM_CURRENT_SETTINGS)
    }

    /// Modes 32 bits >= 1024x768 de l'écran principal, du plus grand au plus petit.
    pub fn modes() -> Vec<Mode> {
        let mut v: Vec<Mode> = vec![];
        let mut i = 0;
        while let Some(m) = devmode(ENUM_DISPLAY_SETTINGS_MODE(i)) {
            if m.w >= 1024 && m.h >= 768 && m.hz > 1 && !v.contains(&m) {
                v.push(m);
            }
            i += 1;
        }
        v.sort_by_key(|m| std::cmp::Reverse((m.w * m.h, m.hz)));
        v
    }

    /// Applique et enregistre le mode (l'enregistrement évite de passer pour un plein écran exclusif).
    pub fn set_mode(m: Mode) -> bool {
        let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        dm.dmPelsWidth = m.w;
        dm.dmPelsHeight = m.h;
        dm.dmDisplayFrequency = m.hz;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&dm), None, CDS_UPDATEREGISTRY, None) == DISP_CHANGE_SUCCESSFUL }
    }

    pub fn set_brightness(pct: u32) {
        with_monitor(|h| {
            let (min, _, max) = range(h)?;
            unsafe { SetMonitorBrightness(h, min + (max - min) * pct.min(100) / 100) };
            Some(())
        });
    }
}

pub mod radio {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};

    fn wifi() -> Option<Radio> {
        let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
        radios.into_iter().find(|r| r.Kind().is_ok_and(|k| k == RadioKind::WiFi))
    }

    /// `None` : pas de carte Wi-Fi.
    pub fn wifi_on() -> Option<bool> {
        Some(wifi()?.State().ok()? == RadioState::On)
    }

    pub fn set_wifi(on: bool) {
        let _ = Radio::RequestAccessAsync().and_then(|a| a.join());
        if let Some(r) = wifi() {
            let state = if on { RadioState::On } else { RadioState::Off };
            let _ = r.SetStateAsync(state).and_then(|a| a.join());
        }
    }
}

pub mod radio_ext {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};

    pub fn bluetooth_on() -> Option<bool> {
        let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
        let r = radios.into_iter().find(|r| r.Kind().is_ok_and(|k| k == RadioKind::Bluetooth))?;
        Some(r.State().ok()? == RadioState::On)
    }

    pub fn set_bluetooth(on: bool) {
        let _ = Radio::RequestAccessAsync().and_then(|a| a.join());
        let Some(radios) = Radio::GetRadiosAsync().ok().and_then(|a| a.join().ok()) else { return };
        if let Some(r) = radios.into_iter().find(|r| r.Kind().is_ok_and(|k| k == RadioKind::Bluetooth)) {
            let _ = r.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).and_then(|a| a.join());
        }
    }
}

pub mod wifi {
    use windows::core::HSTRING;
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Security::Credentials::PasswordCredential;
    use windows::Devices::WiFi::{WiFiAdapter, WiFiAvailableNetwork, WiFiConnectionStatus, WiFiReconnectionKind};
    use windows::Networking::Connectivity::NetworkAuthenticationType;

    pub struct Net {
        pub ssid: String,
        pub bars: u8,
        pub secured: bool,
        pub connected: bool,
    }

    static LAST_ERROR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

    /// Dernière raison d'échec (accès refusé, pas de carte…), vide si tout va bien.
    pub fn last_error() -> String {
        LAST_ERROR.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn fail<T>(msg: String) -> Option<T> {
        if let Ok(mut s) = LAST_ERROR.lock() {
            *s = msg;
        }
        None
    }

    fn adapter() -> Option<WiFiAdapter> {
        if let Ok(mut s) = LAST_ERROR.lock() {
            s.clear();
        }
        match WiFiAdapter::RequestAccessAsync().and_then(|a| a.join()) {
            Ok(windows::Devices::WiFi::WiFiAccessStatus::Allowed) => {}
            Ok(st) => return fail(format!("accès Wi-Fi : {st:?}")),
            Err(e) => return fail(format!("accès Wi-Fi : {e}")),
        }
        match WiFiAdapter::FindAllAdaptersAsync().and_then(|a| a.join()) {
            Ok(l) => l.into_iter().next().or_else(|| fail("aucune carte Wi-Fi".into())),
            Err(e) => fail(format!("cartes Wi-Fi : {e}")),
        }
    }

    fn connected_ssid() -> Option<String> {
        let p = NetworkInformation::GetInternetConnectionProfile().ok()?;
        p.IsWlanConnectionProfile().ok()?.then(|| p.WlanConnectionProfileDetails().ok()?.GetConnectedSsid().ok().map(|s| s.to_string())).flatten()
    }

    /// Réseaux visibles (bloquant : à appeler hors du thread UI), triés par signal.
    pub fn scan() -> Vec<Net> {
        let Some(a) = adapter() else { return super::netsh::scan() };
        let _ = a.ScanAsync().and_then(|s| s.join());
        let cur = connected_ssid();
        let mut v: Vec<Net> = a
            .NetworkReport()
            .and_then(|r| r.AvailableNetworks())
            .map(|l| {
                l.into_iter()
                    .filter_map(|n| {
                        let ssid = n.Ssid().ok()?.to_string();
                        (!ssid.is_empty()).then(|| Net {
                            bars: n.SignalBars().unwrap_or(0),
                            secured: n.SecuritySettings().and_then(|s| s.NetworkAuthenticationType()).is_ok_and(|t| t != NetworkAuthenticationType::Open80211),
                            connected: cur.as_deref() == Some(ssid.as_str()),
                            ssid,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|n| (!n.connected, std::cmp::Reverse(n.bars)));
        v.dedup_by(|a, b| a.ssid == b.ssid);
        v
    }

    fn find(a: &WiFiAdapter, ssid: &str) -> Option<WiFiAvailableNetwork> {
        a.NetworkReport().ok()?.AvailableNetworks().ok()?.into_iter().find(|n| n.Ssid().is_ok_and(|s| s == ssid))
    }

    /// Connexion (mot de passe pour un réseau sécurisé). Bloquant.
    pub fn connect(ssid: &str, password: Option<&str>) -> bool {
        let Some(a) = adapter() else { return super::netsh::connect(ssid, password) };
        let Some(net) = find(&a, ssid) else { return false };
        let kind = WiFiReconnectionKind::Automatic;
        let res = match password {
            Some(p) => {
                let Ok(c) = PasswordCredential::new() else { return false };
                let _ = c.SetPassword(&HSTRING::from(p));
                a.ConnectWithPasswordCredentialAsync(&net, kind, &c)
            }
            None => a.ConnectAsync(&net, kind),
        };
        res.and_then(|o| o.join()).and_then(|r| r.ConnectionStatus()).is_ok_and(|s| s == WiFiConnectionStatus::Success)
    }
}

pub mod pads {
    use windows::Gaming::Input::RawGameController;

    /// Manette connectée : nom, batterie (%) si le pilote l'expose, identifiant stable (ordre, bureau).
    pub struct Pad {
        pub name: String,
        pub battery: Option<u32>,
        pub id: String,
        pub vendor: u16,
    }

    /// Identifiant Sony : utile pour repérer la manette virtuelle Xbox 360 que Hearth crée lui-même
    /// (ViGEmBus) quand une manette Sony est branchée (voir `shell::settings::ordered_pads`).
    pub const SONY_VENDOR: u16 = 0x054C;
    /// Identifiant Microsoft (Xbox) : celui de cette manette virtuelle.
    pub const XBOX_VENDOR: u16 = 0x045E;

    /// Manettes connectées, dans l'ordre où Windows les signale (voir `shell::settings::ordered_pads`
    /// pour l'ordre choisi par l'utilisateur, qui filtre aussi la manette virtuelle).
    pub fn list() -> Vec<Pad> {
        let mut pads: Vec<Pad> = RawGameController::RawGameControllers()
            .map(|l| {
                l.into_iter()
                    .filter_map(|c| {
                        let name = c.DisplayName().ok()?.to_string();
                        let vendor = c.HardwareVendorId().unwrap_or(0);
                        let battery = c.TryGetBatteryReport().ok().and_then(|r| {
                            let cur = r.RemainingCapacityInMilliwattHours().ok()?.Value().ok()?;
                            let full = r.FullChargeCapacityInMilliwattHours().ok()?.Value().ok()?;
                            (full > 0).then(|| (cur as u32 * 100 / full as u32).min(100))
                        });
                        // Stable d'une session à l'autre pour la même manette (branchement/appairage) ; à défaut, le nom.
                        let id = c.NonRoamableId().map(|s| s.to_string()).unwrap_or_else(|_| name.clone());
                        Some(Pad { name, battery, id, vendor })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Batterie DualSense lue en HID brut (fiable y compris en Bluetooth, contrairement à
        // Windows.Gaming.Input) : seulement quand une seule manette Sony est branchée, pour ne pas
        // risquer de l'attribuer à la mauvaise manette (la lecture HID ne dit pas laquelle elle a lue).
        let sony: Vec<usize> = pads.iter().enumerate().filter(|(_, p)| p.vendor == SONY_VENDOR).map(|(i, _)| i).collect();
        if let [i] = sony[..] {
            if let Some((pct, _charging)) = crate::dualsense::battery() {
                pads[i].battery = Some(pct);
            }
        }
        pads
    }
}

pub mod updates {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::System::UpdateAgent::{IUpdateSession, UpdateSession};

    /// Nombre de mises à jour logicielles en attente (Windows Update). Bloquant, parfois long.
    pub fn pending() -> Result<i32, String> {
        search("IsInstalled=0 and Type='Software' and IsHidden=0")
    }

    /// Pilotes proposés par Windows Update (chipset, périphériques ; la carte graphique passe par les sources du constructeur).
    pub fn pending_drivers() -> Result<i32, String> {
        search("IsInstalled=0 and Type='Driver' and IsHidden=0")
    }

    fn search(query: &str) -> Result<i32, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let s: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            let r = s
                .CreateUpdateSearcher()
                .and_then(|u| u.Search(&BSTR::from(query)))
                .map_err(|e| e.to_string())?;
            r.Updates().and_then(|u| u.Count()).map_err(|e| e.to_string())
        }
    }
}

pub mod storage {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};

    pub struct Drive {
        pub name: String,
        pub free: u64,
        pub total: u64,
        pub removable: bool,
    }

    /// Disques fixes et amovibles montés.
    pub fn drives() -> Vec<Drive> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26)
            .filter(|i| mask & (1 << i) != 0)
            .filter_map(|i| {
                let name = format!("{}:\\", (b'A' + i as u8) as char);
                let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                let kind = unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) };
                // 2 amovible, 3 fixe
                if kind != 2 && kind != 3 {
                    return None;
                }
                let (mut free, mut total) = (0u64, 0u64);
                unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), Some(&mut total), None) }.ok()?;
                Some(Drive { name, free, total, removable: kind == 2 })
            })
            .collect()
    }
}

/// Repli quand WinRT refuse l'accès Wi-Fi (localisation désactivée) : `netsh wlan`.
mod netsh {
    use super::wifi::Net;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    fn run(args: &str) -> String {
        // chcp 65001 : sortie UTF-8 (noms de réseaux accentués).
        let out = Command::new("cmd")
            .raw_arg(format!("/c chcp 65001 >nul & netsh wlan {args}"))
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    }

    fn value(line: &str) -> Option<&str> {
        line.split_once(':').map(|(_, v)| v.trim())
    }

    fn connected() -> Option<String> {
        let out = run("show interfaces");
        let mut ssid = None;
        let mut up = false;
        for l in out.lines() {
            let t = l.trim_start();
            if t.starts_with("SSID") && !t.starts_with("BSSID") {
                ssid = value(t).map(String::from);
            } else if t.to_lowercase().contains("connect") && value(t).is_some_and(|v| v.to_lowercase().starts_with("connect") || v.to_lowercase().starts_with("connecté")) {
                up = true;
            }
        }
        up.then_some(ssid).flatten()
    }

    pub fn scan() -> Vec<Net> {
        let cur = connected();
        let out = run("show networks mode=bssid");
        let mut v: Vec<Net> = vec![];
        for l in out.lines() {
            let t = l.trim();
            if t.starts_with("SSID") && !t.starts_with("BSSID") {
                let ssid = value(t).unwrap_or("").to_string();
                v.push(Net { connected: cur.as_deref() == Some(ssid.as_str()), ssid, bars: 0, secured: true });
            } else if let Some(n) = v.last_mut() {
                let low = t.to_lowercase();
                if low.starts_with("authentification") || low.starts_with("authentication") {
                    n.secured = !(low.contains("ouvert") || low.contains("open"));
                } else if t.ends_with('%') && value(t).is_some() {
                    let pct: u32 = value(t).and_then(|p| p.trim_end_matches('%').trim().parse().ok()).unwrap_or(0);
                    n.bars = n.bars.max((pct / 25).min(4) as u8);
                }
            }
        }
        v.retain(|n| !n.ssid.is_empty());
        v.sort_by_key(|n| (!n.connected, std::cmp::Reverse(n.bars)));
        v.dedup_by(|a, b| a.ssid == b.ssid);
        v
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    /// Supprime les profils XML (mot de passe en clair) qu'un plantage entre l'écriture et la suppression
    /// aurait pu laisser d'une précédente exécution.
    fn clean_stale_wlan_profiles() {
        let dir = std::env::temp_dir();
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for e in rd.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with("hearth-wlan-") && n.ends_with(".xml") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Supprime le fichier à la destruction même si `run()` panique entre-temps (le mot de passe y est
    /// en clair) : seule une coupure brutale du processus (crash, coupure de courant) peut encore le
    /// laisser traîner, auquel cas `clean_stale_wlan_profiles` le rattrape à l'appel suivant.
    struct TempProfile(std::path::PathBuf);
    impl Drop for TempProfile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    pub fn connect(ssid: &str, password: Option<&str>) -> bool {
        clean_stale_wlan_profiles();
        if let Some(pw) = password {
            let xml = format!(
                "<?xml version=\"1.0\"?><WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\"><name>{n}</name><SSIDConfig><SSID><name>{n}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security><authEncryption><authentication>WPA2PSK</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption><sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{k}</keyMaterial></sharedKey></security></MSM></WLANProfile>",
                n = escape(ssid),
                k = escape(pw)
            );
            let path = std::env::temp_dir().join(format!("hearth-wlan-{}.xml", std::process::id()));
            if std::fs::write(&path, xml).is_err() {
                return false;
            }
            let _guard = TempProfile(path.clone());
            run(&format!("add profile filename=\"{}\"", path.display()));
        }
        let out = run(&format!("connect name=\"{ssid}\""));
        // Le message de succès varie selon la langue : on vérifie l'état réel après quelques secondes.
        let _ = out;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if connected().as_deref() == Some(ssid) {
                return true;
            }
        }
        false
    }
}

pub mod drivers {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    pub struct Gpu {
        pub name: String,
        pub version: String,
        pub vendor: &'static str,
    }

    fn vendor(pnp: &str) -> &'static str {
        let p = pnp.to_uppercase();
        if p.contains("VEN_10DE") {
            "NVIDIA"
        } else if p.contains("VEN_1002") {
            "AMD"
        } else if p.contains("VEN_8086") {
            "Intel"
        } else {
            ""
        }
    }

    /// Cartes graphiques et version de pilote installée (détection PnP via WMI).
    pub fn gpus() -> Vec<Gpu> {
        let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name + '|' + $_.DriverVersion + '|' + $_.PNPDeviceID }";
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x0800_0000)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let mut p = l.trim().split('|');
                let (name, version, pnp) = (p.next()?, p.next()?, p.next()?);
                (!name.is_empty()).then(|| Gpu { name: name.into(), version: version.into(), vendor: vendor(pnp) })
            })
            .collect()
    }
}

pub mod admin;
pub mod dualsense;
pub mod sfx;
mod extra;
pub use extra::{about, bluetooth, install, net, power};

/// Ouvre une adresse (steam://, com.epicgames.launcher://, https://…) dans l'application associée, sans fenêtre de console.
pub fn open_uri(uri: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd").args(["/c", "start", "", uri]).creation_flags(0x0800_0000).spawn();
}
