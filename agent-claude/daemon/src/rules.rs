//! The mapping rules of an agent tool as a table: each row has the hook
//! events, a condition and a report. For one event, the first row with the
//! event and a true condition decides. An event without a row gives no
//! report.
//!
//! A row reports for the agent that the event comes from: the main agent
//! (the Agent run), or the subagent or teammate of the `agent_id` field
//! (a Child work). `README.md` of the extension has the same table.
//!
//! A second tool with other event names is a second table of the same
//! types.

use nomux_proto::wire::v1::NeedsYouKind;

/// The hooks of one agent tool.
#[derive(Debug)]
pub struct Tool {
    /// The tool name of `nomux hook <tool> <event>`.
    pub name: &'static str,
    /// The settings file of the tool below the home directory, on Linux and
    /// on macOS.
    pub settings_path: &'static str,
    /// The event whose `background_tasks` list, when present, reconciles
    /// the Child work before the table decides.
    pub reconciles_on: &'static str,
    /// The rows of the table.
    pub rules: &'static [Rule],
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// The hook events of the row.
    pub events: &'static [&'static str],
    /// When the row applies.
    pub when: When,
    /// What the adapter reports.
    pub then: Then,
}

/// The condition of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Each event.
    Always,
    /// The text field has this value.
    FieldIs(&'static str, &'static str),
    /// The tool call is the call that waits for a permission of the user.
    AnswersPermission,
    /// The turn of the agent ended: the event is late.
    TurnEnded,
    /// A Child work of the main agent runs.
    ChildrenRun,
}

/// The report of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    /// No report.
    Nothing,
    /// The Agent run is idle, with the details.
    Idle,
    /// The agent works. `soft` for a state that only a tool event shows.
    Working {
        /// `true` if the state is soft.
        soft: bool,
    },
    /// The agent waits for the user.
    NeedsYou(NeedsYouKind, Wait),
    /// The turn of the main agent is complete.
    Done,
    /// The turn of the main agent is complete, and its Child work runs.
    DoneInBackground,
    /// The turn of the main agent failed.
    Error,
    /// A subagent or a teammate starts.
    StartChild,
    /// A subagent stops.
    EndChild,
    /// A teammate is idle: its work is complete, and it stays.
    ChildIdle,
    /// The session of the tool ended: each Child work ends, and the Agent
    /// run is `done` unless it failed.
    EndSession,
    /// The model changed: the next report has it in its details.
    RememberModel,
}

/// What the message of a wait says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// The tool and what the call does.
    ToolCall,
    /// The text of the notification.
    Notification,
    /// The MCP server that asks.
    McpServer,
}

/// The tool events of one tool call.
const TOOL_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "PostToolUseFailure"];
/// The tool events after a tool call.
const TOOL_RESULT_EVENTS: &[&str] = &["PostToolUse", "PostToolUseFailure"];

/// Claude Code. The event and field names are from
/// <https://code.claude.com/docs/en/hooks>.
pub const CLAUDE: Tool = Tool {
    name: "claude",
    settings_path: "~/.claude/settings.json",
    reconciles_on: "Stop",
    rules: &[
        Rule {
            events: &["SessionStart"],
            when: When::FieldIs("source", "compact"),
            then: Then::Working { soft: false },
        },
        Rule {
            events: &["SessionStart"],
            when: When::Always,
            then: Then::Idle,
        },
        Rule {
            events: &["UserPromptSubmit"],
            when: When::Always,
            then: Then::Working { soft: false },
        },
        Rule {
            events: &["PermissionRequest"],
            when: When::Always,
            then: Then::NeedsYou(NeedsYouKind::Permission, Wait::ToolCall),
        },
        Rule {
            events: TOOL_RESULT_EVENTS,
            when: When::AnswersPermission,
            then: Then::Working { soft: false },
        },
        Rule {
            events: TOOL_EVENTS,
            when: When::TurnEnded,
            then: Then::Nothing,
        },
        Rule {
            events: TOOL_EVENTS,
            when: When::Always,
            then: Then::Working { soft: true },
        },
        Rule {
            events: &["PostToolBatch"],
            when: When::TurnEnded,
            then: Then::Nothing,
        },
        Rule {
            events: &["PostToolBatch"],
            when: When::Always,
            then: Then::Working { soft: false },
        },
        Rule {
            events: &["Notification"],
            when: When::FieldIs("notification_type", "permission_prompt"),
            then: Then::NeedsYou(NeedsYouKind::Permission, Wait::Notification),
        },
        Rule {
            events: &["Notification"],
            when: When::FieldIs("notification_type", "elicitation_dialog"),
            then: Then::NeedsYou(NeedsYouKind::Question, Wait::Notification),
        },
        Rule {
            events: &["Notification"],
            when: When::FieldIs("notification_type", "elicitation_url_dialog"),
            then: Then::NeedsYou(NeedsYouKind::Auth, Wait::Notification),
        },
        Rule {
            events: &["Notification"],
            when: When::FieldIs("notification_type", "idle_prompt"),
            then: Then::NeedsYou(NeedsYouKind::Prompt, Wait::Notification),
        },
        Rule {
            events: &["Notification"],
            when: When::FieldIs("notification_type", "agent_needs_input"),
            then: Then::NeedsYou(NeedsYouKind::Question, Wait::Notification),
        },
        Rule {
            events: &["Elicitation"],
            when: When::FieldIs("mode", "url"),
            then: Then::NeedsYou(NeedsYouKind::Auth, Wait::McpServer),
        },
        Rule {
            events: &["Elicitation"],
            when: When::Always,
            then: Then::NeedsYou(NeedsYouKind::Question, Wait::McpServer),
        },
        Rule {
            events: &["ElicitationResult"],
            when: When::Always,
            then: Then::Working { soft: false },
        },
        Rule {
            events: &["SubagentStart"],
            when: When::Always,
            then: Then::StartChild,
        },
        Rule {
            events: &["SubagentStop"],
            when: When::Always,
            then: Then::EndChild,
        },
        Rule {
            events: &["TeammateIdle"],
            when: When::Always,
            then: Then::ChildIdle,
        },
        Rule {
            events: &["Stop"],
            when: When::ChildrenRun,
            then: Then::DoneInBackground,
        },
        Rule {
            events: &["Stop"],
            when: When::Always,
            then: Then::Done,
        },
        Rule {
            events: &["StopFailure"],
            when: When::Always,
            then: Then::Error,
        },
        Rule {
            events: &["SessionEnd"],
            when: When::Always,
            then: Then::EndSession,
        },
        Rule {
            events: &["PostModelSwitch"],
            when: When::Always,
            then: Then::RememberModel,
        },
    ],
};

impl Then {
    /// `true` for a report that only an event of the main agent gives.
    #[must_use]
    pub fn is_for_the_run(self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::Done
                | Self::DoneInBackground
                | Self::Error
                | Self::EndSession
                | Self::RememberModel
        )
    }

    /// `true` for a report that only an event of a subagent or a teammate
    /// gives.
    #[must_use]
    pub fn is_for_a_child(self) -> bool {
        matches!(self, Self::StartChild | Self::EndChild | Self::ChildIdle)
    }
}

impl Tool {
    /// The hook events that the table has, each one time, in the sequence
    /// of the table.
    #[must_use]
    pub fn events(&self) -> Vec<&'static str> {
        let mut events: Vec<&'static str> = Vec::new();
        for event in self.rules.iter().flat_map(|rule| rule.events.iter()) {
            if !events.contains(event) {
                events.push(event);
            }
        }
        events
    }
}
