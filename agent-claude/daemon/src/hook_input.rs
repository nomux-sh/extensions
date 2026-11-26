//! The data of one hook event: the JSON object that Claude Code writes to
//! the standard input of the hook.
//!
//! The adapter takes the data only if it is a JSON object, if its
//! `session_id` is a text, and if its `hook_event_name`, when it has one,
//! is the event of the hook command. A field with a different type than
//! the documentation of Claude Code gives counts as absent.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::{Map, Value};

/// The data of one hook event.
#[derive(Debug, Clone, PartialEq)]
pub struct HookInput {
    fields: Map<String, Value>,
}

/// One item of the `background_tasks` list of a `Stop` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTask {
    /// The task identifier.
    pub id: String,
    /// The type label, for example `subagent`.
    pub task_type: String,
    /// The type of the subagent, or the name of a teammate or a workflow.
    pub name: Option<String>,
    /// The free text of the task.
    pub description: Option<String>,
}

impl HookInput {
    /// The data of the event `event`, or `None` if it is not a valid hook
    /// event. A payload that the daemon omitted because it was too large is
    /// an event without fields.
    #[must_use]
    pub fn parse(event: &str, payload: &[u8], omitted: bool) -> Option<Self> {
        if omitted {
            return Some(Self { fields: Map::new() });
        }
        let Ok(Value::Object(fields)) = serde_json::from_slice::<Value>(payload) else {
            return None;
        };
        if !matches!(fields.get("session_id"), Some(Value::String(_))) {
            return None;
        }
        match fields.get("hook_event_name") {
            None => {}
            Some(Value::String(name)) if name == event => {}
            Some(_) => return None,
        }
        Some(Self { fields })
    }

    /// The text of `key`, or `None` if the field is absent, empty or not a
    /// text.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        text_of(&self.fields, key)
    }

    /// The subagent or teammate that the event comes from, or `None` for
    /// the main agent.
    #[must_use]
    pub fn agent_id(&self) -> Option<&str> {
        self.text("agent_id")
    }

    /// A short text that says what a tool call does: the command or the
    /// path of the file, never the content of a file. `None` for a tool
    /// without such a field.
    #[must_use]
    pub fn tool_detail(&self) -> Option<&str> {
        let input = self.fields.get("tool_input")?.as_object()?;
        ["command", "file_path", "notebook_path", "url"]
            .iter()
            .find_map(|key| text_of(input, key))
    }

    /// A number for the tool name and the input of a tool call. A
    /// permission request and the tool events of the same call have the
    /// same number. The adapter keeps the number, not the input.
    #[must_use]
    pub fn tool_call_fingerprint(&self) -> Option<u64> {
        let name = self.text("tool_name")?;
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        self.fields
            .get("tool_input")
            .map(Value::to_string)
            .hash(&mut hasher);
        Some(hasher.finish())
    }

    /// The `background_tasks` list, or `None` if the event does not have
    /// one. An item without a text `id` and `type` is not in the list.
    #[must_use]
    pub fn background_tasks(&self) -> Option<Vec<BackgroundTask>> {
        let tasks = self.fields.get("background_tasks")?.as_array()?;
        Some(
            tasks
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|task| {
                    Some(BackgroundTask {
                        id: text_of(task, "id")?.to_owned(),
                        task_type: text_of(task, "type")?.to_owned(),
                        name: text_of(task, "agent_type")
                            .or_else(|| text_of(task, "name"))
                            .map(str::to_owned),
                        description: text_of(task, "description").map(str::to_owned),
                    })
                })
                .collect(),
        )
    }
}

fn text_of<'a>(fields: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}
