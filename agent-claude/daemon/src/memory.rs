//! What the adapter remembers between two hook events of a session, in
//! memory only. The host stops the extension after 60 seconds without an
//! event, and the memory goes with it: the next event starts with an empty
//! memory, and the soft rule of the host covers the order of the events
//! then.

use nomux_proto::wire::v1::{AgentDetails, AgentState};

use crate::resume_command::SessionFacts;

/// The largest number of sessions that the adapter remembers. A new session
/// takes the place of the session that had no event for the longest time.
const MAX_SESSIONS: usize = 256;

/// The largest number of Child work items of a session, as the host has.
const MAX_CHILDREN: usize = 64;

/// The memory of each session of the host.
#[derive(Debug, Default)]
pub struct Memory {
    /// The sessions, the most recent event last.
    sessions: Vec<(String, SessionMemory)>,
}

/// Where an agent is in its turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Turn {
    /// The agent works, or the adapter does not know.
    #[default]
    Running,
    /// The turn of the agent ended (`Stop`, `StopFailure`, `TeammateIdle`).
    /// A tool event of the same turn that arrives now is late.
    Ended,
    /// The subagent stopped (`SubagentStop`). Only a new start counts.
    Closed,
}

/// The memory of the main agent or of one subagent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentMemory {
    /// Where the agent is in its turn.
    pub turn: Turn,
    /// The tool call that waits for a permission of the user.
    pub pending_permission: Option<u64>,
    /// The label of a Child work: the type of the subagent.
    pub title: String,
    /// What a Child work does, from the `background_tasks` list.
    pub description: String,
}

/// The memory of one session.
#[derive(Debug, Default)]
pub struct SessionMemory {
    /// The main agent.
    pub run: AgentMemory,
    /// The subagents and teammates, the oldest first.
    pub children: Vec<(String, AgentMemory)>,
    /// What the adapter knows for the details.
    pub facts: SessionFacts,
    /// The details of the last report that had details.
    pub sent_details: Option<AgentDetails>,
    /// `true` after a `done` report with Child work that runs in the
    /// background.
    pub in_background: bool,
    /// The state of the last report of the Agent run.
    pub run_state: Option<AgentState>,
}

impl Memory {
    /// The memory of the session `id`. A session that the adapter does not
    /// know gets an empty memory.
    pub fn session(&mut self, id: &str) -> &mut SessionMemory {
        let index = match self.sessions.iter().position(|(known, _)| known == id) {
            Some(index) => index,
            None => {
                if self.sessions.len() >= MAX_SESSIONS {
                    self.sessions.remove(0);
                }
                self.sessions
                    .push((id.to_owned(), SessionMemory::default()));
                self.sessions.len() - 1
            }
        };
        let last = self.sessions.len() - 1;
        self.sessions[index..].rotate_left(1);
        &mut self.sessions[last].1
    }

    /// Forgets the session `id`.
    pub fn forget(&mut self, id: &str) {
        self.sessions.retain(|(known, _)| known != id);
    }
}

impl SessionMemory {
    /// The memory of the child `id`, or `None` if the adapter does not know
    /// it.
    pub fn child(&mut self, id: &str) -> Option<&mut AgentMemory> {
        self.children
            .iter_mut()
            .find(|(known, _)| known == id)
            .map(|(_, child)| child)
    }

    /// The memory of the child `id`. A child that the adapter does not know
    /// gets an empty memory; the oldest closed child makes room for it.
    pub fn child_or_new(&mut self, id: &str) -> &mut AgentMemory {
        if let Some(index) = self.children.iter().position(|(known, _)| known == id) {
            return &mut self.children[index].1;
        }
        if self.children.len() >= MAX_CHILDREN {
            let oldest = self
                .children
                .iter()
                .position(|(_, child)| child.turn == Turn::Closed)
                .unwrap_or(0);
            self.children.remove(oldest);
        }
        self.children.push((id.to_owned(), AgentMemory::default()));
        let last = self.children.len() - 1;
        &mut self.children[last].1
    }

    /// The identifiers of the children that run.
    pub fn running_children(&self) -> impl Iterator<Item = &str> {
        self.children
            .iter()
            .filter(|(_, child)| child.turn == Turn::Running)
            .map(|(id, _)| id.as_str())
    }
}
