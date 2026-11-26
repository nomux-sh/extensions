//! The paths that a watch observes, and what a change to a path means.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use gix::bstr::BStr;
use gix::index::entry::Mode;
use gix_notify::{Event, PathKind, Watch, Watcher};

use crate::error::{Error, Result};

/// The files of the git directory of a worktree whose change can change the
/// status: `HEAD`, the index and the state of an operation that stopped.
const GIT_DIRECTORY_ENTRIES: [&str; 9] = [
    "HEAD",
    "index",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

/// What a change to a path means for a watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relevance {
    /// The status cannot change: the path is ignored, or it is a file of git
    /// that the status does not read.
    None,
    /// The status can change.
    Status,
    /// The status can change, and the set of watched directories can change:
    /// a directory or an ignore file changed.
    Coverage,
}

/// The directories of one worktree that a watch observes: the git
/// directories and each directory of the worktree that is not ignored.
pub struct Coverage {
    worktree: PathBuf,
    git_directory: PathBuf,
    common_directory: PathBuf,
    directories: HashSet<PathBuf>,
}

impl Coverage {
    /// Walks the worktree with the ignore rules of the repository. An ignored
    /// directory is not read, if the index has no file in it.
    pub fn discover(repository: &gix::Repository) -> Result<Self> {
        let no_worktree = || Error::failed("the repository has no worktree");
        let worktree = repository
            .workdir()
            .ok_or_else(no_worktree)?
            .canonicalize()?;
        let index = repository.index_or_empty()?;
        let checkout = repository.worktree().ok_or_else(no_worktree)?;
        let mut excludes = checkout.excludes(None)?;
        let mut directories = HashSet::new();
        let mut unread = vec![PathBuf::new()];
        while let Some(relative) = unread.pop() {
            let directory = worktree.join(&relative);
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                // Another program removed the directory during the walk.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            directories.insert(directory);
            for entry in entries {
                let entry = entry?;
                let child = relative.join(entry.file_name());
                // A directory with `.git` is a different repository.
                if !entry.file_type()?.is_dir()
                    || entry.file_name() == ".git"
                    || worktree.join(&child).join(".git").exists()
                {
                    continue;
                }
                let Some(prefix) = child.to_str().map(|child| format!("{child}/")) else {
                    continue;
                };
                let has_tracked_files = index.prefixed_entries(BStr::new(&prefix)).is_some();
                if has_tracked_files || !excludes.at_path(&child, Some(Mode::DIR))?.is_excluded() {
                    unread.push(child);
                }
            }
        }
        Ok(Self {
            worktree,
            git_directory: repository.git_dir().canonicalize()?,
            common_directory: repository.common_dir().canonicalize()?,
            directories,
        })
    }

    /// The watches that observe the coverage.
    pub fn watches(&self) -> Vec<Watch> {
        let watch = |path: PathBuf, recursive| Watch { path, recursive };
        let mut watches = vec![
            watch(self.git_directory.clone(), false),
            watch(self.common_directory.clone(), false),
        ];
        for (name, recursive) in [("refs", true), ("reftable", true), ("info", false)] {
            let directory = self.common_directory.join(name);
            if directory.is_dir() {
                watches.push(watch(directory, recursive));
            }
        }
        if Watcher::supports_native_recursive_watches() {
            watches.push(watch(self.worktree.clone(), true));
        } else {
            watches.extend(
                self.directories
                    .iter()
                    .map(|directory| watch(directory.clone(), false)),
            );
        }
        watches
    }

    /// What an event means: the largest relevance of its paths.
    pub fn relevance(&self, event: &Event) -> Relevance {
        event
            .paths
            .iter()
            .map(|path| self.path_relevance(path, event.path_kind))
            .max()
            .unwrap_or(Relevance::None)
    }

    fn path_relevance(&self, path: &Path, kind: PathKind) -> Relevance {
        if let Ok(relative) = path.strip_prefix(&self.git_directory)
            && first_part(relative).is_some_and(|part| GIT_DIRECTORY_ENTRIES.contains(&part))
        {
            return Relevance::Status;
        }
        if let Ok(relative) = path.strip_prefix(&self.common_directory) {
            return common_directory_relevance(relative);
        }
        let Ok(relative) = path.strip_prefix(&self.worktree) else {
            return Relevance::None;
        };
        let in_watched_directory = path
            .parent()
            .is_some_and(|parent| self.directories.contains(parent));
        if first_part(relative) == Some(".git") || !in_watched_directory {
            return Relevance::None;
        }
        let is_directory = match kind {
            PathKind::Directory => true,
            PathKind::File => false,
            PathKind::Any => path.is_dir(),
        };
        if is_directory || self.directories.contains(path) || path.ends_with(".gitignore") {
            return Relevance::Coverage;
        }
        Relevance::Status
    }
}

/// The relevance of a path in the directory that the worktrees of a
/// repository share. Lock files and objects are not relevant.
fn common_directory_relevance(relative: &Path) -> Relevance {
    let is_lock = relative
        .extension()
        .is_some_and(|extension| extension == "lock");
    match first_part(relative) {
        Some("refs" | "reftable" | "packed-refs" | "config") if !is_lock => Relevance::Status,
        Some("info") if relative.ends_with("exclude") => Relevance::Coverage,
        _ => Relevance::None,
    }
}

fn first_part(path: &Path) -> Option<&str> {
    path.components().next()?.as_os_str().to_str()
}
