#!/usr/bin/env bash
# Usage: open-release-pr.sh <report-file>
# Opens the release PR "chore(release): VERSION": creates the branch release/VERSION at BASE_SHA,
# commits the version in Cargo.toml and Cargo.lock, and opens the PR into main. GH_TOKEN is the
# dfns-sdk-semver app's token, so the branch, the commit (Verified) and the PR are the app's, and
# the PR's CI runs.
# Env: GH_TOKEN, REPO, BASE_SHA (main), VERSION, PUBLISHED, REQUIRED (what cargo-semver-checks requires),
#      MODE (computed|forced). Run from a checkout of BASE_SHA.
set -euo pipefail

report="$1"
script_dir=$(cd "$(dirname "$0")" && pwd)
branch="release/$VERSION"

"$script_dir/set-version.sh" "$VERSION" Cargo.toml Cargo.lock

# Fails if the branch already exists, e.g. a release PR for this version is still open
gh api "repos/$REPO/git/refs" -f ref="refs/heads/$branch" -f sha="$BASE_SHA" > /dev/null

jq -n \
  --arg repo "$REPO" --arg branch "$branch" --arg head "$BASE_SHA" \
  --arg message "chore(release): $VERSION" \
  --arg toml "$(base64 -w0 Cargo.toml)" --arg lock "$(base64 -w0 Cargo.lock)" \
  '{query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }",
    variables: {input: {
      branch: {repositoryNameWithOwner: $repo, branchName: $branch},
      expectedHeadOid: $head,
      message: {headline: $message},
      fileChanges: {additions: [{path: "Cargo.toml", contents: $toml}, {path: "Cargo.lock", contents: $lock}]}
    }}}' > release-commit.json
gh api graphql --input release-commit.json --jq .data.createCommitOnBranch.commit.oid

body=$(mktemp)
{
  if [ "$MODE" = "forced" ]; then
    echo "Release \`$VERSION\` (version set by hand; published: \`$PUBLISHED\`)."
  else
    echo "Release \`$VERSION\`, the version \`cargo-semver-checks\` requires against the published \`$PUBLISHED\`."
  fi
  echo
  if [ "$VERSION" != "$REQUIRED" ]; then
    echo "For reference, \`cargo-semver-checks\` requires \`$REQUIRED\`."
    echo
  fi
  if grep -q '^--- failure' "$report"; then
    echo "Breaking changes against \`$PUBLISHED\`:"
    echo
    echo "~~~"
    grep -E '^(--- failure|  )' "$report" | head -80
    echo "~~~"
    echo
  fi
  echo "After merging, tag the merge commit to publish: \`git tag -s $VERSION <merge-sha> && git push origin $VERSION\`."
} > "$body"

gh pr create --repo "$REPO" --base main --head "$branch" --title "chore(release): $VERSION" --body-file "$body"
