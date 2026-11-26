//! The methods that change the worktrees of a repository for a clean of the
//! nomux daemon: the move to the trash place and back, the removal and the
//! prune. The messages are in `nomux-proto`, because the daemon reads them.
//!
//! The daemon checks each change in its plan. Each method checks the facts
//! of the worktree here again before it runs `git`, and it never passes
//! `--force`.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use nomux_extension::Permissions;
use nomux_proto::wire::v1::{
    WorktreeDeleteRequest, WorktreeDeleteResponse, WorktreeFacts, WorktreeFactsRequest,
    WorktreeMergedBy, WorktreeMoveFromTrashRequest, WorktreeMoveFromTrashResponse,
    WorktreeMoveToTrashRequest, WorktreeMoveToTrashResponse, WorktreePruneRequest,
    WorktreePruneResponse, WorktreeRefusal,
};

use crate::error::{Error, Result};
use crate::path_safety::GrantedDirectories;
use crate::repository::Repository;
use crate::worktree_facts::{of_linked, worktree_facts};

/// The reason of the lock of a worktree in the trash place of nomux. The
/// extension removes no other lock.
pub const TRASH_LOCK_REASON: &str = "nomux-trash";

/// Moves a linked worktree to the trash place and locks it, if the move
/// loses no work at the end of the grace time.
pub fn worktree_move_to_trash(
    permissions: &Permissions,
    request: WorktreeMoveToTrashRequest,
) -> Result<WorktreeMoveToTrashResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let granted = GrantedDirectories::new(&permissions.fs);
    let path = granted.existing(&request.path)?;
    let trash_path = granted.new_directory(&request.trash_path)?;
    let facts = of_linked(repository.gix(), &path, &granted, &request.base_branch)?;
    let checks = Checks {
        expected_head: &request.expected_head,
        allowed_ignored_paths: &request.allowed_ignored_paths,
        allow_unpushed: request.allow_unpushed,
        allow_trash_lock: false,
    };
    let mut refusals = checks.refusals(&facts)?;
    if !on_same_file_system(&path, &trash_path) {
        refusals.push(WorktreeRefusal::OtherFileSystem);
    }
    if !refusals.is_empty() {
        return Ok(WorktreeMoveToTrashResponse {
            message: message(&refusals, &facts),
            refusals: refusals.into_iter().map(Into::into).collect(),
        });
    }
    repository
        .git()?
        .arg("worktree")
        .arg("move")
        .paths([&path, &trash_path])
        .run()?;
    lock(&repository, &trash_path)?;
    Ok(WorktreeMoveToTrashResponse::default())
}

/// Unlocks a worktree of the trash place and moves it back.
pub fn worktree_move_from_trash(
    permissions: &Permissions,
    request: WorktreeMoveFromTrashRequest,
) -> Result<WorktreeMoveFromTrashResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let granted = GrantedDirectories::new(&permissions.fs);
    let trash_path = granted.existing(&request.trash_path)?;
    let path = granted.new_directory(&request.path)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(Error::failed(format!("{} exists", path.display())));
    }
    let facts = of_linked(repository.gix(), &trash_path, &granted, "")?;
    if !is_in_trash(&facts) {
        return Err(Error::failed(format!(
            "{} is not locked with the reason `{TRASH_LOCK_REASON}`",
            trash_path.display()
        )));
    }
    unlock(&repository, &trash_path)?;
    let moved = repository
        .git()?
        .arg("worktree")
        .arg("move")
        .paths([&trash_path, &path])
        .run();
    if let Err(error) = moved {
        lock(&repository, &trash_path)?;
        return Err(error);
    }
    Ok(WorktreeMoveFromTrashResponse {})
}

