// Everything one hook call or command needs about where it runs (plugin/context.py), for Claude Code.
use crate::config::Config;
use crate::economizer::{state_root, Store};
use crate::paths::{abspath, repo_identity, RepoId};
use std::path::{Path, PathBuf};

pub const HOST: &str = "claude";

pub struct Ctx {
    pub root: PathBuf,
    pub repo: RepoId,
    pub store: Store,
    pub config: Config,
}

impl Ctx {
    pub fn with_repo(root: PathBuf, repo: RepoId) -> Self {
        let repo_dir = root.join("repos").join(&repo.key);
        let config = Config::load(&root, &repo_dir);

        Ctx { store: Store { base: repo_dir.join(HOST) }, root, repo, config }
    }

    pub fn for_cwd(cwd: &Path) -> Self {
        let root = abspath(&state_root());
        let repo = repo_identity(cwd);

        Self::with_repo(root, repo)
    }

    // Per repository, shared by every agent.
    pub fn repo_dir(&self) -> PathBuf {
        self.root.join("repos").join(&self.repo.key)
    }
}
