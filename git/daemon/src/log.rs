//! The `log` method: one page of the history.

use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;
use nomux_extension::Permissions;

use crate::commit_info::commit_info;
use crate::error::Result;
use crate::repository::Repository;
use crate::revision;
use crate::wire::v1::{LogRequest, LogResponse};

/// The number of commits in a page when the request gives no limit.
const DEFAULT_LIMIT: u32 = 50;

/// The largest number of commits in a page.
pub const MAX_LIMIT: u32 = 500;

/// Lists the commits that a revision can reach, the newest first.
pub fn log(permissions: &Permissions, request: LogRequest) -> Result<LogResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let gix = repository.gix();
    let mut response = LogResponse::default();
    if request.revision.is_empty() && gix.head()?.is_unborn() {
        return Ok(response);
    }
    let limit = match request.limit {
        0 => DEFAULT_LIMIT,
        limit => limit.min(MAX_LIMIT),
    } as usize;
    let start = revision::commit(gix, &request.revision)?.id;
    let walk = gix
        .rev_walk([start])
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()?;
    for info in walk.skip(request.skip as usize) {
        if response.commits.len() == limit {
            response.has_more = true;
            break;
        }
        response.commits.push(commit_info(&info?.object()?)?);
    }
    Ok(response)
}
