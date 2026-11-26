# extensions

The official nomux extensions. Each extension uses the same public extension API as an extension from a third party.

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. Versions earlier than 1.0.0 are pre-releases: interfaces can change between minor versions.

No extension has a release yet.

## Extensions

| Extension | Identifier | Directory | State |
|---|---|---|---|
| Claude Code adapter | `nomux.agent-claude` | [agent-claude](agent-claude) | The daemon part is available: the status of a Claude Code session and of its subagents from the hooks of Claude Code, and the install description of the hooks. The UI part is not available yet. |
| Git | `nomux.git` | [git](git) | The daemon part is available: status, diff, staging, commit, branches, log, blame, worktrees and a status that comes from file events. The UI part is not available yet. |

No other extension exists yet. The [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md) gives the plan.

An extension uses only the public extension API: the crate [`nomux-extension`](https://crates.io/crates/nomux-extension) of the [`extension-api`](https://github.com/nomux-sh/extension-api) repository. [ADR-0005](https://github.com/nomux-sh/docs/blob/main/adr/0005-extension-model.md) records the extension model.

## Layout

The repository is one Cargo workspace. Each extension has one directory at the repository root.

| Path | Content |
|---|---|
| `<name>/extension.toml` | The manifest of the extension. |
| `<name>/README.md` | Methods, events, permissions and limits of the extension. |
| `<name>/CHANGELOG.md` | The changelog of the extension. |
| `<name>/proto/` | Protobuf schemas of the messages of the daemon part. |
| `<name>/daemon/` | The crate of the daemon part, with the generated code (committed) if the extension has schemas, and the tests. |
| `<name>/ui/`, `<name>/locales/` | The UI part and its texts. No extension has them yet. |
| `tools/protoc-gen-nomux-prost/` | Local `buf` plugin that runs `prost-build`. Not published. |
| `scripts/check-generated.sh` | Fails if the generated code is stale. |
| `buf.yaml`, `buf.gen.yaml` | `buf` configuration. |
| `deny.toml` | `cargo-deny` configuration. |
| `cliff.toml` | `git-cliff` configuration for all extensions. |
| `AGENTS.md` | Rules for contributors and coding agents, and the layout rule of an extension. |

## Tools

| Tool | Version | Used for |
|------|---------|----------|
| Rust | 1.100.0 (pinned in `rust-toolchain.toml`) | build, test |
| git | 2.55 (the version of the development computer; CI uses the git of the runner) | the tests, and the `git` extension at run time |
| [buf](https://github.com/bufbuild/buf/releases) | 1.75.0 | lint, format, code generation |
| [cargo-nextest](https://nexte.st) | 0.9.152 | tests |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | 0.22.1 | license and advisory check |
| [cargo-machete](https://github.com/bnjbvr/cargo-machete) | 0.10.1 | unused dependency check |
| [git-cliff](https://git-cliff.org) | 2.14.2 | changelogs |
| [actionlint](https://github.com/rhysd/actionlint) | 1.7.12 | workflow lint |

To build an extension, only Rust is necessary. `buf` is necessary only to change the schemas. `protoc` is not necessary.

## Code generation

The generated Rust code of each daemon part that has schemas is committed in `<name>/daemon/src/generated/`. No crate has a `build.rs`.

`buf generate` starts a plugin that is a binary in this workspace (`tools/protoc-gen-nomux-prost`), and the plugin calls `prost-build` with the descriptors from `buf`. `Cargo.lock` pins `prost-build` and the `prost` runtime to the same version, and no remote plugin or schema registry is used.

After you change a `.proto` file, do these steps:

1. Format the schemas:

   ```sh
   buf format -w
   ```

2. Generate the code:

   ```sh
   buf generate
   ```

3. Commit the schemas and the generated code in the same commit.

## Checks

Run these commands from the repository root before you open a pull request:

```sh
buf lint
buf format --diff --exit-code
scripts/check-generated.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo deny check
cargo machete
actionlint
```

CI runs the same checks with the reusable workflows of the [`nomux-sh/.github`](https://github.com/nomux-sh/.github) repository. The tests run on Linux and on macOS, because the file watcher of the `git` extension is different on each host.

## Releases

The [release guide](https://github.com/nomux-sh/.github/blob/main/RELEASING.md) of the organization gives the rules for versions and changelogs. This repository has more than one product, thus these rules are different here:

- **Each extension has its own version.** The version is in `<name>/extension.toml` and in the manifest of the crate of the daemon part. The two values are equal.
- **The tag of a release is the directory of the extension, `@` and the version:** `git@1.4.0`. The repository has no tag of the form `vX.Y.Z`.
- **Each extension has its own changelog:** `<name>/CHANGELOG.md`. The repository root has no `CHANGELOG.md`. `git-cliff` writes a changelog from the commits that change the directory of the extension. The commands are at the top of `cliff.toml`.
- **A release pull request has the title `chore: release <name> X.Y.Z`.** It changes the two versions and the changelog of one extension.
- The extension API version that an extension needs is `api` in its `extension.toml`. It is independent of the version of the extension.

The release workflow does not exist yet: nothing builds a package, signs it or publishes it to the registry. Do not push a release tag until that workflow exists.

## Security

To report a vulnerability, obey the [security policy](https://github.com/nomux-sh/.github/blob/main/SECURITY.md) of the nomux-sh organization.

The README of each extension says which permissions the extension asks for and what its limits are.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Copyright The nomux Authors.
