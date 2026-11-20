//! Superviseur : déclaré comme shell Windows, lance et relance hearth-shell.exe.
//! `hearth install|uninstall` bascule le shell de l'utilisateur courant ; `hearth console` quitte le bureau.
#![windows_subsystem = "windows"]

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ};
use windows::Win32::System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::GetShellWindow;

const EXIT_DESKTOP: i32 = 10; // cf. hearth-shell
const CONSOLE_EVENT: PCWSTR = w!(r"Local\HearthConsoleMode");
const WINLOGON: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
const SHELL_EXE: &str = "hearth-shell.exe";
const MAX_CRASHES: usize = 5; // en CRASH_WINDOW, avant repli sur le bureau
const CRASH_WINDOW: Duration = Duration::from_secs(60);

fn main() {
    match env::args().nth(1).as_deref() {
        Some("install") => cli(install()),
        Some("uninstall") => cli(uninstall()),
        Some("setup-admin") => cli(setup_admin()),
        Some("elevated") => elevated(),
        Some("console") => unsafe {
            if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, CONSOLE_EVENT) {
                let _ = SetEvent(ev);
            }
        },
        _ => supervise(),
    }
}

fn supervise() {
    apply_config();
    let shell = exe_dir().join(SHELL_EXE);
    let event = unsafe { CreateEventW(None, false, false, CONSOLE_EVENT) }.expect("CreateEventW");
    let mut crashes: Vec<Instant> = Vec::new();
    loop {
        apply_staged();
        // Après un plantage, on retente avec le rendu logiciel (au cas où le rendu GPU Skia serait en cause).
        let mut cmd = Command::new(&shell);
        if !crashes.is_empty() {
            cmd.env("HEARTH_BACKEND", "winit-software");
        }
        let code = cmd.status().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        // Le shell masque le curseur hors de Hearth : on le rétablit à chaque sortie (plantage, retour au bureau).
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                windows::Win32::UI::WindowsAndMessaging::SPI_SETCURSORS,
                0,
                None,
                windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        if code == 11 {
            log("changement de profil");
            continue;
        }
        if code == EXIT_DESKTOP || code == 0 {
            log("retour au bureau demandé");
            desktop_until_console(event);
            continue;
        }
        log(&format!("shell terminé (code {code}), relance"));
        crashes.retain(|t| t.elapsed() < CRASH_WINDOW);
        crashes.push(Instant::now());
        if crashes.len() >= MAX_CRASHES {
            log("trop de plantages : repli sur le bureau");
            crashes.clear();
            desktop_until_console(event);
            continue;
        }
        thread::sleep(Duration::from_millis(500 * crashes.len() as u64));
    }
}

/// Ouvre explorer et attend `hearth console`. Si un shell existe déjà (superviseur lancé
/// hors Winlogon, en test), on ne touche pas à explorer.
fn desktop_until_console(event: windows::Win32::Foundation::HANDLE) {
    let own_explorer = unsafe { GetShellWindow() }.is_invalid();
    if own_explorer {
        let _ = Command::new("explorer.exe").spawn();
    }
    unsafe { WaitForSingleObject(event, INFINITE) };
    if own_explorer {
        let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).status();
    }
    log("retour en mode console");
}

