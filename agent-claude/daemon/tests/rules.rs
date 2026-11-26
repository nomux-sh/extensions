//! One test for each row of the rule table of Claude Code, in the sequence
//! of the table in `README.md`.

mod support;

use nomux_agent_claude::{CLAUDE, Memory, reports};
use serde_json::json;
use support::{event, fixture_event, fixtures, send};

const SUBAGENT: &str = "a45f24c823c444cb5";

fn documented(name: &str) -> Vec<String> {
    let mut memory = Memory::default();
    documented_with(&mut memory, name)
}

fn documented_with(memory: &mut Memory, name: &str) -> Vec<String> {
    let path = fixtures().join("documented").join(format!("{name}.json"));
    reports(memory, &fixture_event(&path))
        .iter()
        .map(support::describe)
        .collect()
}

/// A memory after a prompt: the turn of the main agent runs.
fn running() -> Memory {
    let mut memory = Memory::default();
    send(&mut memory, "UserPromptSubmit", json!({"prompt": "fix it"}));
    memory
}

/// A memory after a prompt and the start of one subagent.
fn with_subagent() -> Memory {
    let mut memory = running();
    send(
        &mut memory,
        "SubagentStart",
        json!({"agent_id": SUBAGENT, "agent_type": "Explore"}),
    );
    memory
}

#[test]
fn a_session_start_after_a_compaction_works() {
    assert_eq!(documented("SessionStart-compact"), ["run working details"]);
}

#[test]
fn a_session_start_is_idle_with_the_details() {
    let mut memory = Memory::default();
    let path = fixtures().join("documented/SessionStart-startup-with-model.json");
    let reports = reports(&mut memory, &fixture_event(&path));
    let [nomux_agent_claude::Action::Run(report)] = reports.as_slice() else {
        panic!("one report of the Agent run: {reports:?}");
    };
    assert_eq!(support::describe(&reports[0]), "run idle details");
    let details = report.details.as_ref().unwrap();
    assert_eq!(details.model, "claude-haiku-4-5");
    assert_eq!(
        details.provider_session_id,
        "00000000-0000-4000-8000-000000000100"
    );
    assert_eq!(
        details.resume_command,
        "claude --resume 00000000-0000-4000-8000-000000000100 --model claude-haiku-4-5 --agent code-reviewer"
    );
}

#[test]
fn a_prompt_submit_works_firm_and_never_reports_the_prompt() {
    let mut memory = Memory::default();
    let lines = send(
        &mut memory,
        "UserPromptSubmit",
        json!({"prompt": "deploy with token sk-ant-secret"}),
    );
    assert_eq!(lines, ["run working details"]);
    assert!(!format!("{lines:?}").contains("deploy"));
}

#[test]
fn a_permission_request_needs_you_with_the_tool_and_the_command() {
    let path = fixtures().join("captured/permission-request/04-PermissionRequest.json");
    let lines: Vec<String> = reports(&mut running(), &fixture_event(&path))
        .iter()
        .map(support::describe)
        .collect();
    assert_eq!(
        lines,
        [r#"run needs-you permission tool=Bash message="touch b.txt" details"#]
    );
}

#[test]
fn a_permission_request_shows_a_short_detail_and_never_the_content() {
    let mut memory = running();
    let lines = send(
        &mut memory,
        "PermissionRequest",
        json!({"tool_name": "Write", "tool_input": {"file_path": "/home/user/project/a.rs", "content": "fn main() {}"}}),
    );
    assert_eq!(
        lines,
        [r#"run needs-you permission tool=Write message="/home/user/project/a.rs""#]
    );
    let command = format!("curl -H 'Authorization: Bearer abc' {}", "x".repeat(400));
    let lines = send(
        &mut memory,
        "PermissionRequest",
        json!({"tool_name": "Bash", "tool_input": {"command": command}}),
    );
    assert!(lines[0].contains("Bearer [hidden]"), "{lines:?}");
    assert!(!lines[0].contains("abc"), "{lines:?}");
    assert!(lines[0].len() < 260, "{lines:?}");
}

#[test]
fn the_tool_event_of_the_permission_request_works_firm() {
    let mut memory = running();
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "touch b.txt"}});
    send(&mut memory, "PermissionRequest", call.clone());
    assert_eq!(
        send(&mut memory, "PostToolUse", call.clone()),
        ["run working tool=Bash"]
    );
    assert_eq!(
        send(&mut memory, "PostToolUse", call),
        ["run working soft tool=Bash"],
        "the permission was answered: the next event is soft again"
    );
}

