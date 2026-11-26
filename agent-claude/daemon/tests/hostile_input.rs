//! Hook data that is not valid, has wrong types or is very large: the
//! adapter never panics, reports nothing for data that is not a valid hook
//! event, and each report that it makes passes the checks of the host.

mod support;

use nomux_agent_claude::{CLAUDE, Memory, reports};
use nomux_proto::wire::v1::HookEvent;
use serde_json::{Value, json};
use support::{assert_valid_for_host, data, event, fixture_event, fixture_files};

fn all_reports(memory: &mut Memory, event: &HookEvent) -> usize {
    let reports = reports(memory, event);
    reports.iter().for_each(assert_valid_for_host);
    reports.len()
}

#[test]
fn data_that_is_not_a_hook_event_gives_no_report() {
    let mut memory = Memory::default();
    let session_id = r#""session_id":"00000000-0000-4000-8000-000000000001""#;
    for payload in [
        String::new(),
        "not json".to_owned(),
        "[1,2,3]".to_owned(),
        "null".to_owned(),
        r#"{"hook_event_name":"Stop"}"#.to_owned(),
        r#"{"session_id":7,"hook_event_name":"Stop"}"#.to_owned(),
        format!(r#"{{{session_id},"hook_event_name":"CwdChanged"}}"#),
        format!(r#"{{{session_id},"hook_event_name":5}}"#),
        format!(r#"{{{session_id},"hook_event_name":"Stop""#),
        "\u{0}\u{1}\u{2}".to_owned(),
    ] {
        for name in CLAUDE.events() {
            assert_eq!(
                all_reports(&mut memory, &event(name, &payload)),
                0,
                "{name}: {payload:?}"
            );
        }
    }
}

#[test]
fn an_event_of_a_different_tool_gives_no_report() {
    let mut event = event("Stop", &data("Stop", json!({})));
    event.tool = "codex".to_owned();
    assert!(reports(&mut Memory::default(), &event).is_empty());
}

#[test]
fn a_field_with_a_wrong_type_counts_as_absent() {
    let mut memory = Memory::default();
    let wrong = json!({
        "tool_name": 5,
        "tool_input": "rm -rf /",
        "agent_type": ["Explore"],
        "background_tasks": "all",
        "notification_type": {"type": "permission_prompt"},
        "message": false,
        "error": 429,
        "model": 1.5,
        "to_model": null,
        "permission_mode": 0,
    });
    for name in CLAUDE.events() {
        all_reports(&mut memory, &event(name, &data(name, wrong.clone())));
    }
    let lines = support::send(&mut memory, "PreToolUse", wrong);
    assert_eq!(lines, ["run working soft details"]);
}

#[test]
fn an_agent_identifier_that_is_not_a_child_work_identifier_gives_no_report() {
    let mut memory = Memory::default();
    for agent_id in [json!("a/b"), json!("x".repeat(129)), json!("line\nbreak")] {
        let fields = json!({"agent_id": agent_id, "agent_type": "Explore"});
        for name in CLAUDE.events() {
            assert_eq!(
                all_reports(&mut memory, &event(name, &data(name, fields.clone()))),
                0,
                "{name}"
            );
        }
    }
}

#[test]
fn huge_texts_become_short_lines() {
    let mut memory = Memory::default();
    let huge = format!("{}\n{}", "é".repeat(60_000), "\u{7}".repeat(1000));
    let fields = json!({
        "agent_id": "x".repeat(128),
        "agent_type": huge,
        "tool_name": huge,
        "tool_input": {"command": huge, "file_path": huge},
        "message": huge,
        "notification_type": "permission_prompt",
        "error": huge,
        "error_details": huge,
        "mcp_server_name": huge,
        "teammate_name": huge,
        "model": huge,
        "to_model": huge,
        "session_id": huge,
        "background_tasks": [{"id": "t", "type": "subagent", "description": huge, "agent_type": huge}],
    });
    for agent in [true, false] {
        let mut fields = fields.clone();
        if !agent {
            fields.as_object_mut().unwrap().remove("agent_id");
        }
        for name in CLAUDE.events() {
            all_reports(&mut memory, &event(name, &data(name, fields.clone())));
        }
    }
}

#[test]
fn an_omitted_payload_reports_from_the_event_name_only() {
    let mut memory = Memory::default();
    let mut stop = event("Stop", "");
    stop.payload_omitted = true;
    let lines: Vec<String> = reports(&mut memory, &stop)
        .iter()
        .map(support::describe)
        .collect();
    assert_eq!(lines, ["run done"]);
}

/// Each captured event with each prefix of its bytes, and with each field
/// replaced by a value of a different type.
#[test]
fn damaged_captured_events_never_panic_and_give_valid_reports() {
    let mut memory = Memory::default();
    for directory in [
        "captured/tool-call",
        "captured/background-subagent",
        "captured/permission-request",
        "captured/not-logged-in",
    ] {
        for path in fixture_files(directory) {
            let original = fixture_event(&path);
            for end in (0..original.payload.len()).step_by(7) {
                let mut damaged = original.clone();
                damaged.payload.truncate(end);
                all_reports(&mut memory, &damaged);
            }
            let fields: Value = serde_json::from_slice(&original.payload).unwrap();
            for key in fields.as_object().unwrap().keys() {
                for replacement in [json!(null), json!(1), json!([]), json!({}), json!("")] {
                    let mut changed = fields.clone();
                    changed[key] = replacement;
                    let mut damaged = original.clone();
                    damaged.payload = changed.to_string().into_bytes();
                    all_reports(&mut memory, &damaged);
                }
            }
        }
    }
}
