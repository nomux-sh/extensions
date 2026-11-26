//! Applies the table of [`crate::rules`] to one hook event: the reports for
//! the host services `agent.report` and `agent.child`.

use nomux_proto::wire::v1::{
    AgentDetails, AgentReport, AgentState, ChildWorkChange, ChildWorkReport, NeedsYouKind,
};

use crate::hook_input::{BackgroundTask, HookInput};
use crate::memory::{AgentMemory, Memory, SessionMemory, Turn};
use crate::resume_command::{is_session_id, resume_command};
use crate::rules::{Rule, Then, Tool, Wait, When};
use crate::safe_text::{MAX_LINE, one_line, one_line_without_secrets};

/// The largest number of bytes of the detail of a tool call: the command or
/// the path.
const MAX_DETAIL: usize = 160;

/// The largest number of bytes of the identifier of a Child work, as the
/// host has.
const MAX_CHILD_ID: usize = 128;

/// The task types of the `background_tasks` list that are Child work.
const CHILD_TASK_TYPES: [&str; 2] = ["subagent", "teammate"];

/// One report for the host. The session identifier is empty: the caller
/// sets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A report of the Agent run for `agent.report`.
    Run(AgentReport),
    /// A report of a Child work for `agent.child`.
    Child(ChildWorkReport),
}

/// Who an event reports for.
#[derive(Debug, Clone, Copy)]
enum Target<'a> {
    Run,
    Child(&'a str),
}

/// The reports for the event `event` of the nomux session `session_id`,
/// after the rules of `tool`. Updates the memory of the session.
pub fn map(
    tool: &Tool,
    memory: &mut Memory,
    session_id: &str,
    event: &str,
    input: &HookInput,
) -> Vec<Action> {
    let target = match input.agent_id() {
        None => Target::Run,
        Some(id) if is_child_id(id) => Target::Child(id),
        Some(_) => return Vec::new(),
    };
    let session = memory.session(session_id);
    remember_facts(session, input, target);
    let mut actions = Vec::new();
    if event == tool.reconciles_on
        && matches!(target, Target::Run)
        && let Some(tasks) = input.background_tasks()
    {
        actions = reconcile(session, &tasks);
    }
    let Some(rule) = tool
        .rules
        .iter()
        .find(|rule| applies(rule, event, input, session, target))
    else {
        return actions;
    };
    let fits = match target {
        Target::Run => !rule.then.is_for_a_child(),
        Target::Child(id) => {
            !rule.then.is_for_the_run()
                && (rule.then == Then::StartChild
                    || session
                        .child(id)
                        .is_none_or(|child| child.turn != Turn::Closed))
        }
    };
    if !fits {
        return actions;
    }
    actions.extend(apply(rule.then, input, session, target));
    if rule.then == Then::EndSession {
        memory.forget(session_id);
    }
    actions
}

fn applies(
    rule: &Rule,
    event: &str,
    input: &HookInput,
    session: &mut SessionMemory,
    target: Target<'_>,
) -> bool {
    if !rule.events.contains(&event) {
        return false;
    }
    match rule.when {
        When::Always => true,
        When::FieldIs(key, value) => input.text(key) == Some(value),
        When::AnswersPermission => {
            let pending = known_agent(session, target).and_then(|agent| agent.pending_permission);
            pending.is_some() && pending == input.tool_call_fingerprint()
        }
        When::TurnEnded => {
            known_agent(session, target).is_some_and(|agent| agent.turn != Turn::Running)
        }
        When::ChildrenRun => session.running_children().next().is_some(),
    }
}

fn apply(
    then: Then,
    input: &HookInput,
    session: &mut SessionMemory,
    target: Target<'_>,
) -> Vec<Action> {
    match then {
        Then::Nothing => Vec::new(),
        Then::Idle => {
            session.run = AgentMemory::default();
            session.in_background = false;
            vec![run_report(session, AgentState::Idle, Report::default())]
        }
        Then::Working { soft } => working(input, session, target, soft),
        Then::NeedsYou(kind, wait) => needs_you(input, session, target, kind, wait),
        Then::Done => turn_end(session, AgentState::Done, String::new()),
        Then::DoneInBackground => {
            let count = session.running_children().count();
            let tasks = if count == 1 { "task runs" } else { "tasks run" };
            let message = format!("{count} {tasks} in the background");
            let actions = turn_end(session, AgentState::Done, message);
            session.in_background = true;
            actions
        }
        Then::Error => {
            let message = match (input.text("error"), input.text("error_details")) {
                (Some(error), Some(details)) => format!("{error}: {details}"),
                (error, details) => error.or(details).unwrap_or_default().to_owned(),
            };
            turn_end(
                session,
                AgentState::Error,
                one_line_without_secrets(&message, MAX_LINE),
            )
        }
        Then::StartChild => match target {
            Target::Child(id) => start_child(session, id, input.text("agent_type"), None),
            Target::Run => Vec::new(),
        },
        Then::EndChild => match target {
            Target::Child(id) => end_child(session, id),
            Target::Run => Vec::new(),
        },
        Then::ChildIdle => match target {
            Target::Child(id) => child_idle(session, id, input.text("teammate_name")),
            Target::Run => Vec::new(),
        },
        Then::EndSession => end_session(session),
        Then::RememberModel => {
            if let Some(model) = input.text("to_model") {
                session.facts.model = Some(model.to_owned());
            }
            Vec::new()
        }
    }
}

