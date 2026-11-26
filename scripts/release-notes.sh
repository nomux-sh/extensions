#!/bin/sh
# Prints the release notes of one version of an extension: the section of the
# version in <name>/CHANGELOG.md. Fails if the changelog has no such section.
# Usage: scripts/release-notes.sh <name> <version>, for example git 0.1.0.
set -eu

name=$1
version=$2

cd "$(git rev-parse --show-toplevel)"

section="$(awk -v heading="## [$version]" '
  index($0, heading) == 1 { found = 1; next }
  found && /^## \[/ { exit }
  found { print }
' "$name/CHANGELOG.md")"
if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
  echo "release-notes.sh: $name/CHANGELOG.md has no section for version $version." >&2
  exit 1
fi

printf '%s\n' "$section"
