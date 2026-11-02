use std::io::{BufRead, Write};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("dam-economizer {}", env!("CARGO_PKG_VERSION")),
        Some("hook") => dam::economizer::hook(),
        Some("replay") => {
            let mut out = std::io::stdout().lock();
            for line in std::io::stdin().lock().lines() {
                writeln!(out, "{}", dam::loopguard::replay(&line?))?;
            }
        }
        Some("run-spec") if args.len() == 2 => std::process::exit(dam::session::run_spec(&args[1])),
        _ => std::process::exit(dam::cli::main(&args)),
    }

    Ok(())
}
