// dam: one local program for the plugin. `dam squeeze` condenses a command's output without
// dropping a line, `dam graph` builds and answers from a code graph. No network, no model.
mod folds;
mod graph;
mod json;
mod scan;
mod squeeze;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("dam {}", env!("CARGO_PKG_VERSION")),
        Some("squeeze") => squeeze::run(args.get(1).map(String::as_str))?,
        Some("graph") => graph::run(&args[1..]),
        _ => {
            eprintln!("usage: dam squeeze [--decode] | dam graph update | dam graph query \"question\"");
            std::process::exit(2);
        }
    }

    Ok(())
}
