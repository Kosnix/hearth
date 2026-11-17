//! Sonde : bibliothèque installée, catalogue unifié et recherche (réseau).
fn main() {
    for s in stores::all() {
        println!("== {} (détecté : {}, {} jeux installés)", s.name(), s.detected(), s.installed().len());
    }
    let cat = stores::catalog();
    println!("catalogue unifié : {} offres", cat.len());
    for o in cat.iter().take(8) {
        println!("  * {} | {} | {} | {}", o.title, o.price, o.store, o.url);
    }
    for q in ["witcher", "hades"] {
        let r = stores::search(q);
        println!("recherche « {q} » : {} résultats", r.len());
        for o in r.iter().take(6) {
            println!("  * {} | {} | {}", o.title, o.price, o.store);
        }
    }
    for (name, s) in stores::all().iter().map(|s| (s.name(), s)) {
        println!("{name}: {} résultats pour « celeste »", s.search("celeste").len());
    }
}
