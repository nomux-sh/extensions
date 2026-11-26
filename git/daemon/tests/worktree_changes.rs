//! The changes of git of a clean, with real repositories: the move to the
//! trash place and back, the removal with the branch, the prune, and each
//! refusal that the extension checks itself.

mod repositories;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use nomux_git::worktree_changes::{
    TRASH_LOCK_REASON, worktree_delete, worktree_move_from_trash, worktree_move_to_trash,
    worktree_prune,
};
use nomux_proto::wire::v1::{
    WorktreeDeleteRequest, WorktreeMoveFromTrashRequest, WorktreeMoveToTrashRequest,
    WorktreePruneRequest, WorktreeRefusal,
};
use repositories::{TestRepository, git_in};

/// A repository with `a.txt` on `main`, pushed to `origin`.
fn pushed() -> TestRepository {
    let repository =
        TestRepository::with_files(&[(".gitignore", "target/\n.env\n"), ("a.txt", "a\n")]);
    repository.add_remote();
    repository.git(&["remote", "set-head", "origin", "main"]);
    repository
}

fn add_worktree(repository: &TestRepository, name: &str) -> PathBuf {
    let path = repository.granted().join(name);
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        name,
        path.to_str().unwrap(),
    ]);
    path
}

fn commit_in(worktree: &Path, file: &str) {
    fs::write(worktree.join(file), file).unwrap();
    git_in(worktree, &["add", "--all"]);
    git_in(worktree, &["commit", "--quiet", "--message", file]);
}

fn head(worktree: &Path) -> String {
    git_in(worktree, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// A worktree whose branch `main` has: merged by the strict rule.
fn merged(repository: &TestRepository, name: &str) -> PathBuf {
    let worktree = add_worktree(repository, name);
    commit_in(&worktree, &format!("{name}.txt"));
    repository.git(&["merge", "--quiet", "--ff-only", name]);
    repository.git(&["push", "--quiet", "origin", "main"]);
    worktree
}

fn trash_path(repository: &TestRepository, name: &str) -> PathBuf {
    let trash = repository.granted().join(".nomux-trash");
    fs::create_dir_all(&trash).unwrap();
    trash.join(name)
}

fn to_trash(
    repository: &TestRepository,
    worktree: &Path,
    trash: &Path,
) -> WorktreeMoveToTrashRequest {
    WorktreeMoveToTrashRequest {
        repository: repository.path(),
        path: worktree.display().to_string(),
        trash_path: trash.display().to_string(),
        expected_head: head(worktree),
        ..WorktreeMoveToTrashRequest::default()
    }
}

fn refusals_of_move(repository: &TestRepository, worktree: &Path) -> Vec<WorktreeRefusal> {
    let request = to_trash(repository, worktree, &trash_path(repository, "t"));
    worktree_move_to_trash(&repository.permissions(), request)
        .unwrap()
        .refusals()
        .collect()
}

fn delete_request(
    repository: &TestRepository,
    path: &Path,
    expected_head: String,
) -> WorktreeDeleteRequest {
    WorktreeDeleteRequest {
        repository: repository.path(),
        path: path.display().to_string(),
        expected_head,
        delete_branch: true,
        ..WorktreeDeleteRequest::default()
    }
}

fn branches(repository: &TestRepository) -> String {
    repository.git(&["branch", "--list", "--format=%(refname:short)"])
}

#[test]
fn a_worktree_moves_to_the_trash_locked_and_back() {
    let repository = pushed();
    let worktree = merged(&repository, "done");
    let trash = trash_path(&repository, "done-1");

    let moved = worktree_move_to_trash(
        &repository.permissions(),
        to_trash(&repository, &worktree, &trash),
    )
    .unwrap();

    assert!(moved.refusals.is_empty(), "{moved:?}");
    assert!(!worktree.exists());
    let list = repository.git(&["worktree", "list", "--porcelain"]);
    assert!(
        list.contains(&format!(
            "worktree {}\nHEAD",
            trash.canonicalize().unwrap().display()
        )),
        "{list}"
    );
    assert!(
        list.contains(&format!("locked {TRASH_LOCK_REASON}")),
        "{list}"
    );

    worktree_move_from_trash(
        &repository.permissions(),
        WorktreeMoveFromTrashRequest {
            repository: repository.path(),
            trash_path: trash.display().to_string(),
            path: worktree.display().to_string(),
        },
    )
    .unwrap();

    assert!(worktree.join("done.txt").exists());
    assert!(!trash.exists());
    let list = repository.git(&["worktree", "list", "--porcelain"]);
    assert!(!list.contains("locked"), "{list}");
}

#[test]
fn the_move_refuses_each_kind_of_work_that_the_removal_would_lose() {
    let repository = pushed();
    let dirty = merged(&repository, "dirty");
    fs::write(dirty.join("a.txt"), "changed\n").unwrap();
    let untracked = merged(&repository, "untracked");
    fs::write(untracked.join("new.txt"), "new\n").unwrap();
    let ahead = add_worktree(&repository, "ahead");
    commit_in(&ahead, "b.txt");
    let locked = merged(&repository, "locked");
    repository.git(&[
        "worktree",
        "lock",
        "--reason",
        "agent 7",
        locked.to_str().unwrap(),
    ]);
    let submodule = merged(&repository, "submodule");
    fs::write(submodule.join(".gitmodules"), "").unwrap();
    git_in(&submodule, &["add", ".gitmodules"]);
    git_in(&submodule, &["commit", "--quiet", "--message", "modules"]);
    repository.git(&["merge", "--quiet", "--ff-only", "submodule"]);
    repository.git(&["push", "--quiet", "origin", "main"]);
    let user_files = merged(&repository, "user-files");
    fs::write(user_files.join(".env"), "SECRET=1\n").unwrap();
    let detached = repository.granted().join("detached");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        detached.to_str().unwrap(),
    ]);
    commit_in(&detached, "d.txt");

    let cases = [
        (&dirty, WorktreeRefusal::UncommittedChanges),
        (&untracked, WorktreeRefusal::UncommittedChanges),
        (&ahead, WorktreeRefusal::UnpushedCommits),
        (&locked, WorktreeRefusal::Locked),
        (&submodule, WorktreeRefusal::Submodule),
        (&user_files, WorktreeRefusal::IgnoredUserFiles),
        (&detached, WorktreeRefusal::DetachedCommits),
    ];
    for (worktree, refusal) in cases {
        let refusals = refusals_of_move(&repository, worktree);
        assert!(
            refusals.contains(&refusal),
            "{}: {refusals:?}",
            worktree.display()
        );
        assert!(worktree.exists(), "{}", worktree.display());
    }
    let main = repository.root();
    let request = WorktreeMoveToTrashRequest {
        expected_head: head(&main),
        ..to_trash(&repository, &main, &trash_path(&repository, "main"))
    };
    assert!(worktree_move_to_trash(&repository.permissions(), request).is_err());
}

