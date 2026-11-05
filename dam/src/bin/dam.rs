// The dam as one program for every host and job: `dam squeeze`, `dam graph`, `dam beaver`, `dam te`. One build per
// system to ship, sign and update. A process still runs one job, and pages in only that job's code.
use std::io::{BufRead, Read, Write};

const USAGE: &str = "usage: dam squeeze [--web URL | --html | --prune DIR KEEP | --decode] | dam graph update | query QUESTION | dam beaver activate | subagent | track | dam te ...";

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    // The names of the four programs before 0.21 too: a session open across an update still runs the hooks it loaded.
    match args.first().map(String::as_str) {
        Some("--version") => println!("dam {}", env!("CARGO_PKG_VERSION")),
        Some("squeeze" | "dam-squeeze") => squeeze(rest)?,
        Some("graph" | "dam-graph") if rest.first().map(String::as_str) == Some("facts") => {
            if !dam::answers::facts_command(&rest[1..]) {
                std::process::exit(1);
            }
        }
        Some("graph" | "dam-graph") => dam::graph::run(rest),
        Some("beaver" | "dam-beaver") => {
            // Its hooks get 20 s at most.
            dam::watchdog(25);
            dam::beaver::run(rest);
        }
        Some("te" | "economizer" | "dam-economizer") => economizer(rest)?,
        // The Codex and Copilot files, written from the one declaration (or checked against it).
        Some("hosts") => std::process::exit(dam::hostgen::run(rest.first().map(String::as_str) == Some("--check"))),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }

    Ok(())
}

fn stdin_text() -> std::io::Result<String> {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn squeeze(args: &[String]) -> std::io::Result<()> {
    let arg = args.first().map(String::as_str);
    match arg {
        // Codex's and Copilot's hook: condenses a long tool output, answers code names from the map. Never fails the agent.
        Some("hook") => {
            dam::watchdog(25);
            dam::answers::hook();
            Ok(())
        }
        // The mod's call under Claude Code: the same decision as the hook, its footer and accounting left to the mod.
        Some("condense") => {
            if !dam::answers::condense_command(args.get(1).map(String::as_str)) {
                std::process::exit(1);
            }
            Ok(())
        }
        Some("--prune") => {
            if let (Some(dir), Some(keep)) = (args.get(1), args.get(2).and_then(|n| n.parse().ok())) {
                dam::squeeze::prune(std::path::Path::new(dir), keep);
            }
            Ok(())
        }
        // HTML as Markdown, all of it: what the converter's tests compare.
        Some("--html") => std::io::stdout().write_all(dam::web::convert_all(&stdin_text()?, args.get(1).map(String::as_str)).as_bytes()),
        // A web page a tool fetched, as Markdown; exit 3 where the input is no web page.
        Some("--web") => {
            let base = args.get(1).map(String::as_str).filter(|url| url.starts_with("http://") || url.starts_with("https://"));
            let Some(page) = dam::web::convert(&stdin_text()?, base) else { std::process::exit(3) };
            std::io::stdout().write_all(dam::web::render(&page, base).as_bytes())
        }
        Some("profile") => {
            dam::squeeze::profile(&args[1..]);
            Ok(())
        }
        Some("bench") => {
            dam::squeeze::bench(&args[1..]);
            Ok(())
        }
        _ => dam::squeeze::run(arg),
    }
}

fn economizer(args: &[String]) -> std::io::Result<()> {
    match args.first().map(String::as_str) {
        Some("hook") => {
            // Its hooks get 60 s at most, for the judge model.
            dam::watchdog(70);
            dam::economizer::hook()
        }
        Some("replay") => {
            let mut out = std::io::stdout().lock();
            for line in std::io::stdin().lock().lines() {
                writeln!(out, "{}", dam::loopguard::replay(&line?))?;
            }
        }
        Some("run-spec") if args.len() == 2 => std::process::exit(dam::session::run_spec(&args[1])),
        _ => std::process::exit(dam::cli::main(args)),
    }

    Ok(())
}
