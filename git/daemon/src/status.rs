//! The status of a worktree: its branch, its upstream and its files that are
//! not clean.

use std::collections::BTreeMap;

use gix::ObjectId;
use gix::bstr::{BStr, BString};
use gix::diff::index::Change;
use gix::dir::entry::Kind;
use gix::index::entry::Mode;
use gix::state::InProgress;
use gix::status::index_worktree::Item;
use gix::status::plumbing::index_as_worktree::{Conflict, EntryStatus};
use gix::status::plumbing::index_as_worktree_with_renames::Summary;
use gix::status::{Item as StatusItem, UntrackedFiles};

use crate::error::Result;
use crate::upstream;
use crate::wire::v1::{ChangeKind, ConflictKind, FileStatus, Operation, Status};

/// Computes the status of the worktree, as `git status` does with the
/// configuration of the repository.
pub fn status(repository: &gix::Repository) -> Result<Status> {
    let mut status = Status {
        operation: operation(repository.state()).into(),
        files: files(repository, Changes::default())?,
        ..Status::default()
    };
    let head = repository.head()?;
    let commit = head.id().map(gix::Id::detach);
    status.head = commit.map(|id| id.to_string()).unwrap_or_default();
    let Some(branch) = head.referent_name() else {
        return Ok(status);
    };
    status.branch = branch.shorten().to_string();
    let Some(upstream) = upstream::of_branch(repository, branch)? else {
        return Ok(status);
    };
    status.upstream = upstream.shorten().to_string();
    if let (Some(commit), Some(upstream)) =
        (commit, upstream::commit(repository, upstream.as_ref())?)
    {
        let counts = repository.ahead_behind_counts(commit, upstream)?;
        status.ahead = u32::try_from(counts.ahead).unwrap_or(u32::MAX);
        status.behind = u32::try_from(counts.behind).unwrap_or(u32::MAX);
    }
    Ok(status)
}

/// The part of the changes that [`files`] reports.
#[derive(Default)]
pub struct Changes<'a> {
    /// The tree that the index is compared with. `None` for the tree of
    /// `HEAD`.
    pub tree: Option<ObjectId>,
    /// Only this file. `None` for all files.
    pub path: Option<&'a str>,
    /// Report each untracked file, not one entry for a directory that has
    /// only untracked files.
    pub each_untracked_file: bool,
}

/// The files that are not clean, in the sequence of their paths. A file that
/// has a staged and an unstaged change is one item.
pub fn files(repository: &gix::Repository, changes: Changes<'_>) -> Result<Vec<FileStatus>> {
    // Like `git status`, a rename is only found between `HEAD` and the index.
    let mut platform = repository
        .status(gix::progress::Discard)?
        .index_worktree_rewrites(None);
    if let Some(tree) = changes.tree {
        platform = platform.head_tree(tree);
    }
    if changes.each_untracked_file {
        platform = platform.untracked_files(UntrackedFiles::Files);
    }
    let patterns = changes
        .path
        .map(|path| BString::from(format!(":(literal){path}")));
    let mut files = BTreeMap::new();
    for item in platform.into_iter(patterns)? {
        match item? {
            StatusItem::TreeIndex(change) => record_staged(&mut files, change),
            StatusItem::IndexWorktree(item) => record_unstaged(&mut files, &item),
        }
    }
    Ok(files.into_values().collect())
}

fn file<'a>(files: &'a mut BTreeMap<String, FileStatus>, path: &BStr) -> &'a mut FileStatus {
    let path = path.to_string();
    files.entry(path.clone()).or_insert_with(|| FileStatus {
        path,
        ..FileStatus::default()
    })
}

fn record_staged(files: &mut BTreeMap<String, FileStatus>, change: Change) {
    let file = file(files, change.location());
    let kind = match change {
        Change::Addition { .. } => ChangeKind::Added,
        Change::Deletion { .. } => ChangeKind::Deleted,
        Change::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } if file_type(previous_entry_mode) != file_type(entry_mode) => ChangeKind::TypeChanged,
        Change::Modification { .. } => ChangeKind::Modified,
        Change::Rewrite {
            source_location,
            copy,
            ..
        } => {
            file.original_path = source_location.to_string();
            if copy {
                ChangeKind::Copied
            } else {
                ChangeKind::Renamed
            }
        }
    };
    file.staged = kind.into();
}

/// The file type bits of a mode, without the permission bits.
fn file_type(mode: Mode) -> u32 {
    mode.bits() & 0o170000
}

fn record_unstaged(files: &mut BTreeMap<String, FileStatus>, item: &Item) {
    if let Item::Modification {
        status: EntryStatus::Conflict { summary, .. },
        rela_path,
        ..
    } = item
    {
        file(files, rela_path.as_ref()).conflict = conflict_kind(*summary).into();
        return;
    }
    let kind = match item.summary() {
        Some(Summary::Added) => ChangeKind::Untracked,
        Some(Summary::IntentToAdd) => ChangeKind::Added,
        Some(Summary::Removed) => ChangeKind::Deleted,
        Some(Summary::TypeChange) => ChangeKind::TypeChanged,
        Some(Summary::Modified | Summary::Renamed | Summary::Copied) => ChangeKind::Modified,
        Some(Summary::Conflict) | None => return,
    };
    let mut path = item.rela_path().to_owned();
    if let Item::DirectoryContents { entry, .. } = item
        && matches!(entry.disk_kind, Some(Kind::Directory | Kind::Repository))
    {
        path.push(b'/');
    }
    file(files, path.as_ref()).unstaged = kind.into();
}

fn conflict_kind(conflict: Conflict) -> ConflictKind {
    match conflict {
        Conflict::BothDeleted => ConflictKind::BothDeleted,
        Conflict::AddedByUs => ConflictKind::AddedByUs,
        Conflict::DeletedByThem => ConflictKind::DeletedByThem,
        Conflict::AddedByThem => ConflictKind::AddedByThem,
        Conflict::DeletedByUs => ConflictKind::DeletedByUs,
        Conflict::BothAdded => ConflictKind::BothAdded,
        Conflict::BothModified => ConflictKind::BothModified,
    }
}

fn operation(state: Option<InProgress>) -> Operation {
    match state {
        None => Operation::Unspecified,
        Some(InProgress::Merge) => Operation::Merge,
        Some(InProgress::Rebase | InProgress::RebaseInteractive) => Operation::Rebase,
        Some(InProgress::CherryPick | InProgress::CherryPickSequence) => Operation::CherryPick,
        Some(InProgress::Revert | InProgress::RevertSequence) => Operation::Revert,
        Some(InProgress::Bisect) => Operation::Bisect,
        Some(InProgress::ApplyMailbox | InProgress::ApplyMailboxRebase) => Operation::ApplyMailbox,
    }
}
