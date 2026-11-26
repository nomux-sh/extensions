//! The hunks of a diff, its flags and its limits.

mod repositories;

use std::fs;

use nomux_git::diff::{MAX_RESPONSE_BYTES, diff};
use nomux_git::wire::v1::{ChangeKind, DiffKind, DiffRequest, DiffResponse, FileDiff, LineKind};
use repositories::TestRepository;

fn diff_of(repository: &TestRepository, kind: DiffKind, path: &str) -> DiffResponse {
    let request = DiffRequest {
        repository: repository.path(),
        kind: kind.into(),
        path: path.to_owned(),
        ..DiffRequest::default()
    };
    diff(&repository.permissions(), request).unwrap()
}

/// The lines of the hunks in the form of a unified diff, with the two line
/// numbers of each line.
fn rendered(file: &FileDiff) -> Vec<String> {
    let mut lines = Vec::new();
    for hunk in &file.hunks {
        lines.push(format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));
        for line in &hunk.lines {
            let sign = match line.kind() {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
                LineKind::Unspecified => '?',
            };
            let content = String::from_utf8_lossy(&line.content);
            lines.push(format!(
                "{sign}{}:{} {}",
                line.old_number,
                line.new_number,
                content.trim_end()
            ));
        }
    }
    lines
}

fn numbered_lines(count: u32) -> String {
    (1..=count)
        .map(|number| format!("line {number}\n"))
        .collect()
}

#[test]
fn changes_that_are_far_apart_are_two_hunks_with_three_lines_of_context() {
    let repository = TestRepository::with_files(&[("file.txt", &numbered_lines(20))]);
    let changed = numbered_lines(20)
        .replace("line 2\n", "line two\n")
        .replace("line 15\n", "line 15\nnew line\n");
    repository.write("file.txt", &changed);

    let response = diff_of(&repository, DiffKind::Unstaged, "");

    assert_eq!(response.files.len(), 1);
    assert_eq!(response.files[0].change(), ChangeKind::Modified);
    assert_eq!(
        rendered(&response.files[0]),
        [
            "@@ -1,5 +1,5 @@",
            " 1:1 line 1",
            "-2:0 line 2",
            "+0:2 line two",
            " 3:3 line 3",
            " 4:4 line 4",
            " 5:5 line 5",
            "@@ -13,6 +13,7 @@",
            " 13:13 line 13",
            " 14:14 line 14",
            " 15:15 line 15",
            "+0:16 new line",
            " 16:17 line 16",
            " 17:18 line 17",
            " 18:19 line 18",
        ]
    );
}

#[test]
fn each_kind_compares_its_two_versions() {
    let repository = TestRepository::with_files(&[("file.txt", "committed\n")]);
    repository.write("file.txt", "staged\n");
    repository.git(&["add", "file.txt"]);
    repository.write("file.txt", "worktree\n");

    let lines = |kind| rendered(&diff_of(&repository, kind, "").files[0]);

    assert_eq!(
        lines(DiffKind::Staged),
        ["@@ -1,1 +1,1 @@", "-1:0 committed", "+0:1 staged"]
    );
    assert_eq!(
        lines(DiffKind::Unstaged),
        ["@@ -1,1 +1,1 @@", "-1:0 staged", "+0:1 worktree"]
    );
    assert_eq!(
        lines(DiffKind::Revision),
        ["@@ -1,1 +1,1 @@", "-1:0 committed", "+0:1 worktree"]
    );
}

#[test]
fn a_revision_diff_starts_at_the_tree_of_the_revision() {
    let repository = TestRepository::with_files(&[("file.txt", "first\n")]);
    repository.write("file.txt", "second\n");
    repository.commit_all("second");
    repository.write("file.txt", "third\n");
    let request = DiffRequest {
        repository: repository.path(),
        kind: DiffKind::Revision.into(),
        revision: "HEAD~1".to_owned(),
        ..DiffRequest::default()
    };

    let response = diff(&repository.permissions(), request).unwrap();

    assert_eq!(
        rendered(&response.files[0]),
        ["@@ -1,1 +1,1 @@", "-1:0 first", "+0:1 third"]
    );
}

