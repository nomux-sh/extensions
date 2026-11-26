//! The hook install description and the manifest.
//!
//! nomux applies the plan to the settings file. `apply` here does the same
//! as the rule of `docs/adapters.md` of the `nomux` repository: it makes
//! each object and the array of the JSON pointer, and it adds a value only
//! if the array does not have it. The end-to-end run with the real `nomux
//! hooks install` and `nomux hooks uninstall` is in the pull request.

use std::path::Path;

use nomux_agent_claude::{CLAUDE, ID, install_plan};
use nomux_extension::ApiVersion;
use nomux_extension::manifest::Manifest;
use nomux_proto::wire::v1::{HookInstallPlan, HookPlatform};
use serde_json::{Value, json};

fn apply(plan: &HookInstallPlan, mut settings: Value) -> Value {
    for entry in &plan.entries {
        let value: Value = serde_json::from_str(&entry.value_json).unwrap();
        let mut place = &mut settings;
        for key in entry.pointer.trim_start_matches('/').split('/') {
            place = place
                .as_object_mut()
                .unwrap()
                .entry(key)
                .or_insert_with(|| json!({}));
        }
        if !place.is_array() {
            *place = json!([]);
        }
        let array = place.as_array_mut().unwrap();
        if !array.contains(&value) {
            array.push(value);
        }
    }
    settings
}

#[test]
fn the_plan_names_the_settings_file_of_claude_code_on_linux_and_macos() {
    let plan = install_plan(&CLAUDE);
    let paths: Vec<(i32, &str)> = plan
        .settings_paths
        .iter()
        .map(|path| (path.platform, path.path.as_str()))
        .collect();
    assert_eq!(
        paths,
        [
            (HookPlatform::Linux as i32, "~/.claude/settings.json"),
            (HookPlatform::Macos as i32, "~/.claude/settings.json"),
        ]
    );
}

#[test]
fn each_entry_runs_nomux_hook_without_a_shell_for_one_event() {
    let plan = install_plan(&CLAUDE);
    assert_eq!(plan.entries.len(), CLAUDE.events().len());
    for (entry, event) in plan.entries.iter().zip(CLAUDE.events()) {
        assert_eq!(entry.pointer, format!("/hooks/{event}"));
        let value: Value = serde_json::from_str(&entry.value_json).unwrap();
        assert_eq!(
            value,
            json!({"hooks": [{"type": "command", "command": "nomux", "args": ["hook", "claude", event]}]})
        );
    }
    let text = format!("{:?}", plan.entries);
    for forbidden in ["dangerously", "bypass", "sh -c", "&&", "|", "matcher"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn the_plan_makes_a_valid_settings_file_from_an_empty_file() {
    let plan = install_plan(&CLAUDE);
    let settings = apply(&plan, json!({}));
    let hooks = settings["hooks"].as_object().unwrap();
    assert_eq!(hooks.len(), CLAUDE.events().len());
    for (event, groups) in hooks {
        let [group] = groups.as_array().unwrap().as_slice() else {
            panic!("one matcher group for {event}");
        };
        assert_eq!(group["hooks"][0]["args"][2], json!(event));
    }
    assert_eq!(
        apply(&plan, settings.clone()),
        settings,
        "a second install changes nothing"
    );
}

#[test]
fn the_plan_keeps_the_hooks_and_the_settings_of_the_user() {
    let user_hooks =
        json!([{"matcher": "Bash", "hooks": [{"type": "command", "command": "~/bin/check.sh"}]}]);
    let settings = json!({
        "model": "opus",
        "permissions": {"allow": ["Bash(cargo test)"]},
        "hooks": {"PreToolUse": user_hooks.clone(), "FileChanged": [{"matcher": ".env", "hooks": []}]},
    });
    let installed = apply(&install_plan(&CLAUDE), settings.clone());

    assert_eq!(installed["model"], settings["model"]);
    assert_eq!(installed["permissions"], settings["permissions"]);
    assert_eq!(
        installed["hooks"]["FileChanged"],
        settings["hooks"]["FileChanged"]
    );
    let pre_tool_use = installed["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre_tool_use.len(), 2);
    assert_eq!(
        pre_tool_use[0], user_hooks[0],
        "the hook of the user stays first"
    );
}

#[test]
fn the_manifest_asks_for_the_two_adapter_scopes_only() {
    let manifest: Manifest = include_str!("../../extension.toml").parse().unwrap();
    assert_eq!(manifest.id, ID);
    assert_eq!(manifest.version.to_string(), env!("CARGO_PKG_VERSION"));
    assert!(manifest.api.matches(ApiVersion::CURRENT));
    let daemon = manifest.daemon.as_ref().unwrap();
    assert_eq!(
        Path::new(daemon.command.as_str()).file_name(),
        Path::new(env!("CARGO_BIN_EXE_nomux-agent-claude")).file_name()
    );
    assert!(daemon.lazy);
    assert_eq!(manifest.adapter.unwrap().tools, [CLAUDE.name]);
    let permissions = manifest.permissions;
    assert_eq!(permissions.scopes, ["hooks:read", "agent:report"]);
    assert!(permissions.fs.is_empty());
    assert!(permissions.process.is_empty());
    assert!(!permissions.network);
    assert!(manifest.ui.is_none());
}
