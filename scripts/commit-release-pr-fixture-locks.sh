#!/usr/bin/env bash
# Set the new version in the fixture locks of the open release PR, and commit the change to the
# PR branch.
#
# release-plz opens the release PR, but it does not change the fixture locks (see
# scripts/update-fixture-locks.sh). The "Release" workflow runs this script after
# `release-plz release-pr`. You can also run it by hand.
#
# The script makes the commit with the GraphQL mutation createCommitOnBranch, as release-plz does,
# so GitHub signs it. Use a token that is not GITHUB_TOKEN, so that the commit starts CI on the PR.
#
# Environment:
#   GH_TOKEN           a token with contents:write on the repository
#   GITHUB_REPOSITORY  owner/name (default: the repository of the current directory)
#
# Usage: scripts/commit-release-pr-fixture-locks.sh [branch]
#   Without a branch, the script uses the open PR that has the label "release".

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

repository=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}

branch=${1-}
if [[ -z $branch ]]; then
  branches=$(gh pr list --repo "$repository" --state open --label release \
    --json headRefName --jq '.[].headRefName')
  [[ -n $branches ]] || { echo "no open release PR"; exit 0; }
  [[ $(grep -c . <<<"$branches") == 1 ]] \
    || { echo "error: more than one open release PR:" >&2; echo "$branches" >&2; exit 1; }
  branch=$branches
fi

# Work in a temporary worktree, so that the current checkout does not change.
worktree=$(mktemp -d -t release-pr-locks.XXXXXX)
trap 'cd "$repo_root" && git worktree remove --force "$worktree"; rm -rf "$worktree"' EXIT
git fetch --quiet origin "$branch"
git worktree add --quiet --detach "$worktree" FETCH_HEAD
cd "$worktree"
head=$(git rev-parse HEAD)

scripts/update-fixture-locks.sh
changed=$(git diff --name-only)
if [[ -z $changed ]]; then
  echo "the fixture locks on $branch are already correct"
  exit 0
fi

version=$(cargo metadata --format-version 1 --no-deps \
  | sed -n 's/.*"name":"recursa","version":"\([^"]*\)".*/\1/p')

additions=$(while IFS= read -r path; do
  jq -n --arg path "$path" --arg contents "$(base64 <"$path" | tr -d '\n')" \
    '{path: $path, contents: $contents}'
done <<<"$changed" | jq -s .)

jq -n \
  --arg repository "$repository" \
  --arg branch "$branch" \
  --arg head "$head" \
  --arg headline "test: set the recursa version in the fixture locks for $version" \
  --argjson additions "$additions" \
  '{
    query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid url } } }",
    variables: {
      input: {
        branch: {repositoryNameWithOwner: $repository, branchName: $branch},
        message: {headline: $headline},
        expectedHeadOid: $head,
        fileChanges: {additions: $additions}
      }
    }
  }' \
  | gh api graphql --input - --jq '.data.createCommitOnBranch.commit | "committed \(.oid) \(.url)"'

