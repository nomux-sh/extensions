//! The upstream branch of a local branch and the commits that it does not
//! have.

use gix::ObjectId;
use gix::refs::{FullName, FullNameRef};
use gix::remote::Direction;

use crate::error::Result;

/// The remote-tracking branch that `branch` follows, from the configuration
/// of the repository.
pub fn of_branch(repository: &gix::Repository, branch: &FullNameRef) -> Result<Option<FullName>> {
    Ok(repository
        .branch_remote_tracking_ref_name(branch, Direction::Fetch)
        .transpose()?)
}

/// The commit of the upstream branch. `None` when the reference does not
/// exist, for example before the first fetch.
pub fn commit(repository: &gix::Repository, upstream: &FullNameRef) -> Result<Option<ObjectId>> {
    let Some(reference) = repository.try_find_reference(upstream)? else {
        return Ok(None);
    };
    Ok(Some(reference.into_fully_peeled_id()?.detach()))
}

/// The number of commits that `commit` has and that no remote-tracking
/// branch has.
pub fn unpushed_commits(repository: &gix::Repository, commit: ObjectId) -> Result<usize> {
    let mut pushed = Vec::new();
    for reference in repository.references()?.remote_branches()? {
        pushed.push(reference?.into_fully_peeled_id()?.detach());
    }
    let mut count = 0;
    for commit in repository.rev_walk([commit]).with_hidden(pushed).all()? {
        commit?;
        count += 1;
    }
    Ok(count)
}