/// Removes a linked worktree, in the trash place or not, if the removal
/// loses no work. Then, if asked, deletes its branch when the base branch
/// has each change of it.
pub fn worktree_delete(
    permissions: &Permissions,
    request: WorktreeDeleteRequest,
) -> Result<WorktreeDeleteResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let granted = GrantedDirectories::new(&permissions.fs);
    let path = granted.existing(&request.path)?;
    let facts = of_linked(repository.gix(), &path, &granted, &request.base_branch)?;
    let checks = Checks {
        expected_head: &request.expected_head,
        allowed_ignored_paths: &request.allowed_ignored_paths,
        allow_unpushed: request.allow_unpushed,
        allow_trash_lock: true,
    };
    let refusals = checks.refusals(&facts)?;
    if !refusals.is_empty() {
        return Ok(WorktreeDeleteResponse {
            message: message(&refusals, &facts),
            refusals: refusals.into_iter().map(Into::into).collect(),
            ..WorktreeDeleteResponse::default()
        });
    }
    let was_in_trash = is_in_trash(&facts);
    if was_in_trash {
        unlock(&repository, &path)?;
    }
    let removed = repository
        .git()?
        .arg("worktree")
        .arg("remove")
        .paths([&path])
        .run();
    if let Err(error) = removed {
        if was_in_trash {
            lock(&repository, &path)?;
        }
        return Err(error);
    }
    let mut response = WorktreeDeleteResponse::default();
    if request.delete_branch {
        match branch_kept_reason(&facts) {
            Some(reason) => response.branch_kept_reason = reason,
            None => {
                repository
                    .git()?
                    .arg("branch")
                    .arg("-D")
                    .value(&facts.branch)?
                    .run()?;
                response.branch_deleted = true;
            }
        }
    }
    Ok(response)
}

/// Removes the records of the linked worktrees whose directory is missing,
/// if they are the records that the plan saw.
pub fn worktree_prune(
    permissions: &Permissions,
    request: WorktreePruneRequest,
) -> Result<WorktreePruneResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let facts = worktree_facts(
        permissions,
        WorktreeFactsRequest {
            repository: request.repository.clone(),
            base_branch: String::new(),
        },
    )?;
    let prunable: BTreeSet<&str> = facts
        .worktrees
        .iter()
        .filter(|worktree| worktree.is_missing && !worktree.is_locked)
        .map(|worktree| worktree.path.as_str())
        .collect();
    let expected: BTreeSet<&str> = request.paths.iter().map(String::as_str).collect();
    if prunable != expected {
        return Ok(WorktreePruneResponse {
            pruned: Vec::new(),
            refusal: format!(
                "the records to prune changed: git would prune {prunable:?}, the plan has \
                 {expected:?}"
            ),
        });
    }
    repository.git()?.arg("worktree").arg("prune").run()?;
    Ok(WorktreePruneResponse {
        pruned: request.paths,
        refusal: String::new(),
    })
}

/// What a removal must not lose.
struct Checks<'a> {
    expected_head: &'a str,
    allowed_ignored_paths: &'a [String],
    allow_unpushed: bool,
    /// A lock with the reason of the trash place is not a refusal.
    allow_trash_lock: bool,
}

