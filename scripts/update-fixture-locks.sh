#!/usr/bin/env bash
# Set the version of the recursa path crates in the checked fixture lock files to the workspace
# version.
#
# The generation fixtures under recursa-codegen/tests/fixtures/generation build with --locked, so
# their lock files must name the current version of each recursa crate. `release-plz update`
# changes the versions in the manifests and in the root Cargo.lock, but not in these files.
# scripts/release.sh runs this script after `release-plz update`.
#
# Usage: scripts/update-fixture-locks.sh

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

version=$(cargo metadata --format-version 1 --no-deps \
  | sed -n 's/.*"name":"recursa","version":"\([^"]*\)".*/\1/p')
[[ -n $version ]] || { echo "error: cannot read the recursa version" >&2; exit 1; }

# In a lock file, a path package has no `source` line. Each recursa crate in a fixture lock is a
# path package, so only its `version` line follows its `name` line directly.
find recursa-codegen/tests/fixtures/generation -name '*.lock' -print0 \
  | xargs -0 perl -0pi -e \
    's/(\[\[package\]\]\nname = "recursa(?:-core|-macros|-codegen|-diagrams)?"\nversion = )"[^"]*"/$1"'"$version"'"/g'

echo "fixture locks name recursa $version"
