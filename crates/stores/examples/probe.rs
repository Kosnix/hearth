//! Sonde : affiche la bibliothèque installée et le catalogue de chaque boutique (réseau).
fn main() {
    for s in stores::all() {
        let games = s.installed();
        println!("== {} (détecté : {}, {} jeux installés)", s.name(), s.detected(), games.len());
        for g in games.iter().take(5) {
            println!("  - {} [{}] {:?}", g.title, g.id, g.launch);
        }
        let offers = s.featured();
        println!("  catalogue : {} offres", offers.len());
        for o in offers.iter().take(5) {
            println!("  * {} | {} | {} | {}", o.title, o.price, o.url, o.image);
        }
        if let Some(o) = offers.first() {
            println!("  image : {:?}", stores::fetch_image(&o.image));
        }
    }
    println!("qr : {:?}", stores::qr::rgb("https://store.steampowered.com/app/440", 6).map(|(w, h, p)| (w, h, p.len())));
}
