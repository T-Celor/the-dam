use std::io::{BufRead, Write};

fn main() -> std::io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version") => println!("dam-economizer {}", env!("CARGO_PKG_VERSION")),
        Some("hook") => dam::economizer::hook(),
        Some("replay") => {
            let mut out = std::io::stdout().lock();
            for line in std::io::stdin().lock().lines() {
                writeln!(out, "{}", dam::loopguard::replay(&line?))?;
            }
        }
        _ => {
            eprintln!("usage: dam-economizer hook | replay");
            std::process::exit(2);
        }
    }

    Ok(())
}
