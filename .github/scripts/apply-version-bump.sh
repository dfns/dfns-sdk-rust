#!/usr/bin/env bash
# Usage: apply-version-bump.sh <semver-result-dir>
# Runs from the default branch (Version Bump workflow). Everything read from <semver-result-dir> comes from
# a pull_request run and is untrusted: values are validated, and the only change written to the sync PR is
# the version line in Cargo.toml and Cargo.lock, made here.
# Env: GH_TOKEN, REPO (owner/name), RUN_HEAD_SHA and RUN_HEAD_BRANCH (from the workflow_run event).
set -euo pipefail

dir="$1"
script_dir=$(cd "$(dirname "$0")" && pwd)

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

# The PR must still be an open, same-repository sync PR into main, at the commit that was checked
pr=$(gh api "repos/$REPO/pulls/$pr_number")
state=$(jq -r .state <<< "$pr")
pr_head_ref=$(jq -r .head.ref <<< "$pr")
pr_head_repo=$(jq -r .head.repo.full_name <<< "$pr")
pr_head_sha=$(jq -r .head.sha <<< "$pr")
pr_base=$(jq -r .base.ref <<< "$pr")
if [ "$state" != "open" ] || [ "$pr_base" != "main" ] || [ "$pr_head_repo" != "$REPO" ] \
  || [ "$pr_head_ref" != "$RUN_HEAD_BRANCH" ] || [[ "$pr_head_ref" != sdk-sync/* ]]; then
  echo "PR #$pr_number is not an open sync PR of $REPO into main" >&2
  exit 1
fi
if [ "$pr_head_sha" != "$head_sha" ]; then
  echo "::notice::PR #$pr_number moved since the check ran; the run for the new head will apply the version"
  exit 0
fi

# Edit the PR head's Cargo.toml and Cargo.lock
work=$(mktemp -d)
for file in Cargo.toml Cargo.lock; do
  gh api -H "Accept: application/vnd.github.raw" "repos/$REPO/contents/$file?ref=$head_sha" > "$work/$file"
done
cp "$work/Cargo.toml" "$work/Cargo.toml.orig"
"$script_dir/set-version.sh" "$version" "$work/Cargo.toml" "$work/Cargo.lock"

if cmp -s "$work/Cargo.toml" "$work/Cargo.toml.orig"; then
  echo "Cargo.toml is already at $version"
else
  # Committed through the API so GitHub signs it (Verified), and only if the branch is still at head_sha
  jq -n \
    --arg repo "$REPO" --arg branch "$pr_head_ref" --arg head "$head_sha" \
    --arg message "chore(release): $version" \
    --arg toml "$(base64 -w0 "$work/Cargo.toml")" --arg lock "$(base64 -w0 "$work/Cargo.lock")" \
    '{query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }",
      variables: {input: {
        branch: {repositoryNameWithOwner: $repo, branchName: $branch},
        expectedHeadOid: $head,
        message: {headline: $message},
        fileChanges: {additions: [{path: "Cargo.toml", contents: $toml}, {path: "Cargo.lock", contents: $lock}]}
      }}}' > "$work/mutation.json"
  gh api graphql --input "$work/mutation.json" --jq .data.createCommitOnBranch.commit.oid
fi

# Label the release level, replacing labels from an earlier run on this PR
for name in release:patch release:minor release:major breaking; do
  gh pr edit "$pr_number" --repo "$REPO" --remove-label "$name" > /dev/null 2>&1 || true
done
gh label create "release:$level" --repo "$REPO" --color 0E8A16 --force > /dev/null
gh pr edit "$pr_number" --repo "$REPO" --add-label "release:$level"

if [ "$breaking" = "true" ]; then
  gh label create breaking --repo "$REPO" --color B60205 --force > /dev/null
  gh pr edit "$pr_number" --repo "$REPO" --add-label breaking
  # A breaking release needs a person to merge it
  gh pr merge "$pr_number" --repo "$REPO" --disable-auto > /dev/null 2>&1 || true
  {
    echo "**Breaking change:** this sync requires \`$version\` (published: \`$published\`). Auto-merge is disabled."
    echo
    echo "~~~"
    # Untrusted text: keep the cargo-semver-checks failure list only, without code fences
    grep -E '^(--- failure|  )' "$dir/report.txt" | tr -d '`~' | head -80 || true
    echo "~~~"
  } > "$work/comment.md"
  gh pr comment "$pr_number" --repo "$REPO" --body-file "$work/comment.md"
fi
