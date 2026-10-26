#!/usr/bin/env bash
# Usage: commit-version.sh
# Sync PRs only. Checks the PR really is a monorepo sync PR, then sets TARGET in Cargo.toml and Cargo.lock
# of the PR head and commits it to the PR branch as "chore(release): TARGET". The commit goes through the
# GitHub API, so GitHub signs it (Verified), and only lands if the branch is still at HEAD_SHA.
# Writes committed=true|false to $GITHUB_OUTPUT.
# Env: GH_TOKEN, REPO, PR_NUMBER, BRANCH, HEAD_SHA, TARGET (validated by validate-semver-result.sh),
#      SYNC_APP_USER_ID (the sync app's bot user), ACTIONS_BOT_USER_ID (github-actions[bot]).
set -euo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)

# A sync PR is one the monorepo's sync app opened, whose commits were all made through the GitHub API
# (committer web-flow, Verified) by that app or by this workflow's version commit. Anyone with write
# access can name a branch sdk-sync/, and a commit's author can be set by its email, but a pushed commit
# is never a Verified web-flow commit attributed to the app.
pr_author_id=$(gh api "repos/$REPO/pulls/$PR_NUMBER" --jq .user.id)
foreign_commits=$(gh api --paginate "repos/$REPO/pulls/$PR_NUMBER/commits" \
  | jq -s --arg sync "$SYNC_APP_USER_ID" --arg actions "$ACTIONS_BOT_USER_ID" \
    '[add[] | select(
        ((.author.id // 0 | tostring) as $a | ($a != $sync and $a != $actions))
        or .committer.login != "web-flow"
        or .commit.verification.verified != true
      )] | length')
if [ "$pr_author_id" != "$SYNC_APP_USER_ID" ] || [ "$foreign_commits" != "0" ]; then
  echo "::warning::PR #$PR_NUMBER has a sync branch name, but was not opened by the sync app or has commits from elsewhere: not setting its version"
  echo "committed=false" >> "$GITHUB_OUTPUT"
  exit 0
fi

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
echo "committed=true" >> "$GITHUB_OUTPUT"
