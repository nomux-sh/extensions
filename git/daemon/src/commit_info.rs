//! The data of a commit that the lists of commits have.

use crate::error::Result;
use crate::wire::v1::CommitInfo;

/// Reads the identity, the author and the summary of a commit.
pub fn commit_info(commit: &gix::Commit<'_>) -> Result<CommitInfo> {
    let author = commit.author()?;
    Ok(CommitInfo {
        id: commit.id.to_string(),
        parent_ids: commit.parent_ids().map(|id| id.to_string()).collect(),
        author_name: author.name.to_string(),
        author_email: author.email.to_string(),
        author_time: author.time()?.seconds,
        summary: commit.message()?.summary().to_string(),
    })
}