#[test]
fn a_tool_event_after_the_stop_reports_nothing() {
    let mut memory = running();
    send(&mut memory, "Stop", json!({"background_tasks": []}));
    for name in ["PreToolUse", "PostToolUse", "PostToolUseFailure"] {
        assert!(
            send(&mut memory, name, json!({"tool_name": "Read"})).is_empty(),
            "{name}"
        );
    }
}

#[test]
fn a_tool_event_works_soft_with_the_tool() {
    let mut memory = running();
    for name in ["PreToolUse", "PostToolUse"] {
        assert_eq!(
            send(&mut memory, name, json!({"tool_name": "Read"})),
            ["run working soft tool=Read"]
        );
    }
    assert_eq!(
        documented_with(&mut memory, "PostToolUseFailure"),
        ["run working soft tool=Bash"]
    );
}

#[test]
fn a_tool_event_of_a_subagent_works_soft_on_its_child_work() {
    let mut memory = with_subagent();
    assert_eq!(
        send(
            &mut memory,
            "PreToolUse",
            json!({"agent_id": SUBAGENT, "agent_type": "Explore", "tool_name": "Grep"})
        ),
        [format!(
            "child {SUBAGENT} start working soft title=Explore tool=Grep"
        )]
    );
}

#[test]
fn a_tool_batch_after_the_stop_reports_nothing() {
    let mut memory = running();
    send(&mut memory, "Stop", json!({}));
    assert!(send(&mut memory, "PostToolBatch", json!({"tool_calls": []})).is_empty());
}

#[test]
fn a_tool_batch_works_firm() {
    assert_eq!(
        send(&mut running(), "PostToolBatch", json!({"tool_calls": []})),
        ["run working"]
    );
}

