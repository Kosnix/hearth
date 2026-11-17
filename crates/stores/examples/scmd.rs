fn main() {
    let pid = std::sync::atomic::AtomicU32::new(0);
    let r = stores::epiccmd::install("Hazelnut", &pid, |p, s, e| println!("{p}% {:.1} MB/s eta {e:?}", s / 1048576.0));
    println!("{r:?}");
}