/// The parts of a report that are not the state.
#[derive(Debug, Default)]
struct Report {
    kind: Option<NeedsYouKind>,
    soft: bool,
    message: String,
    tool: String,
}

fn working(
    input: &HookInput,
    session: &mut SessionMemory,
    target: Target<'_>,
    soft: bool,
) -> Vec<Action> {
    let report = Report {
        soft,
        tool: tool_name(input),
        ..Report::default()
    };
    let agent = agent_or_new(session, target);
    if !soft {
        agent.turn = Turn::Running;
        agent.pending_permission = None;
    }
    match target {
        Target::Run => {
            session.in_background = false;
            vec![run_report(session, AgentState::Working, report)]
        }
        Target::Child(id) => vec![child_report(session, id, AgentState::Working, report)],
    }
}

fn needs_you(
    input: &HookInput,
    session: &mut SessionMemory,
    target: Target<'_>,
    kind: NeedsYouKind,
    wait: Wait,
) -> Vec<Action> {
    let message = match wait {
        Wait::ToolCall => input.tool_detail().map_or_else(String::new, |detail| {
            one_line_without_secrets(detail, MAX_DETAIL)
        }),
        Wait::Notification => input.text("message").map_or_else(String::new, |message| {
            one_line_without_secrets(message, MAX_LINE)
        }),
        Wait::McpServer => input
            .text("mcp_server_name")
            .map_or_else(String::new, |server| one_line(server, MAX_LINE)),
    };
    let report = Report {
        kind: Some(kind),
        message,
        tool: if wait == Wait::ToolCall {
            tool_name(input)
        } else {
            String::new()
        },
        ..Report::default()
    };
    let agent = agent_or_new(session, target);
    if wait == Wait::ToolCall {
        agent.pending_permission = input.tool_call_fingerprint();
    }
    match target {
        Target::Run => {
            session.in_background = false;
            vec![run_report(session, AgentState::NeedsYou, report)]
        }
        Target::Child(id) => vec![child_report(session, id, AgentState::NeedsYou, report)],
    }
}

/// The end of a turn of the main agent.
fn turn_end(session: &mut SessionMemory, state: AgentState, message: String) -> Vec<Action> {
    session.run.turn = Turn::Ended;
    session.run.pending_permission = None;
    session.in_background = false;
    vec![run_report(
        session,
        state,
        Report {
            message,
            ..Report::default()
        },
    )]
}

fn start_child(
    session: &mut SessionMemory,
    id: &str,
    title: Option<&str>,
    description: Option<&str>,
) -> Vec<Action> {
    let child = session.child_or_new(id);
    *child = AgentMemory {
        title: title.map_or_else(String::new, |title| one_line(title, MAX_LINE)),
        description: description
            .map_or_else(String::new, |text| one_line_without_secrets(text, MAX_LINE)),
        ..AgentMemory::default()
    };
    vec![child_report(
        session,
        id,
        AgentState::Working,
        Report::default(),
    )]
}

fn end_child(session: &mut SessionMemory, id: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    match session.child(id) {
        Some(child) if child.turn == Turn::Closed => return actions,
        Some(_) => {}
        None => actions.extend(start_child(session, id, None, None)),
    }
    let child = session.child_or_new(id);
    child.turn = Turn::Closed;
    child.pending_permission = None;
    let child = child.clone();
    actions.push(Action::Child(ChildWorkReport {
        change: ChildWorkChange::End.into(),
        id: id.to_owned(),
        state: AgentState::Done.into(),
        title: child.title,
        message: child.description,
        ..ChildWorkReport::default()
    }));
    if session.in_background && session.running_children().next().is_none() {
        actions.push(run_report(session, AgentState::Done, Report::default()));
        session.in_background = false;
    }
    actions
}

fn child_idle(session: &mut SessionMemory, id: &str, name: Option<&str>) -> Vec<Action> {
    let child = session.child_or_new(id);
    child.turn = Turn::Ended;
    child.pending_permission = None;
    if child.title.is_empty()
        && let Some(name) = name
    {
        child.title = one_line(name, MAX_LINE);
    }
    vec![child_report(
        session,
        id,
        AgentState::Done,
        Report::default(),
    )]
}

