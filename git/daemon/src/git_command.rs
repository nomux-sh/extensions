//! The runner of the `git` program of the user.
//!
//! The methods that write use it, thus the hooks, the signing and the
//! configuration of the user apply. Each argument goes to the program as one
//! argument: no shell reads the command.

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// One run of `git` in a worktree.
pub struct GitCommand {
    command: Command,
    input: Option<Vec<u8>>,
}

impl GitCommand {
    /// Starts the definition of a command that runs in `worktree`.
    pub fn new(worktree: &Path) -> Self {
        let mut command = Command::new("git");
        command
            .current_dir(worktree)
            // The process has no terminal: a question would wait without end.
            .env("GIT_TERMINAL_PROMPT", "0");
        Self {
            command,
            input: None,
        }
    }

    /// Adds an argument that the extension wrote.
    pub fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.command.arg(argument);
        self
    }

    /// Adds a name or a revision of a request. A value that git can read as
    /// an option is refused.
    pub fn value(self, value: &str) -> Result<Self> {
        if value.is_empty() || value.starts_with('-') {
            return Err(Error::invalid_argument(format!(
                "`{value}` is not a name or a revision"
            )));
        }
        Ok(self.arg(value))
    }

    /// Adds `--` and the paths. git reads no argument after `--` as an
    /// option.
    pub fn paths<Path: AsRef<OsStr>>(mut self, paths: impl IntoIterator<Item = Path>) -> Self {
        self.command.arg("--").args(paths);
        self
    }

    /// Sets the bytes that the program reads from its standard input.
    pub fn input(mut self, input: Vec<u8>) -> Self {
        self.input = Some(input);
        self
    }

    /// Runs the command and returns its standard output. The output of a
    /// command that fails is the message of the error.
    pub fn run(mut self) -> Result<Vec<u8>> {
        let subcommand = self
            .command
            .get_args()
            .next()
            .map(|argument| argument.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut child = self
            .command
            .stdin(if self.input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| Error::failed(format!("the program `git` did not start: {error}")))?;
        let written = match (self.input, child.stdin.take()) {
            (Some(input), Some(mut stdin)) => stdin.write_all(&input),
            _ => Ok(()),
        };
        let output = child.wait_with_output()?;
        if !output.status.success() {
            // Some refusals are on the standard output, for example "nothing
            // to commit".
            let reason = match output.stderr.trim_ascii() {
                [] => output.stdout.trim_ascii(),
                stderr => stderr,
            };
            return Err(Error::failed(format!(
                "git {subcommand}: {}",
                String::from_utf8_lossy(reason)
            )));
        }
        written?;
        Ok(output.stdout)
    }
}
