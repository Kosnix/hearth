use windows::core::PCWSTR;
use windows::Win32::Media::Multimedia::{mciGetErrorStringW, mciSendStringW};

fn cmd(c: &str) {
    let w: Vec<u16> = c.encode_utf16().chain(Some(0)).collect();
    let mut buf = [0u16; 256];
    let r = unsafe { mciSendStringW(PCWSTR(w.as_ptr()), Some(&mut buf), None) };
    let mut msg = [0u16; 256];
    unsafe {
        let _ = mciGetErrorStringW(r, &mut msg);
    }
    println!("{c} -> {r} {}", String::from_utf16_lossy(&msg).trim_end_matches('\0'));
}

fn main() {
    system::sfx::init();
    let d = std::env::var("LOCALAPPDATA").unwrap();
    cmd(&format!("open \"{d}\\Hearth\\sfx\\select-v3.wav\" type waveaudio alias t1"));
    cmd("play t1 from 0 wait");
    cmd("status t1 mode");
    system::sfx::play(system::sfx::Sfx::Select);
    std::thread::sleep(std::time::Duration::from_millis(800));
    println!("music: {}", system::sfx::music(true));
    std::thread::sleep(std::time::Duration::from_secs(2));
}
