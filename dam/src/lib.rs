// The dam: one application with one small program per job, so that each process holds only
// the code of its own job. No network and no model anywhere.
pub mod economizer;
pub mod folds;
pub mod graph;
pub mod json;
pub mod loopguard;
pub mod ponytail;
pub mod scan;
pub mod squeeze;

// The plugin's folder: given by Claude Code to a hook, else found above the running program.
pub fn plugin_root() -> Option<std::path::PathBuf> {
    if let Some(root) = std::env::var_os("CLAUDE_PLUGIN_ROOT").filter(|root| !root.is_empty()) {
        return Some(root.into());
    }
    let program = std::env::current_exe().ok()?;

    program.ancestors().find(|dir| dir.join("scripts").join("te_bootstrap.py").is_file()).map(std::path::Path::to_path_buf)
}
