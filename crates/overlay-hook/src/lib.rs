//! Prototype d'overlay par accroche de Present (DXGI) : DLL injectée dans un jeu Direct3D 11, accroche
//! `IDXGISwapChain::Present` pour agir à chaque image rendue par le jeu — seule technique qui fonctionne
//! en plein écran exclusif (une fenêtre Hearth classique, même topmost, ne peut pas s'afficher au-dessus
//! d'un tel jeu). Étape de preuve : on se contente de journaliser que l'accroche se déclenche vraiment
//! (fichier `%TEMP%\hearth-overlay-hook.log`), avant de dessiner quoi que ce soit dans l'image du jeu.
//! À comparer ensuite avec le prototype par capture de fenêtre (Windows.Graphics.Capture) : garder le
//! meilleur des deux pour la vraie intégration (menu guide affiché par-dessus un jeu sans l'overlay).

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::mem::{size_of, ManuallyDrop};
use std::ptr::null;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use retour::GenericDetour;
use windows::core::{Interface, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::UI::WindowsAndMessaging::*;

type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> windows::core::HRESULT;

static HOOK: OnceLock<GenericDetour<PresentFn>> = OnceLock::new();
static FRAMES: AtomicU64 = AtomicU64::new(0);

fn log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("hearth-overlay-hook.log")
}

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "{msg}");
    }
}

unsafe extern "system" fn present_hook(this: *mut c_void, sync: u32, flags: u32) -> windows::core::HRESULT {
    let n = FRAMES.fetch_add(1, Ordering::Relaxed);
    // Une ligne toutes les ~120 images (2 s à 60 i/s) : assez pour prouver que l'accroche se déclenche
    // en continu, sans inonder le disque à chaque image.
    if n % 120 == 0 {
        log(&format!("present #{n}"));
    }
    draw_marker(this);
    HOOK.get().unwrap().call(this, sync, flags)
}

/// Dessine un carré magenta dans le coin haut-gauche de l'image que le jeu vient de rendre : preuve
/// qu'on peut écrire dans son image (pas seulement intercepter l'appel), avant le vrai menu guide.
/// `this` est un pointeur emprunté (propriété du jeu) : `ManuallyDrop` pour ne jamais le relâcher.
unsafe fn draw_marker(this: *mut c_void) {
    let swapchain = ManuallyDrop::new(IDXGISwapChain::from_raw(this));
    let Ok(buffer) = swapchain.GetBuffer::<ID3D11Texture2D>(0) else { return };
    let Ok(device) = swapchain.GetDevice::<ID3D11Device>() else { return };
    let mut rtv = None;
    if device.CreateRenderTargetView(&buffer, None, Some(&mut rtv)).is_err() {
        return;
    }
    let Some(rtv) = rtv else { return };
    let ctx = device.GetImmediateContext();
    let Ok(ctx) = ctx else { return };
    let Ok(ctx1) = ctx.cast::<ID3D11DeviceContext1>() else { return };
    let rect = RECT { left: 10, top: 10, right: 74, bottom: 74 };
    ctx1.ClearView(&rtv, &[1.0, 0.0, 1.0, 1.0], Some(&[rect]));
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(_module: HMODULE, call_reason: u32, _reserved: *mut c_void) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        std::thread::spawn(|| unsafe {
            if let Err(e) = install_hook() {
                log(&format!("échec de l'accroche : {e}"));
            }
        });
    }
    true.into()
}

unsafe fn install_hook() -> Result<(), Box<dyn std::error::Error>> {
    log("démarrage, recherche du pointeur Present…");
    let present = find_present_ptr()?;
    log(&format!("Present trouvé à {present:p}"));
    let target: PresentFn = std::mem::transmute(present);
    let hook = GenericDetour::new(target, present_hook)?;
    hook.enable()?;
    log("accroche active");
    let _ = HOOK.set(hook);
    Ok(())
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
}

/// Crée un périphérique/chaîne d'échange D3D11 jetables (fenêtre invisible 1x1) pour lire le pointeur
/// `Present` de la table de méthodes — le même pointeur que celui que le jeu utilise réellement, car la
/// table de méthodes dépend de l'implémentation DXGI du pilote, pas de l'instance.
unsafe fn find_present_ptr() -> windows::core::Result<*const ()> {
    let class_name = HSTRING::from("HearthOverlayHookProbe");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: GetModuleHandleW(None)?.into(),
        hIcon: HICON::default(),
        hCursor: Default::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCWSTR(null()),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        hIconSm: HICON::default(),
        ..Default::default()
    };
    RegisterClassExW(&class);
    let hwnd = CreateWindowExW(Default::default(), class.lpszClassName, class.lpszClassName, WS_OVERLAPPEDWINDOW, 0, 0, 1, 1, None, None, Some(class.hInstance), None)?;

    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC { Width: 1, Height: 1, RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 }, Format: DXGI_FORMAT_R8G8B8A8_UNORM, ..Default::default() },
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: hwnd,
        Windowed: true.into(),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        ..Default::default()
    };
    let levels = [D3D_FEATURE_LEVEL_11_0];
    let mut swapchain = None;
    let mut device = None;
    D3D11CreateDeviceAndSwapChain(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), Default::default(), Some(&levels), D3D11_SDK_VERSION, Some(&desc), Some(&mut swapchain), Some(&mut device), None, None)?;
    let swapchain: IDXGISwapChain = swapchain.unwrap();
    let vtbl: &IDXGISwapChain_Vtbl = swapchain.vtable();
    let present = vtbl.Present as *const ();
    DestroyWindow(hwnd).ok();
    UnregisterClassW(class.lpszClassName, Some(class.hInstance)).ok();
    Ok(present)
}
