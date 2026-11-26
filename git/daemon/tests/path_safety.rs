//! Requests that try to leave the directories that the user approved.

mod repositories;

use std::fs;
use std::os::unix::fs::symlink;

use nomux_extension::{ErrorCode, Permissions};
use nomux_git::blame::blame;
use nomux_git::diff::diff;
use nomux_git::staging::stage;
use nomux_git::wire::v1::{BlameRequest, DiffKind, DiffRequest, LineSelection, StageRequest};
use repositories::TestRepository;

fn stage_path(repository: &TestRepository, path: &str) -> nomux_git::error::Error {
    let request = StageRequest {
        repository: repository.path(),
        paths: vec![path.to_owned()],
        ..StageRequest::default()
    };
    stage(&repository.permissions(), request).unwrap_err()
}

#[test]
fn a_path_that_leaves_the_worktree_is_refused_by_each_method_that_takes_a_path() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    fs::write(repository.granted().join("outside.txt"), "secret\n").unwrap();
    let outside = repository
        .granted()
        .join("outside.txt")
        .display()
        .to_string();
    repository.write("directory/inside.txt", "x\n");

    let escapes = [
        "../outside.txt",
        "directory/../../outside.txt",
        "missing/../../outside.txt",
        outside.as_str(),
        "",
    ];
    for path in escapes {
        assert_eq!(
            stage_path(&repository, path).code,
            ErrorCode::InvalidArgument,
            "{path}"
        );
        let request = DiffRequest {
            repository: repository.path(),
            kind: DiffKind::Unstaged.into(),
            path: path.to_owned(),
            ..DiffRequest::default()
        };
        if !path.is_empty() {
            let error = diff(&repository.permissions(), request).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{path}");
        }
        let request = BlameRequest {
            repository: repository.path(),
            path: path.to_owned(),
            ..BlameRequest::default()
        };
        let error = blame(&repository.permissions(), request).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{path}");
        let request = StageRequest {
            repository: repository.path(),
            selections: vec![LineSelection {
                path: path.to_owned(),
                ..LineSelection::default()
            }],
            ..StageRequest::default()
        };
        let error = stage(&repository.permissions(), request).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{path}");
    }
    assert_eq!(
        repository.git(&["status", "--porcelain"]),
        "?? directory/\n"
    );
}

#[test]
fn a_symbolic_link_that_leaves_the_worktree_is_not_followed() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
    symlink(outside.path(), repository.root().join("link")).unwrap();

    let error = stage_path(&repository, "link/secret.txt");

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(repository.git(&["diff", "--cached", "--name-only"]), "");
}

#[test]
fn an_absolute_path_in_the_worktree_is_accepted() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.write("a.txt", "b\n");
    let request = StageRequest {
        repository: repository.path(),
        paths: vec![repository.root().join("a.txt").display().to_string()],
        ..StageRequest::default()
    };

    stage(&repository.permissions(), request).unwrap();

    assert_eq!(repository.git(&["status", "--porcelain"]), "M  a.txt\n");
}

#[test]
fn a_repository_outside_the_approved_directories_is_refused() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let other = TestRepository::with_files(&[("a.txt", "a\n")]);
    fs::write(other.root().join("a.txt"), "changed\n").unwrap();
    symlink(other.root(), repository.granted().join("link")).unwrap();
    let through_link = repository.granted().join("link").display().to_string();
    let relative = format!("{}/../{}", repository.path(), "repository");

    let expected = [
        (other.path(), ErrorCode::PermissionDenied),
        (through_link, ErrorCode::PermissionDenied),
        ("repository".to_owned(), ErrorCode::InvalidArgument),
    ];
    for (path, code) in expected {
        let request = StageRequest {
            repository: path.clone(),
            paths: vec!["a.txt".to_owned()],
            ..StageRequest::default()
        };
        let error = stage(&repository.permissions(), request).unwrap_err();
        assert_eq!(error.code, code, "{path}");
    }
    assert_eq!(other.git(&["status", "--porcelain"]), " M a.txt\n");
    // The path is in the approved directory after `..` is resolved.
    let request = DiffRequest {
        repository: relative,
        kind: DiffKind::Unstaged.into(),
        ..DiffRequest::default()
    };
    assert!(diff(&repository.permissions(), request).is_ok());
}

#[test]
fn only_the_root_of_a_worktree_is_a_repository_of_a_request() {
    let repository = TestRepository::with_files(&[("directory/a.txt", "a\n")]);
    let expected = [
        ("directory", ErrorCode::NotFound),
        (".git", ErrorCode::InvalidArgument),
    ];

    for (directory, code) in expected {
        let request = DiffRequest {
            repository: repository.root().join(directory).display().to_string(),
            kind: DiffKind::Unstaged.into(),
            ..DiffRequest::default()
        };
        let error = diff(&repository.permissions(), request).unwrap_err();
        assert_eq!(error.code, code, "{directory}");
    }
}

#[test]
fn a_method_that_writes_needs_the_approval_of_the_git_program() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.write("a.txt", "b\n");
    let permissions = Permissions {
        process: Vec::new(),
        ..repository.permissions()
    };
    let request = StageRequest {
        repository: repository.path(),
        paths: vec!["a.txt".to_owned()],
        ..StageRequest::default()
    };

    let error = stage(&permissions, request).unwrap_err();

    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(repository.git(&["status", "--porcelain"]), " M a.txt\n");
}
