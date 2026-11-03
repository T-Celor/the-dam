fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("dam-beaver {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Its hooks get 20 s at most.
    dam::watchdog(25);
    dam::beaver::run(&args);
}