/// Réappliquée à chaque démarrage : les mises à jour Windows peuvent la réactiver.
fn apply_config() {
    for (key, name) in [
        (r"Software\Microsoft\GameBar", "UseNexusForGameBarEnabled"), // bouton guide -> Game Bar
        (r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled"),
        (r"System\GameConfigStore", "GameDVR_Enabled"),
    ] {
        if let Err(e) = set_dword(key, name, 0) {
            log(&format!("registre {key}\\{name} : {e}"));
        }
    }
}

const ICON: &[u8] = include_bytes!("../../../assets/hearth.ico");

fn install() -> Result<String, String> {
    let dst = data_dir().join("app");
    fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
    for f in ["hearth.exe", SHELL_EXE] {
        fs::copy(exe_dir().join(f), dst.join(f)).map_err(|e| format!("copie de {f} : {e}"))?;
    }
    let exe = dst.join("hearth.exe");
    set_string(WINLOGON, "Shell", &exe.to_string_lossy()).map_err(|e| e.to_string())?;
    // Icône du raccourci : le logo Hearth embarqué (remplace l'ancienne icône générée).
    let _ = fs::write(data_dir().join("hearth.ico"), ICON);
    let script = data_dir().join("shortcut.ps1");
    // BOM : PowerShell 5.1 lit sinon le script en ANSI.
    fs::write(&script, format!("\u{feff}{}", include_str!("shortcut.ps1"))).map_err(|e| e.to_string())?;
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Exe")
        .arg(&exe)
        .arg("-Icon")
        .arg(data_dir().join("hearth.ico"))
        .status();
    Ok(format!("Installé dans {}. Actif à la prochaine ouverture de session.", dst.display()))
}

/// Pilote ViGEmBus (manette Xbox virtuelle pour la DualSense) : installé avec Hearth s'il manque, une validation administrateur.
#[allow(dead_code)]
fn install_gamepad_driver() {
    // Si l'assistant administrateur existe déjà, la console installe le pilote sans invite (au démarrage du shell).
    let present = Command::new("sc").args(["query", "ViGEmBus"]).output().is_ok_and(|o| o.status.success())
        || Command::new("schtasks").args(["/query", "/tn", ELEVATED_TASK]).output().is_ok_and(|o| o.status.success());
    if present {
        return;
    }
    let _ = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Start-Process winget -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList 'install','-e','--id','ViGEm.ViGEmBus','--silent','--accept-package-agreements','--accept-source-agreements'",
        ])
        .status();
}

const ELEVATED_TASK: &str = "HearthElevated";
/// Seuls ces paquets winget peuvent être installés par l'assistant administrateur.
const ALLOWED_WINGET: [&str; 6] = ["Valve.Steam", "EpicGames.EpicGamesLauncher", "GOG.Galaxy", "ViGEm.ViGEmBus", "Microsoft.VCRedist.2015+.x64", "Microsoft.VCRedist.2015+.x86"];

/// À lancer élevé (un seul consentement UAC) : copie l'assistant dans Program Files (non modifiable
/// sans droits) et crée la tâche planifiée « privilèges maximum » qui le lance sans invite.
fn setup_admin() -> Result<String, String> {
    let pf = env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let dir = PathBuf::from(pf).join("Hearth");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join("hearth-helper.exe");
    fs::copy(env::current_exe().map_err(|e| e.to_string())?, &dst).map_err(|e| format!("copie de l'assistant : {e}"))?;
    let user = format!("{}\\{}", env::var("USERDOMAIN").unwrap_or_default(), env::var("USERNAME").unwrap_or_default());
    let ps = format!(
        "$a = New-ScheduledTaskAction -Execute '{}' -Argument 'elevated'; \
         $p = New-ScheduledTaskPrincipal -UserId '{user}' -LogonType Interactive -RunLevel Highest; \
         $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 3); \
         Register-ScheduledTask -TaskName {ELEVATED_TASK} -Action $a -Principal $p -Settings $s -Force | Out-Null",
        dst.display()
    );
    let st = Command::new("powershell").args(["-NoProfile", "-Command", &ps]).status().map_err(|e| e.to_string())?;
    if st.success() {
        Ok("Autorisation administrateur enregistrée.".into())
    } else {
        Err("création de la tâche planifiée refusée".into())
    }
}

