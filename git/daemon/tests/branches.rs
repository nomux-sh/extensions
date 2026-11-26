//! The list of branches and the branch operations.

mod repositories;

use nomux_extension::ErrorCode;
use nomux_git::branches::{branches, create_branch, switch};
use nomux_git::wire::v1::{BranchesRequest, CreateBranchRequest, SwitchRequest};
use repositories::TestRepository;

fn create(
    repository: &TestRepository,
    name: &str,
    start_point: &str,
    switch: bool,
) -> nomux_git::error::Result<()> {
    let request = CreateBranchRequest {
        repository: repository.path(),
        name: name.to_owned(),
        start_point: start_point.to_owned(),
        switch,
    };
    create_branch(&repository.permissions(), request).map(|_| ())
}

#[test]
fn the_list_has_each_branch_with_its_upstream_and_its_last_commit() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.add_remote();
    repository.git(&["branch", "feature"]);
    repository.write("a.txt", "b\n");
    repository.commit_all("second on main");
    let request = BranchesRequest {
        repository: repository.path(),
    };

    let list = branches(&repository.permissions(), request)
        .unwrap()
        .branches;

    let rows: Vec<_> = list
        .iter()
        .map(|branch| {
            let commit = branch.commit.as_ref().unwrap();
            (
                branch.name.as_str(),
                branch.is_remote,
                branch.is_head,
                branch.upstream.as_str(),
                commit.summary.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("feature", false, false, "", "first"),
            ("main", false, true, "origin/main", "second on main"),
            ("origin/main", true, false, "", "first"),
        ]
    );
    let commit = list[1].commit.as_ref().unwrap();
    assert_eq!(commit.id, repository.git(&["rev-parse", "main"]).trim());
    assert_eq!(
        commit.parent_ids,
        [repository.git(&["rev-parse", "main~1"]).trim()]
    );
    assert_eq!(
        (commit.author_name.as_str(), commit.author_email.as_str()),
        ("Test Author", "author@example.com")
    );
    assert_eq!(
        commit.author_time.to_string(),
        repository
            .git(&["log", "-1", "--format=%at", "main"])
            .trim()
    );
}

#[test]
fn a_branch_is_created_at_a_start_point_and_the_worktree_switches_to_it() {
    let repository = TestRepository::with_files(&[("a.txt", "first\n")]);
    repository.write("a.txt", "second\n");
    repository.commit_all("second");

    create(&repository, "from-first", "HEAD~1", false).unwrap();
    assert_eq!(repository.git(&["branch", "--show-current"]), "main\n");
    assert_eq!(
        repository.git(&["log", "-1", "--format=%s", "from-first"]),
        "first\n"
    );

    let request = SwitchRequest {
        repository: repository.path(),
        branch: "from-first".to_owned(),
    };
    switch(&repository.permissions(), request).unwrap();
    assert_eq!(
        repository.git(&["branch", "--show-current"]),
        "from-first\n"
    );
    assert_eq!(repository.read("a.txt"), "first\n");

    create(&repository, "and-switch", "", true).unwrap();
    assert_eq!(
        repository.git(&["branch", "--show-current"]),
        "and-switch\n"
    );
}

#[test]
fn a_name_that_git_can_read_as_an_option_is_refused() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.git(&["branch", "victim"]);

    let as_name = create(&repository, "--delete", "victim", false).unwrap_err();
    let as_start_point = create(&repository, "new", "--force", false).unwrap_err();
    let request = SwitchRequest {
        repository: repository.path(),
        branch: "--detach".to_owned(),
    };
    let as_branch = switch(&repository.permissions(), request).unwrap_err();

    for error in [as_name, as_start_point, as_branch] {
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
    assert_eq!(
        repository.git(&["branch", "--format=%(refname:short)"]),
        "main\nvictim\n"
    );
    assert_eq!(repository.git(&["branch", "--show-current"]), "main\n");
}
