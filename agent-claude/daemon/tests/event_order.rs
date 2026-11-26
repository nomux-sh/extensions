//! The order problems of the hooks of Claude Code: hooks run in parallel,
//! and an event can arrive after the end of the work it belongs to. The
//! captured sequences come from real runs of Claude Code 2.1.332.

mod support;

use nomux_agent_claude::Memory;
use serde_json::json;
use support::{replay, send};

const SUBAGENT: &str = "a45f24c823c444cb5";

fn running_with_subagent() -> Memory {
    let mut memory = Memory::default();
    send(&mut memory, "UserPromptSubmit", json!({"prompt": "go"}));
    send(
        &mut memory,
        "SubagentStart",
        json!({"agent_id": SUBAGENT, "agent_type": "Explore"}),
    );
    memory
}

#[test]
fn tool_events_that_arrive_after_the_stop_leave_the_result() {
    let mut memory = Memory::default();
    send(&mut memory, "UserPromptSubmit", json!({"prompt": "go"}));
    assert_eq!(
        send(&mut memory, "Stop", json!({"background_tasks": []})),
        ["run done"]
    );
    assert!(send(&mut memory, "PostToolUse", json!({"tool_name": "Read"})).is_empty());
    assert!(send(&mut memory, "PostToolBatch", json!({})).is_empty());
    assert_eq!(
        send(&mut memory, "UserPromptSubmit", json!({"prompt": "next"})),
        ["run working"],
        "a new prompt starts a new turn"
    );
}

#[test]
fn a_permission_request_and_then_a_different_tool_of_the_batch_still_needs_you() {
    let mut memory = Memory::default();
    send(&mut memory, "UserPromptSubmit", json!({"prompt": "go"}));
    send(
        &mut memory,
        "PermissionRequest",
        json!({"tool_name": "Bash", "tool_input": {"command": "rm -r build"}}),
    );
    assert_eq!(
        send(
            &mut memory,
            "PostToolUse",
            json!({"tool_name": "Read", "tool_input": {"file_path": "a.txt"}})
        ),
        ["run working soft tool=Read"],
        "a soft report: the host keeps the wait for the user"
    );
    assert_eq!(
        send(
            &mut memory,
            "PostToolUse",
            json!({"tool_name": "Bash", "tool_input": {"command": "rm -r build"}})
        ),
        ["run working tool=Bash"]
    );
}

#[test]
fn a_denied_permission_request_ends_with_the_tool_batch_or_the_stop() {
    let mut memory = Memory::default();
    send(&mut memory, "UserPromptSubmit", json!({"prompt": "go"}));
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "touch b.txt"}});
    send(&mut memory, "PermissionRequest", call.clone());
    assert_eq!(
        send(&mut memory, "PostToolBatch", json!({})),
        ["run working"]
    );

    send(&mut memory, "PermissionRequest", call);
    assert_eq!(send(&mut memory, "Stop", json!({})), ["run done"]);
}

