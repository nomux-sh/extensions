//! The daemon part of the nomux adapter extension for Claude Code.
//!
//! The extension has two methods: `hook` maps one hook event of Claude Code
//! to reports for the host services `agent.report` and `agent.child`, and
//! `hooks.plan` describes the hook entries of the settings file of Claude
//! Code. `README.md` of the extension has the rules.

mod hook_input;
mod install_plan;
mod memory;
mod reports;
mod resume_command;
mod rules;
mod safe_text;

use nomux_extension::{Call, CallError, Extension};
use nomux_proto::wire::v1::{AgentReportResult, HookAnswer, HookEvent};

pub use hook_input::{BackgroundTask, HookInput};
pub use install_plan::install_plan;
pub use memory::Memory;
pub use reports::{Action, map};
pub use resume_command::{SessionFacts, resume_command};
pub use rules::{CLAUDE, Rule, Then, Tool, Wait, When};
pub use safe_text::{looks_like_secret, one_line, one_line_without_secrets};

/// The identifier of the extension, as in `extension.toml`.
pub const ID: &str = "nomux.agent-claude";

/// The extension with its methods.
#[must_use]
pub fn extension() -> Extension {
    let mut memory = Memory::default();
    Extension::new(ID, env!("CARGO_PKG_VERSION"))
        .method("hook", move |call: &Call, event: HookEvent| {
            hook(call, &mut memory, &event)
        })
        .method("hooks.plan", |_: &Call, (): ()| Ok(install_plan(&CLAUDE)))
}

/// The reports for one hook event, or none for an event of a different
/// tool or with data that is not valid.
#[must_use]
pub fn reports(memory: &mut Memory, event: &HookEvent) -> Vec<Action> {
    if event.tool != CLAUDE.name {
        return Vec::new();
    }
    let Some(input) = HookInput::parse(&event.event, &event.payload, event.payload_omitted) else {
        return Vec::new();
    };
    map(&CLAUDE, memory, &event.session_id, &event.event, &input)
}

fn hook(call: &Call, memory: &mut Memory, event: &HookEvent) -> Result<HookAnswer, CallError> {
    let mut first_error = None;
    for action in reports(memory, event) {
        let sent = match action {
            Action::Run(mut report) => {
                report.session_id.clone_from(&event.session_id);
                call.host()
                    .call::<_, AgentReportResult>("agent.report", &report)
            }
            Action::Child(mut report) => {
                report.session_id.clone_from(&event.session_id);
                call.host()
                    .call::<_, AgentReportResult>("agent.child", &report)
            }
        };
        if let Err(error) = sent {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(HookAnswer::default()), Err)
}
