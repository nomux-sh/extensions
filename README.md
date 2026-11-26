# extensions

The official nomux extensions. Each extension uses the same public extension API as an extension from a third party.

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. Versions earlier than 1.0.0 are pre-releases: interfaces can change between minor versions.

## Extensions

| Extension | Identifier | Directory | Version | Needs nomux | State |
|---|---|---|---|---|---|
| Claude Code adapter | `nomux.agent-claude` | [agent-claude](agent-claude) | [0.1.1](https://github.com/nomux-sh/extensions/releases/tag/agent-claude@0.1.1) | 0.4.0 or later. The kind `prompt` needs 0.5.0 or later | The daemon part is available: the status of a Claude Code session and of its subagents from the hooks of Claude Code, and the install description of the hooks. The UI part is not available yet. |
| Git | `nomux.git` | [git](git) | [0.1.0](https://github.com/nomux-sh/extensions/releases/tag/git@0.1.0) | 0.4.0 or later | The daemon part is available: status, diff, staging, commit, branches, log, blame, worktrees and a status that comes from file events. The UI part is not available yet. |

The manifest of an extension cannot state the nomux version that it needs: the README of the extension states it. No other extension exists yet. The [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md) gives the plan.

An extension uses only the public extension API: the crate [`nomux-extension`](https://crates.io/crates/nomux-extension) of the [`extension-api`](https://github.com/nomux-sh/extension-api) repository. [ADR-0005](https://github.com/nomux-sh/docs/blob/main/adr/0005-extension-model.md) records the extension model.

## Install a released extension

Each release of an extension has one archive for each target and the file `SHA256SUMS`:

| Target | Computer |
|---|---|
| `x86_64-unknown-linux-musl` | Linux on x86-64. The program is static. |
| `aarch64-unknown-linux-musl` | Linux on ARM64. The program is static. |
| `x86_64-apple-darwin` | macOS on Intel. |
| `aarch64-apple-darwin` | macOS on Apple silicon. |

The archive `nomux-<name>-<version>-<target>.tar.gz` has one directory. This directory is the package of the extension:

```
nomux-<name>-<version>-<target>/
  extension.toml
  bin/nomux-<name>
  LICENSE-APACHE
  LICENSE-MIT
```

The install steps are different for each nomux version:

| nomux | `nomux ext install` reads | Steps |
|---|---|---|
| 0.4.0 and 0.5.0 | A package directory only. It does not read an archive, a URL or a name | [With a released nomux](#with-a-released-nomux) |
| The release after 0.5.0 (not released yet) | A package directory, a release archive, an `https` address of an archive, or a name | [With the next nomux release](#with-the-next-nomux-release) |

`nomux --version` shows your version.

### With a released nomux

Download the archive of your computer, examine its checksum, extract it, and give the directory to `nomux ext install`. For the `git` extension 0.1.0 on Linux x86-64:

```sh
tag=git@0.1.0
archive=nomux-git-0.1.0-x86_64-unknown-linux-musl
gh release download "$tag" --repo nomux-sh/extensions --pattern "$archive.tar.gz" --pattern SHA256SUMS
grep " $archive.tar.gz$" SHA256SUMS | sha256sum --check   # on macOS: shasum -a 256 --check
tar -xzf "$archive.tar.gz"
nomux ext install "$archive" --grant-all --grant fs=<directory with your worktrees>
nomux ext ls
```

Without `gh`, download the two files from `https://github.com/nomux-sh/extensions/releases/download/<tag>/<file>`.

- `ext install` without an approval prints the permissions that the extension asks for and installs nothing. The README of each extension says why it asks for each permission.
- `--grant-all` does not approve a directory that starts with `${worktree}`. Give each directory with `--grant fs=<directory>`.
- To install a new version, remove the old version first: `nomux ext rm <id>`.
- The package must stay a directory of regular files. The host refuses a symbolic link.

### With the next nomux release

The `main` branch of nomux ([nomux-sh/nomux#120](https://github.com/nomux-sh/nomux/pull/120)) downloads the archive of the host, examines the checksum and extracts the package itself. The first nomux release after 0.5.0 will have this change. nomux 0.5.0 does not have it.

```sh
nomux ext install git --grant-all --grant fs=<directory with your worktrees>   # the newest release
nomux ext install git@0.1.0 --grant-all --grant fs=<directory with your worktrees>
```

- The name is the directory of the extension in this repository: `git` or `agent-claude`.
- The command shows the extension, its version, its target, the result of the checksum and its permissions, and then asks you to accept. `--yes` accepts without the question.
- The rules for the permissions and for a new version are the same as above.

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
| `scripts/build-extension.sh`, `scripts/check-extension-archive.sh` | Build, pack and examine the release archive of one extension for one target. |
| `scripts/extension-version.sh`, `scripts/release-notes.sh` | Print the version of an extension and the changelog section of a version. The release workflow uses them. |
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

The workflow [`release.yml`](.github/workflows/release.yml) makes the release. A tag `<name>@<version>` starts it. It builds only the extension of the tag:

1. The `check` job stops the release if the tag is not `<name>@<version>` of `<name>/extension.toml`, if `<name>/daemon/Cargo.toml` has a different version, if the commit is not on `main`, if `<name>/CHANGELOG.md` has no section for the version, or if `nomux ext validate` of a released nomux refuses the manifest.
2. After the checks, the `build` jobs build the daemon part for the four targets. `scripts/build-extension.sh` packs and examines each archive.
3. The `checksums` job writes `SHA256SUMS`.
4. The `github-release` job makes the GitHub release with the name of the tag, the archives, `SHA256SUMS` and notes: the changelog section of the version and the oldest nomux version that the extension supports (`NOMUX_VERSION` in the workflow). A version earlier than 1.0.0 is a pre-release.

A manual run with the inputs `extension` and `dry-run` (the default) does the checks, except the tag and the `main` branch, and builds the archives as workflow artifacts. It makes no release:

```sh
gh workflow run release.yml --ref <branch> -f extension=git -f dry-run=true
```

The steps of a release:

1. Open the release pull request `chore: release <name> X.Y.Z`. It changes the two versions and writes the changelog: `git cliff --include-path '<name>/**' --tag-pattern '^<name>@' --tag <name>@X.Y.Z --output <name>/CHANGELOG.md`.
2. Merge the pull request after all checks pass.
3. Put the tag on the merge result and push it: `git tag -a <name>@X.Y.Z -m <name>@X.Y.Z`, then `git push origin <name>@X.Y.Z`.
4. Examine the release: download one archive and install it as [Install a released extension](#install-a-released-extension) says.

Nothing signs a package or publishes it to a registry yet.

## Security

To report a vulnerability, obey the [security policy](https://github.com/nomux-sh/.github/blob/main/SECURITY.md) of the nomux-sh organization.

The README of each extension says which permissions the extension asks for and what its limits are.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Copyright The nomux Authors.
