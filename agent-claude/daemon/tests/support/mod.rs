//! Helpers of the tests: hook events from JSON and from the fixtures, and a
//! short text for each report.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use nomux_agent_claude::{Action, Memory, reports};
use nomux_proto::wire::v1::{
    AgentReport, AgentState, ChildWorkChange, ChildWorkReport, HookEvent, NeedsYouKind,
};

/// The nomux session of the tests.
pub const SESSION: &str = "k3xq2abc";

/// A hook event of Claude Code with the JSON data `payload`.
pub fn event(name: &str, payload: &str) -> HookEvent {
    HookEvent {
        session_id: SESSION.to_owned(),
        tool: "claude".to_owned(),
        event: name.to_owned(),
        payload: payload.as_bytes().to_vec(),
        payload_omitted: false,
    }
}

/// The data of an event of the main agent with the common fields and
/// `fields`.
pub fn data(name: &str, fields: serde_json::Value) -> String {
    let mut data = serde_json::json!({
        "session_id": "00000000-0000-4000-8000-000000000100",
        "transcript_path": "/home/user/.claude/projects/p/s.jsonl",
        "cwd": "/home/user/project",
        "permission_mode": "default",
        "hook_event_name": name,
    });
    let fields = fields
        .as_object()
        .expect("the fields are an object")
        .clone();
    data.as_object_mut().expect("an object").extend(fields);
    data.to_string()
}

/// The reports of one event, as text.
pub fn send(memory: &mut Memory, name: &str, fields: serde_json::Value) -> Vec<String> {
    let reports = reports(memory, &event(name, &data(name, fields)));
    reports.iter().map(describe).collect()
}

/// The directory of the fixtures.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The event of a fixture file: its name is `<sequence>-<event>.json` or
/// `<event>-<variant>.json`.
pub fn fixture_event(path: &Path) -> HookEvent {
    let payload = std::fs::read_to_string(path).expect("the fixture is readable");
    let name: serde_json::Value = serde_json::from_str(&payload).expect("the fixture is JSON");
    let name = name["hook_event_name"]
        .as_str()
        .expect("the fixture names its event");
    event(name, &payload)
}

/// The fixture files of a directory, in the sequence of their names.
pub fn fixture_files(directory: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(fixtures().join(directory))
        .expect("the fixture directory exists")
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    files.sort();
    files
}

/// Replays the fixtures of `directory` and gives the reports of each event,
/// as `<event>: <report>`.
pub fn replay(directory: &str) -> Vec<String> {
    let mut memory = Memory::default();
    let mut lines = Vec::new();
    for path in fixture_files(directory) {
        let event = fixture_event(&path);
        for action in reports(&mut memory, &event) {
            lines.push(format!("{}: {}", event.event, describe(&action)));
        }
    }
    lines
}

/// A short text for a report, for example `run working soft tool=Read`.
pub fn describe(action: &Action) -> String {
    match action {
        Action::Run(report) => describe_run(report),
        Action::Child(report) => describe_child(report),
    }
}

fn describe_run(report: &AgentReport) -> String {
    let mut parts = vec!["run".to_owned(), state(report.state)];
    parts.extend(common(
        report.soft,
        report.kind,
        "",
        &report.tool,
        &report.message,
    ));
    if report.details.is_some() {
        parts.push("details".to_owned());
    }
    parts.join(" ")
}

fn describe_child(report: &ChildWorkReport) -> String {
    let change = match ChildWorkChange::try_from(report.change) {
        Ok(ChildWorkChange::Start) => "start",
        Ok(ChildWorkChange::Update) => "update",
        Ok(ChildWorkChange::End) => "end",
        _ => "unspecified",
    };
    let mut parts = vec![
        "child".to_owned(),
        report.id.clone(),
        change.to_owned(),
        state(report.state),
    ];
    parts.extend(common(
        report.soft,
        report.kind,
        &report.title,
        &report.tool,
        &report.message,
    ));
    parts.join(" ")
}

fn common(soft: bool, kind: i32, title: &str, tool: &str, message: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if soft {
        parts.push("soft".to_owned());
    }
    match NeedsYouKind::try_from(kind) {
        Ok(NeedsYouKind::Permission) => parts.push("permission".to_owned()),
        Ok(NeedsYouKind::Question) => parts.push("question".to_owned()),
        Ok(NeedsYouKind::Auth) => parts.push("auth".to_owned()),
        _ => {}
    }
    if !title.is_empty() {
        parts.push(format!("title={title}"));
    }
    if !tool.is_empty() {
        parts.push(format!("tool={tool}"));
    }
    if !message.is_empty() {
        parts.push(format!("message={message:?}"));
    }
    parts
}

fn state(state: i32) -> String {
    match AgentState::try_from(state) {
        Ok(AgentState::Idle) => "idle",
        Ok(AgentState::Working) => "working",
        Ok(AgentState::Done) => "done",
        Ok(AgentState::NeedsYou) => "needs-you",
        Ok(AgentState::Error) => "error",
        _ => "unspecified",
    }
    .to_owned()
}

/// Fails if the host would refuse a part of the report: the checks of
/// `spec/adapter.md` of the `proto` repository.
pub fn assert_valid_for_host(action: &Action) {
    let line = |text: &str| {
        assert!(text.len() <= 512, "{} bytes: {text:?}", text.len());
        assert!(!text.chars().any(char::is_control), "{text:?}");
    };
    match action {
        Action::Run(report) => {
            assert_ne!(report.state, AgentState::Unspecified as i32);
            for text in [&report.message, &report.title, &report.tool] {
                line(text);
            }
            if let Some(details) = &report.details {
                for text in [
                    &details.model,
                    &details.provider_session_id,
                    &details.resume_command,
                ] {
                    line(text);
                }
            }
        }
        Action::Child(report) => {
            assert!(!report.id.is_empty() && report.id.len() <= 128);
            assert!(!report.id.contains('/') && !report.id.chars().any(char::is_control));
            let state = AgentState::try_from(report.state).expect("a known state");
            match ChildWorkChange::try_from(report.change).expect("a known change") {
                ChildWorkChange::End => {
                    assert!(matches!(state, AgentState::Done | AgentState::Error));
                }
                ChildWorkChange::Start | ChildWorkChange::Update => {
                    assert!(!matches!(state, AgentState::Idle | AgentState::Unspecified));
                }
                ChildWorkChange::Unspecified => panic!("a change of a Child work"),
            }
            for text in [&report.message, &report.title, &report.tool] {
                line(text);
            }
        }
    }
}
