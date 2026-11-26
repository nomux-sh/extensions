//! The list of worktrees, the new worktree and the rule that a removal loses
//! no work.

mod repositories;

use nomux_extension::ErrorCode;
use nomux_git::wire::v1::{
    RefusalReason, WorktreeAddRequest, WorktreeRemoveRequest, WorktreeRemoveResponse,
    WorktreesRequest,
};
use nomux_git::worktrees::{worktree_add, worktree_remove, worktrees};
use repositories::{TestRepository, git_in};

/// A repository whose `main` is pushed, with a linked worktree `linked` on
/// the new branch `task` at the commit of `main`.
fn with_linked_worktree() -> TestRepository {
    let repository = TestRepository::with_files(&[(".gitignore", "target/\n"), ("a.txt", "a\n")]);
    repository.add_remote();
    let request = WorktreeAddRequest {
        repository: repository.path(),
        path: linked(&repository),
        branch: "main".to_owned(),
        new_branch: "task".to_owned(),
    };
    worktree_add(&repository.permissions(), request).unwrap();
    repository
}

fn linked(repository: &TestRepository) -> String {
    repository.granted().join("linked").display().to_string()
}

fn remove(repository: &TestRepository) -> WorktreeRemoveResponse {
    let request = WorktreeRemoveRequest {
        repository: repository.path(),
        path: linked(repository),
    };
    worktree_remove(&repository.permissions(), request).unwrap()
}

#[test]
fn a_new_worktree_is_in_the_list_with_its_branch() {
    let repository = with_linked_worktree();
    let request = WorktreesRequest {
        repository: repository.path(),
    };

    let list = worktrees(&repository.permissions(), request)
        .unwrap()
        .worktrees;

    let head = repository.git(&["rev-parse", "main"]);
    let rows: Vec<_> = list
        .iter()
        .map(|worktree| {
            (
                worktree.path.clone(),
                worktree.branch.as_str(),
                worktree.head.as_str(),
                worktree.is_main,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (repository.path(), "main", head.trim(), true),
            (linked(&repository), "task", head.trim(), false),
        ]
    );
}

#[test]
fn a_worktree_without_work_is_removed_with_its_ignored_files() {
    let repository = with_linked_worktree();
    let build_output = repository.granted().join("linked/target/output");
    std::fs::create_dir_all(build_output.parent().unwrap()).unwrap();
    std::fs::write(build_output, "can be made again").unwrap();

    let response = remove(&repository);

    assert!(response.removed);
    assert_eq!(response.refusal, None);
    assert!(!repository.granted().join("linked").exists());
    assert_eq!(repository.git(&["branch", "--list", "task"]).trim(), "task");
}

#[test]
fn a_worktree_with_a_change_that_is_not_committed_stays() {
    for (path, content) in [("a.txt", "changed\n"), ("untracked.txt", "new\n")] {
        let repository = with_linked_worktree();
        let file = repository.granted().join("linked").join(path);
        std::fs::write(&file, content).unwrap();

        let response = remove(&repository);

        assert!(!response.removed);
        let refusal = response.refusal.unwrap();
        assert_eq!(refusal.reason(), RefusalReason::UncommittedChanges);
        assert_eq!(
            refusal.message,
            "the worktree has 1 paths with changes that are not committed"
        );
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
    }
}

#[test]
fn a_worktree_with_a_commit_that_is_not_pushed_stays_until_the_push() {
    let repository = with_linked_worktree();
    let worktree = repository.granted().join("linked");
    std::fs::write(worktree.join("a.txt"), "work\n").unwrap();
    git_in(
        &worktree,
        &["commit", "--quiet", "--all", "--message", "work"],
    );

    let response = remove(&repository);

    assert!(!response.removed);
    let refusal = response.refusal.unwrap();
    assert_eq!(refusal.reason(), RefusalReason::UnpushedCommits);
    assert_eq!(
        refusal.message,
        "the worktree has 1 commits that no remote-tracking branch has"
    );
    assert!(worktree.join("a.txt").exists());

    git_in(&worktree, &["push", "--quiet", "origin", "task"]);
    assert!(remove(&repository).removed);
}

#[test]
fn a_worktree_is_added_and_removed_only_in_an_approved_directory() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("escaped");
    let request = WorktreeAddRequest {
        repository: repository.path(),
        path: target.display().to_string(),
        branch: "main".to_owned(),
        new_branch: "task".to_owned(),
    };

    let added = worktree_add(&repository.permissions(), request).unwrap_err();

    assert_eq!(added.code, ErrorCode::PermissionDenied);
    assert!(!target.exists());
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "task",
        target.to_str().unwrap(),
    ]);
    let request = WorktreeRemoveRequest {
        repository: repository.path(),
        path: target.display().to_string(),
    };
    let removed = worktree_remove(&repository.permissions(), request).unwrap_err();
    assert_eq!(removed.code, ErrorCode::PermissionDenied);
    assert!(target.exists());
}

#[test]
fn the_main_worktree_is_not_removed() {
    let repository = with_linked_worktree();
    let request = WorktreeRemoveRequest {
        repository: linked(&repository),
        path: repository.path(),
    };

    let error = worktree_remove(&repository.permissions(), request).unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(repository.root().join("a.txt").exists());
}
