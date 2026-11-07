// The dam: one application with one small program per job, so that each process holds only
// the code of its own job. No network and no model anywhere.
pub mod config;
pub mod context;
pub mod economizer;
pub mod folds;
pub mod graph;
pub mod json;
pub mod loopguard;
pub mod paths;
pub mod beaver;
pub mod scan;
pub mod squeeze;
pub mod status;
pub mod trace;
pub mod toolprune;
pub mod sidecall;
pub mod session;
pub mod pricing;
pub mod microskills;
pub mod judge;
pub mod fold;
pub mod denylist;
pub mod docs;
pub mod web;
pub mod cli;
pub mod channels;
pub mod hosts;
pub mod hostgen;
pub mod answers;
pub mod journal;
pub mod models;
pub mod goldfish;
pub mod upstream;
pub mod autoreview;
pub mod gate;

// The plugin's folder: given by Claude Code to a hook, else found above the running program.
pub fn plugin_root() -> Option<std::path::PathBuf> {
    // Codex and Copilot name it PLUGIN_ROOT.
    for name in ["CLAUDE_PLUGIN_ROOT", "PLUGIN_ROOT"] {
        if let Some(root) = std::env::var_os(name).filter(|root| !root.is_empty()) {
            return Some(root.into());
        }
    }
    let program = std::env::current_exe().ok()?;

    program.ancestors().find(|dir| dir.join("scripts").join("dam-run").is_file()).map(std::path::Path::to_path_buf)
}

// A hook's program ends by itself once Claude Code has given up on it: a stuck one would keep its .exe locked and the plugin from updating.
pub fn watchdog(seconds: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        std::process::exit(0);
    });
}
