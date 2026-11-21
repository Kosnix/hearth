//! Injecteur minimal : charge `overlay_hook.dll` dans un processus déjà démarré (technique classique
//! `CreateRemoteThread` + `LoadLibraryW`, utilisée par la quasi-totalité des overlays de jeu).
//! Usage : `inject.exe <pid> [chemin\vers\overlay_hook.dll]`.

use std::env;
use std::ffi::c_void;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{CreateRemoteThread, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_ALL_ACCESS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let Some(pid) = args.get(1).and_then(|s| s.parse::<u32>().ok()) else {
        eprintln!("usage : inject <pid> [chemin\\vers\\overlay_hook.dll]");
        std::process::exit(1);
    };
    let dll = args.get(2).cloned().unwrap_or_else(|| {
        env::current_exe().unwrap().with_file_name("overlay_hook.dll").to_string_lossy().into_owned()
    });
    println!("PID {pid} <- {dll}");

    let mut wide: Vec<u16> = dll.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let process = OpenProcess(PROCESS_ALL_ACCESS, false, pid)?;
        let remote = VirtualAllocEx(process, None, wide.len() * 2, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if remote.is_null() {
            return Err("VirtualAllocEx a échoué".into());
        }
        windows::Win32::System::Diagnostics::Debug::WriteProcessMemory(process, remote, wide.as_mut_ptr() as *const c_void, wide.len() * 2, None)?;

        let kernel32 = GetModuleHandleW(windows::core::w!("kernel32.dll"))?;
        let load_library = GetProcAddress(kernel32, windows::core::s!("LoadLibraryW")).ok_or("LoadLibraryW introuvable")?;
        let thread = CreateRemoteThread(process, None, 0, Some(std::mem::transmute(load_library)), Some(remote), 0, None)?;
        WaitForSingleObject(thread, INFINITE);
        let _ = CloseHandle(thread);
        VirtualFreeEx(process, remote, 0, MEM_RELEASE)?;
        let _ = CloseHandle(process);
    }
    println!("injecté");
    Ok(())
}
