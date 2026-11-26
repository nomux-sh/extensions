//! The git facts of the worktrees, with real repositories: the merge rules,
//! the changes, the commits that are not pushed, the locks, the submodules,
//! the missing directories, the detached commits, the ignored paths, and the
//! proof that the method changes no file.

mod repositories;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nomux_extension::Permissions;
use nomux_git::worktree_facts::worktree_facts;
use nomux_proto::wire::v1::{
    WorktreeFacts, WorktreeFactsRequest, WorktreeFactsResponse, WorktreeMergedBy,
};
use repositories::{TestRepository, git_in};

fn facts(repository: &TestRepository) -> WorktreeFactsResponse {
    facts_with(repository, &repository.permissions())
}

fn facts_with(repository: &TestRepository, permissions: &Permissions) -> WorktreeFactsResponse {
    let request = WorktreeFactsRequest {
        repository: repository.path(),
    };
    worktree_facts(permissions, request).unwrap()
}

/// The facts of the worktree in the directory `name` next to the repository.
fn of(response: &WorktreeFactsResponse, name: &str) -> WorktreeFacts {
    response
        .worktrees
        .iter()
        .find(|worktree| Path::new(&worktree.path).file_name().unwrap() == name)
        .unwrap_or_else(|| panic!("no worktree {name} in {response:?}"))
        .clone()
}

/// A repository with `a.txt` on `main`, pushed to `origin`, with
/// `origin/HEAD`.
fn pushed() -> TestRepository {
    let repository =
        TestRepository::with_files(&[(".gitignore", "target/\n.env\n"), ("a.txt", "1\n2\n3\n")]);
    repository.add_remote();
    repository.git(&["remote", "set-head", "origin", "main"]);
    repository
}

/// Adds the linked worktree `name` next to the repository on the new
/// branch `branch`, and returns its directory.
fn add_worktree(repository: &TestRepository, name: &str, branch: &str) -> PathBuf {
    let path = repository.granted().join(name);
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        branch,
        path.to_str().unwrap(),
    ]);
    path
}

/// Puts the commit of `worktree` on `main` with a new hash, as a rebase
/// merge of a forge does: `main` gets a commit of its own first.
fn rebase_merge(repository: &TestRepository, worktree: &Path) {
    repository.write("other.txt", "other\n");
    repository.commit_all("other");
    let commit = git_in(worktree, &["rev-parse", "HEAD"]);
    repository.git(&["cherry-pick", commit.trim()]);
}

fn commit_in(worktree: &Path, file: &str, content: &str) {
    fs::write(worktree.join(file), content).unwrap();
    git_in(worktree, &["add", "--all"]);
    git_in(worktree, &["commit", "--quiet", "--message", file]);
}

#[test]
fn the_merge_rules_find_a_merge_a_rebase_and_a_squash_without_a_later_change() {
    let repository = pushed();
    let merged = add_worktree(&repository, "merged", "merged");
    commit_in(&merged, "m.txt", "m\n");
    repository.git(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "merge",
        "merged",
    ]);
    let rebased = add_worktree(&repository, "rebased", "rebased");
    commit_in(&rebased, "r.txt", "r\n");
    rebase_merge(&repository, &rebased);
    let squashed = add_worktree(&repository, "squashed", "squashed");
    commit_in(&squashed, "s1.txt", "s\n");
    commit_in(&squashed, "s2.txt", "s\n");
    repository.git(&["merge", "--quiet", "--squash", "squashed"]);
    repository.git(&["commit", "--quiet", "--message", "squash"]);
    let open = add_worktree(&repository, "open", "open");
    commit_in(&open, "o.txt", "o\n");
    repository.git(&["push", "--quiet", "origin", "main"]);

    let response = facts(&repository);

    assert_eq!(response.base_branch, "origin/main");
    let merged_by = |name| of(&response, name).merged_by();
    assert_eq!(merged_by("merged"), WorktreeMergedBy::Ancestor);
    assert_eq!(merged_by("rebased"), WorktreeMergedBy::Tree);
    assert_eq!(merged_by("squashed"), WorktreeMergedBy::Tree);
    assert_eq!(merged_by("open"), WorktreeMergedBy::Unspecified);
    // The base branch itself is never merged into itself.
    assert_eq!(merged_by("repository"), WorktreeMergedBy::Unspecified);
}

#[test]
fn a_squash_with_a_later_change_of_the_same_lines_is_merged_only_by_a_gone_upstream() {
    let repository = pushed();
    let squashed = add_worktree(&repository, "squashed", "squashed");
    commit_in(&squashed, "a.txt", "1\ntwo\n3\n");
    git_in(
        &squashed,
        &["push", "--quiet", "--set-upstream", "origin", "squashed"],
    );
    repository.git(&["merge", "--quiet", "--squash", "squashed"]);
    repository.git(&["commit", "--quiet", "--message", "squash"]);
    repository.write("a.txt", "1\nTWO\n3\n");
    repository.commit_all("later change");
    repository.git(&["push", "--quiet", "origin", "main"]);

    let before = of(&facts(&repository), "squashed");
    repository.git(&["push", "--quiet", "origin", "--delete", "squashed"]);
    repository.git(&["fetch", "--quiet", "--prune"]);
    let after = of(&facts(&repository), "squashed");

    assert_eq!(before.merged_by(), WorktreeMergedBy::Unspecified);
    assert!(!before.upstream_gone);
    assert_eq!(after.merged_by(), WorktreeMergedBy::UpstreamGone);
    assert!(after.upstream_gone);
}

