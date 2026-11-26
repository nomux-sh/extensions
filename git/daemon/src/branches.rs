//! The `branches`, `switch` and `create_branch` methods.

use gix::refs::FullName;
use nomux_extension::Permissions;

use crate::commit_info::commit_info;
use crate::error::Result;
use crate::repository::Repository;
use crate::upstream;
use crate::wire::v1::{
    Branch, BranchesRequest, BranchesResponse, CreateBranchRequest, CreateBranchResponse,
    SwitchRequest, SwitchResponse,
};

/// Lists the local branches and the remote-tracking branches.
pub fn branches(permissions: &Permissions, request: BranchesRequest) -> Result<BranchesResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let gix = repository.gix();
    let head = gix.head_name()?;
    let references = gix.references()?;
    let mut branches = Vec::new();
    for reference in references.local_branches()? {
        branches.push(branch(gix, reference?, head.as_ref(), false)?);
    }
    for reference in references.remote_branches()? {
        let reference = reference?;
        // `origin/HEAD` names the default branch of the remote. It is not a
        // branch.
        if reference.name().shorten().ends_with(b"/HEAD") {
            continue;
        }
        branches.push(branch(gix, reference, head.as_ref(), true)?);
    }
    Ok(BranchesResponse { branches })
}

fn branch(
    repository: &gix::Repository,
    mut reference: gix::Reference<'_>,
    head: Option<&FullName>,
    is_remote: bool,
) -> Result<Branch> {
    let upstream = match is_remote {
        true => None,
        false => upstream::of_branch(repository, reference.name())?,
    };
    Ok(Branch {
        name: reference.name().shorten().to_string(),
        is_remote,
        is_head: head.is_some_and(|head| head.as_ref() == reference.name()),
        upstream: upstream.map_or_else(String::new, |upstream| upstream.shorten().to_string()),
        commit: Some(commit_info(&reference.peel_to_commit()?)?),
    })
}

/// Switches the worktree to a branch.
pub fn switch(permissions: &Permissions, request: SwitchRequest) -> Result<SwitchResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    repository
        .git()?
        .arg("switch")
        .value(&request.branch)?
        .run()?;
    Ok(SwitchResponse {})
}

/// Creates a branch, and switches to it if the request says so.
pub fn create_branch(
    permissions: &Permissions,
    request: CreateBranchRequest,
) -> Result<CreateBranchResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let command = repository.git()?;
    let mut command = match request.switch {
        true => command.arg("switch").arg("--create"),
        false => command.arg("branch"),
    }
    .value(&request.name)?;
    if !request.start_point.is_empty() {
        command = command.value(&request.start_point)?;
    }
    command.run()?;
    Ok(CreateBranchResponse {})
}