#[test]
fn a_permission_prompt_notification_needs_you_for_a_permission() {
    assert_eq!(
        documented("Notification-permission_prompt"),
        [r#"run needs-you permission message="Claude needs your permission to use Bash" details"#]
    );
}

#[test]
fn an_elicitation_dialog_notification_needs_an_answer() {
    assert_eq!(
        documented("Notification-elicitation_dialog"),
        [r#"run needs-you question message="my-mcp-server needs input" details"#]
    );
}

#[test]
fn an_elicitation_url_dialog_notification_needs_a_login() {
    assert_eq!(
        documented("Notification-elicitation_url_dialog"),
        [r#"run needs-you auth message="my-mcp-server asks you to open a URL" details"#]
    );
}

/// `idle_prompt` comes about 60 seconds after the end of a turn, when the
/// user did not type: the `done` result becomes a wait for the user.
#[test]
fn an_idle_prompt_notification_needs_an_answer() {
    let mut memory = running();
    send(&mut memory, "Stop", json!({"background_tasks": []}));
    assert_eq!(
        documented_with(&mut memory, "Notification-idle_prompt"),
        [r#"run needs-you question message="Claude is waiting for your input""#]
    );
}

#[test]
fn an_agent_needs_input_notification_needs_an_answer() {
    assert_eq!(
        documented("Notification-agent_needs_input"),
        [r#"run needs-you question message="A background session needs your input" details"#]
    );
}

#[test]
fn a_notification_of_a_different_type_reports_nothing() {
    assert!(documented("Notification-auth_success").is_empty());
}

#[test]
fn a_url_elicitation_needs_a_login() {
    assert_eq!(
        documented("Elicitation-url"),
        [r#"run needs-you auth message="my-mcp-server" details"#]
    );
}

#[test]
fn a_form_elicitation_needs_an_answer() {
    assert_eq!(
        documented("Elicitation-form"),
        [r#"run needs-you question message="my-mcp-server" details"#]
    );
}

#[test]
fn an_elicitation_result_works_firm() {
    let mut memory = running();
    documented_with(&mut memory, "Elicitation-form");
    assert_eq!(
        documented_with(&mut memory, "ElicitationResult"),
        ["run working"]
    );
}

#[test]
fn a_subagent_start_starts_a_child_work_with_its_type() {
    let mut memory = running();
    assert_eq!(
        send(
            &mut memory,
            "SubagentStart",
            json!({"agent_id": SUBAGENT, "agent_type": "Explore"})
        ),
        [format!("child {SUBAGENT} start working title=Explore")]
    );
}

#[test]
fn a_subagent_stop_ends_its_child_work() {
    let mut memory = with_subagent();
    assert_eq!(
        send(
            &mut memory,
            "SubagentStop",
            json!({"agent_id": SUBAGENT, "agent_type": "Explore", "last_assistant_message": "found it"})
        ),
        [format!("child {SUBAGENT} end done title=Explore")]
    );
}

#[test]
fn a_teammate_idle_event_marks_its_child_work_done_and_keeps_it() {
    let mut memory = running();
    let lines = documented_with(&mut memory, "TeammateIdle");
    assert_eq!(
        lines,
        ["child researcher@session-a1b2c3d4 start done title=researcher"]
    );
    let again = send(
        &mut memory,
        "SubagentStart",
        json!({"agent_id": "researcher@session-a1b2c3d4", "agent_type": "researcher"}),
    );
    assert_eq!(
        again,
        ["child researcher@session-a1b2c3d4 start working title=researcher"]
    );
}

#[test]
fn a_teammate_idle_event_without_an_agent_identifier_reports_nothing() {
    assert!(
        send(
            &mut running(),
            "TeammateIdle",
            json!({"teammate_name": "researcher"})
        )
        .is_empty()
    );
}

#[test]
fn a_stop_with_a_running_subagent_is_done_with_work_in_the_background() {
    let mut memory = with_subagent();
    assert_eq!(
        send(&mut memory, "Stop", json!({})),
        [r#"run done message="1 task runs in the background""#]
    );
}

#[test]
fn a_stop_without_a_running_child_is_done() {
    assert_eq!(
        send(&mut running(), "Stop", json!({"background_tasks": []})),
        ["run done"]
    );
}

#[test]
fn a_stop_failure_is_an_error_with_the_reason() {
    assert_eq!(
        documented("StopFailure-rate-limit"),
        [r#"run error message="rate_limit: 429 Too Many Requests" details"#]
    );
}

#[test]
fn a_session_end_ends_each_child_work_and_the_run_is_done() {
    let mut memory = with_subagent();
    assert_eq!(
        documented_with(&mut memory, "SessionEnd-clear"),
        [
            format!("child {SUBAGENT} end done title=Explore"),
            "run done".to_owned(),
        ]
    );
}

#[test]
fn a_session_end_after_a_failure_keeps_the_error() {
    let mut memory = Memory::default();
    documented_with(&mut memory, "StopFailure-rate-limit");
    assert!(documented_with(&mut memory, "SessionEnd-clear").is_empty());
}

#[test]
fn a_model_switch_gives_the_model_to_the_next_report() {
    let mut memory = running();
    assert!(documented_with(&mut memory, "PostModelSwitch").is_empty());
    let reports = reports(
        &mut memory,
        &event("Stop", &support::data("Stop", json!({}))),
    );
    let [nomux_agent_claude::Action::Run(report)] = reports.as_slice() else {
        panic!("one report: {reports:?}");
    };
    let details = report.details.as_ref().unwrap();
    assert_eq!(details.model, "claude-sonnet-4-6");
    assert!(
        details
            .resume_command
            .ends_with(" --model claude-sonnet-4-6")
    );
}

#[test]
fn an_event_without_a_row_reports_nothing() {
    assert!(send(&mut running(), "CwdChanged", json!({"new_cwd": "/tmp"})).is_empty());
}

#[test]
fn the_table_has_each_event_that_the_install_plan_hooks() {
    assert_eq!(
        CLAUDE.events(),
        [
            "SessionStart",
            "UserPromptSubmit",
            "PermissionRequest",
            "PostToolUse",
            "PostToolUseFailure",
            "PreToolUse",
            "PostToolBatch",
            "Notification",
            "Elicitation",
            "ElicitationResult",
            "SubagentStart",
            "SubagentStop",
            "TeammateIdle",
            "Stop",
            "StopFailure",
            "SessionEnd",
            "PostModelSwitch",
        ]
    );
}
