#!/usr/bin/env bash
# Usage: label-release.sh <semver-result-dir>
# Labels the PR with its release level (release:patch|minor|major), replacing labels from an earlier run.
# On a breaking change, also adds `breaking`, disables auto-merge and comments with the
# cargo-semver-checks failures (one comment per PR, updated on later runs).
# Env: GH_TOKEN, REPO, PR_NUMBER, LEVEL, BREAKING, VERSION, PUBLISHED (validated by validate-semver-result.sh),
#      SUGGEST_VERSION (optional: the version to tell the author to set).
set -euo pipefail

dir="$1"

for name in release:patch release:minor release:major breaking; do
  gh pr edit "$PR_NUMBER" --repo "$REPO" --remove-label "$name" > /dev/null 2>&1 || true
done
gh label create "release:$LEVEL" --repo "$REPO" --color 0E8A16 --force > /dev/null
gh pr edit "$PR_NUMBER" --repo "$REPO" --add-label "release:$LEVEL"

if [ "$BREAKING" != "true" ]; then
  exit 0
fi

gh label create breaking --repo "$REPO" --color B60205 --force > /dev/null
gh pr edit "$PR_NUMBER" --repo "$REPO" --add-label breaking
# A breaking release needs a person to merge it
gh pr merge "$PR_NUMBER" --repo "$REPO" --disable-auto > /dev/null 2>&1 || true

comment=$(mktemp)
{
  echo "**Breaking change** against the published \`$PUBLISHED\`: the next release must be at least \`$VERSION\`. Auto-merge is disabled."
  if [ -n "${SUGGEST_VERSION:-}" ]; then
    echo
    echo "If this PR should be released, set \`$SUGGEST_VERSION\` in \`Cargo.toml\` and \`Cargo.lock\`."
  fi
  echo
  echo "~~~"
  # The report comes from the pull_request run: keep the failure list only, without code fences
  grep -E '^(--- failure|  )' "$dir/report.txt" | tr -d '`~' | head -80 || true
  echo "~~~"
} > "$comment"
gh pr comment "$PR_NUMBER" --repo "$REPO" --edit-last --body-file "$comment" 2> /dev/null \
  || gh pr comment "$PR_NUMBER" --repo "$REPO" --body-file "$comment"
