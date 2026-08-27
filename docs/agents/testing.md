# Test execution policy

The default local and continuous-integration test commands exercise correctness without running
benchmarks, enforcing execution-time budgets, or recursively rerunning acceptance commands.

## Default suites

Stable Rust owns exhaustive test execution. The default exhaustive commands are:

```sh
cargo test --workspace --all-targets
cargo test --workspace --all-targets --all-features
```

The Rust 1.88 lane proves MSRV compatibility without executing that suite a second time. It checks
every workspace target and feature, then executes one representative generated downstream crate:

```sh
cargo +1.88.0 check --workspace --all-targets --all-features --locked
RUSTUP_TOOLCHAIN=1.88.0 cargo +1.88.0 test -p recursa-codegen \
  --test generated_lexing --locked \
  a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text -- --exact
```

Keep `RUSTUP_TOOLCHAIN` set for the smoke test because its test harness starts a nested Cargo
process. This ensures the downstream generated crate is also compiled with Rust 1.88.

Tests reached by these commands must assert behavior, structure, diagnostics, or compatibility—not
wall-clock duration. A test may use a timeout only to prove termination or prevent a hung process;
the elapsed time is not a performance contract.

## Manual performance and aggregate evidence

Benchmarks, representative-scale performance checks, execution-time budgets, and tests that rerun
other test commands must be opt-in. Mark such Rust tests `#[ignore]` with a reason and document one
explicit `--ignored --exact` command. Do not add that command to the default CI workflow.

The generated-behavior aggregate is intentionally manual because it executes the same acceptance
commands that the default workspace suite already runs independently:

```sh
cargo test -p recursa-codegen --test generation_functional_matrix --locked \
  every_green_generation_evidence_command_executes_without_pending_rows -- \
  --ignored --exact
```

Representative-scale and benchmark work associated with the deferred performance ticket remains
manual until the user explicitly requests it. Correctness failures discovered by ordinary tests
remain part of the default suite even when their fixture compilation is relatively slow.

## Adding tests

- Keep functional correctness tests in the default suite.
- Put Criterion benches or equivalent harnesses under an explicit benchmark command.
- Keep machine-speed thresholds out of default tests and CI.
- Avoid aggregate tests that invoke other default test commands; when historical acceptance requires
  one, keep it ignored and manual as above.
- Keep exhaustive execution in the Stable lane. The MSRV lane checks all targets and features and
  runs only the representative downstream-generation smoke test.
- Exercise planner diagnostics directly at the source-analysis or emission-planning seam. Retain a
  small downstream-Cargo smoke matrix for errors whose observable contract includes Cargo or Rust
  compiler integration.
- When many scenarios need the same downstream fixture, use one serialized, stable fixture
  workspace and reset its authored and generated state between scenarios. Reuse compiled dependency
  artifacts only; do not share a target directory between independent temporary package roots.
