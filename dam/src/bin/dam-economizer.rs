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
            let args: Vec<String> = std::env::args().skip(1).collect();
            match dam::status::run(&args) {
                Some(text) => println!("{text}"),
                // Not ported yet: the same command, handed over whole.
                None => std::process::exit(dam::economizer::run_python_cli(&args)),
            }
        }
    }

    Ok(())
}
