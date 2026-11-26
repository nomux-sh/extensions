//! The revisions that a request names.

use crate::error::{Error, Result};

/// Finds the commit of a revision, for example `HEAD~2` or a branch name. An
/// empty revision is `HEAD`.
pub fn commit<'repo>(
    repository: &'repo gix::Repository,
    revision: &str,
) -> Result<gix::Commit<'repo>> {
    let revision = if revision.is_empty() {
        "HEAD"
    } else {
        revision
    };
    let id = repository.rev_parse_single(revision).map_err(|error| {
        Error::not_found(format!("the revision `{revision}` was not found: {error}"))
    })?;
    Ok(id.object()?.peel_to_commit()?)
}
