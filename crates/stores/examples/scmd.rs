fn main() {
    println!("{:?}", stores::steamcmd::ensure());
    println!("{:?}", stores::steamcmd::installed_ids());
}