/// Each Child work ends, and the Agent run keeps its result, as the host
/// does for a session that loses its process: work that ran or waited is
/// `done`, and an error stays.
fn end_session(session: &mut SessionMemory) -> Vec<Action> {
    session.in_background = false;
    let open: Vec<String> = session
        .children
        .iter()
        .filter(|(_, child)| child.turn != Turn::Closed)
        .map(|(id, _)| id.clone())
        .collect();
    let mut actions: Vec<Action> = open.iter().flat_map(|id| end_child(session, id)).collect();
    if session.run_state != Some(AgentState::Error) {
        actions.push(run_report(session, AgentState::Done, Report::default()));
    }
    actions
}

/// Closes each Child work that the list does not have, and starts each
/// one that the adapter did not see.
fn reconcile(session: &mut SessionMemory, tasks: &[BackgroundTask]) -> Vec<Action> {
    let listed: Vec<&BackgroundTask> = tasks
        .iter()
        .filter(|task| CHILD_TASK_TYPES.contains(&task.task_type.as_str()) && is_child_id(&task.id))
        .collect();
    let missing: Vec<String> = session
        .children
        .iter()
        .filter(|(id, child)| {
            child.turn != Turn::Closed && !listed.iter().any(|task| &task.id == id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    let mut actions: Vec<Action> = missing
        .iter()
        .flat_map(|id| end_child(session, id))
        .collect();
    for task in listed {
        let Some(child) = session.child(&task.id) else {
            actions.extend(start_child(
                session,
                &task.id,
                task.name.as_deref(),
                task.description.as_deref(),
            ));
            continue;
        };
        if child.turn == Turn::Running
            && child.description.is_empty()
            && let Some(description) = task.description.as_deref()
        {
            child.description = one_line_without_secrets(description, MAX_LINE);
            let soft = Report {
                soft: true,
                ..Report::default()
            };
            actions.push(child_report(session, &task.id, AgentState::Working, soft));
        }
    }
    actions
}

fn run_report(session: &mut SessionMemory, state: AgentState, report: Report) -> Action {
    session.run_state = Some(state);
    let details = new_details(session);
    Action::Run(AgentReport {
        state: state.into(),
        kind: report.kind.map_or(0, Into::into),
        soft: report.soft,
        message: report.message,
        tool: report.tool,
        details,
        ..AgentReport::default()
    })
}

fn child_report(
    session: &mut SessionMemory,
    id: &str,
    state: AgentState,
    report: Report,
) -> Action {
    let child = session.child_or_new(id).clone();
    let message = if report.message.is_empty() {
        child.description
    } else {
        report.message
    };
    Action::Child(ChildWorkReport {
        change: ChildWorkChange::Start.into(),
        id: id.to_owned(),
        state: state.into(),
        kind: report.kind.map_or(0, Into::into),
        soft: report.soft,
        message,
        title: child.title,
        tool: report.tool,
        ..ChildWorkReport::default()
    })
}

/// The details of the session if they changed since the last report.
fn new_details(session: &mut SessionMemory) -> Option<AgentDetails> {
    let facts = &session.facts;
    let details = AgentDetails {
        model: facts
            .model
            .as_deref()
            .map_or_else(String::new, |model| one_line(model, MAX_LINE)),
        provider_session_id: facts
            .session_id
            .clone()
            .filter(|id| is_session_id(id))
            .unwrap_or_default(),
        resume_command: resume_command(facts).unwrap_or_default(),
    };
    if details == AgentDetails::default() || session.sent_details.as_ref() == Some(&details) {
        return None;
    }
    session.sent_details = Some(details.clone());
    Some(details)
}

fn remember_facts(session: &mut SessionMemory, input: &HookInput, target: Target<'_>) {
    let facts = &mut session.facts;
    if let Some(id) = input.text("session_id") {
        facts.session_id = Some(id.to_owned());
    }
    if let Some(model) = input.text("model") {
        facts.model = Some(model.to_owned());
    }
    if let Some(mode) = input.text("permission_mode") {
        facts.permission_mode = Some(mode.to_owned());
    }
    if let (Target::Run, Some(agent)) = (target, input.text("agent_type")) {
        facts.agent = Some(agent.to_owned());
    }
}

/// The memory of the agent of `target`, or `None` for a Child work that the
/// adapter does not know.
fn known_agent<'a>(
    session: &'a mut SessionMemory,
    target: Target<'_>,
) -> Option<&'a mut AgentMemory> {
    match target {
        Target::Run => Some(&mut session.run),
        Target::Child(id) => session.child(id),
    }
}

fn agent_or_new<'a>(session: &'a mut SessionMemory, target: Target<'_>) -> &'a mut AgentMemory {
    match target {
        Target::Run => &mut session.run,
        Target::Child(id) => session.child_or_new(id),
    }
}

fn tool_name(input: &HookInput) -> String {
    input
        .text("tool_name")
        .map_or_else(String::new, |name| one_line(name, MAX_LINE))
}

/// `true` for an identifier that the host takes for a Child work: 1 to 128
/// bytes without `/` and without a control character.
fn is_child_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_CHILD_ID
        && !id.contains('/')
        && !id.chars().any(char::is_control)
}
