#!/usr/bin/env bash
# Release the recursa crates from this machine, without GitHub Actions.
#
# Steps:
#   1. Check the tools, the tokens, and that main is clean and in step with
#      origin/main.
#   2. Run the default test suite (skip with --skip-tests).
#   3. `release-plz update`: bump the version group and the changelogs.
#   4. Show the diff, commit it signed, and push main.
#   5. `cargo publish --workspace --dry-run`, then `release-plz release`: publish to
#      crates.io, push the `<crate>-v<version>` tags, make the GitHub releases.
#
# The script is safe to run again. If step 3 changes nothing (for example a
# previous run committed the bump but crates.io refused a publish), it goes
# straight to step 5, and release-plz skips the crates that are already
# published.
#
# Tokens:
#   crates.io  CARGO_REGISTRY_TOKEN, or the token from `cargo login` in
#              ~/.cargo/credentials.toml. Scopes: publish-new, publish-update.
#   GitHub     GIT_TOKEN, or `gh auth token`.
#
# Usage: scripts/release.sh [--skip-tests] [--yes]

set -euo pipefail

skip_tests=0
assume_yes=0
for arg in "$@"; do
  case "$arg" in
    --skip-tests) skip_tests=1 ;;
    --yes) assume_yes=1 ;;
    -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

step() { printf '\n==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
confirm() {
  [[ $assume_yes == 1 ]] && return 0
  local reply
  read -r -p "$1 [y/N] " reply
  [[ $reply == y || $reply == Y ]]
}

step "Checking tools and tokens"
for tool in git cargo release-plz gh; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done

if [[ -z ${CARGO_REGISTRY_TOKEN:-} ]] \
  && ! grep -qs '^token' "${CARGO_HOME:-$HOME/.cargo}/credentials.toml"; then
  die "no crates.io token: set CARGO_REGISTRY_TOKEN or run 'cargo login'"
fi

if [[ -z ${GIT_TOKEN:-} ]]; then
  GIT_TOKEN=$(gh auth token 2>/dev/null) || die "no GitHub token: set GIT_TOKEN or run 'gh auth login'"
fi
export GIT_TOKEN

# Signed commits are required. Load the key from the Keychain if the agent is
# empty (see ~/.claude/CLAUDE.md).
if ! ssh-add -l >/dev/null 2>&1; then
  ssh-add --apple-load-keychain >/dev/null 2>&1 || true
  ssh-add -l >/dev/null 2>&1 || die "ssh-agent has no key; signing and pushing will fail"
fi

step "Checking the working tree"
[[ $(git branch --show-current) == main ]] || die "not on main"
[[ -z $(git status --porcelain) ]] || die "working tree is not clean"
git fetch --quiet origin main
local_head=$(git rev-parse HEAD)
remote_head=$(git rev-parse origin/main)
if [[ $local_head != "$remote_head" ]]; then
  git merge-base --is-ancestor origin/main HEAD \
    || die "main and origin/main have diverged; pull first"
  echo "main is ahead of origin/main; those commits are pushed with the release"
fi

if [[ $skip_tests == 0 ]]; then
  step "Running the default test suite"
  cargo test --workspace
fi

step "release-plz update"
release-plz update

if [[ -n $(git status --porcelain) ]]; then
  git --no-pager diff --stat
  git --no-pager diff -- '*Cargo.toml'
  confirm "Commit this version bump and push main?" || {
    echo "Stopped. Discard the changes with: git checkout -- . && git clean -fd"
    exit 1
  }
  version=$(cargo metadata --format-version 1 --no-deps \
    | sed -n 's/.*"name":"recursa","version":"\([^"]*\)".*/\1/p')
  git add -A
  git commit -S -m "chore: release v${version:-unknown}"
else
  echo "release-plz update changed nothing; going on to publish"
fi

step "Pushing main"
git push origin main

# release_always = false in release-plz.toml makes `release` publish only from
# a merged release PR. There is no PR in this flow, so use a copy of the
# config with release_always = true.
config=$(mktemp -t release-plz.XXXXXX.toml)
trap 'rm -f "$config"' EXIT
sed 's/^release_always = false/release_always = true/' release-plz.toml >"$config"
grep -q '^release_always = true' "$config" || die "could not override release_always"

# `release-plz release --dry-run` cannot check a crate whose sibling is not on
# crates.io yet (every crate on the first release). `cargo publish --workspace`
# packages the crates against one another in a local registry instead.
step "cargo publish --workspace --dry-run"
cargo publish --workspace --dry-run

confirm "Publish to crates.io, push tags, and make the GitHub releases?" || exit 1

step "release-plz release"
if ! release-plz release --config "$config"; then
  cat >&2 <<'EOF'

The release did not complete. If crates.io answered 429 (too many new crates),
wait 10 minutes and run this script again: it skips the version bump and the
crates that are already published.
EOF
  exit 1
fi

step "Done"
git --no-pager tag --points-at HEAD
