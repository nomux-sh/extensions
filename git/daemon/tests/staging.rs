//! The staging of files, hunks and lines, and the commit of the staged part.

mod repositories;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use nomux_extension::ErrorCode;
use nomux_git::commit::commit;
use nomux_git::diff::diff;
use nomux_git::staging::{stage, unstage};
use nomux_git::wire::v1::{
    CommitRequest, DiffKind, DiffRequest, LineRange, LineSelection, StageRequest, UnstageRequest,
};
use repositories::TestRepository;

const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";

fn range(start: u32, count: u32) -> Vec<LineRange> {
    vec![LineRange { start, count }]
}

fn stage_lines(repository: &TestRepository, path: &str, old: Vec<LineRange>, new: Vec<LineRange>) {
    let request = StageRequest {
        repository: repository.path(),
        selections: vec![LineSelection {
            path: path.to_owned(),
            old_lines: old,
            new_lines: new,
        }],
        ..StageRequest::default()
    };
    stage(&repository.permissions(), request).unwrap();
}

fn commit_staged(repository: &TestRepository) {
    let request = CommitRequest {
        repository: repository.path(),
        message: "staged part\n".to_owned(),
        amend: false,
    };
    commit(&repository.permissions(), request).unwrap();
}

#[test]
fn a_staged_hunk_is_the_only_change_of_the_commit() {
    let repository = TestRepository::with_files(&[("file.txt", BASE)]);
    let worktree = BASE
        .replace("two\n", "TWO\n")
        .replace("eleven\n", "ELEVEN\nextra\n");
    repository.write("file.txt", &worktree);
    let request = DiffRequest {
        repository: repository.path(),
        kind: DiffKind::Unstaged.into(),
        ..DiffRequest::default()
    };
    let hunks = diff(&repository.permissions(), request)
        .unwrap()
        .files
        .remove(0)
        .hunks;
    assert_eq!(hunks.len(), 2);
    let second = &hunks[1];

    stage_lines(
        &repository,
        "file.txt",
        range(second.old_start, second.old_lines),
        range(second.new_start, second.new_lines),
    );
    commit_staged(&repository);

    assert_eq!(
        repository.git(&["show", "HEAD:file.txt"]),
        BASE.replace("eleven\n", "ELEVEN\nextra\n")
    );
    assert_eq!(repository.read("file.txt"), worktree);
    assert_eq!(repository.git(&["status", "--porcelain"]), " M file.txt\n");
}

#[test]
fn staged_lines_are_the_only_change_of_the_commit() {
    let repository = TestRepository::with_files(&[("file.txt", BASE)]);
    repository.write(
        "file.txt",
        &BASE.replace("three\n", "a\nb\nc\n").replace("six\n", ""),
    );

    // Line 4 of the worktree is `b`. Line 6 of the index is `six`.
    stage_lines(&repository, "file.txt", range(6, 1), range(4, 1));
    commit_staged(&repository);

    assert_eq!(
        repository.git(&["show", "HEAD:file.txt"]),
        BASE.replace("three\n", "three\nb\n").replace("six\n", "")
    );
}

#[test]
fn lines_of_an_untracked_file_are_staged_with_the_mode_of_the_file() {
    let repository = TestRepository::with_files(&[("other.txt", "x\n")]);
    repository.write("run.sh", "first\nsecond\nthird\n");
    let script = repository.root().join("run.sh");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    stage_lines(&repository, "run.sh", Vec::new(), range(2, 2));
    commit_staged(&repository);

    assert_eq!(repository.git(&["show", "HEAD:run.sh"]), "second\nthird\n");
    assert!(
        repository
            .git(&["ls-tree", "HEAD", "run.sh"])
            .starts_with("100755 ")
    );
}

#[test]
fn unstaged_lines_leave_the_other_staged_lines_in_the_index() {
    let repository = TestRepository::with_files(&[("file.txt", BASE)]);
    let worktree = BASE.replace("two\n", "TWO\n").replace("nine\n", "NINE\n");
    repository.write("file.txt", &worktree);
    repository.git(&["add", "file.txt"]);
    let request = UnstageRequest {
        repository: repository.path(),
        selections: vec![LineSelection {
            path: "file.txt".to_owned(),
            old_lines: range(2, 1),
            new_lines: range(2, 1),
        }],
        ..UnstageRequest::default()
    };

    unstage(&repository.permissions(), request).unwrap();

    assert_eq!(
        repository.git(&["show", ":file.txt"]),
        BASE.replace("nine\n", "NINE\n")
    );
    assert_eq!(repository.read("file.txt"), worktree);
}

#[test]
fn files_are_staged_and_unstaged_completely() {
    let repository = TestRepository::with_files(&[("kept.txt", "a\n"), ("gone.txt", "a\n")]);
    repository.write("kept.txt", "b\n");
    repository.write("new directory/new.txt", "n\n");
    fs::remove_file(repository.root().join("gone.txt")).unwrap();
    let paths = vec![
        "kept.txt".to_owned(),
        "gone.txt".to_owned(),
        "new directory".to_owned(),
    ];
    let request = StageRequest {
        repository: repository.path(),
        paths: paths.clone(),
        ..StageRequest::default()
    };

    stage(&repository.permissions(), request).unwrap();
    assert_eq!(
        repository.git(&["status", "--porcelain"]),
        "D  gone.txt\nM  kept.txt\nA  \"new directory/new.txt\"\n"
    );
    let request = UnstageRequest {
        repository: repository.path(),
        paths: paths[..2].to_vec(),
        ..UnstageRequest::default()
    };
    unstage(&repository.permissions(), request).unwrap();

    assert_eq!(
        repository.git(&["status", "--porcelain"]),
        " D gone.txt\n M kept.txt\nA  \"new directory/new.txt\"\n"
    );
}

#[test]
fn a_file_is_unstaged_before_the_first_commit() {
    let repository = TestRepository::new();
    repository.write("new.txt", "a\n");
    repository.git(&["add", "new.txt"]);
    let request = UnstageRequest {
        repository: repository.path(),
        paths: vec!["new.txt".to_owned()],
        ..UnstageRequest::default()
    };

    unstage(&repository.permissions(), request).unwrap();

    assert_eq!(repository.git(&["status", "--porcelain"]), "?? new.txt\n");
}

#[test]
fn a_selection_without_a_changed_line_is_refused_and_the_index_stays() {
    let repository = TestRepository::with_files(&[("file.txt", BASE)]);
    repository.write("file.txt", &BASE.replace("two\n", "TWO\n"));
    let request = StageRequest {
        repository: repository.path(),
        selections: vec![LineSelection {
            path: "file.txt".to_owned(),
            old_lines: range(7, 2),
            new_lines: range(7, 2),
        }],
        ..StageRequest::default()
    };

    let error = stage(&repository.permissions(), request).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(repository.git(&["status", "--porcelain"]), " M file.txt\n");
}
