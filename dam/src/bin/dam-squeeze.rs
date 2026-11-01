fn main() -> std::io::Result<()> {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("--version") {
        println!("dam-squeeze {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    dam::squeeze::run(arg.as_deref())
}
