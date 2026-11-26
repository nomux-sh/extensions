#!/usr/bin/env bash
# Checks one release archive of an extension: it has exactly the files of the
# package, the manifest starts the program that the archive has, and the
# program of a musl target is a static binary. Prints the files with their
# sizes and the type of the program.
# Usage: scripts/check-extension-archive.sh <archive> <name>
set -euo pipefail

archive="$1"
name="$2"

package="$(basename "$archive" .tar.gz)"
binary="nomux-$name"

expected="$(printf '%s\n' "$package/" "$package/LICENSE-APACHE" "$package/LICENSE-MIT" \
  "$package/bin/" "$package/bin/$binary" "$package/extension.toml" | sort)"
actual="$(tar -tzf "$archive" | sort)"
if [ "$actual" != "$expected" ]; then
  echo "error: $archive does not have the files of a package. It has:" >&2
  echo "$actual" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work"
cd "$work/$package"
ls -lR

if ! grep -qx "command = \"bin/$binary\"" extension.toml; then
  echo "error: extension.toml of $package does not start bin/$binary." >&2
  exit 1
fi
if [ ! -x "bin/$binary" ]; then
  echo "error: bin/$binary of $package is not executable." >&2
  exit 1
fi
description="$(file -b "bin/$binary")"
echo "bin/$binary: $description"
if [[ "$package" == *-linux-musl && ! "$description" =~ static(-pie|ally)\ linked ]]; then
  echo "error: bin/$binary of $package is not a static binary." >&2
  exit 1
fi
