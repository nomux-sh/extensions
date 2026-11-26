//! The command that continues a Claude Code session.
//!
//! The hook data has no command line of Claude Code, thus the adapter makes
//! the command from the fields that it knows. The rule:
//!
//! - `claude --resume <session>`. Without a valid session identifier, there
//!   is no command.
//! - `--model <model>` and `--agent <agent>` if the value is a plain name:
//!   letters, digits, `.`, `_`, `:` and `-`, 128 bytes at most, and nothing
//!   that looks like a secret. Any other value is dropped, not quoted.
//! - `--permission-mode <mode>` only for `plan`, `acceptEdits` and
//!   `dontAsk`. A mode that skips or relaxes the permission checks
//!   (`bypassPermissions`, `auto`) is dropped: nomux never starts an agent
//!   with fewer permission checks than the default.
//! - No prompt text, no other option, no environment variable.

use crate::safe_text::looks_like_secret;

/// The largest number of bytes of one value in the command.
const MAX_VALUE: usize = 128;

/// The permission modes that a resume command keeps.
const KEPT_PERMISSION_MODES: [&str; 3] = ["plan", "acceptEdits", "dontAsk"];

/// What the adapter knows about a Claude Code session.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    /// The session identifier of Claude Code.
    pub session_id: Option<String>,
    /// The model, from `SessionStart` or `PostModelSwitch`.
    pub model: Option<String>,
    /// The agent of the session (`claude --agent`).
    pub agent: Option<String>,
    /// The permission mode of the last event.
    pub permission_mode: Option<String>,
}

/// The resume command of the session, or `None` without a valid session
/// identifier.
#[must_use]
pub fn resume_command(facts: &SessionFacts) -> Option<String> {
    let session = facts.session_id.as_deref().filter(|id| is_session_id(id))?;
    let mut command = format!("claude --resume {session}");
    for (option, value) in [("--model", &facts.model), ("--agent", &facts.agent)] {
        if let Some(value) = value.as_deref().filter(|value| is_plain_name(value)) {
            command.push_str(&format!(" {option} {value}"));
        }
    }
    if let Some(mode) = facts
        .permission_mode
        .as_deref()
        .filter(|mode| KEPT_PERMISSION_MODES.contains(mode))
    {
        command.push_str(&format!(" --permission-mode {mode}"));
    }
    Some(command)
}

/// `true` for a session identifier of Claude Code: a UUID, or letters,
/// digits, `-` and `_`.
#[must_use]
pub fn is_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_VALUE
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn is_plain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
        && !looks_like_secret(value)
}
