fn main() {
    let l = stores::account::library("Epic Games");
    println!("{} jeux: {:?}", l.len(), l.iter().take(8).map(|o| (&o.title, o.images.len())).collect::<Vec<_>>());
}
