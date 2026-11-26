//! Sonde : liste les périphériques HID Sony, lit leurs rapports bruts pendant 4 s et affiche le dernier
//! (numéro de rapport 0x01 = USB ou Bluetooth simplifié, 0x31/0x11 = Bluetooth étendu) et son décodage.
//! `cargo run -p input --example hid_probe`
fn main() {
    let api = hidapi::HidApi::new().expect("hidapi");
    for d in api.device_list().filter(|d| d.vendor_id() == 0x054C) {
        println!("{:04x} page={:04x} usage={:04x} {}", d.product_id(), d.usage_page(), d.usage(), d.path().to_string_lossy());
        let Ok(dev) = api.open_path(d.path()) else { println!("  ouverture impossible"); continue };
        let mut buf = [0u8; 128];
        let t0 = std::time::Instant::now();
        let (mut n_reports, mut last) = (0, None);
        while t0.elapsed().as_secs() < 4 {
            if let Ok(n) = dev.read_timeout(&mut buf, 250) {
                if n > 0 {
                    n_reports += 1;
                    last = Some((buf[0], n));
                }
            }
        }
        println!("  {n_reports} rapports en 4 s, dernier : {last:02x?}");
        if let Some((_, n)) = last {
            println!("  décodé : {:?}", input::probe_parse(d.product_id(), &buf[..n]));
        }
    }
}
