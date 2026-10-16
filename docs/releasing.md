# Releasing

Releases use [release-plz](https://release-plz.dev) and are started by hand.
Merging to `main` never opens a release PR. There are two ways to release:
from your machine with `scripts/release.sh`, or with the `Release` GitHub
Actions workflow (`.github/workflows/release-plz.yml`). Both use the same
`release-plz.toml`.

## Release from your machine

Run `scripts/release.sh` from the repository root on a clean `main`. It does
not use GitHub Actions.

1. It checks the tools (`git`, `cargo`, `release-plz`, `gh`), the tokens, and
   the ssh-agent. It also checks that `main` has not diverged from
   `origin/main`.
2. It runs `cargo test --workspace`. Use `--skip-tests` to skip this step.
3. It runs `release-plz update`. release-plz reads the conventional commits
   since the last tag (`feat:` bumps the minor version, `fix:` the patch
   version, a breaking change the major version), bumps the six published
   crates together, and updates their changelogs.
4. It shows the diff, makes a signed commit `chore: release vX.Y.Z`, and
   pushes `main`. There is no release PR.
5. It runs `cargo publish --workspace --dry-run`, then `release-plz release`.
   release-plz publishes the crates to crates.io in dependency order, tags
   each crate (`<crate>-v<version>`), and creates the GitHub releases.

The script asks before the commit and before the publish. Use `--yes` to
skip the two questions.

You can run the script again. If `release-plz update` changes nothing, the
script goes directly to step 5, and release-plz skips the crates that are
already on crates.io. Thus, if crates.io refuses a publish (for example HTTP
429 when you publish many new crates), wait 10 minutes and run the script
again.

Notes:

- `release-plz release --dry-run` cannot check a crate if its sibling is not
  on crates.io yet. This applies to all the crates on the first release. The
  script uses `cargo publish --workspace --dry-run` instead, which packages
  the crates against one another in a local registry.
- `release_always = false` in `release-plz.toml` lets `release-plz release`
  publish only from a merged release PR. The local flow has no PR, so the
  script uses a temporary copy of the configuration with
  `release_always = true`.

Tokens for the local flow:

- crates.io: `CARGO_REGISTRY_TOKEN`, or the token that `cargo login` stores in
  `~/.cargo/credentials.toml`. It must have the `publish-new` and
  `publish-update` scopes.
- GitHub: `GIT_TOKEN`, or the token from `gh auth token`. release-plz uses it
  to create the GitHub releases.

## Release with GitHub Actions

1. Run the `Release` workflow with `command = release-pr`. release-plz bumps
   the versions and changelogs as in step 3 above, and opens a PR labelled
   `release`.
2. Review and merge that PR.
3. Run the `Release` workflow with `command = release`. release-plz publishes
   the crates, tags them, and creates the GitHub releases.

## What moves together

`recursa`, `recursa-core`, `recursa-macros`, `recursa-codegen`,
`recursa-diagrams` and `recursa-explorer` form one version group in
`release-plz.toml`. They pin one another with exact versions and the
generation protocol stamps the crate version into generated output, so a
consumer must upgrade them as a set. `recursa-test` and the examples are
never released: they have `release = false` and `publish = false`.

## Repository configuration for GitHub Actions

- Secret `CARGO_REGISTRY_TOKEN`: a crates.io token with the `publish-new`
  and `publish-update` scopes.
- Secret `RELEASE_PLZ_TOKEN`: a fine-grained personal access token with
  `contents: write` and `pull-requests: write` on this repository. A PR
  opened with the default `GITHUB_TOKEN` does not trigger CI.
- Settings > Actions > General: allow GitHub Actions to create and approve
  pull requests.
