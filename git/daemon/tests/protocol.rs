//! Runs the program of the extension as a child process, as the nomux daemon
//! does, and speaks the extension protocol with it.

mod repositories;
mod test_host;

use std::path::Path;

use nomux_extension::manifest::Manifest;
use nomux_extension::wire::v1::{Welcome, response};
use nomux_extension::{ApiVersion, ErrorCode, Permissions};
use nomux_git::wire::v1::{
    ChangeKind, CommitRequest, CommitResponse, DiffKind, DiffRequest, DiffResponse, StageRequest,
    StageResponse, StatusChanged, StatusRequest, StatusResponse, UnwatchRequest, UnwatchResponse,
    WatchRequest, WatchResponse,
};
use prost::Message;
use repositories::TestRepository;
use test_host::TestHost;

const PROGRAM: &str = env!("CARGO_BIN_EXE_nomux-git");

fn manifest() -> Manifest {
    include_str!("../../extension.toml").parse().unwrap()
}

#[test]
fn the_manifest_describes_the_program_and_asks_for_the_worktree_and_git_only() {
    let manifest = manifest();

    assert_eq!(manifest.id.to_string(), "nomux.git");
    assert_eq!(manifest.version.to_string(), env!("CARGO_PKG_VERSION"));
    assert!(manifest.api.matches(ApiVersion::CURRENT));
    let daemon = manifest.daemon.as_ref().unwrap();
    assert_eq!(
        Path::new(daemon.command.as_str()).file_name(),
        Path::new(PROGRAM).file_name()
    );
    assert!(daemon.lazy);
    let permissions = manifest.permissions;
    let fs: Vec<_> = permissions.fs.iter().map(ToString::to_string).collect();
    assert_eq!(fs, ["${worktree}"]);
    assert_eq!(permissions.process, ["git"]);
    assert!(!permissions.network);
    assert!(permissions.scopes.is_empty());
    assert!(manifest.ui.is_none());
}

#[test]
fn the_program_serves_a_host_from_the_handshake_to_the_shutdown() {
    let repository = TestRepository::with_files(&[("a.txt", "one\n")]);
    let mut host = TestHost::start(PROGRAM);

    let welcome = host.hello("nomux.git", repository.permissions());
    assert_eq!(
        welcome,
        response::Result::Welcome(Welcome {
            api_version: Some(ApiVersion::CURRENT),
            extension_id: "nomux.git".to_owned(),
            extension_version: env!("CARGO_PKG_VERSION").to_owned(),
        })
    );

    let watch = WatchRequest {
        repository: repository.path(),
    };
    let watched: WatchResponse = host.call("watch", &watch).unwrap();
    assert_eq!(watched.status.unwrap().files, []);

    repository.write("a.txt", "two\n");
    let event = host.next_event();
    assert_eq!(event.name, "status_changed");
    let changed = StatusChanged::decode(event.payload.as_slice()).unwrap();
    assert_eq!(changed.repository, repository.path());
    assert_eq!(
        changed.status.unwrap().files[0].unstaged(),
        ChangeKind::Modified
    );

    let diff = DiffRequest {
        repository: repository.path(),
        kind: DiffKind::Unstaged.into(),
        ..DiffRequest::default()
    };
    let diff: DiffResponse = host.call("diff", &diff).unwrap();
    assert_eq!(diff.files[0].hunks[0].lines[1].content, b"two\n");

    let stage = StageRequest {
        repository: repository.path(),
        paths: vec!["a.txt".to_owned()],
        ..StageRequest::default()
    };
    host.call::<StageResponse>("stage", &stage).unwrap();
    // The status after a method that writes is not the status of the watch
    // from before the method.
    let status = StatusRequest {
        repository: repository.path(),
    };
    let status: StatusResponse = host.call("status", &status).unwrap();
    assert_eq!(
        status.status.unwrap().files[0].staged(),
        ChangeKind::Modified
    );

    let commit = CommitRequest {
        repository: repository.path(),
        message: "second\n".to_owned(),
        amend: false,
    };
    let commit: CommitResponse = host.call("commit", &commit).unwrap();
    assert_eq!(
        commit.commit_id,
        repository.git(&["rev-parse", "HEAD"]).trim()
    );

    let unwatch = UnwatchRequest {
        subscription: watched.subscription,
    };
    host.call::<UnwatchResponse>("unwatch", &unwatch).unwrap();
    assert!(host.shutdown().success());
}

#[test]
fn the_program_refuses_a_repository_that_the_user_did_not_approve() {
    let repository = TestRepository::with_files(&[("a.txt", "one\n")]);
    let mut host = TestHost::start(PROGRAM);
    host.hello("nomux.git", Permissions::default());
    let status = StatusRequest {
        repository: repository.path(),
    };

    let error = host.call::<StatusResponse>("status", &status).unwrap_err();

    assert_eq!(error.code, i32::from(ErrorCode::PermissionDenied));
    assert!(host.shutdown().success());
}
