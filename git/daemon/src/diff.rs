//! The `diff` method: the hunks of the changed files.

use nomux_extension::Permissions;

use crate::content::{Content, Contents, Version};
use crate::error::{Error, Result};
use crate::line_diff::{self, Change};
use crate::repository::Repository;
use crate::revision;
use crate::status::{self, Changes};
use crate::wire::v1::{
    ChangeKind, DiffKind, DiffRequest, DiffResponse, FileDiff, FileStatus, Hunk, Line, LineKind,
};

/// The number of unchanged lines before and after the changes of a hunk.
const CONTEXT_LINES: u32 = 3;

/// The largest sum of the hunks of one result. A result must go into one
/// envelope of the control protocol of nomux, which has 1 MiB.
pub const MAX_RESPONSE_BYTES: usize = 768 * 1024;

/// The bytes that one line adds to a result without its content.
const LINE_OVERHEAD_BYTES: usize = 16;

/// Compares two versions of the changed files, or of one file.
pub fn diff(permissions: &Permissions, request: DiffRequest) -> Result<DiffResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let gix = repository.gix();
    let path = match request.path.as_str() {
        "" => None,
        path => Some(repository.path(path)?),
    };
    let (old, new) = versions(gix, request.kind(), &request.revision)?;
    let changed = status::files(
        gix,
        Changes {
            tree: match old {
                Version::Tree(tree) => Some(tree),
                Version::Index | Version::Worktree => None,
            },
            path: path.as_deref(),
            each_untracked_file: true,
        },
    )?;
    let mut contents = Contents::new(gix)?;
    let mut budget = MAX_RESPONSE_BYTES;
    let mut files = Vec::new();
    for file in changed
        .iter()
        .filter(|file| is_changed(request.kind(), file))
    {
        files.extend(file_diff(&mut contents, old, new, file, &mut budget)?);
    }
    Ok(DiffResponse { files })
}

fn versions(
    repository: &gix::Repository,
    kind: DiffKind,
    revision: &str,
) -> Result<(Version, Version)> {
    match kind {
        DiffKind::Unstaged => Ok((Version::Index, Version::Worktree)),
        DiffKind::Staged => Ok((
            Version::Tree(repository.head_tree_id_or_empty()?.detach()),
            Version::Index,
        )),
        DiffKind::Revision => {
            let tree = revision::commit(repository, revision)?.tree_id()?.detach();
            Ok((Version::Tree(tree), Version::Worktree))
        }
        DiffKind::Unspecified => Err(Error::invalid_argument("the kind of the diff is not set")),
    }
}

fn is_changed(kind: DiffKind, file: &FileStatus) -> bool {
    let staged = file.staged() != ChangeKind::Unspecified;
    let unstaged = file.unstaged() != ChangeKind::Unspecified || file.conflict != 0;
    match kind {
        DiffKind::Unstaged => unstaged,
        DiffKind::Staged => staged,
        DiffKind::Revision | DiffKind::Unspecified => staged || unstaged,
    }
}

/// Makes the diff of one file. `None` when the two versions are equal.
fn file_diff(
    contents: &mut Contents<'_>,
    old: Version,
    new: Version,
    file: &FileStatus,
    budget: &mut usize,
) -> Result<Option<FileDiff>> {
    let renamed = matches!(old, Version::Tree(_)) && !file.original_path.is_empty();
    let old_path = if renamed {
        &file.original_path
    } else {
        &file.path
    };
    let mut diff = FileDiff {
        path: file.path.clone(),
        original_path: if renamed {
            old_path.clone()
        } else {
            String::new()
        },
        ..FileDiff::default()
    };
    if *budget == 0 {
        diff.change = ChangeKind::Modified.into();
        diff.too_large = true;
        return Ok(Some(diff));
    }
    let old = contents.read(old, old_path)?;
    let new = contents.read(new, &file.path)?;
    diff.change = match (&old, &new) {
        (Content::Missing, Content::Missing) => return Ok(None),
        (Content::Missing, _) => ChangeKind::Added,
        (_, Content::Missing) => ChangeKind::Deleted,
        _ if renamed => file.staged(),
        _ => ChangeKind::Modified,
    }
    .into();
    match (old, new) {
        (Content::TooLarge, _) | (_, Content::TooLarge) => diff.too_large = true,
        (Content::Binary, _) | (_, Content::Binary) => diff.binary = true,
        (old, new) => {
            let (old, new) = (text(old), text(new));
            if old == new && !renamed {
                return Ok(None);
            }
            let hunks = hunks(&old, &new);
            let size: usize = hunks.iter().map(hunk_size).sum();
            if size > *budget {
                *budget = 0;
                diff.too_large = true;
            } else {
                *budget -= size;
                diff.hunks = hunks;
            }
        }
    }
    Ok(Some(diff))
}

fn text(content: Content) -> Vec<u8> {
    match content {
        Content::Text(text) => text,
        Content::Missing | Content::Binary | Content::TooLarge => Vec::new(),
    }
}

fn hunk_size(hunk: &Hunk) -> usize {
    hunk.lines
        .iter()
        .map(|line| line.content.len() + LINE_OVERHEAD_BYTES)
        .sum()
}

/// Groups the changes into hunks with [`CONTEXT_LINES`] around them, as a
/// unified diff does.
fn hunks(old: &[u8], new: &[u8]) -> Vec<Hunk> {
    let (old_lines, new_lines) = (line_diff::lines(old), line_diff::lines(new));
    let changes = line_diff::changes(old, new);
    changes
        .chunk_by(|first, second| second.before.start - first.before.end <= 2 * CONTEXT_LINES)
        .map(|group| hunk(group, &old_lines, &new_lines))
        .collect()
}

fn hunk(changes: &[Change], old_lines: &[&[u8]], new_lines: &[&[u8]]) -> Hunk {
    let (first, last) = (&changes[0], &changes[changes.len() - 1]);
    let leading = first.before.start.min(CONTEXT_LINES);
    let old_end = (last.before.end + CONTEXT_LINES).min(old_lines.len() as u32);
    let mut old = first.before.start - leading;
    let mut new = first.after.start - leading;
    let mut hunk = Hunk {
        old_start: old,
        new_start: new,
        ..Hunk::default()
    };
    let mut push = |kind: LineKind, old: Option<u32>, new: Option<u32>| {
        let content = match (old, new) {
            (_, Some(new)) => new_lines[new as usize],
            (Some(old), None) => old_lines[old as usize],
            (None, None) => unreachable!("a line is in one version at least"),
        };
        hunk.lines.push(Line {
            kind: kind.into(),
            old_number: old.map_or(0, |old| old + 1),
            new_number: new.map_or(0, |new| new + 1),
            content: content.to_vec(),
        });
    };
    for change in changes {
        while old < change.before.start {
            push(LineKind::Context, Some(old), Some(new));
            (old, new) = (old + 1, new + 1);
        }
        for removed in change.before.clone() {
            push(LineKind::Removed, Some(removed), None);
        }
        for added in change.after.clone() {
            push(LineKind::Added, None, Some(added));
        }
        (old, new) = (change.before.end, change.after.end);
    }
    while old < old_end {
        push(LineKind::Context, Some(old), Some(new));
        (old, new) = (old + 1, new + 1);
    }
    hunk.old_lines = old - hunk.old_start;
    hunk.new_lines = new - hunk.new_start;
    // As in a unified diff: a side with lines starts at its first line, and a
    // side without lines names the line before the change.
    hunk.old_start += u32::from(hunk.old_lines > 0);
    hunk.new_start += u32::from(hunk.new_lines > 0);
    hunk
}
