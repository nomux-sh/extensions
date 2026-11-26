//! One worktree that a request names, after the permission checks.

use std::path::{Path, PathBuf};

use nomux_extension::Permissions;

use crate::error::{Error, Result};
use crate::git_command::GitCommand;
use crate::path_safety::{self, GrantedDirectories};

/// A worktree in a directory that the user approved.
pub struct Repository {
    gix: gix::Repository,
    worktree: PathBuf,
    may_run_git: bool,
}

impl Repository {
    /// Opens the worktree whose root is `path`.
    pub fn open(permissions: &Permissions, path: &str) -> Result<Self> {
        let worktree = GrantedDirectories::new(&permissions.fs).existing(path)?;
        let gix = gix::open(&worktree).map_err(|error| {
            Error::not_found(format!(
                "{} is not a git worktree: {error}",
                worktree.display()
            ))
        })?;
        let root = gix.workdir().and_then(|root| root.canonicalize().ok());
        if root.as_deref() != Some(worktree.as_path()) {
            return Err(Error::invalid_argument(format!(
                "{} is not the root of a git worktree",
                worktree.display()
            )));
        }
        Ok(Self {
            gix,
            worktree,
            may_run_git: permissions.process.iter().any(|program| program == "git"),
        })
    }

    /// The repository, for the methods that read.
    pub fn gix(&self) -> &gix::Repository {
        &self.gix
    }

    /// The real path of the root of the worktree.
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }

    /// Checks a file path of a request and makes it relative to the worktree.
    pub fn path(&self, path: &str) -> Result<String> {
        path_safety::worktree_path(&self.worktree, path)
    }

    /// Starts a `git` command in the worktree, for the methods that write.
    pub fn git(&self) -> Result<GitCommand> {
        if !self.may_run_git {
            return Err(Error::permission_denied(
                "the user did not approve the program `git` for the extension",
            ));
        }
        Ok(GitCommand::new(&self.worktree))
    }
}
