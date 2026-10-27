#!/usr/bin/env bash
# Set the version of the recursa path crates in the checked fixture lock files to the workspace
# version.
#
# The generation fixtures under recursa-codegen/tests/fixtures/generation build with --locked, so
# their lock files must name the current version of each recursa crate. `release-plz update`
# changes the versions in the manifests and in the root Cargo.lock, but not in these files.
# scripts/release.sh runs this script after `release-plz update`. A release PR that release-plz
# opens does not run it, so run it on that PR branch and commit the result.
#
# With --check, the script changes no file. It names each fixture lock that has a different
# version and exits with status 1. CI runs this check.
#
# Usage: scripts/update-fixture-locks.sh [--check]

set -euo pipefail

check=0
case "${1-}" in
  "") ;;
  --check) check=1 ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

version=$(cargo metadata --format-version 1 --no-deps \
  | sed -n 's/.*"name":"recursa","version":"\([^"]*\)".*/\1/p')
[[ -n $version ]] || { echo "error: cannot read the recursa version" >&2; exit 1; }

# In a lock file, a path package has no `source` line. Each recursa crate in a fixture lock is a
# path package, so only its `version` line follows its `name` line directly.
rewrite='s/(\[\[package\]\]\nname = "recursa(?:-core|-macros|-codegen|-diagrams)?"\nversion = )"[^"]*"/$1"'"$version"'"/g'

if [[ $check == 0 ]]; then
  find recursa-codegen/tests/fixtures/generation -name '*.lock' -print0 \
    | xargs -0 perl -0pi -e "$rewrite"
  echo "fixture locks name recursa $version"
  exit 0
fi

stale=0
while IFS= read -r -d '' lock; do
  if ! perl -0pe "$rewrite" "$lock" | cmp -s - "$lock"; then
    echo "error: $lock does not name recursa $version" >&2
    stale=1
  fi
done < <(find recursa-codegen/tests/fixtures/generation -name '*.lock' -print0)

if [[ $stale == 1 ]]; then
  echo "Run scripts/update-fixture-locks.sh and commit the result." >&2
  exit 1
fi
echo "fixture locks name recursa $version"