#[test]
fn allowed_paths_and_allow_unpushed_remove_their_refusals_only() {
    let repository = pushed();
    let worktree = add_worktree(&repository, "build");
    commit_in(&worktree, "b.txt");
    fs::create_dir_all(worktree.join("target/debug")).unwrap();
    fs::write(worktree.join("target/debug/app"), "binary").unwrap();
    fs::write(worktree.join(".env"), "SECRET=1\n").unwrap();
    let mut request = to_trash(&repository, &worktree, &trash_path(&repository, "build"));
    request.allowed_ignored_paths = vec!["target/".to_owned()];

    let refused = worktree_move_to_trash(&repository.permissions(), request.clone()).unwrap();
    assert_eq!(
        refused.refusals().collect::<Vec<_>>(),
        [
            WorktreeRefusal::UnpushedCommits,
            WorktreeRefusal::IgnoredUserFiles
        ]
    );

    request.allow_unpushed = true;
    request.allowed_ignored_paths.push(".env".to_owned());
    let moved = worktree_move_to_trash(&repository.permissions(), request).unwrap();
    assert!(moved.refusals.is_empty(), "{moved:?}");
    assert!(branches(&repository).contains("build"));
}

#[test]
fn a_head_that_moved_after_the_plan_is_an_error_and_nothing_moves() {
    let repository = pushed();
    let worktree = merged(&repository, "moved");
    let request = to_trash(&repository, &worktree, &trash_path(&repository, "moved"));
    commit_in(&worktree, "later.txt");

    let error = worktree_move_to_trash(&repository.permissions(), request).unwrap_err();

    assert!(error.message.contains("the plan saw"), "{error}");
    assert!(worktree.exists());
}

#[test]
fn a_trash_place_on_a_different_file_system_is_refused() {
    let Ok(shared_memory) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    let repository = pushed();
    if fs::metadata(shared_memory.path()).unwrap().dev()
        == fs::metadata(repository.granted()).unwrap().dev()
    {
        return;
    }
    let worktree = merged(&repository, "far");
    let mut permissions = repository.permissions();
    permissions
        .fs
        .push(shared_memory.path().display().to_string());
    let request = to_trash(&repository, &worktree, &shared_memory.path().join("far"));

    let refused = worktree_move_to_trash(&permissions, request).unwrap();

    assert_eq!(
        refused.refusals().collect::<Vec<_>>(),
        [WorktreeRefusal::OtherFileSystem]
    );
    assert!(worktree.exists());
}