impl Checks<'_> {
    /// The reasons why the removal is refused, the most important first. A
    /// `HEAD` that moved after the plan is an error: the plan is old.
    fn refusals(&self, facts: &WorktreeFacts) -> Result<Vec<WorktreeRefusal>> {
        if facts.head != self.expected_head {
            return Err(Error::failed(format!(
                "HEAD of {} is `{}`, and the plan saw `{}`",
                facts.path, facts.head, self.expected_head
            )));
        }
        let changes_in_base = is_merged_strictly(facts);
        let locked = facts.is_locked && !(self.allow_trash_lock && is_in_trash(facts));
        let candidates = [
            (facts.is_main, WorktreeRefusal::MainWorktree),
            (facts.is_missing, WorktreeRefusal::DirectoryMissing),
            (!facts.error.is_empty(), WorktreeRefusal::FactsUnknown),
            (facts.is_dirty, WorktreeRefusal::UncommittedChanges),
            (
                facts.unpushed_commits > 0 && !self.allow_unpushed && !changes_in_base,
                WorktreeRefusal::UnpushedCommits,
            ),
            (facts.has_detached_commits, WorktreeRefusal::DetachedCommits),
            (locked, WorktreeRefusal::Locked),
            (facts.has_submodules, WorktreeRefusal::Submodule),
            (
                !self.user_paths(facts).is_empty() || facts.ignored_paths_truncated,
                WorktreeRefusal::IgnoredUserFiles,
            ),
        ];
        Ok(candidates
            .into_iter()
            .filter_map(|(set, refusal)| set.then_some(refusal))
            .collect())
    }

    /// The ignored paths that the request does not allow: files of the user.
    fn user_paths<'f>(&self, facts: &'f WorktreeFacts) -> Vec<&'f str> {
        facts
            .ignored_paths
            .iter()
            .map(String::as_str)
            .filter(|path| {
                !self.allowed_ignored_paths.iter().any(|allowed| {
                    path == allowed || allowed.ends_with('/') && path.starts_with(allowed.as_str())
                })
            })
            .collect()
    }
}

/// The strict merge rule: the base branch has each change of the branch.
fn is_merged_strictly(facts: &WorktreeFacts) -> bool {
    matches!(
        facts.merged_by(),
        WorktreeMergedBy::Ancestor | WorktreeMergedBy::Tree
    )
}

/// Why the branch of the worktree stays. `None` when it may be deleted.
fn branch_kept_reason(facts: &WorktreeFacts) -> Option<String> {
    if facts.branch.is_empty() {
        return Some("the worktree has no branch".to_owned());
    }
    if !is_merged_strictly(facts) {
        return Some(format!(
            "the base branch does not have each change of the branch {}",
            facts.branch
        ));
    }
    None
}

fn is_in_trash(facts: &WorktreeFacts) -> bool {
    facts.is_locked && facts.locked_reason == TRASH_LOCK_REASON
}

fn message(refusals: &[WorktreeRefusal], facts: &WorktreeFacts) -> String {
    let reasons: Vec<&str> = refusals
        .iter()
        .map(|refusal| match refusal {
            WorktreeRefusal::MainWorktree => "it is the main worktree",
            WorktreeRefusal::DirectoryMissing => "its directory is missing",
            WorktreeRefusal::FactsUnknown => "its facts are not known",
            WorktreeRefusal::UncommittedChanges => "it has changes that are not committed",
            WorktreeRefusal::UnpushedCommits => "it has commits that are not pushed",
            WorktreeRefusal::DetachedCommits => "its detached HEAD has commits that no branch has",
            WorktreeRefusal::Locked => "it is locked",
            WorktreeRefusal::Submodule => "it has a submodule",
            WorktreeRefusal::IgnoredUserFiles => "it has ignored files that are not build output",
            WorktreeRefusal::OtherFileSystem => "the trash place is on a different file system",
            _ => "it is refused",
        })
        .collect();
    format!("{} stays: {}", facts.path, reasons.join(", "))
}

fn on_same_file_system(path: &Path, new_path: &Path) -> bool {
    let parent = new_path.parent().unwrap_or(new_path);
    match (fs::metadata(path), fs::metadata(parent)) {
        (Ok(path), Ok(parent)) => path.dev() == parent.dev(),
        _ => false,
    }
}

fn lock(repository: &Repository, path: &Path) -> Result<()> {
    repository
        .git()?
        .arg("worktree")
        .arg("lock")
        .arg("--reason")
        .arg(TRASH_LOCK_REASON)
        .paths([path])
        .run()?;
    Ok(())
}

fn unlock(repository: &Repository, path: &Path) -> Result<()> {
    repository
        .git()?
        .arg("worktree")
        .arg("unlock")
        .paths([path])
        .run()?;
    Ok(())
}
