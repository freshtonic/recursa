# Test execution policy

The default local and continuous-integration test commands exercise correctness without running
benchmarks, enforcing execution-time budgets, or recursively rerunning acceptance commands.

## Default suites

The default commands are:

```sh
cargo test --workspace --all-targets
cargo test --workspace --all-targets --all-features
```

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
