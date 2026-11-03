fn main() -> std::io::Result<()> {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("--version") {
        println!("dam-squeeze {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if arg.as_deref() == Some("--prune") {
        let mut args = std::env::args().skip(2);
        if let (Some(dir), Some(keep)) = (args.next(), args.next().and_then(|n| n.parse().ok())) {
            dam::squeeze::prune(std::path::Path::new(&dir), keep);
        }
        return Ok(());
    }

    dam::squeeze::run(arg.as_deref())
}
