#!/usr/bin/env bash
# Usage: validate-semver-result.sh <semver-result-dir>
# Runs from the default branch (Version Bump workflow). Everything in <semver-result-dir> comes from a
# pull_request run and is untrusted: each value is validated, and the PR must still be an open
# same-repository PR into main at the commit that was checked.
# Writes the validated values to $GITHUB_OUTPUT for the label and commit steps.
# Env: GH_TOKEN, REPO (owner/name), RUN_HEAD_SHA and RUN_HEAD_BRANCH (from the workflow_run event).
set -euo pipefail

dir="$1"

read -r version level breaking < "$dir/result.txt"
pr_number=$(cat "$dir/pr-number.txt")
head_sha=$(cat "$dir/head-sha.txt")
published=$(cat "$dir/published.txt")

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version: $version" >&2; exit 1; }
[[ "$published" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid published version" >&2; exit 1; }
[[ "$level" =~ ^(patch|minor|major)$ ]] || { echo "invalid level: $level" >&2; exit 1; }
[[ "$breaking" =~ ^(true|false)$ ]] || { echo "invalid breaking flag: $breaking" >&2; exit 1; }
[[ "$pr_number" =~ ^[0-9]+$ ]] || { echo "invalid PR number" >&2; exit 1; }
[[ "$head_sha" == "$RUN_HEAD_SHA" ]] || { echo "artifact head SHA does not match the triggering run" >&2; exit 1; }

pr=$(gh api "repos/$REPO/pulls/$pr_number")
state=$(jq -r .state <<< "$pr")
pr_head_ref=$(jq -r .head.ref <<< "$pr")
pr_head_repo=$(jq -r .head.repo.full_name <<< "$pr")
pr_head_sha=$(jq -r .head.sha <<< "$pr")
pr_base=$(jq -r .base.ref <<< "$pr")
if [ "$state" != "open" ] || [ "$pr_base" != "main" ] || [ "$pr_head_repo" != "$REPO" ] \
  || [ "$pr_head_ref" != "$RUN_HEAD_BRANCH" ]; then
  echo "PR #$pr_number is not an open same-repository PR into main" >&2
  exit 1
fi

moved=false
if [ "$pr_head_sha" != "$head_sha" ]; then
  echo "::notice::PR #$pr_number moved since the check ran; the run for the new head will handle it"
  moved=true
fi

current=$(gh api -H "Accept: application/vnd.github.raw" "repos/$REPO/contents/Cargo.toml?ref=$head_sha" \
  | awk '/^\[/ { p = ($0 == "[package]") } p && /^version = / { gsub(/"/, "", $3); print $3; exit }')
[[ "$current" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "unexpected Cargo.toml version: $current" >&2; exit 1; }

# Never lower the version: Cargo.toml may already carry an unreleased bump (an earlier merged
# sync PR, or one set by hand), which this release must keep
target=$(printf '%s\n%s\n' "$version" "$current" | sort -V | tail -1)

{
  echo "pr-number=$pr_number"
  echo "head-sha=$head_sha"
  echo "moved=$moved"
  echo "published=$published"
  echo "version=$version"
  echo "level=$level"
  echo "breaking=$breaking"
  echo "current=$current"
  echo "target=$target"
} >> "$GITHUB_OUTPUT"
