//! The commit: the message, the hooks of the user and the amend flag.

mod repositories;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use nomux_extension::ErrorCode;
use nomux_git::commit::commit;
use nomux_git::wire::v1::CommitRequest;
use repositories::TestRepository;

fn request(repository: &TestRepository, message: &str, amend: bool) -> CommitRequest {
    CommitRequest {
        repository: repository.path(),
        message: message.to_owned(),
        amend,
    }
}

#[test]
fn the_message_of_the_commit_is_the_message_of_the_request() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.write("a.txt", "b\n");
    repository.git(&["add", "a.txt"]);
    // git removes the comment line and the spaces with its default cleanup.
    let message = "subject  \n\n# not a comment\nbody\n\n\n";

    let response = commit(
        &repository.permissions(),
        request(&repository, message, false),
    )
    .unwrap();

    assert_eq!(
        response.commit_id,
        repository.git(&["rev-parse", "HEAD"]).trim()
    );
    assert_eq!(
        repository
            .git(&["cat-file", "commit", "HEAD"])
            .split_once("\n\n")
            .unwrap()
            .1,
        message
    );
}

#[test]
fn the_commit_runs_the_hooks_of_the_user() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let hook = repository.root().join(".git/hooks/commit-msg");
    fs::write(&hook, "#!/bin/sh\necho 'Added-by: hook' >> \"$1\"\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    repository.write("a.txt", "b\n");
    repository.git(&["add", "a.txt"]);

    commit(
        &repository.permissions(),
        request(&repository, "subject\n", false),
    )
    .unwrap();

    assert_eq!(
        repository.git(&["log", "-1", "--format=%B"]),
        "subject\nAdded-by: hook\n\n"
    );
}

#[test]
fn an_amend_replaces_the_last_commit_and_an_empty_message_keeps_its_message() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.write("a.txt", "b\n");
    repository.git(&["add", "a.txt"]);

    commit(&repository.permissions(), request(&repository, "", true)).unwrap();

    assert_eq!(repository.git(&["rev-list", "--count", "HEAD"]), "1\n");
    assert_eq!(repository.git(&["log", "-1", "--format=%s"]), "first\n");
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "b\n");
}

#[test]
fn an_empty_message_without_amend_is_refused() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.write("a.txt", "b\n");
    repository.git(&["add", "a.txt"]);

    let error = commit(&repository.permissions(), request(&repository, "", false)).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(repository.git(&["rev-list", "--count", "HEAD"]), "1\n");
}

#[test]
fn a_commit_that_git_refuses_fails_with_the_reason_of_git() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);

    let error = commit(
        &repository.permissions(),
        request(&repository, "empty\n", false),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Failed);
    assert!(
        error.message.contains("nothing to commit"),
        "{}",
        error.message
    );
}
