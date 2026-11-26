//! The `blame` method: the commit that added each line of one page of a
//! file.

use std::collections::BTreeSet;

use gix::blame::BlameRanges;
use gix::bstr::BStr;
use gix::repository::blame_file::Options;
use nomux_extension::Permissions;

use crate::commit_info::commit_info;
use crate::error::Result;
use crate::line_diff;
use crate::repository::Repository;
use crate::revision;
use crate::wire::v1::{BlameHunk, BlameRequest, BlameResponse};

/// The number of lines in a page when the request gives no limit.
const DEFAULT_LINES: u32 = 500;

/// The largest number of lines in a page.
pub const MAX_LINES: u32 = 2000;

/// Finds the commits that added the lines of one page of a file, in the
/// version of a revision.
pub fn blame(permissions: &Permissions, request: BlameRequest) -> Result<BlameResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let gix = repository.gix();
    let path = repository.path(&request.path)?;
    let commit = revision::commit(gix, &request.revision)?;
    let first = request.start_line.max(1);
    let count = match request.max_lines {
        0 => DEFAULT_LINES,
        count => count.min(MAX_LINES),
    };
    let options = Options {
        ranges: BlameRanges::from_one_based_inclusive_range(
            first..=first.saturating_add(count - 1),
        )?,
        ..Options::default()
    };
    let outcome = gix.blame_file(BStr::new(&path), commit.id, options)?;
    let hunks: Vec<BlameHunk> = outcome
        .entries
        .iter()
        .map(|entry| BlameHunk {
            start_line: entry.start_in_blamed_file + 1,
            line_count: entry.len.get(),
            commit_id: entry.commit_id.to_string(),
            original_start_line: entry.start_in_source_file + 1,
            original_path: entry
                .source_file_name
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
        })
        .collect();
    let commits: BTreeSet<_> = outcome
        .entries
        .iter()
        .map(|entry| entry.commit_id)
        .collect();
    Ok(BlameResponse {
        hunks,
        commits: commits
            .into_iter()
            .map(|id| commit_info(&gix.find_commit(id)?))
            .collect::<Result<_>>()?,
        total_lines: line_diff::lines(&outcome.blob).len() as u32,
    })
}
