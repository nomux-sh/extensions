#!/bin/sh
# Builds the daemon part of one extension for one target, packs the release
# archive, checks it and prints its path. The archive has one directory, which
# is the package that `nomux ext install <directory>` reads:
#
#   nomux-<name>-<version>-<target>/
#     extension.toml
#     bin/nomux-<name>
#     LICENSE-APACHE
#     LICENSE-MIT
#
# Usage: scripts/build-extension.sh <name> <target>
# Example: scripts/build-extension.sh git x86_64-unknown-linux-musl
set -eu

name=$1
target=$2

cd "$(git rev-parse --show-toplevel)"

version="$(scripts/extension-version.sh "$name")"
binary="nomux-$name"
target_directory="${CARGO_TARGET_DIR:-target}"
output="$target_directory/dist"
package="nomux-$name-$version-$target"

rustup target add "$target" >&2
cargo build --release --locked --target "$target" -p "$binary" >&2

rm -rf "${output:?}/$package"
mkdir -p "$output/$package/bin"
cp "$name/extension.toml" LICENSE-APACHE LICENSE-MIT "$output/$package/"
cp "$target_directory/$target/release/$binary" "$output/$package/bin/"
# macOS tar adds the extended attributes as `._` files without this variable.
COPYFILE_DISABLE=1 tar -czf "$output/$package.tar.gz" -C "$output" "$package"
rm -rf "${output:?}/$package"

scripts/check-extension-archive.sh "$output/$package.tar.gz" "$name" >&2
echo "$output/$package.tar.gz"
