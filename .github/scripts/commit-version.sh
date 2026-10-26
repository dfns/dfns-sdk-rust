#!/usr/bin/env bash
# Usage: commit-version.sh
# Sets TARGET in Cargo.toml and Cargo.lock of the PR head and commits it to the PR branch as
# "chore(release): TARGET". The commit goes through the GitHub API, so GitHub signs it (Verified),
# and only lands if the branch is still at HEAD_SHA.
# Env: GH_TOKEN, REPO, BRANCH, HEAD_SHA, TARGET (validated by validate-semver-result.sh).
set -euo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)

for file in Cargo.toml Cargo.lock; do
  gh api -H "Accept: application/vnd.github.raw" "repos/$REPO/contents/$file?ref=$HEAD_SHA" > "$work/$file"
done
"$script_dir/set-version.sh" "$TARGET" "$work/Cargo.toml" "$work/Cargo.lock"

jq -n \
  --arg repo "$REPO" --arg branch "$BRANCH" --arg head "$HEAD_SHA" \
  --arg message "chore(release): $TARGET" \
  --arg toml "$(base64 -w0 "$work/Cargo.toml")" --arg lock "$(base64 -w0 "$work/Cargo.lock")" \
  '{query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }",
    variables: {input: {
      branch: {repositoryNameWithOwner: $repo, branchName: $branch},
      expectedHeadOid: $head,
      message: {headline: $message},
      fileChanges: {additions: [{path: "Cargo.toml", contents: $toml}, {path: "Cargo.lock", contents: $lock}]}
    }}}' > "$work/mutation.json"
gh api graphql --input "$work/mutation.json" --jq .data.createCommitOnBranch.commit.oid