#[test]
fn new_deleted_and_renamed_files_have_their_change_kind() {
    let long = numbered_lines(10);
    let repository = TestRepository::with_files(&[("gone.txt", "a\nb\n"), ("old.txt", &long)]);
    fs::remove_file(repository.root().join("gone.txt")).unwrap();
    repository.write("untracked.txt", "x\ny\n");
    repository.git(&["mv", "old.txt", "new.txt"]);
    repository.write("new.txt", &long.replace("line 5\n", "line five\n"));
    repository.git(&["add", "new.txt"]);

    let unstaged = diff_of(&repository, DiffKind::Unstaged, "");
    let staged = diff_of(&repository, DiffKind::Staged, "");

    assert_eq!(unstaged.files.len(), 2);
    assert_eq!(unstaged.files[0].change(), ChangeKind::Deleted);
    assert_eq!(
        rendered(&unstaged.files[0]),
        ["@@ -1,2 +0,0 @@", "-1:0 a", "-2:0 b"]
    );
    assert_eq!(unstaged.files[1].change(), ChangeKind::Added);
    assert_eq!(
        rendered(&unstaged.files[1]),
        ["@@ -0,0 +1,2 @@", "+0:1 x", "+0:2 y"]
    );
    assert_eq!(staged.files.len(), 1);
    assert_eq!(staged.files[0].change(), ChangeKind::Renamed);
    assert_eq!(
        (
            staged.files[0].path.as_str(),
            staged.files[0].original_path.as_str()
        ),
        ("new.txt", "old.txt")
    );
    assert_eq!(
        rendered(&staged.files[0])[4..6],
        ["-5:0 line 5", "+0:5 line five"]
    );
}

#[test]
fn a_path_selects_one_file() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    repository.write("a.txt", "changed\n");
    repository.write("b.txt", "changed\n");

    let response = diff_of(&repository, DiffKind::Unstaged, "b.txt");

    let paths: Vec<_> = response
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, ["b.txt"]);
}

#[test]
fn a_binary_file_is_flagged_and_has_no_hunks() {
    let repository = TestRepository::with_files(&[("image.bin", "text\n")]);
    fs::write(repository.root().join("image.bin"), [0x89, b'P', 0, 1, 2]).unwrap();

    let response = diff_of(&repository, DiffKind::Unstaged, "");

    assert!(response.files[0].binary);
    assert!(!response.files[0].too_large);
    assert!(response.files[0].hunks.is_empty());
}

#[test]
fn a_file_over_the_size_limit_is_flagged_and_has_no_hunks() {
    let repository = TestRepository::with_files(&[("large.txt", "small\n"), ("small.txt", "a\n")]);
    repository.write("large.txt", &"0123456789abcde\n".repeat(70_000));
    repository.write("small.txt", "b\n");

    let response = diff_of(&repository, DiffKind::Unstaged, "");

    assert!(response.files[0].too_large);
    assert!(response.files[0].hunks.is_empty());
    assert!(!response.files[1].too_large);
    assert_eq!(response.files[1].hunks.len(), 1);
}

#[test]
fn a_result_over_the_size_limit_flags_the_files_that_do_not_fit() {
    let repository = TestRepository::new();
    let content = "0123456789abcde\n".repeat(20_000);
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        repository.write(name, &content);
    }

    let response = diff_of(&repository, DiffKind::Unstaged, "");

    let flags: Vec<_> = response.files.iter().map(|file| file.too_large).collect();
    assert_eq!(flags, [false, true, true, true]);
    let size: usize = response
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.lines)
        .map(|line| line.content.len())
        .sum();
    assert!(size <= MAX_RESPONSE_BYTES);
    let one = diff_of(&repository, DiffKind::Unstaged, "c.txt");
    assert!(!one.files[0].too_large);
}
