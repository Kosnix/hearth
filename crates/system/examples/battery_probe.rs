//! Diagnostic : dump des octets bruts du rapport HID de la DualSense, pour retrouver l'offset réel
//! de l'octet de batterie (le calcul actuel dans dualsense.rs semble faux : testé à 100% réel, affiche 5%).
//! Lance plusieurs lectures pour voir si l'octet de batterie varie/bouge dans le flux.

use hidapi::HidApi;

const VENDOR: u16 = 0x054C;
const PRODUCT: u16 = 0x0CE6;

fn main() {
    let api = HidApi::new().expect("hidapi init");
    let devices: Vec<_> = api.device_list().filter(|d| d.vendor_id() == VENDOR && d.product_id() == PRODUCT).collect();
    println!("{} interface(s) DualSense trouvée(s) :", devices.len());
    for d in &devices {
        println!(
            "  path={:?} usage_page={:#06x} usage={:#06x} interface_number={}",
            d.path(),
            d.usage_page(),
            d.usage(),
            d.interface_number()
        );
    }
    let Some(d) = devices.iter().find(|d| d.usage_page() == 0x01 && d.usage() == 0x05) else {
        println!("Aucune interface usage_page=0x01 usage=0x05 (gamepad) trouvée.");
        return;
    };
    let dev = api.open_path(d.path()).expect("open");
    println!("\nOuvert : {:?}\n", d.path());
    for i in 0..5 {
        let mut buf = [0u8; 78];
        match dev.read_timeout(&mut buf, 500) {
            Ok(n) => {
                println!("--- lecture {i} : {n} octets, report_id={:#04x} ---", buf[0]);
                for (idx, chunk) in buf[..n].chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                    println!("{:3}: {}", idx * 16, hex.join(" "));
                }
            }
            Err(e) => println!("--- lecture {i} : erreur {e} ---"),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    println!("\nbattery() actuel : {:?}", system::dualsense::battery());
}
