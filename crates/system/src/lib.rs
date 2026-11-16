//! Réglages système rapides. `display` et `radio` bloquent (DDC/CI, WinRT async) :
//! à appeler hors du thread de l'UI.

pub mod audio {
    use windows::core::Result;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

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
}

pub mod display {
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Foundation::{HANDLE, POINT};
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};

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
