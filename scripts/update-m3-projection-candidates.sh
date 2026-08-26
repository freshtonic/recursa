#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

cargo test -p recursa-codegen --lib m3_projection_tests::write_m3_projection_candidates --locked -- --ignored --exact --nocapture

echo "Review target/m3-projection-candidates/. This command never writes committed expectations."