#[test]
fn a_subagent_that_stops_after_the_parent_stopped_ends_the_work_in_the_background() {
    let mut memory = running_with_subagent();
    assert_eq!(
        send(
            &mut memory,
            "Stop",
            json!({"background_tasks": [
                {"id": SUBAGENT, "type": "subagent", "status": "running", "description": "read a.txt", "agent_type": "Explore"}
            ]})
        ),
        [
            format!(r#"child {SUBAGENT} start working soft title=Explore message="read a.txt""#),
            r#"run done message="1 task runs in the background""#.to_owned(),
        ]
    );
    assert_eq!(
        send(
            &mut memory,
            "PostToolUse",
            json!({"agent_id": SUBAGENT, "tool_name": "Read"})
        ),
        [format!(
            r#"child {SUBAGENT} start working soft title=Explore tool=Read message="read a.txt""#
        )],
        "the subagent still works after the stop of the parent"
    );
    assert_eq!(
        send(&mut memory, "SubagentStop", json!({"agent_id": SUBAGENT})),
        [
            format!(r#"child {SUBAGENT} end done title=Explore message="read a.txt""#),
            "run done".to_owned(),
        ]
    );
    assert!(
        send(
            &mut memory,
            "PostToolUse",
            json!({"agent_id": SUBAGENT, "tool_name": "Read"})
        )
        .is_empty(),
        "a late event of a stopped subagent does not open it again"
    );
}

#[test]
fn a_stop_without_background_tasks_closes_no_child_work() {
    let mut memory = running_with_subagent();
    assert_eq!(
        send(&mut memory, "Stop", json!({})),
        [r#"run done message="1 task runs in the background""#]
    );
}

#[test]
fn a_stop_with_background_tasks_closes_the_missing_children_and_adds_the_unseen_ones() {
    let mut memory = running_with_subagent();
    assert_eq!(
        send(
            &mut memory,
            "Stop",
            json!({"background_tasks": [
                {"id": "b2", "type": "subagent", "status": "running", "description": "review", "agent_type": "Plan"},
                {"id": "t1", "type": "teammate", "status": "running", "description": "tests", "name": "tester"},
                {"id": "s1", "type": "shell", "status": "running", "description": "tail logs", "command": "tail -f x"}
            ]})
        ),
        [
            format!("child {SUBAGENT} end done title=Explore"),
            r#"child b2 start working title=Plan message="review""#.to_owned(),
            r#"child t1 start working title=tester message="tests""#.to_owned(),
            r#"run done message="2 tasks run in the background""#.to_owned(),
        ]
    );
}

#[test]
fn a_captured_tool_call_works_and_ends_done() {
    assert_eq!(
        replay("captured/tool-call"),
        [
            "SessionStart: run idle details",
            "UserPromptSubmit: run working",
            "PreToolUse: run working soft tool=Read",
            "PostToolUse: run working soft tool=Read",
            "PostToolBatch: run working",
            "Stop: run done",
            "SessionEnd: run done",
        ]
    );
}

#[test]
fn a_captured_background_subagent_is_child_work_after_the_stop_of_its_parent() {
    assert_eq!(
        replay("captured/background-subagent"),
        [
            "SessionStart: run idle details",
            "UserPromptSubmit: run working",
            "PreToolUse: run working soft tool=Agent",
            &format!("SubagentStart: child {SUBAGENT} start working title=general-purpose"),
            "PostToolUse: run working soft tool=Agent",
            "PostToolBatch: run working",
            &format!(
                r#"Stop: child {SUBAGENT} start working soft title=general-purpose message="read a.txt""#
            ),
            r#"Stop: run done message="1 task runs in the background""#,
            &format!(
                r#"PreToolUse: child {SUBAGENT} start working soft title=general-purpose tool=Read message="read a.txt""#
            ),
            &format!(
                r#"PostToolUse: child {SUBAGENT} start working soft title=general-purpose tool=Read message="read a.txt""#
            ),
            &format!(
                r#"PostToolBatch: child {SUBAGENT} start working title=general-purpose message="read a.txt""#
            ),
            &format!(
                r#"SubagentStop: child {SUBAGENT} end done title=general-purpose message="read a.txt""#
            ),
            "SubagentStop: run done",
            "UserPromptSubmit: run working",
            "Stop: run done",
            "SessionEnd: run done",
        ]
    );
}

#[test]
fn a_captured_permission_request_needs_you_until_the_tool_batch() {
    assert_eq!(
        replay("captured/permission-request"),
        [
            "SessionStart: run idle details",
            "UserPromptSubmit: run working",
            "PreToolUse: run working soft tool=Bash",
            r#"PermissionRequest: run needs-you permission tool=Bash message="touch b.txt""#,
            "PostToolBatch: run working",
            "Stop: run done",
            "SessionEnd: run done",
        ]
    );
}

#[test]
fn a_captured_failure_to_log_in_is_an_error() {
    assert_eq!(
        replay("captured/not-logged-in"),
        [r#"StopFailure: run error message="authentication_failed" details"#]
    );
}
