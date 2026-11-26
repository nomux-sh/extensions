//! The status of a worktree in each state of a file.

mod repositories;

use std::fs;

use nomux_git::repository::Repository;
use nomux_git::status::status;
use nomux_git::wire::v1::{ChangeKind, ConflictKind, FileStatus, Operation, Status};
use repositories::{TestRepository, git_in};

fn status_of(repository: &TestRepository) -> Status {
    let opened = Repository::open(&repository.permissions(), &repository.path()).unwrap();
    status(opened.gix()).unwrap()
}

fn file(path: &str, staged: ChangeKind, unstaged: ChangeKind) -> FileStatus {
    FileStatus {
        path: path.to_owned(),
        staged: staged.into(),
        unstaged: unstaged.into(),
        ..FileStatus::default()
    }
}

const NONE: ChangeKind = ChangeKind::Unspecified;

#[test]
fn each_file_state_has_its_kind_and_an_ignored_file_is_not_reported() {
    let long = "one\ntwo\nthree\nfour\nfive\nsix\n";
    let repository = TestRepository::with_files(&[
        (".gitignore", "*.log\n"),
        ("both.txt", "a\n"),
        ("deleted-in-index.txt", "a\n"),
        ("deleted-in-worktree.txt", "a\n"),
        ("modified-in-index.txt", "a\n"),
        ("modified-in-worktree.txt", "a\n"),
        ("old-name.txt", long),
        ("script.sh", "echo\n"),
    ]);
    repository.git(&["rm", "--quiet", "deleted-in-index.txt"]);
    fs::remove_file(repository.root().join("deleted-in-worktree.txt")).unwrap();
    repository.write("modified-in-index.txt", "b\n");
    repository.git(&["add", "modified-in-index.txt"]);
    repository.write("modified-in-worktree.txt", "b\n");
    repository.git(&["mv", "old-name.txt", "new-name.txt"]);
    repository.write("both.txt", "b\n");
    repository.git(&["add", "both.txt"]);
    repository.write("both.txt", "c\n");
    repository.write("added.txt", "a new file\n");
    repository.git(&["add", "added.txt"]);
    repository.write("untracked.txt", "a\n");
    repository.write("untracked-directory/one.txt", "a\n");
    repository.write("untracked-directory/two.txt", "a\n");
    repository.write("build.log", "ignored\n");
    fs::remove_file(repository.root().join("script.sh")).unwrap();
    std::os::unix::fs::symlink("both.txt", repository.root().join("script.sh")).unwrap();

    let renamed = FileStatus {
        original_path: "old-name.txt".to_owned(),
        ..file("new-name.txt", ChangeKind::Renamed, NONE)
    };
    assert_eq!(
        status_of(&repository).files,
        [
            file("added.txt", ChangeKind::Added, NONE),
            file("both.txt", ChangeKind::Modified, ChangeKind::Modified),
            file("deleted-in-index.txt", ChangeKind::Deleted, NONE),
            file("deleted-in-worktree.txt", NONE, ChangeKind::Deleted),
            file("modified-in-index.txt", ChangeKind::Modified, NONE),
            file("modified-in-worktree.txt", NONE, ChangeKind::Modified),
            renamed,
            file("script.sh", NONE, ChangeKind::TypeChanged),
            file("untracked-directory/", NONE, ChangeKind::Untracked),
            file("untracked.txt", NONE, ChangeKind::Untracked),
        ]
    );
}

#[test]
fn a_repository_without_a_commit_has_a_branch_and_no_head() {
    let repository = TestRepository::new();
    repository.write("new.txt", "a\n");

    let status = status_of(&repository);

    assert_eq!((status.branch.as_str(), status.head.as_str()), ("main", ""));
    assert_eq!(status.files, [file("new.txt", NONE, ChangeKind::Untracked)]);
}

#[test]
fn the_branch_reports_its_upstream_and_the_commits_on_each_side() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.add_remote();
    git_in(
        repository.granted(),
        &["clone", "--quiet", "remote.git", "other"],
    );
    let other = repository.granted().join("other");
    repositories::configure(&other);
    fs::write(other.join("theirs.txt"), "t\n").unwrap();
    git_in(&other, &["add", "--all"]);
    git_in(&other, &["commit", "--quiet", "--message", "theirs"]);
    git_in(&other, &["push", "--quiet"]);
    for name in ["one", "two"] {
        repository.write(name, "x\n");
        repository.commit_all(name);
    }
    repository.git(&["fetch", "--quiet"]);

    let status = status_of(&repository);

    assert_eq!(status.branch, "main");
    assert_eq!(status.upstream, "origin/main");
    assert_eq!((status.ahead, status.behind), (2, 1));
    assert_eq!(status.head, repository.git(&["rev-parse", "HEAD"]).trim());
}

#[test]
fn a_merge_that_stopped_reports_each_conflict_and_the_operation() {
    let repository = TestRepository::with_files(&[("both.txt", "base\n"), ("gone.txt", "base\n")]);
    repository.git(&["switch", "--quiet", "--create", "theirs"]);
    repository.write("both.txt", "theirs\n");
    repository.git(&["rm", "--quiet", "gone.txt"]);
    repository.write("added.txt", "theirs\n");
    repository.commit_all("theirs");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("both.txt", "ours\n");
    repository.write("gone.txt", "ours\n");
    repository.write("added.txt", "ours\n");
    repository.commit_all("ours");
    let merge = std::process::Command::new("git")
        .current_dir(repository.root())
        .args(["merge", "--quiet", "theirs"])
        .output()
        .unwrap();
    assert!(!merge.status.success());

    let status = status_of(&repository);

    assert_eq!(status.operation(), Operation::Merge);
    let conflicts: Vec<_> = status
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.conflict()))
        .collect();
    assert_eq!(
        conflicts,
        [
            ("added.txt", ConflictKind::BothAdded),
            ("both.txt", ConflictKind::BothModified),
            ("gone.txt", ConflictKind::DeletedByThem),
        ]
    );
}

#[test]
fn a_detached_head_has_no_branch() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.git(&["switch", "--quiet", "--detach"]);

    let status = status_of(&repository);

    assert_eq!(status.branch, "");
    assert_eq!(status.head, repository.git(&["rev-parse", "HEAD"]).trim());
}
