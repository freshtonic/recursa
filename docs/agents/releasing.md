# Releasing

The recursa crates share one version. A release changes that version in each manifest and in the
root `Cargo.lock`.

## The fixture locks

The generation fixtures under `recursa-codegen/tests/fixtures/generation` have their own
`Cargo.lock` and `Cargo.core.lock` files. The fixture tests build them with `--locked`. Each of
these lock files names the version of each recursa crate. When the version changes, you must
update these lock files too:

```sh
scripts/update-fixture-locks.sh
```

If you do not do this step, the fixture tests fail with this error:

```text
error: cannot update the lock file .../Cargo.lock because --locked was passed to prevent this
```

`release-plz update` does not change the fixture locks.

## How to make a release

- **Local release (the usual method).** Run `scripts/release.sh`. It runs
  `scripts/update-fixture-locks.sh` after `release-plz update`. You do not have to run it
  yourself.
- **Release PR from the "Release" workflow (`release-pr`).** release-plz opens the PR. Then the
  workflow runs `scripts/commit-release-pr-fixture-locks.sh`. This script runs
  `scripts/update-fixture-locks.sh` on the PR branch and commits the result through the GitHub
  API, so GitHub signs the commit. If this step fails, run the script on your machine. It finds
  the open PR that has the label `release`, and it does not change your checkout.
- **Manual version change.** Run `scripts/update-fixture-locks.sh` after you change the version.

## CI check

The CI job "Fixture locks" runs `scripts/update-fixture-locks.sh --check`. This command changes no
file. It fails when a fixture lock names a version that is not the workspace version. If this job
fails, run `scripts/update-fixture-locks.sh` and commit the result.
