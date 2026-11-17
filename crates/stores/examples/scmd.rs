fn main() {
    for s in ["Steam", "Epic Games"] {
        println!("{s}: {:?}", stores::account::session(s).map(|x| x.name));
    }
    println!("steamcmd session_ok = {}", stores::steamcmd::session_ok("nikosss38"));
    println!("legendary logged_in = {}", stores::epiccmd::logged_in());
}
