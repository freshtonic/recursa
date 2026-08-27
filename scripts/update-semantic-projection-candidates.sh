#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

cargo test -p recursa-codegen --lib semantic_projection_tests::write_semantic_projection_candidates --locked -- --ignored --exact --nocapture

echo "Review target/semantic-projection-candidates/. This command never writes committed expectations."
