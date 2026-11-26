//! The rule of the resume command: the session, plain model and agent
//! names, a permission mode that keeps the checks, and nothing else.

use nomux_agent_claude::{SessionFacts, resume_command};

const SESSION: &str = "0f82b0bc-bad3-4ad6-8593-9c58b02b8817";

fn facts(model: &str, agent: &str, permission_mode: &str) -> SessionFacts {
    let text = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    SessionFacts {
        session_id: Some(SESSION.to_owned()),
        model: text(model),
        agent: text(agent),
        permission_mode: text(permission_mode),
    }
}

#[test]
fn the_command_resumes_the_session_with_the_model_the_agent_and_the_mode() {
    assert_eq!(
        resume_command(&facts("claude-haiku-4-5", "my-plugin:reviewer", "plan")).unwrap(),
        format!(
            "claude --resume {SESSION} --model claude-haiku-4-5 --agent my-plugin:reviewer --permission-mode plan"
        )
    );
    assert_eq!(
        resume_command(&facts("", "", "acceptEdits")).unwrap(),
        format!("claude --resume {SESSION} --permission-mode acceptEdits")
    );
}

#[test]
fn a_mode_that_skips_or_relaxes_the_permission_checks_is_dropped() {
    for mode in [
        "bypassPermissions",
        "auto",
        "default",
        "--dangerously-skip-permissions",
    ] {
        assert_eq!(
            resume_command(&facts("", "", mode)).unwrap(),
            format!("claude --resume {SESSION}"),
            "{mode}"
        );
    }
}

#[test]
fn a_value_that_is_not_a_plain_name_or_looks_like_a_secret_is_dropped() {
    for value in [
        "fix the bug in main.rs",
        "x; rm -rf ~",
        "$(id)",
        "--settings=/tmp/s.json",
        "claude-opus-4-6[1m]",
        "sk-ant-api03-abcdef",
        "ghp_example",
        "aGVsbG8gd29ybGQgdGhpcyBpcyBhIHNlY3JldDEyMw",
        &"m".repeat(129),
    ] {
        assert_eq!(
            resume_command(&facts(value, value, "")).unwrap(),
            format!("claude --resume {SESSION}"),
            "{value}"
        );
    }
}

#[test]
fn a_session_without_a_valid_identifier_has_no_command() {
    for id in [None, Some(""), Some("a b"), Some("../x"), Some("id;ls")] {
        let facts = SessionFacts {
            session_id: id.map(str::to_owned),
            model: Some("claude-haiku-4-5".to_owned()),
            ..SessionFacts::default()
        };
        assert_eq!(resume_command(&facts), None, "{id:?}");
    }
}
