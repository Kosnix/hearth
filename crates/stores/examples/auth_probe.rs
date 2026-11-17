//! Sonde des flux de connexion (sans compte) : Epic code appareil, Steam QR.
fn main() {
    println!("epic: {:?}", stores::account::epic_begin().map(|c| (c.url, c.code)));
    println!("steam: {:?}", stores::account::steam_begin().map(|c| (c.url, c.code)));
}
