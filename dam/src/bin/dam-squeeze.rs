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

    // HTML as Markdown, all of it: what the converter's tests compare.
    if arg.as_deref() == Some("--html") {
        use std::io::{Read, Write};
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        let base = std::env::args().nth(2);
        return std::io::stdout().write_all(dam::web::convert_all(&String::from_utf8_lossy(&bytes), base.as_deref()).as_bytes());
    }

    // A web page a tool fetched, as Markdown; exit 3 where the input is no web page.
    if arg.as_deref() == Some("--web") {
        use std::io::{Read, Write};
        let base = std::env::args().nth(2).filter(|url| url.starts_with("http://") || url.starts_with("https://"));
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        let Some(page) = dam::web::convert(&String::from_utf8_lossy(&bytes), base.as_deref()) else { std::process::exit(3) };
        return std::io::stdout().write_all(dam::web::render(&page, base.as_deref()).as_bytes());
    }

    dam::squeeze::run(arg.as_deref())
}
