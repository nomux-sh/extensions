//! The `worktrees`, `worktree_add` and `worktree_remove` methods.

use std::path::Path;

use nomux_extension::Permissions;

use crate::error::{Error, Result};
use crate::path_safety::GrantedDirectories;
use crate::repository::Repository;
use crate::status::{self, Changes};
use crate::upstream;
use crate::wire::v1::{
    RefusalReason, RemovalRefusal, Worktree, WorktreeAddRequest, WorktreeAddResponse,
    WorktreeRemoveRequest, WorktreeRemoveResponse, WorktreesRequest, WorktreesResponse,
};

/// Lists the main worktree and the linked worktrees of the repository.
pub fn worktrees(
    permissions: &Permissions,
    request: WorktreesRequest,
) -> Result<WorktreesResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let gix = repository.gix();
    let mut worktrees = Vec::new();
    worktrees.extend(worktree(&gix.main_repo()?, true, false)?);
    for proxy in gix.worktrees()? {
        let is_locked = proxy.is_locked();
        let linked = proxy.into_repo_with_possibly_inaccessible_worktree()?;
        worktrees.extend(worktree(&linked, false, is_locked)?);
    }
    Ok(WorktreesResponse { worktrees })
}

/// Describes one worktree. `None` for a bare repository.
fn worktree(
    repository: &gix::Repository,
    is_main: bool,
    is_locked: bool,
) -> Result<Option<Worktree>> {
    let Some(path) = repository.workdir() else {
        return Ok(None);
    };
    let head = repository.head()?;
    Ok(Some(Worktree {
        path: path
            .canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .display()
            .to_string(),
        branch: head
            .referent_name()
            .map_or_else(String::new, |name| name.shorten().to_string()),
        head: head.id().map_or_else(String::new, |id| id.to_string()),
        is_main,
        is_locked,
    }))
}

/// Adds a linked worktree in a directory that the user approved.
pub fn worktree_add(
    permissions: &Permissions,
    request: WorktreeAddRequest,
) -> Result<WorktreeAddResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let path = GrantedDirectories::new(&permissions.fs).new_directory(&request.path)?;
    let mut command = repository.git()?.arg("worktree").arg("add");
    if !request.new_branch.is_empty() {
        command = command.arg("-b").value(&request.new_branch)?;
    }
    command.paths([&path]).value(&request.branch)?.run()?;
    Ok(WorktreeAddResponse {})
}

/// Removes a linked worktree if the removal loses no work. If it has work
/// that is not committed or not pushed, the worktree stays and the result
/// has the reason.
pub fn worktree_remove(
    permissions: &Permissions,
    request: WorktreeRemoveRequest,
) -> Result<WorktreeRemoveResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let path = GrantedDirectories::new(&permissions.fs).existing(&request.path)?;
    let linked = linked_worktree(repository.gix(), &path)?;
    if let Some(refusal) = refusal(&linked)? {
        return Ok(WorktreeRemoveResponse {
            removed: false,
            refusal: Some(refusal),
        });
    }
    repository
        .git()?
        .arg("worktree")
        .arg("remove")
        .paths([&path])
        .run()?;
    Ok(WorktreeRemoveResponse {
        removed: true,
        refusal: None,
    })
}

fn linked_worktree(repository: &gix::Repository, path: &Path) -> Result<gix::Repository> {
    for proxy in repository.worktrees()? {
        if proxy.base()?.canonicalize().is_ok_and(|base| base == path) {
            return Ok(proxy.into_repo()?);
        }
    }
    Err(Error::not_found(format!(
        "{} is not a linked worktree of the repository",
        path.display()
    )))
}

/// The work that the removal of the worktree would lose.
fn refusal(worktree: &gix::Repository) -> Result<Option<RemovalRefusal>> {
    let changed = status::files(worktree, Changes::default())?.len();
    if changed > 0 {
        return Ok(Some(RemovalRefusal {
            reason: RefusalReason::UncommittedChanges.into(),
            message: format!(
                "the worktree has {changed} paths with changes that are not committed"
            ),
        }));
    }
    let Some(head) = worktree.head()?.id() else {
        return Ok(None);
    };
    let unpushed = upstream::unpushed_commits(worktree, head.detach())?;
    if unpushed > 0 {
        return Ok(Some(RemovalRefusal {
            reason: RefusalReason::UnpushedCommits.into(),
            message: format!(
                "the worktree has {unpushed} commits that no remote-tracking branch has"
            ),
        }));
    }
    Ok(None)
}