#[test]
fn changes_and_commits_that_are_not_pushed_are_found() {
    let repository = pushed();
    let changed = add_worktree(&repository, "changed", "changed");
    fs::write(changed.join("a.txt"), "changed\n").unwrap();
    let untracked = add_worktree(&repository, "untracked", "untracked");
    fs::write(untracked.join("new.txt"), "new\n").unwrap();
    let ignored = add_worktree(&repository, "ignored", "ignored");
    fs::write(ignored.join(".env"), "SECRET=1\n").unwrap();
    let ahead = add_worktree(&repository, "ahead", "ahead");
    commit_in(&ahead, "b.txt", "b\n");
    commit_in(&ahead, "c.txt", "c\n");

    let response = facts(&repository);

    assert!(of(&response, "changed").is_dirty);
    assert!(of(&response, "untracked").is_dirty);
    let ignored = of(&response, "ignored");
    assert!(!ignored.is_dirty);
    assert_eq!(ignored.ignored_paths, [".env"]);
    assert_eq!(of(&response, "ahead").unpushed_commits, 2);
    assert_eq!(of(&response, "changed").unpushed_commits, 0);
}

#[test]
fn without_a_remote_the_commits_that_the_base_branch_does_not_have_are_not_pushed() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let ahead = add_worktree(&repository, "ahead", "ahead");
    commit_in(&ahead, "b.txt", "b\n");

    let response = facts(&repository);

    assert_eq!(response.base_branch, "main");
    assert_eq!(of(&response, "ahead").unpushed_commits, 1);
    assert_eq!(of(&response, "repository").unpushed_commits, 0);
}

#[test]
fn a_lock_a_submodule_a_missing_directory_and_detached_commits_are_found() {
    let repository = pushed();
    let locked = add_worktree(&repository, "locked", "locked");
    repository.git(&[
        "worktree",
        "lock",
        "--reason",
        "agent 7 runs",
        locked.to_str().unwrap(),
    ]);
    let with_submodule = add_worktree(&repository, "with-submodule", "with-submodule");
    fs::write(with_submodule.join(".gitmodules"), "").unwrap();
    let missing = add_worktree(&repository, "missing", "missing");
    fs::remove_dir_all(&missing).unwrap();
    let detached = repository.granted().join("detached");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        detached.to_str().unwrap(),
    ]);
    let clean_detached = repository.granted().join("clean-detached");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        clean_detached.to_str().unwrap(),
    ]);
    commit_in(&detached, "d.txt", "d\n");

    let response = facts(&repository);

    let locked = of(&response, "locked");
    assert!(locked.is_locked);
    assert_eq!(locked.locked_reason, "agent 7 runs");
    assert!(of(&response, "with-submodule").has_submodules);
    let missing = of(&response, "missing");
    assert!(missing.is_missing);
    assert_eq!(missing.branch, "missing");
    let detached = of(&response, "detached");
    assert!(detached.has_detached_commits);
    assert!(detached.branch.is_empty());
    assert!(!of(&response, "clean-detached").has_detached_commits);
}

#[test]
fn a_worktree_that_a_tool_made_below_the_repository_is_listed_with_its_ignored_build_output() {
    let repository = pushed();
    repository.write(".gitignore", "target/\n.env\n.claude/\n");
    repository.commit_all("ignore the tool directory");
    let tool = repository.root().join(".claude/worktrees/agent-1");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "worktree-agent-1",
        tool.to_str().unwrap(),
    ]);
    fs::create_dir_all(tool.join("target/debug")).unwrap();
    fs::write(tool.join("target/debug/program"), "built").unwrap();

    let response = facts(&repository);

    let paths: Vec<&str> = response
        .worktrees
        .iter()
        .map(|worktree| worktree.path.as_str())
        .collect();
    assert_eq!(paths, [repository.path(), tool.display().to_string()]);
    let agent = of(&response, "agent-1");
    assert_eq!(agent.branch, "worktree-agent-1");
    assert_eq!(agent.ignored_paths, ["target/"]);
    assert!(response.worktrees[0].is_main);
}

#[test]
fn a_linked_worktree_outside_the_approved_directories_has_only_the_facts_of_the_git_directory() {
    let repository = pushed();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().canonicalize().unwrap().join("outside");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "outside",
        path.to_str().unwrap(),
    ]);
    fs::write(path.join("a.txt"), "changed\n").unwrap();

    let response = facts(&repository);

    let outside = of(&response, "outside");
    assert_eq!(outside.branch, "outside");
    assert!(
        outside
            .error
            .contains("not in a directory that the user approved")
    );
    assert!(!outside.is_dirty);
}

#[test]
fn the_method_needs_no_git_permission_and_changes_no_file_of_the_repository() {
    let repository = pushed();
    let open = add_worktree(&repository, "open", "open");
    commit_in(&open, "o.txt", "o\n");
    fs::write(open.join("a.txt"), "changed\n").unwrap();
    let merged = add_worktree(&repository, "merged", "merged");
    commit_in(&merged, "m.txt", "m\n");
    rebase_merge(&repository, &merged);
    repository.git(&["push", "--quiet", "origin", "main"]);
    let permissions = Permissions {
        process: Vec::new(),
        ..repository.permissions()
    };
    let before = listing(repository.granted());

    let response = facts_with(&repository, &permissions);

    assert_eq!(of(&response, "merged").merged_by(), WorktreeMergedBy::Tree);
    assert_eq!(listing(repository.granted()), before);
}

/// Each file and directory below `root` with its size and its modification
/// time, also in the git directories.
fn listing(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut listing = BTreeMap::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                directories.push(entry.path());
            }
            listing.insert(entry.path(), (metadata.len(), metadata.modified().unwrap()));
        }
    }
    listing
}
