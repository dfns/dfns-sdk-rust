#!/usr/bin/env bash
# Usage: next-version.sh <published-version> <report-file>
# Tries the candidate versions after <published-version> from smallest to largest, and prints
# "<version> <level> <breaking>" for the first one cargo-semver-checks accepts against the published crate.
# In 0.y.z a breaking change bumps y (Cargo's semver rule), so 0.x never needs a major bump.
# The cargo-semver-checks output for the rejected smaller bump is written to <report-file>.
set -euo pipefail

published="$1"
report="$2"
script_dir=$(cd "$(dirname "$0")" && pwd)

if ! [[ "$published" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "invalid published version: $published" >&2
  exit 1
fi
major=${BASH_REMATCH[1]}
minor=${BASH_REMATCH[2]}
patch=${BASH_REMATCH[3]}

if [ "$major" -eq 0 ]; then
  candidates=("0.$minor.$((patch + 1)) patch false" "0.$((minor + 1)).0 minor true")
else
  candidates=("$major.$minor.$((patch + 1)) patch false" "$major.$((minor + 1)).0 minor false" "$((major + 1)).0.0 major true")
fi

: > "$report"
for candidate in "${candidates[@]}"; do
  read -r version level breaking <<< "$candidate"
  "$script_dir/set-version.sh" "$version" Cargo.toml Cargo.lock
  # The release type is derived from the version change between the baseline and Cargo.toml
  if cargo semver-checks --baseline-version "$published" > semver-output.txt 2>&1; then
    rm -f semver-output.txt
    echo "$version $level $breaking"
    exit 0
  fi
  # Any other failure (build, network) must not be mistaken for a breaking change
  if ! grep -q "semver requires new .* version" semver-output.txt; then
    cat semver-output.txt >&2
    exit 1
  fi
  # Keep the output of the first rejected bump: it lists what broke
  if [ ! -s "$report" ]; then
    cp semver-output.txt "$report"
  fi
done

rm -f semver-output.txt
echo "no candidate version passed cargo-semver-checks" >&2
exit 1
