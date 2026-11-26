# Claude Code adapter

The extension `nomux.agent-claude` is the adapter of the tool `claude` ([Claude Code](https://code.claude.com)). It receives the hook events that `nomux hook claude <event>` sends, and reports the status of the Agent run of the session and of its Child work to the daemon. `nomux status`, `nomux status --tree` and `nomux agent explain` show the reports with the source `adapter`.

[`docs/adapters.md`](https://github.com/nomux-sh/nomux/blob/main/docs/adapters.md) of the `nomux` repository describes how an adapter works. [`docs/status.md`](https://github.com/nomux-sh/nomux/blob/main/docs/status.md) describes the status model.

## What the adapter shows

- The state of the Agent run: `working`, `needs you` with the kind (`permission`, `question` or `auth`), `done` and `error`.
- For a permission request: the tool and a short detail of the call (the command, the file path or the URL), 160 bytes at most.
- Each subagent and teammate as Child work: its identifier, its type and, from the `background_tasks` list, its description.
- Work in the background: `done` with the message `1 task runs in the background` while a subagent of the finished turn still runs.
- The details of the run: the session identifier of Claude Code, the model, and a resume command.

## Methods

| Method | Argument | Result |
|---|---|---|
| `hook` | `HookEvent` of `nomux-proto` | `HookAnswer`, always empty. The reports go to the host services `agent.report` and `agent.child` |
| `hooks.plan` | empty | `HookInstallPlan`: the settings file of Claude Code and one hook entry for each event of the table |

The extension sends no event of its own and has no schema of its own: the messages are in `nomux-proto`.

## Rules

`daemon/src/rules.rs` has the table. For one event, the first row with the event and a true condition decides. A row reports for the agent that the event comes from: the main agent (the Agent run), or the subagent or teammate of the `agent_id` field (a Child work). An event without a row gives no report. The rows of `SessionStart`, `Stop`, `StopFailure`, `SessionEnd` and `PostModelSwitch` apply only to an event of the main agent, and the rows of `SubagentStart`, `SubagentStop` and `TeammateIdle` only to an event with an `agent_id`.

Before the table decides, the `background_tasks` list of a `Stop` event reconciles the Child work, if the event has the list: a subagent or teammate that the list does not have ends as `done`, and an item of type `subagent` or `teammate` that the adapter did not see starts. A `Stop` event without the list changes no Child work. The list of a `SubagentStop` event is not used: in a real run it still has the subagent that stops.

| Event | Condition | Report | Test |
|---|---|---|---|
| `SessionStart` | `source` is `compact` | working, firm | `a_session_start_after_a_compaction_works` |
| `SessionStart` | | idle, with the details | `a_session_start_is_idle_with_the_details` |
| `UserPromptSubmit` | | working, firm. The prompt is not reported | `a_prompt_submit_works_firm_and_never_reports_the_prompt` |
| `PermissionRequest` | | needs you: permission, the tool and the detail of the call | `a_permission_request_needs_you_with_the_tool_and_the_command`, `a_permission_request_shows_a_short_detail_and_never_the_content` |
| `PostToolUse`, `PostToolUseFailure` | the call is the call of the permission request | working, firm: the wait ends | `the_tool_event_of_the_permission_request_works_firm` |
| `PreToolUse`, `PostToolUse`, `PostToolUseFailure` | the turn of the agent ended | nothing: the event is late | `a_tool_event_after_the_stop_reports_nothing` |
| `PreToolUse`, `PostToolUse`, `PostToolUseFailure` | | working, soft, with the tool | `a_tool_event_works_soft_with_the_tool`, `a_tool_event_of_a_subagent_works_soft_on_its_child_work` |
| `PostToolBatch` | the turn of the agent ended | nothing | `a_tool_batch_after_the_stop_reports_nothing` |
| `PostToolBatch` | | working, firm: each call of the batch has its answer | `a_tool_batch_works_firm` |
| `Notification` | `permission_prompt` | needs you: permission | `a_permission_prompt_notification_needs_you_for_a_permission` |
| `Notification` | `elicitation_dialog` | needs you: question | `an_elicitation_dialog_notification_needs_an_answer` |
| `Notification` | `elicitation_url_dialog` | needs you: auth | `an_elicitation_url_dialog_notification_needs_a_login` |
| `Notification` | `idle_prompt` | needs you: prompt | `an_idle_prompt_notification_waits_at_the_prompt` |
| `Notification` | `agent_needs_input` | needs you: question | `an_agent_needs_input_notification_needs_an_answer` |
| `Elicitation` | `mode` is `url` | needs you: auth, with the MCP server | `a_url_elicitation_needs_a_login` |
| `Elicitation` | | needs you: question, with the MCP server | `a_form_elicitation_needs_an_answer` |
| `ElicitationResult` | | working, firm | `an_elicitation_result_works_firm` |
| `SubagentStart` | | a Child work starts: the identifier and the type | `a_subagent_start_starts_a_child_work_with_its_type` |
| `SubagentStop` | | the Child work ends as `done`. If it was the last work in the background, the Agent run is `done` without the message | `a_subagent_stop_ends_its_child_work`, `a_subagent_that_stops_after_the_parent_stopped_ends_the_work_in_the_background` |
| `TeammateIdle` | | the Child work of the teammate is `done` and stays: a new `SubagentStart` starts it again. Without `agent_id`, nothing | `a_teammate_idle_event_marks_its_child_work_done_and_keeps_it`, `a_teammate_idle_event_without_an_agent_identifier_reports_nothing` |
| `Stop` | a Child work runs | done, with the number of tasks in the background | `a_stop_with_a_running_subagent_is_done_with_work_in_the_background` |
| `Stop` | | done | `a_stop_without_a_running_child_is_done` |
| `StopFailure` | | error, with the error type and the details | `a_stop_failure_is_an_error_with_the_reason` |
| `SessionEnd` | | each Child work ends; the Agent run is `done`, or keeps its error | `a_session_end_ends_each_child_work_and_the_run_is_done`, `a_session_end_after_a_failure_keeps_the_error` |
| `PostModelSwitch` | | nothing; the next report has the new model in its details | `a_model_switch_gives_the_model_to_the_next_report` |

Why these rules:

- **Soft and firm.** The hooks of Claude Code run in parallel, thus a tool event can arrive after the `Stop` of its turn or while a different call of the batch waits for a permission. A tool event gives a soft `working` state: the host does not let it replace `needs you` or work in the background. The submit of a prompt, the end of a tool batch and the answer to a permission request are firm.
- **The end of a wait.** A soft state cannot end a wait for the user. The adapter remembers the tool call of a permission request (a number made from the tool name and its input, not the input), and a `PostToolUse` or `PostToolUseFailure` of the same call is firm. A denied request gives no tool event: the `PostToolBatch` of the batch or the `Stop` of the turn ends the wait.
- **Late events.** After `Stop` or `StopFailure`, a tool event of the main agent has no report until the next `UserPromptSubmit`. After `SubagentStop`, an event of that subagent has no report until a new `SubagentStart`.
- **`idle_prompt`.** Claude Code sends it about 60 seconds after the end of a turn if the user did not type. The `done` result becomes a wait at the input prompt (`prompt`): the user must type the next prompt, and nothing that Claude Code asked for is pending. A dialog or an elicitation is `question`. The host can thus hibernate a session at its prompt and keeps a session that waits for an answer.
- **`SessionEnd`.** The host has no `ended` state. The adapter does what the host does for a session that loses its process: work that ran or waited becomes `done`, and an error stays.
- **Data that is not valid.** The adapter reports nothing for data that is not a JSON object, that has no text `session_id`, or whose `hook_event_name` is not the event of the hook command. A field with a different type than the documentation of Claude Code gives counts as absent. Each text of a report is one line of 512 bytes at most.

The adapter keeps its memory (the turn of each agent, the call of a permission request, the Child work, the details) in memory only, never on disk. The host stops the extension after 60 seconds without an event: the next event starts with an empty memory, and the soft rule of the host applies then.

## Details and the resume command

The hook data has no command line of Claude Code, thus the adapter makes the resume command from the fields that it knows:

- `claude --resume <session>`. Without a valid session identifier (letters, digits, `-` and `_`), the details have no resume command.
- `--model <model>` from `SessionStart` or `PostModelSwitch`, and `--agent <agent>` from the `agent_type` field of the main agent, if the value is a plain name: letters, digits, `.`, `_`, `:` and `-`, 128 bytes at most, and nothing that looks like a secret. A different value is dropped, not quoted.
- `--permission-mode <mode>` only for `plan`, `acceptEdits` and `dontAsk`. A mode that skips or relaxes the permission checks (`bypassPermissions`, `auto`) is dropped.
- No prompt text, no other option and no environment variable.

`tests/resume_command.rs` has the tests. In the real runs of Claude Code 2.1.332 in print mode, `SessionStart` had no `model` field, thus the details had no model.

## Permissions

| Permission | Why |
|---|---|
| `scope=hooks:read` | The daemon gives the hook events of `claude` to the extension |
| `scope=agent:report` | The host services `agent.report` and `agent.child` |

The extension asks for no file, process or network permission. It reads no transcript and no file of Claude Code. `nomux ext install` asks the user to approve the two scopes. The package directory has `extension.toml` and the program `bin/nomux-agent-claude`. The extension needs nomux 0.4.0 or later. The archive of a release has this directory: the [root README](../README.md#install-a-released-extension) tells how to install it.

```console
$ nomux ext install ./nomux-agent-claude-0.1.0-x86_64-unknown-linux-musl --grant-all
extension nomux.agent-claude 0.1.0 installed with the permissions: scope=hooks:read scope=agent:report
```

An adapter with the scope `agent:report` can report for each session of the host: [`docs/adapters.md`](https://github.com/nomux-sh/nomux/blob/main/docs/adapters.md#host-services) says why.

## Install and remove the hooks

`nomux hooks install claude` prints the change of `~/.claude/settings.json` as a unified diff and writes nothing. `--apply` writes the change. `nomux hooks uninstall claude --apply` gives the file its bytes from before the install again, or removes only the entries of nomux if the file changed after the install.

```sh
nomux hooks install claude            # show the diff
nomux hooks install claude --apply    # write the file
nomux hooks status                    # installed, missing or different
nomux hooks uninstall claude --apply  # remove the hooks
```

Each event of the table gets one matcher group without a matcher. The handler runs `nomux` in the exec form of Claude Code, without a shell:

```json
{"hooks": [{"type": "command", "command": "nomux", "args": ["hook", "claude", "PreToolUse"]}]}
```

`nomux` must be on the `PATH` of Claude Code. The entries have no wrapper script, no shell, no timeout and no option that skips the permission checks of Claude Code. The settings path is the same on Linux and on macOS. A Claude Code that uses a different configuration directory (`CLAUDE_CONFIG_DIR`) does not read this file: install the hooks there by hand, or pass the file with `claude --settings`.

## What the adapter never does

- It never answers a permission request, an elicitation or a question on behalf of the user. The answer of each hook is empty.
- It never changes the permission mode of Claude Code, and a resume command never has a mode that skips the checks.
- It never reads, keeps or sends a transcript, a prompt, a file content or the full input of a tool.
- It keeps nothing on disk.

## Tested with

- Claude Code 2.1.332 on Linux x86-64: five short runs in print mode with the cheapest model, in a temporary working directory, with the hooks from `nomux hooks install claude` in a temporary file given with `--settings`. `tests/fixtures/captured/` has the hook data of these runs without local paths, user names and identifiers. `tests/fixtures/documented/` has the data of the events that the runs did not show, written from the [hooks reference](https://code.claude.com/docs/en/hooks).
- nomux `main` (commit `e0534b0`), and the released nomux 0.4.0 with the release archive.

## Limits

- When the command of a session is `claude` and it ends, the session ends, and `nomux status` shows nothing of its last result and its resume command: "An ended session has no Agent run" ([`docs/status.md`](https://github.com/nomux-sh/nomux/blob/main/docs/status.md#sessions-without-a-process)). The status stays only if the session has a process after Claude Code, for example a shell. This needs a change of the host, not of the adapter: an ended session could keep its last Agent run as a dormant session does (`working` and `needs you` become `done`, a result stays) and its details, until the user sees the result or removes the session.
- `SessionStart` of Claude Code does not always have the model. Then the details have no model until a `PostModelSwitch`.
- The detail of a permission request comes from the input of the tool. A word that looks like a key, a token or a password is `[hidden]`, but a secret of a different form can show in the detail of a command.
- The Child work is one level below the Agent run: a subagent of a subagent is a Child work of the Agent run.

## Not available yet

- The current tool and the last line of a subagent from its transcript: the host has no way to ask an adapter for a detail on demand.
- Decision answers for approval cards: the host does not send the answer of a hook to Claude Code in this version.
- Agent teams beyond the `TeammateIdle` event (the extension `agent-teams-claude`).
- Usage and cost.
- The UI part.