/// Exécuté par la tâche planifiée (élevé) : traite la requête de l'utilisateur, liste fermée d'opérations.
fn elevated() {
    let dir = PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("elevated");
    let req = fs::read_to_string(dir.join("request.txt")).unwrap_or_default();
    let _ = fs::remove_file(dir.join("request.txt"));
    let mut parts = req.trim_start_matches('\u{feff}').trim().splitn(2, ' ');
    let res: Result<(), String> = match (parts.next(), parts.next()) {
        (Some("winget"), Some(id)) if ALLOWED_WINGET.contains(&id) => {
            let out = Command::new("winget")
                .args(["install", "-e", "--id", id, "--silent", "--accept-package-agreements", "--accept-source-agreements"])
                .output();
            match out {
                Ok(o) if o.status.success() => Ok(()),
                // Déjà installé / aucune mise à jour applicable : pas une erreur.
                Ok(o) if matches!(o.status.code().map(|c| c as u32), Some(0x8A15_002B | 0x8A15_0061)) => Ok(()),
                Ok(o) => Err(String::from_utf8_lossy(&o.stdout).lines().rev().find(|l| l.trim().len() > 3).unwrap_or("échec de winget").trim().to_string()),
                Err(e) => Err(format!("winget introuvable : {e}")),
            }
        }
        // Règle de pare-feu entrant pour l'exécutable d'un jeu lancé par Hearth : Windows ne pose plus sa question en plein jeu.
        (Some("firewall"), Some(path)) if path.to_lowercase().ends_with(".exe") && std::path::Path::new(path).is_absolute() && std::path::Path::new(path).is_file() => {
            let name = format!("Hearth - {path}");
            let exists = Command::new("netsh").args(["advfirewall", "firewall", "show", "rule", &format!("name={name}")]).output().is_ok_and(|o| o.status.success());
            if exists {
                Ok(())
            } else {
                let st = Command::new("netsh")
                    .args(["advfirewall", "firewall", "add", "rule", &format!("name={name}"), "dir=in", "action=allow", &format!("program={path}"), "enable=yes", "profile=any"])
                    .output();
                if st.is_ok_and(|o| o.status.success()) { Ok(()) } else { Err("règle de pare-feu refusée".into()) }
            }
        }
        _ => Err("opération refusée".into()),
    };
    let text = match res {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("error: {e}"),
    };
    let _ = fs::write(dir.join("result.txt"), text);
}

fn uninstall() -> Result<String, String> {
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(WINLOGON), w!("Shell")) };
    r.ok().map_err(|e| e.to_string())?;
    let ps = "Remove-Item ([Environment]::GetFolderPath('Desktop')+'\\Mode console.lnk') -EA SilentlyContinue";
    let _ = Command::new("powershell").args(["-NoProfile", "-Command", ps]).status();
    Ok(format!(
        "Explorateur rétabli à la prochaine ouverture de session. Fichiers restants : {}",
        data_dir().display()
    ))
}

fn cli(r: Result<String, String>) {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    match r {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("Erreur : {e}");
            std::process::exit(1);
        }
    }
}

fn set_dword(key: &str, name: &str, v: u32) -> windows::core::Result<()> {
    let bytes = v.to_le_bytes();
    set_value(key, name, REG_DWORD.0, &bytes)
}

fn set_string(key: &str, name: &str, v: &str) -> windows::core::Result<()> {
    let wide: Vec<u8> = v.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    set_value(key, name, REG_SZ.0, &wide)
}

fn set_value(key: &str, name: &str, kind: u32, data: &[u8]) -> windows::core::Result<()> {
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            kind,
            Some(data.as_ptr().cast()),
            data.len() as u32,
        )
        .ok()
    }
}

fn exe_dir() -> PathBuf {
    env::current_exe().ok().and_then(|p| p.parent().map(Into::into)).unwrap_or_default()
}

fn data_dir() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth")
}

fn log(msg: &str) {
    let _ = fs::create_dir_all(data_dir());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(data_dir().join("supervisor.log")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} {msg}");
    }
}

/// Met à jour le shell depuis `app\staged\` (déposé par `deploy`) : la copie se fait ici, avant le lancement,
/// car un shell en cours d'exécution verrouille son exécutable.
fn apply_staged() {
    let staged = data_dir().join("app").join("staged").join(SHELL_EXE);
    if staged.exists() {
        match fs::rename(&staged, exe_dir().join(SHELL_EXE)).or_else(|_| fs::copy(&staged, exe_dir().join(SHELL_EXE)).map(|_| ()).and_then(|_| fs::remove_file(&staged))) {
            Ok(()) => log("shell mis à jour depuis staged"),
            Err(e) => log(&format!("mise à jour du shell impossible : {e}")),
        }
    }
}
