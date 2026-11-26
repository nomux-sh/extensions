#!/bin/sh
# Prints the version of one extension. Fails if the directory is not an
# extension, or if `extension.toml` and the crate of the daemon part have
# different versions.
# Usage: scripts/extension-version.sh <name>, for example git.
set -eu

name=$1

cd "$(git rev-parse --show-toplevel)"

case "$name" in
  "" | *[!a-z0-9-]* | -*)
    echo "extension-version.sh: '$name' is not the name of an extension directory." >&2
    exit 1
    ;;
esac
if [ ! -f "$name/extension.toml" ] || [ ! -f "$name/daemon/Cargo.toml" ]; then
  echo "extension-version.sh: $name/extension.toml or $name/daemon/Cargo.toml does not exist." >&2
  exit 1
fi

manifest="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$name/extension.toml")"
crate="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$name/daemon/Cargo.toml")"
if [ -z "$manifest" ] || [ "$manifest" != "$crate" ]; then
  echo "extension-version.sh: $name/extension.toml has the version '$manifest' and $name/daemon/Cargo.toml has '$crate'." >&2
  exit 1
fi

echo "$manifest"
