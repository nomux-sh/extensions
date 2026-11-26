//! Real git repositories in temporary directories, made with the `git`
//! program.

// Each test crate that includes this module uses a different part of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nomux_extension::Permissions;
use tempfile::TempDir;

/// A temporary directory that the user approved, with one repository in
/// `repository/`. Worktrees and remotes of a test go next to it.
pub struct TestRepository {
    directory: TempDir,
    granted: PathBuf,
}

impl TestRepository {
    /// Makes a repository with the branch `main` and no commit.
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let granted = directory.path().canonicalize().unwrap();
        let repository = Self { directory, granted };
        git_in(
            &repository.granted,
            &["init", "--quiet", "--initial-branch=main", "repository"],
        );
        configure(&repository.root());
        repository
    }

    /// Makes a repository with one commit that has `files`.
    pub fn with_files(files: &[(&str, &str)]) -> Self {
        let repository = Self::new();
        for (path, content) in files {
            repository.write(path, content);
        }
        repository.commit_all("first");
        repository
    }

    /// The directory that the user approved.
    pub fn granted(&self) -> &Path {
        &self.granted
    }

    pub fn root(&self) -> PathBuf {
        self.granted.join("repository")
    }

    /// The `repository` field of a request.
    pub fn path(&self) -> String {
        self.root().display().to_string()
    }

    pub fn permissions(&self) -> Permissions {
        Permissions {
            fs: vec![self.directory.path().display().to_string()],
            process: vec!["git".to_owned()],
            ..Permissions::default()
        }
    }

    pub fn write(&self, path: &str, content: &str) {
        let file = self.root().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root().join(path)).unwrap()
    }

    /// Runs `git` in the repository and returns its standard output.
    pub fn git(&self, arguments: &[&str]) -> String {
        git_in(&self.root(), arguments)
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
    }

    /// Makes a bare repository next to the repository, adds it as the remote
    /// `origin` and pushes `main` to it.
    pub fn add_remote(&self) {
        git_in(
            &self.granted,
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                "remote.git",
            ],
        );
        self.git(&["remote", "add", "origin", "../remote.git"]);
        self.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
    }
}

/// Runs `git` in a directory and returns its standard output. The test fails
/// when the command fails.
pub fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Sets the configuration that makes a test independent of the configuration
/// of the computer.
pub fn configure(repository: &Path) {
    for (key, value) in [
        ("user.name", "Test Author"),
        ("user.email", "author@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.hooksPath", ".git/hooks"),
        ("status.showUntrackedFiles", "normal"),
    ] {
        git_in(repository, &["config", key, value]);
    }
}
