//! Options de lancement Steam d'un ou plusieurs jeux (SteamCMD, sans compte) :
//! `cargo run -p stores --example launch_opts -- 107410 271590`.
fn main() {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    for (id, opts) in stores::steamcmd::launch_options(&ids) {
        let def = opts.first().map(|o| o.exe.clone()).unwrap_or_default();
        println!("{id} : {} option(s)", opts.len());
        for o in &opts {
            println!("  [{}] {} | {} | {}", if stores::steamcmd::skips_launcher(o, &def) { "sans launcher" } else { "" }, o.exe, o.args, o.desc);
        }
    }
}
