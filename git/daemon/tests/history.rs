//! The pages of the log and of the blame.

mod repositories;

use nomux_git::blame::blame;
use nomux_git::log::log;
use nomux_git::wire::v1::{BlameRequest, BlameResponse, LogRequest, LogResponse};
use repositories::TestRepository;

/// A repository with five commits. Commit `n` adds the line `n` to
/// `file.txt`. The dates of the commits are one minute apart.
fn five_commits() -> TestRepository {
    let repository = TestRepository::new();
    let mut content = String::new();
    for number in 1..=5 {
        content.push_str(&format!("line {number}\n"));
        repository.write("file.txt", &content);
        repository.git(&["add", "--all"]);
        let date = format!("2026-01-01T00:0{number}:00Z");
        let output = std::process::Command::new("git")
            .current_dir(repository.root())
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args([
                "commit",
                "--quiet",
                "--message",
                &format!("commit {number}"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    repository
}

fn log_page(repository: &TestRepository, skip: u32, limit: u32) -> LogResponse {
    let request = LogRequest {
        repository: repository.path(),
        skip,
        limit,
        ..LogRequest::default()
    };
    log(&repository.permissions(), request).unwrap()
}

fn summaries(response: &LogResponse) -> Vec<&str> {
    response
        .commits
        .iter()
        .map(|commit| commit.summary.as_str())
        .collect()
}

#[test]
fn the_pages_of_the_log_have_all_the_commits_one_time_the_newest_first() {
    let repository = five_commits();

    let first = log_page(&repository, 0, 2);
    let second = log_page(&repository, 2, 2);
    let third = log_page(&repository, 4, 2);

    assert_eq!(summaries(&first), ["commit 5", "commit 4"]);
    assert_eq!(summaries(&second), ["commit 3", "commit 2"]);
    assert_eq!(summaries(&third), ["commit 1"]);
    assert_eq!(
        [first.has_more, second.has_more, third.has_more],
        [true, true, false]
    );
}

#[test]
fn the_log_starts_at_the_revision_of_the_request() {
    let repository = five_commits();
    let request = LogRequest {
        repository: repository.path(),
        revision: "HEAD~3".to_owned(),
        ..LogRequest::default()
    };

    let response = log(&repository.permissions(), request).unwrap();

    assert_eq!(summaries(&response), ["commit 2", "commit 1"]);
}

#[test]
fn the_log_of_a_repository_without_a_commit_is_empty() {
    let repository = TestRepository::new();

    assert_eq!(log_page(&repository, 0, 0), LogResponse::default());
}

fn blame_page(repository: &TestRepository, start_line: u32, max_lines: u32) -> BlameResponse {
    let request = BlameRequest {
        repository: repository.path(),
        path: "file.txt".to_owned(),
        start_line,
        max_lines,
        ..BlameRequest::default()
    };
    blame(&repository.permissions(), request).unwrap()
}

#[test]
fn each_line_of_a_blame_page_names_the_commit_that_added_it() {
    let repository = five_commits();

    let response = blame_page(&repository, 2, 3);

    assert_eq!(response.total_lines, 5);
    let mut lines = Vec::new();
    for hunk in &response.hunks {
        let commit = response
            .commits
            .iter()
            .find(|commit| commit.id == hunk.commit_id)
            .unwrap();
        for offset in 0..hunk.line_count {
            lines.push((hunk.start_line + offset, commit.summary.as_str()));
        }
    }
    assert_eq!(lines, [(2, "commit 2"), (3, "commit 3"), (4, "commit 4")]);
    assert_eq!(response.commits.len(), 3);
}

#[test]
fn a_blame_examines_the_version_of_the_revision() {
    let repository = five_commits();
    let request = BlameRequest {
        repository: repository.path(),
        path: "file.txt".to_owned(),
        revision: "HEAD~2".to_owned(),
        ..BlameRequest::default()
    };

    let response = blame(&repository.permissions(), request).unwrap();

    assert_eq!(response.total_lines, 3);
    assert_eq!(
        response
            .hunks
            .iter()
            .map(|hunk| hunk.line_count)
            .sum::<u32>(),
        3
    );
}
