#!/usr/bin/env bash
# Usage: set-version.sh <version> <Cargo.toml> <Cargo.lock>
# Sets the crate version in the [package] table of Cargo.toml and in the crate's own Cargo.lock entry.
# Fails unless exactly one line changes in each file, so nothing else can be rewritten.
set -euo pipefail

version="$1"
manifest="$2"
lockfile="$3"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid version: $version" >&2
  exit 1
fi

# Only the first version key of the [package] table
awk -v v="$version" '
  /^\[/ { in_package = ($0 == "[package]") }
  in_package && !done && /^version = "[^"]*"$/ { print "version = \"" v "\""; done = 1; next }
  { print }
' "$manifest" > "$manifest.new"

# Only the version line right after the crate's own name in Cargo.lock
awk -v v="$version" '
  /^name = "dfns-sdk-rust"$/ { own = 1; print; next }
  own && /^version = "[^"]*"$/ { print "version = \"" v "\""; own = 0; next }
  { own = 0; print }
' "$lockfile" > "$lockfile.new"

for file in "$manifest" "$lockfile"; do
  changed=$(diff "$file" "$file.new" | grep -c '^>' || true)
  if [ "$changed" -gt 1 ]; then
    echo "expected at most one changed line in $file, got $changed" >&2
    rm -f "$file.new"
    exit 1
  fi
  mv "$file.new" "$file"
done
