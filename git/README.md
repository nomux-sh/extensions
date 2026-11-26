# git

The `nomux.git` extension: git status, diff, staging, commits, branches, log, blame and worktrees for a nomux client.

This directory has the daemon part of the extension. The nomux daemon starts the program `nomux-git` when a client calls a method, and stops it when it is idle.

## Status

In development. Version 0.1.0 is the first release. It needs nomux 0.4.0 or later. The messages can change in a minor version until version 1.0.0. The [root README](../README.md#install-a-released-extension) tells how to install a released version.

These parts are not available yet:

- The UI part (the panel, the diff tab) and its texts in each language.
- Stash, rebase and merge.
- Remote operations: fetch, pull and push.
- Notes on diff lines.

## Design

- **Reads use `gix`.** Status, diff, branches, log, blame and the list of worktrees read the repository in the process of the extension. They start no `git` process.
- **Writes use the `git` program of the user.** Stage, unstage, commit, switch, branch creation, and the addition and removal of a worktree run `git`. Thus the hooks, the signing and the configuration of the user apply.
- **The status comes from events.** A worktree with a subscriber has a watch. A worktree without a subscriber has no watch, no thread and no cached status.

## Permissions

`extension.toml` asks for these permissions:

| Permission | Why |
|---|---|
| `fs = ["${worktree}"]` | The extension reads and writes the worktree and its git directory. |
| `process = ["git"]` | The methods that write run `git`. |
| `network = false` | The extension opens no network connection. It has no fetch, pull or push. |

The extension applies the approved permissions to each request:

- The `repository` of a request must be the root of a worktree in an approved directory. The extension resolves symbolic links before the check.
- A method that writes fails with `ERROR_CODE_PERMISSION_DENIED` if the user did not approve the program `git`.
- A new worktree and a worktree to remove must be in an approved directory. To use these two methods, approve a directory that contains the worktrees, not only one worktree.

## Methods

The argument and the result of each method are protobuf messages of the package `nomux.git.v1`. The schemas are in [proto/nomux/git/v1](proto/nomux/git/v1), and they are the authority for the fields. The generated Rust types are in the crate `nomux-git`, module `wire::v1`.

Each request has a `repository` field: the absolute path of the root of a worktree. A path of a file is relative to that root, with `/` between its parts.

| Method | Request, response | What it does |
|---|---|---|
| `status` | `StatusRequest`, `StatusResponse` | The branch, the commit of `HEAD`, the upstream branch, the commits ahead and behind, the operation in progress, and each file that is not clean. |
| `diff` | `DiffRequest`, `DiffResponse` | The hunks of one file or of all changed files. |
| `stage` | `StageRequest`, `StageResponse` | Copies files, hunks or lines from the worktree to the index. |
| `unstage` | `UnstageRequest`, `UnstageResponse` | Removes files, hunks or lines from the index. The worktree does not change. |
| `commit` | `CommitRequest`, `CommitResponse` | Commits the index. With `amend`, replaces the last commit. |
| `branches` | `BranchesRequest`, `BranchesResponse` | The local branches and the remote-tracking branches, each with its upstream and its last commit. |
| `switch` | `SwitchRequest`, `SwitchResponse` | Switches the worktree to a branch. |
| `create_branch` | `CreateBranchRequest`, `CreateBranchResponse` | Creates a branch, and switches to it if `switch` is set. |
| `log` | `LogRequest`, `LogResponse` | One page of the commits that a revision can reach, the newest first. |
| `blame` | `BlameRequest`, `BlameResponse` | For one page of the lines of a file, the commit that added each line. |
| `worktrees` | `WorktreesRequest`, `WorktreesResponse` | The main worktree and the linked worktrees. |
| `worktree_add` | `WorktreeAddRequest`, `WorktreeAddResponse` | Adds a linked worktree for a branch, or for a new branch. |
| `worktree_remove` | `WorktreeRemoveRequest`, `WorktreeRemoveResponse` | Removes a linked worktree if the removal loses no work. |
| `watch` | `WatchRequest`, `WatchResponse` | Starts a subscription to the status of a worktree. |
| `unwatch` | `UnwatchRequest`, `UnwatchResponse` | Ends a subscription. |

### Status

- A file has a `staged` change (from `HEAD` to the index), an `unstaged` change (from the index to the worktree), or the two.
- A rename is found between `HEAD` and the index, with the rename settings of the git configuration. A rename in the worktree that is not staged is a deleted file and an untracked file. `git status` gives the same result.
- A directory that has only untracked files is one item whose path ends with `/`, unless `status.showUntrackedFiles` is `all`.
- A file with a conflict has a `conflict` kind and no change kinds.
- `ahead` and `behind` are 0 when the upstream reference does not exist, for example before the first fetch.

### Diff

`kind` selects the two versions:

| Kind | Old version | New version |
|---|---|---|
| `DIFF_KIND_UNSTAGED` | The index | The worktree |
| `DIFF_KIND_STAGED` | `HEAD` | The index |
| `DIFF_KIND_REVISION` | The tree of `revision` | The worktree |

- A hunk has 3 lines of context. Each line has its number in the old version and in the new version.
- The algorithm is the histogram diff of `imara-diff`, with the slider heuristics of git. The diff does not use `diff.algorithm` of the git configuration.
- The content of a worktree file goes through the filters that `git add` applies (line end conversion, `clean` filters). Thus the diff compares what git would store.
- An untracked file is a new file in the unstaged diff and in the revision diff.

### Stage and unstage

- `paths` stages or unstages files and directories completely. A deleted file is staged as a deletion.
- `selections` stages or unstages lines. A `LineSelection` has ranges of line numbers of the old version (`old_lines`: the removed lines to select) and of the new version (`new_lines`: the added lines to select). The line numbers are those of the diff: the unstaged diff for `stage`, the staged diff for `unstage`.
- To stage a hunk, give the ranges of the hunk: `old_start` with `old_lines`, and `new_start` with `new_lines`.
- A selection without a changed line fails with `ERROR_CODE_INVALID_ARGUMENT`, and the index does not change.
- Lines can be selected only in a text file that is not larger than the file limit. Stage a larger file or a binary file completely.

### Commit

- The extension gives the message to `git commit` on the standard input with `--cleanup=verbatim`. The extension adds no trailer and does not change the message.
- The hooks of the user run. A hook can change the message or refuse the commit. The signing configuration of the user applies.
- An empty message is refused. With `amend`, an empty message keeps the message of the commit.

### Worktrees

`worktree_remove` never removes work that is not committed or not pushed ([ADR-0009](https://github.com/nomux-sh/docs/blob/main/adr/0009-resource-budgets-for-disk-and-memory.md)). The method does not fail in that case: the response has `removed = false` and a `refusal` with the reason.

| Reason | Condition |
|---|---|
| `REFUSAL_REASON_UNCOMMITTED_CHANGES` | The worktree has a staged change, an unstaged change, a conflict or an untracked file. |
| `REFUSAL_REASON_UNPUSHED_COMMITS` | The `HEAD` of the worktree has a commit that no remote-tracking branch has. |

- Ignored files are not work: the removal deletes them. Build output is ignored in most repositories.
- The extension does the two checks itself and then runs `git worktree remove` without `--force`. Thus git refuses a worktree that changed between the check and the removal.
- The branch of the worktree stays.
- A repository without a remote has no remote-tracking branch. Each commit is then "not pushed", and the method removes no worktree that has a commit.
- The main worktree cannot be removed.

## Event

| Event | Payload | When |
|---|---|---|
| `status_changed` | `StatusChanged`: the path of the worktree and its new status | The status of a watched worktree changed. |

`watch` starts a subscription and returns it with the current status. The first subscription of a worktree starts its watch. `unwatch` ends a subscription, and the watch stops with the last subscription. The end of the connection to the host stops all the watches, because the process of the extension ends.

A watch observes:

- In the git directory of the worktree: `HEAD`, `index`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `REBASE_HEAD`, `BISECT_LOG`, `rebase-merge` and `rebase-apply`.
- In the git directory that the worktrees share: `refs`, `packed-refs`, `reftable`, `config` and `info/exclude`. Lock files and objects are not observed.
- Each directory of the worktree that the ignore rules do not exclude. An ignored directory such as `target` or `node_modules` is not read and not observed, if the index has no file in it.

After a relevant file event, the watch waits until no relevant event arrived for 300 ms, but not longer than 1500 ms after the first event. Then it computes the status one time. It emits the event only if the status is different from the last one.

- The watch does not poll, and it starts no `git` process.
- While a worktree has a watch, the `status` method returns the cached status of the watch. After a method that writes, and after a relevant file event, the cached status is not used until the watch computed it again.
- A change of a directory or of an ignore file makes the watch discover its directories again.

### Measurements

The program [daemon/examples/watch_burst.rs](daemon/examples/watch_burst.rs) changes each of 10 000 committed files one time and counts the status computations of the watch. These values are from one Linux development computer (8 cores, tmpfs, release build). One status computation with 10 000 changed files took 11 ms to 42 ms.

| Quiet time, longest wait | Changes written in | Status computations | Last change to the final event |
|---|---|---:|---:|
| 200 ms, 1000 ms | 0.13 s | 1 | 234 ms |
| 300 ms, 1500 ms | 0.18 s | 1 | 334 ms |
| 500 ms, 2000 ms | 0.13 s | 1 | 530 ms |
| 200 ms, 1000 ms | 5 s | 5 | 60 ms |
| 300 ms, 1500 ms | 5 s | 4 | 310 ms |
| 500 ms, 2000 ms | 5 s | 3 | 512 ms |
| 300 ms, 1500 ms | 20 s | 14 | 326 ms |

On a disk file system (XFS), the values for 300 ms and 1500 ms were 1 computation and 317 ms.

The quiet time is 300 ms: the delay of one edit is the quiet time, and 500 ms is visible to a user, while 200 ms saved no computation for a fast burst and added one for a slow burst. The longest wait is 1500 ms: a build or a checkout that writes without a pause computes the status one time in each 1.5 s at most, thus the number of computations depends on the duration of the burst and not on the number of files.

## Limits

| Limit | Value | Result when a request is over the limit |
|---|---|---|
| Size of one version of a file for `diff` and for line staging | 1 MiB | `FileDiff.too_large` is true and the file has no hunks. Line staging fails with `ERROR_CODE_INVALID_ARGUMENT`. |
| Sum of the hunks of one `diff` result | 768 KiB | The files that do not fit have `too_large` set and no hunks. Ask for one of these files with `path`. |
| Commits in one `log` page | 500 (default 50) | The page has 500 commits and `has_more` is true. |
| Lines in one `blame` page | 2000 (default 500) | The page has 2000 lines. `total_lines` gives the number of lines of the file. |

Other limits:

- A file with a zero byte in its first 8000 bytes is binary, as in git. A binary file, a submodule and a directory have `FileDiff.binary` set and no hunks.
- A path that is not valid UTF-8 is not supported.
- The extension runs one method at a time. A `git` hook that waits blocks the other methods of the extension.
- `git` runs with `GIT_TERMINAL_PROMPT=0` and without a terminal. A step that needs an answer from the user fails, for example a signing program that asks for a passphrase in the terminal.
- The nomux daemon gives the process only the `PATH` and `HOME` variables. A step of git that needs a different variable fails, for example signing with a key in an SSH agent (`SSH_AUTH_SOCK`).
- `blame` examines the version of a revision. It does not examine the worktree.
- A nested repository in the worktree is not observed by the watch of the outer repository.
- On Linux, a watch uses one inotify watch for each directory that it observes. The limit is `fs.inotify.max_user_watches`.
- The nomux daemon stops the process of an extension that is idle, and no client can subscribe to the events of an extension yet ([extension host](https://github.com/nomux-sh/nomux/blob/main/docs/extensions.md)). Until the daemon gives the events to the clients, a watch lives only while the host keeps the process.

## Security

The requests come from a client and are not trusted. The content of a repository is not trusted.

What the extension does:

- **Paths stay in the worktree.** The extension refuses a path with a `..` part, an absolute path that is not in the worktree, and a path that leaves the worktree through a symbolic link. The last part of a path can be a symbolic link: git records the link and does not read its target.
- **Repositories stay in the approved directories.** "Permissions" has the rule.
- **No shell.** Each argument goes to `git` as one argument. Paths come after `--`. A branch name or a revision that starts with `-` is refused, thus git cannot read it as an option.
- **No unsafe code** in the crate.

What the extension does not do:

- **It gives no protection against a repository that is hostile.** The methods that write run the `git` of the user in the worktree. git obeys the configuration of the repository (`.git/config`) and runs its hooks: for example `pre-commit` and `commit-msg` for a commit, `post-checkout` for a switch and for a new worktree, `core.fsmonitor`, a signing program, and the `clean` filters for `git add`. The result is the same as when the user runs these git commands in that directory.
- **The methods that read can run programs too.** `diff` and line staging apply the `clean` filters and the filter processes that the configuration and the attributes of the repository name, as `git diff` does.
- **Approve only a directory in which you would run `git status` and `git commit`.**
- The nomux daemon does not enforce the `fs`, `process` and `network` permissions yet. Until it does, the checks of this extension are the only checks.

To report a vulnerability, obey the [security policy](https://github.com/nomux-sh/.github/blob/main/SECURITY.md) of the nomux-sh organization.

## Dependencies

| Crate | Use |
|---|---|
| [`nomux-extension`](https://crates.io/crates/nomux-extension) 0.1 | The extension protocol. |
| [`gix`](https://crates.io/crates/gix) 0.91 | The methods that read. Features: `sha1`, `status`, `blame` and `revision`, without the default features. The build has no network client and no pack cache. |
| [`gix-notify`](https://crates.io/crates/gix-notify) 0.1 | File events: inotify on Linux, FSEvents on macOS. |
| [`prost`](https://crates.io/crates/prost) 0.14 | The messages. |

The line diff is `imara-diff`, which `gix` supplies as `gix-imara-diff`.

The build of the program for Linux has 117 crates, and 53 of them are `gix` crates. The release build for `x86_64-unknown-linux-gnu` has 4.6 MB (4 581 272 bytes, with the release profile of the workspace: LTO, one codegen unit, no unwind tables, no symbols). Each license is in the list of `deny.toml`.

To count the crates:

```sh
cargo tree -p nomux-git -e normal --prefix none --target x86_64-unknown-linux-gnu | sed 's/ (\*)//' | sort -u | grep -vc '^nomux-git '
```

## Development

The crate is `git/daemon`. [AGENTS.md](../AGENTS.md) has the layout, the rules and the checks.

```sh
cargo nextest run -p nomux-git
cargo run --release -p nomux-git --example watch_burst -- 10000 300 1500 0
```

The tests use real repositories in temporary directories and the `git` program of the computer. `tests/protocol.rs` starts the program `nomux-git` as a child process and speaks the extension protocol with it.

To use a build from the sources with a nomux daemon that has the `ext` commands:

```sh
cargo build --release -p nomux-git
mkdir -p package/bin
cp git/extension.toml package/
cp target/release/nomux-git package/bin/
nomux ext install package --grant-all --grant fs=<directory with your worktrees>
nomux ext call nomux.git status --payload @request.bin
```

The payload of a call and the payload of its result are encoded protobuf messages.