#[test]
fn the_removal_of_a_trash_entry_deletes_a_branch_only_by_the_strict_rule() {
    let repository = pushed();
    let strict = merged(&repository, "strict");
    let strict_head = head(&strict);
    let strict_trash = trash_path(&repository, "strict");
    worktree_move_to_trash(
        &repository.permissions(),
        to_trash(&repository, &strict, &strict_trash),
    )
    .unwrap();
    // Gone upstream only: the branch was pushed and deleted on the remote,
    // and `main` does not have its commit.
    let gone = add_worktree(&repository, "gone");
    commit_in(&gone, "g.txt");
    git_in(
        &gone,
        &["push", "--quiet", "--set-upstream", "origin", "gone"],
    );
    repository.git(&["push", "--quiet", "origin", "--delete", "gone"]);
    repository.git(&["fetch", "--quiet", "--prune"]);
    let gone_head = head(&gone);
    let gone_trash = trash_path(&repository, "gone");
    let mut request = to_trash(&repository, &gone, &gone_trash);
    request.allow_unpushed = true;
    worktree_move_to_trash(&repository.permissions(), request).unwrap();

    let strict_removal = worktree_delete(
        &repository.permissions(),
        delete_request(&repository, &strict_trash, strict_head),
    )
    .unwrap();
    let mut gone_request = delete_request(&repository, &gone_trash, gone_head.clone());
    gone_request.allow_unpushed = true;
    let gone_removal = worktree_delete(&repository.permissions(), gone_request).unwrap();

    assert!(strict_removal.branch_deleted, "{strict_removal:?}");
    assert!(!gone_removal.branch_deleted);
    assert!(
        gone_removal
            .branch_kept_reason
            .contains("does not have each change"),
        "{gone_removal:?}"
    );
    assert!(!strict_trash.exists());
    assert!(!gone_trash.exists());
    let branches = branches(&repository);
    assert!(!branches.contains("strict"), "{branches}");
    assert!(branches.contains("gone"), "{branches}");
    assert_eq!(repository.git(&["rev-parse", "gone"]).trim(), gone_head);
}

#[test]
fn the_removal_refuses_a_lock_of_a_user_and_a_file_of_the_user_in_the_trash() {
    let repository = pushed();
    let locked = merged(&repository, "locked");
    repository.git(&[
        "worktree",
        "lock",
        "--reason",
        "mine",
        locked.to_str().unwrap(),
    ]);
    let request = delete_request(&repository, &locked, head(&locked));

    let refused = worktree_delete(&repository.permissions(), request).unwrap();

    assert_eq!(
        refused.refusals().collect::<Vec<_>>(),
        [WorktreeRefusal::Locked]
    );
    assert!(locked.exists());
    let list = repository.git(&["worktree", "list", "--porcelain"]);
    assert!(list.contains("locked mine"), "{list}");

    let in_trash = merged(&repository, "env");
    let trash = trash_path(&repository, "env");
    worktree_move_to_trash(
        &repository.permissions(),
        to_trash(&repository, &in_trash, &trash),
    )
    .unwrap();
    fs::write(trash.join(".env"), "SECRET=1\n").unwrap();
    let request = delete_request(&repository, &trash, head(&trash));

    let refused = worktree_delete(&repository.permissions(), request).unwrap();

    assert_eq!(
        refused.refusals().collect::<Vec<_>>(),
        [WorktreeRefusal::IgnoredUserFiles]
    );
    assert!(trash.join(".env").exists());
    let list = repository.git(&["worktree", "list", "--porcelain"]);
    assert!(
        list.contains(&format!("locked {TRASH_LOCK_REASON}")),
        "{list}"
    );
}

#[test]
fn the_prune_removes_only_the_records_that_the_plan_saw() {
    let repository = pushed();
    let missing = add_worktree(&repository, "missing");
    fs::remove_dir_all(&missing).unwrap();
    let locked = add_worktree(&repository, "locked-missing");
    repository.git(&["worktree", "lock", locked.to_str().unwrap()]);
    fs::remove_dir_all(&locked).unwrap();
    let request = |paths: Vec<String>| WorktreePruneRequest {
        repository: repository.path(),
        paths,
    };

    let refused = worktree_prune(&repository.permissions(), request(Vec::new())).unwrap();
    assert!(refused.pruned.is_empty());
    assert!(!refused.refusal.is_empty());
    assert!(repository.git(&["worktree", "list"]).contains("missing"));

    let pruned = worktree_prune(
        &repository.permissions(),
        request(vec![missing.display().to_string()]),
    )
    .unwrap();

    assert_eq!(pruned.pruned, [missing.display().to_string()]);
    let list = repository.git(&["worktree", "list"]);
    assert!(!list.contains(&format!("{} ", missing.display())), "{list}");
    assert!(list.contains("locked-missing"), "{list}");
}
