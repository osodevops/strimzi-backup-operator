# API versioning and stability

This document states what each API version of the `KafkaBackup` and
`KafkaRestore` custom resources guarantees, how a version graduates, and how
existing resources migrate. It follows the
[Kubernetes API versioning conventions](https://kubernetes.io/docs/reference/using-api/#api-versioning)
and [deprecation policy](https://kubernetes.io/docs/reference/using-api/deprecation-policy/).

## Versions

| API version | Level | Served | Storage | Introduced | Status |
|---|---|---|---|---|---|
| `kafkabackup.com/v1alpha1` | alpha | yes | until 0.3.0 | 0.1.0 | Deprecated from operator 0.3.0; served until at least operator 0.5.0 or six months after 0.3.0, whichever is later |
| `kafkabackup.com/v1` | stable | from 0.3.0 | from 0.3.0 | 0.3.0 | Current |

The `v1` schema is **identical** to `v1alpha1` — the alpha schema only ever
grew additively through the 0.2.x series — so the API server converts between
the two without a conversion webhook (`conversion.strategy: None`), and
`kubectl get` works with either version. Existing objects need no migration
beyond the storage-version bump performed by the CRD update.

## What each level guarantees

**alpha (`v1alpha1`)** — may change or be removed. No backwards-compatibility
guarantee across operator releases. Use it to evaluate; pin the operator
version in production.

**stable (`v1`)** — the compatibility contract is:

- Fields are never removed, renamed, or changed in type within `v1`.
- New fields are optional and default to the previous behaviour, so a
  resource written for an older 0.x operator keeps working unchanged.
- Semantics of an existing field do not change. Behaviour changes that
  affect the data written (for example an engine bump that changes the
  archive format) are called out in the CHANGELOG with whether existing
  archives need re-taking; the resource schema is not the vehicle for them.
- Anything that would break these rules ships as a **new API version**
  (`v2`), served alongside `v1` with conversion, and `v1` keeps being served
  for **at least twelve months or two minor releases**, whichever is longer,
  with a `deprecationWarning` on every request.

## Graduation criteria (how `v1alpha1` became `v1`)

- The schema had no removal, rename or type change since 0.2.0 (all changes
  additive: `backup.config`/`restore.config` pass-through, `stripOffsetHeaders`,
  status `image` fields).
- Backup, restore, PITR, incremental (`offsetStorage`), scheduling, retention,
  engine pinning and the `EngineVersionSupported` guard are covered by the
  unit, integration and minikube end-to-end suites (`scripts/e2e/`).
- The `v1alpha1` API was served unchanged across two operator minor release
  lines (0.1.x and 0.2.x, from 0.1.0 to 0.2.25) before graduation.

## Deprecation and removal of `v1alpha1`

From operator 0.3.0 every request against `kafkabackup.com/v1alpha1` receives a
`deprecationWarning` (visible in `kubectl` output). The version stays served
until **at least operator 0.5.0 or six months after 0.3.0**, whichever is
later. Before it is removed:

1. The removal is announced in the CHANGELOG one minor release ahead.
2. The `status.storedVersions` clean-up procedure is documented in RELEASING.md
   (all objects must have been rewritten under `v1`; `kubectl get kb -A -o yaml | kubectl apply -f -` is sufficient).

## Migrating a resource

Change `apiVersion: kafkabackup.com/v1alpha1` to `apiVersion: kafkabackup.com/v1`.
Nothing else changes. Resources already in the cluster do not need to be
re-applied.

## Operator versioning

The operator follows semantic versioning in the 0.x range: a **minor** release
may add CRD fields (always optional) and bump the default engine image (see the
compatibility policy in the README); a **patch** release changes neither. The
`EngineVersionSupported` condition tells you when a pinned engine is outside
the supported range.
