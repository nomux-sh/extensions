//! The `stage` and `unstage` methods: files, hunks and lines.

use std::os::unix::fs::PermissionsExt;

use gix::index::entry::Mode;
use nomux_extension::Permissions;

use crate::content::{Content, Contents, Version};
use crate::error::{Error, Result};
use crate::line_diff;
use crate::repository::Repository;
use crate::wire::v1::{
    LineRange, LineSelection, StageRequest, StageResponse, UnstageRequest, UnstageResponse,
};

/// Copies changes from the worktree to the index.
pub fn stage(permissions: &Permissions, request: StageRequest) -> Result<StageResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let paths = checked_paths(&repository, &request.paths, &request.selections)?;
    if !paths.is_empty() {
        repository
            .git()?
            .arg("add")
            .arg("--all")
            .paths(paths)
            .run()?;
    }
    for selection in &request.selections {
        apply_selection(&repository, selection, Action::Stage)?;
    }
    Ok(StageResponse {})
}

/// Removes changes from the index. The worktree does not change.
pub fn unstage(permissions: &Permissions, request: UnstageRequest) -> Result<UnstageResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let paths = checked_paths(&repository, &request.paths, &request.selections)?;
    if !paths.is_empty() {
        repository
            .git()?
            .arg("reset")
            .arg("--quiet")
            .paths(paths)
            .run()?;
    }
    for selection in &request.selections {
        apply_selection(&repository, selection, Action::Unstage)?;
    }
    Ok(UnstageResponse {})
}

fn checked_paths(
    repository: &Repository,
    paths: &[String],
    selections: &[LineSelection],
) -> Result<Vec<String>> {
    if paths.is_empty() && selections.is_empty() {
        return Err(Error::invalid_argument(
            "the request names no file and no line",
        ));
    }
    paths.iter().map(|path| repository.path(path)).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Stage,
    Unstage,
}

/// Writes a new version of one file to the index: the old version of the
/// diff with a part of its changes.
fn apply_selection(
    repository: &Repository,
    selection: &LineSelection,
    action: Action,
) -> Result<()> {
    let path = repository.path(&selection.path)?;
    let gix = repository.gix();
    let mut contents = Contents::new(gix)?;
    let (old, new) = match action {
        Action::Stage => (Version::Index, Version::Worktree),
        Action::Unstage => (
            Version::Tree(gix.head_tree_id_or_empty()?.detach()),
            Version::Index,
        ),
    };
    let old = contents.read(old, &path)?;
    let is_new_file = matches!(old, Content::Missing);
    let (old, new) = (text(old, &path)?, text(contents.read(new, &path)?, &path)?);
    // To unstage lines is to keep all the staged changes but those lines.
    let staged = with_changes(&old, &new, selection, action == Action::Stage).ok_or_else(|| {
        Error::invalid_argument(format!("the selection has no changed line of `{path}`"))
    })?;
    if action == Action::Unstage && is_new_file && staged.is_empty() {
        repository
            .git()?
            .arg("reset")
            .arg("--quiet")
            .paths([&path])
            .run()?;
        return Ok(());
    }
    let mode = match contents.index_mode(&path) {
        Some(mode) => mode,
        None => worktree_mode(repository, &path)?,
    };
    let blob = repository
        .git()?
        .arg("hash-object")
        .arg("-w")
        .arg("--stdin")
        .input(staged)
        .run()?;
    let blob = String::from_utf8_lossy(&blob);
    repository
        .git()?
        .arg("update-index")
        .arg("--add")
        .arg("--cacheinfo")
        .arg(format!("{:o},{},{path}", mode.bits(), blob.trim()))
        .run()?;
    Ok(())
}

fn text(content: Content, path: &str) -> Result<Vec<u8>> {
    match content {
        Content::Text(text) => Ok(text),
        Content::Missing => Ok(Vec::new()),
        Content::Binary => Err(Error::invalid_argument(format!(
            "`{path}` is not a text file: stage the file, not its lines"
        ))),
        Content::TooLarge => Err(Error::invalid_argument(format!(
            "`{path}` is too large: stage the file, not its lines"
        ))),
    }
}

/// Makes the text that has the selected changes from `old` to `new`, or all
/// the changes but the selected ones. `None` when the selection has no
/// changed line.
fn with_changes(
    old: &[u8],
    new: &[u8],
    selection: &LineSelection,
    apply_selected: bool,
) -> Option<Vec<u8>> {
    let (old_lines, new_lines) = (line_diff::lines(old), line_diff::lines(new));
    let mut text = Vec::new();
    let mut push = |line: &[u8]| {
        // A last line without a line end gets one when a line follows it.
        if text.last().is_some_and(|byte| *byte != b'\n') {
            text.push(b'\n');
        }
        text.extend_from_slice(line);
    };
    let mut selected_lines = 0;
    let mut applies = |ranges: &[LineRange], index: u32| {
        let is_selected = contains(ranges, index + 1);
        selected_lines += u32::from(is_selected);
        is_selected == apply_selected
    };
    let mut unchanged_from = 0;
    for change in line_diff::changes(old, new) {
        for line in &old_lines[unchanged_from..change.before.start as usize] {
            push(line);
        }
        for removed in change.before.clone() {
            if !applies(&selection.old_lines, removed) {
                push(old_lines[removed as usize]);
            }
        }
        for added in change.after.clone() {
            if applies(&selection.new_lines, added) {
                push(new_lines[added as usize]);
            }
        }
        unchanged_from = change.before.end as usize;
    }
    for line in &old_lines[unchanged_from..] {
        push(line);
    }
    (selected_lines > 0).then_some(text)
}

fn contains(ranges: &[LineRange], number: u32) -> bool {
    ranges
        .iter()
        .any(|range| number >= range.start && number - range.start < range.count)
}

/// The mode that `git add` gives to a file that is not in the index.
fn worktree_mode(repository: &Repository, path: &str) -> Result<Mode> {
    let metadata = repository.worktree().join(path).symlink_metadata()?;
    Ok(if metadata.is_symlink() {
        Mode::SYMLINK
    } else if metadata.permissions().mode() & 0o111 != 0 {
        Mode::FILE_EXECUTABLE
    } else {
        Mode::FILE
    })
}
