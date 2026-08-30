# Releasing

Every merge to `main` that bumps the version is a release: the release gate
validates it, `release.yaml` tags `vX.Y.Z`, builds and pushes the image, and
opens a PR against `osodevops/helm-charts` with the chart. There is no
separate "cut a release" step, so the checklist below is what a release PR
must contain.

## 1. Engine image (only when kafka-backup shipped a new release)

The operator runs the [kafka-backup](https://github.com/osodevops/kafka-backup)
CLI in Jobs. Its default image is compiled in (`DEFAULT_BACKUP_IMAGE` in
`src/engine.rs`) and is the single source of truth — everything else is
checked against it by `scripts/check-engine-image.sh`, which the release gate
and the `Tests` workflow run.

```bash
scripts/bump-engine.sh v0.19.2
```

This rewrites the constant and the README "Current release" line and inserts
a CHANGELOG stub. Then:

- Replace the stub's TODO with what changed in the engine, **whether existing
  archives are affected** (e.g. 0.18.0 null-header fidelity meant archives
  had to be re-taken), and keep the closing "Pin `spec.image` to keep an
  older image."
- Add a row for this operator version to the README "Compatibility" table.
  Raise `MIN_SUPPORTED_ENGINE` in `src/engine.rs` only if the generated
  config now depends on an engine feature — say so in the CHANGELOG.
- Run `scripts/check-engine-image.sh`.

The `engine-compat` CI job pulls the default image and runs the operator's
generated configs through it; it also warns when a newer engine release exists.

## 2. Version bump (every release)

`chore: prepare release X.Y.Z` — the release gate requires all of these to
agree and to be newer than the latest tag:

- `Cargo.toml` `version`
- `Cargo.lock` (`kafka-backup-operator` package entry — `cargo check` updates it)
- `deploy/helm/strimzi-backup-operator/Chart.yaml` `version` and `appVersion`
- `CHANGELOG.md` — a `## X.Y.Z - YYYY-MM-DD` section
- `README.md` "**Current release: X.Y.Z**" line

If CRD doc-comments changed: `cargo run --bin crdgen` and copy
`deploy/crds/*.yaml` into `deploy/helm/strimzi-backup-operator/crds/` (CI
requires them to be byte-identical).

Local gate, same checks as CI:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
helm lint deploy/helm/strimzi-backup-operator
bash scripts/release-gate.sh
```

## 3. After the merge

- Confirm the `Release` workflow published `ghcr.io/osodevops/strimzi-backup-operator:X.Y.Z`
  and merge the chart PR in `osodevops/helm-charts`.
- Sweep the docs site (`osodevops/kafka-backup-docs`):
  - `docs/docs/strimzi-operator/index.md` — current release, default job
    image, `--version` in the install/upgrade commands, the Compatibility table
  - `docs/docs/strimzi-operator/metrics.md` — operator version
  - `docs/docs/intro.md` — "What's New"
- If the default engine changed, mention it in the release notes GitHub
  generates from the CHANGELOG section.
