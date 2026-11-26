//! The hook install description of a tool: the result of the method
//! `hooks.plan`.
//!
//! Each event of the rule table gets one matcher group without a matcher,
//! thus the hook runs for each occurrence of the event. Its handler runs
//! `nomux hook <tool> <event>` in the exec form of Claude Code: `command`
//! and `args`, without a shell.

use nomux_proto::wire::v1::{HookInstallPlan, HookPlatform, SettingsEntry, SettingsPath};

use crate::rules::Tool;

/// The install description of the hooks of `tool`.
#[must_use]
pub fn install_plan(tool: &Tool) -> HookInstallPlan {
    let path = |platform: HookPlatform| SettingsPath {
        platform: platform.into(),
        path: tool.settings_path.to_owned(),
    };
    HookInstallPlan {
        settings_paths: vec![path(HookPlatform::Linux), path(HookPlatform::Macos)],
        entries: tool
            .events()
            .into_iter()
            .map(|event| SettingsEntry {
                pointer: format!("/hooks/{event}"),
                value_json: format!(
                    r#"{{"hooks":[{{"type":"command","command":"nomux","args":["hook","{}","{event}"]}}]}}"#,
                    tool.name
                ),
            })
            .collect(),
    }
}
