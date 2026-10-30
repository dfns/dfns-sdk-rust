#!/usr/bin/env bash
# Usage: next-version.sh <published-version> <report-file>
# Runs cargo-semver-checks once against the published crate and prints "<version> <level> <breaking>"
# for the bump it requires. cargo-semver-checks names the required bump but not the version, so the
# number is derived here. Its full output is written to <report-file>.
set -euo pipefail

published="$1"
report="$2"

if ! [[ "$published" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "invalid published version: $published" >&2
  exit 1
fi
major=${BASH_REMATCH[1]}
minor=${BASH_REMATCH[2]}
patch=${BASH_REMATCH[3]}

if cargo semver-checks --baseline-version "$published" --release-type patch > "$report" 2>&1; then
  required="patch"
elif required=$(grep -oE 'semver requires new (major|minor) version' "$report" | head -1 | awk '{print $4}') \
  && [ -n "$required" ]; then
  :
else
  # Any other failure (build, network) must not be mistaken for a breaking change
  cat "$report" >&2
  exit 1
fi

# Below 1.0, Cargo shifts semver one position: in 0.Y.z, a breaking change bumps Y (0.3.2 -> 0.4.0) and
# anything else bumps z. cargo-semver-checks calls the Y bump "major". The move to 1.0.0 is never
# automatic: set it in Cargo.toml by hand (later runs never lower the version); from then on, breaking
# changes bump the first number.
if [ "$major" -eq 0 ]; then
  case "$required" in
    major) echo "0.$((minor + 1)).0 minor true" ;;
    *) echo "0.$minor.$((patch + 1)) patch false" ;;
  esac
else
  case "$required" in
    major) echo "$((major + 1)).0.0 major true" ;;
    minor) echo "$major.$((minor + 1)).0 minor false" ;;
    *) echo "$major.$minor.$((patch + 1)) patch false" ;;
  esac
fi
