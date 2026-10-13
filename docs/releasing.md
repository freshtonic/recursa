# Releasing

Releases use [release-plz](https://release-plz.dev) and are started by hand.
Merging to `main` never opens a release PR: the `Release` workflow
(`.github/workflows/release-plz.yml`) has only a `workflow_dispatch` trigger.

## One release

1. Run the `Release` workflow with `command = release-pr`. release-plz reads
   the conventional commits since the last tag (`feat:` bumps the minor
   version, `fix:` the patch version, a breaking change the major version),
   bumps the six published crates together, updates their changelogs, and
   opens a PR labelled `release`.
2. Review and merge that PR.
3. Run the `Release` workflow with `command = release`. release-plz publishes
   the crates to crates.io in dependency order, tags each crate
   (`<crate>-v<version>`), and creates the GitHub releases.

## What moves together

`recursa`, `recursa-core`, `recursa-macros`, `recursa-codegen`,
`recursa-diagrams` and `recursa-explorer` form one version group in
`release-plz.toml`. They pin one another with exact versions and the
generation protocol stamps the crate version into generated output, so a
consumer must upgrade them as a set. `recursa-test` and the examples are
never released.

## Repository configuration

- Secret `CARGO_REGISTRY_TOKEN`: a crates.io token with the `publish-new`
  and `publish-update` scopes.
- Secret `RELEASE_PLZ_TOKEN`: a fine-grained personal access token with
  `contents: write` and `pull-requests: write` on this repository. A PR
  opened with the default `GITHUB_TOKEN` does not trigger CI.
- Settings > Actions > General: allow GitHub Actions to create and approve
  pull requests.
