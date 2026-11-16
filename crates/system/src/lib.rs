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
