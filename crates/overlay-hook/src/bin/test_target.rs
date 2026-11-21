//! Cible de test minimale : une fenêtre D3D11 qui boucle sur `Present`, pour vérifier l'accroche sans
//! avoir besoin d'un vrai jeu installé. `cargo run --release --bin test_target`, puis injecter dedans.

use std::mem::size_of;
use std::ptr::null;

use windows::core::HSTRING;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, w, l)
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let class_name = HSTRING::from("HearthOverlayTestTarget");
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: windows::core::PCWSTR(null()),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(Default::default(), class.lpszClassName, windows::core::w!("Hearth – cible de test overlay"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 100, 100, 800, 600, None, None, Some(instance), None)?;

        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC { Width: 800, Height: 600, RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 }, Format: DXGI_FORMAT_R8G8B8A8_UNORM, ..Default::default() },
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

        println!("PID {} – fenêtre ouverte, boucle de Present en cours (Échap pour quitter)", std::process::id());
        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = swapchain.Present(1, DXGI_PRESENT::default());
        }
    }
}
