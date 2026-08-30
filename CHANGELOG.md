# Changelog

All notable changes to this project will be documented in this file.

## 0.2.25 - 2026-08-30

### Added

- A written operator ↔ engine **compatibility policy** (README "Compatibility",
  [#67](https://github.com/osodevops/strimzi-backup-operator/issues/67)):
  which `kafka-backup` release each operator version ships as its default Job
  image, that any newer `0.x` engine may be pinned to pick up fixes without an
  operator release, the minimum engine (`v0.16.0`) below which the generated
  config degrades, and which typed fields need which engine.
- Helm `backupJobs.image` / `backupJobs.imagePullPolicy` (env `BACKUP_JOB_IMAGE`
  / `BACKUP_JOB_IMAGE_PULL_POLICY`): an installation-wide default engine image
  and pull policy for Job pods. Precedence is `spec.image` → `backupJobs.image`
  → the release's compiled-in default; an unset value leaves the compiled-in
  default in charge, so the chart can never pin a tag that drifts from the
  binary. `spec.image` is now documented in the README.
- `EngineVersionSupported` status condition on `KafkaBackup` and
  `KafkaRestore`: `False` / `EngineOlderThanMinimum` when the resolved image
  names a release older than the minimum supported engine (the Job is still
  created), `True` / `EngineVersionUnknown` for tags that are not a release
  (`latest`, digests, custom tags). It survives the other status updates.
- `status.lastBackup.image`, `status.backupHistory[].image` and
  `status.restore.image` record the engine image each Job ran with.
- Start-up log fields `default_job_image`, `default_job_image_source`,
  `job_image_pull_policy`, `min_supported_engine`; metrics
  `strimzi_backup_operator_engine_image_info{image,source}` and
  `strimzi_backup_operator_engine_version_unsupported_total{controller}`.
- CI `Engine Compatibility` job: pulls the compiled-in default image, runs the
  operator's generated backup/restore configs through it and fails on any
  "unknown config key"; nightly it does the same against the latest
  kafka-backup release and warns when the default trails it.
- `RELEASING.md`, `scripts/bump-engine.sh` (rewrites the compiled-in default,
  the README line and a CHANGELOG stub in one go) and
  `scripts/check-engine-image.sh` (run by CI and the release gate: README, CRDs
  and manifests must agree with the compiled-in default).
- `scripts/e2e/`: the engine images the minikube scenarios drive are
  parameters (`ENGINE_OLD`, `ENGINE_NEW` — the latter defaults to this
  checkout's compiled-in default) instead of `v0.19.x` literals, `images.sh`
  patches the constant wherever the source keeps it, and
  `scenario-11-helm-default-image.sh` covers `backupJobs.image`, the
  `spec.image` precedence, `EngineVersionSupported=False` below the minimum
  and the fallback to the compiled-in default.

### Changed

- The CRD descriptions of `spec.image` no longer embed the engine tag — they
  point at the README instead — so a default bump touches one constant, the
  README line and the CHANGELOG. `DEFAULT_BACKUP_IMAGE` moved to
  `src/engine.rs` (re-exported from `reconcilers` for compatibility).

## 0.2.24 - 2026-08-30

### Fixed

- The repository now ships the Apache License 2.0 it has always declared
  ([#66](https://github.com/osodevops/strimzi-backup-operator/issues/66)).
  `README.md` and `Cargo.toml` stated Apache-2.0 but no `LICENSE` file
  existed, so GitHub reported no licence and the README link 404'd. Added
  `LICENSE` and `NOTICE` at the root and in the Helm chart, the
  `artifacthub.io/license` chart annotation, and
  `org.opencontainers.image.licenses` (plus title/source/vendor) labels on the
  container image.

## 0.2.23 - 2026-08-29

### Fixed

- Scheduled backups no longer keep the previous operator version's job image
  after an operator upgrade ([#62](https://github.com/osodevops/strimzi-backup-operator/issues/62)).
  The Deployment had no update strategy, so during `helm upgrade` the old and
  the new operator pod reconciled concurrently and the old pod's last
  server-side apply of the CronJob (drained after SIGTERM) overwrote the new
  pod's; nothing re-applied it for up to 5 minutes. Three layers now prevent
  and heal this: the chart's Deployment rolls out with `maxSurge: 0` /
  `maxUnavailable: 1` (`updateStrategy` value; the outgoing pod is deleted
  before its replacement is created), the operator runs leader election so
  only the lease holder reconciles — the draining pod keeps the lease until
  its reconciles have finished — and the backup controller watches its
  CronJobs and re-reconciles everything 5s and 60s after start-up, so any
  out-of-band change to a scheduled CronJob is reverted within seconds
  (this is what heals the one-off upgrade from a pre-0.2.23 operator, which
  does not take part in the election). `updateStrategy.type: Recreate` is
  supported for fresh installs; an existing release managed with server-side
  apply cannot switch to it in place (Kubernetes forbids the API-defaulted
  `rollingUpdate` block with Recreate and SSA cannot clear it) — see
  `values.yaml`.

### Added

- Lease-based leader election (`coordination.k8s.io/v1`, client-go
  semantics): the chart's `leaderElection.*` values and the
  `LEADER_ELECTION_ENABLED` / `LEADER_ELECTION_LEASE_DURATION` /
  `LEADER_ELECTION_RENEW_DEADLINE` / `LEADER_ELECTION_RETRY_PERIOD` /
  `LEADER_ELECTION_LEASE_NAME` / `OPERATOR_NAMESPACE` environment variables
  were rendered but never read; they are honoured now. Only the holder of the
  `<release>-leader` Lease runs the controllers; other replicas stand by, so
  `replicaCount: 2` (with `updateStrategy.type: RollingUpdate`) gives a warm
  standby that takes over within a few seconds of the leader stopping.
- `/readyz` reports `leader` or `standby` (200) once the replica has observed
  the lease and `leader election pending` (503) before — a replica that cannot
  read the lease (missing RBAC, unreachable API) is not ready, so a
  misconfigured install fails loudly. With leader election disabled it is
  ready immediately, as before.
- `strimzi_backup_operator_leader{identity}` gauge (1 on the leader).
- `scripts/e2e/` + `manifests/e2e/`: minikube-based end-to-end scenarios for
  operator upgrades and leader election (not run in CI).

### Changed

- `leaderElection.enabled` now defaults to `true` (the chart renders the
  `coordination.k8s.io/leases` ClusterRole rule accordingly). Set it to
  `false` to keep the previous single-writer-by-convention behaviour; the
  `maxSurge: 0` rollout and the CronJob watch still cover plain upgrades.
- When the leader cannot renew its lease within `renewDeadline`, or finds the
  lease held by another replica, the process exits non-zero and the kubelet
  restarts it as a candidate — in-flight reconciles cannot be cancelled
  safely, so a restart is the only clean way to stop writing.
- All shutdown paths share one SIGTERM/SIGINT handler: controllers drain
  first, then the lease is released, then the process exits.
- Every reconciliation is bounded (120s) and every lease request is bounded
  (half the renew deadline): an API call that never answers now fails, is
  retried, and can no longer leave a healthy-looking leader doing nothing.
- Logs are written through a lossy non-blocking writer. With a CPU limit the
  runtime has a single worker thread, and a blocking write to a backed-up
  container stdout used to freeze probes, lease renewals and reconciles
  together (observed as 16-minute stalls under heavy host I/O).

## 0.2.22 - 2026-08-29

### Fixed

- Update the default job image to `osodevops/kafka-backup:v0.19.1`. The
  `kafka_backup_snapshot_records_target` / `kafka_backup_snapshot_records_remaining`
  gauges of a scheduled incremental backup (`spec.offsetStorage` +
  `spec.backup.stopAtCurrentOffsets`) were sized from the whole captured offset
  range on every run, so "remaining" started at the size of the entire archive
  even when only a few records were new
  ([#57](https://github.com/osodevops/strimzi-backup-operator/issues/57)).
  Both gauges now describe the current run: `target` is the number of records
  the run will fetch after resuming from its checkpoints, `remaining` counts
  down from it to `0`, and a run with nothing new reports `0` / `0`. Pin
  `spec.image` to keep an older image.

## 0.2.21 - 2026-08-29

### Fixed

- Update the default job image to `osodevops/kafka-backup:v0.19.0`. A record
  header whose value is **null** was archived — and restored — as an empty
  value by every earlier job image
  ([kafka-backup#155](https://github.com/osodevops/kafka-backup/issues/155));
  the loss was on the backup path, so archives taken with older images store
  such headers as empty and must be re-taken where the distinction matters.
  Pin `spec.image` to keep an older image.

### Added

- `spec.restore.stripOffsetHeaders` (`restore.strip_offset_headers`,
  kafka-backup v0.19.0+): remove the `x-original-*` / `x-source-*` headers
  kafka-backup added at backup time so restored records match the source
  header-for-header
  ([kafka-backup#154](https://github.com/osodevops/kafka-backup/issues/154)).

### Changed

- CRD descriptions for `spec.backup.includeOffsetHeaders` (kafka-backup
  default `true`, adds `x-original-offset` / `x-original-timestamp` to every
  archived record) and `spec.restore.includeOriginalOffsetHeader` (default
  `false`) now state their defaults and what they add.

## 0.2.20 - 2026-08-11

### Added

- `spec.backup.config` and `spec.restore.config`: free-form maps merged
  verbatim into the generated kafka-backup config's `backup:`/`restore:`
  sections using kafka-backup's native snake_case key names (the same pattern
  as Strimzi's `spec.kafka.config`). Keys set in `config` take precedence over
  the typed camelCase fields, so any option from the kafka-backup config
  reference can be set without waiting for an operator release. Fixes
  [#53](https://github.com/osodevops/strimzi-backup-operator/issues/53).

### Changed

- Update the default job image to `osodevops/kafka-backup:v0.16.0`, which adds
  `backup.fetch_max_bytes` and `backup.segment_max_records` and warns on
  unknown config keys at startup instead of silently ignoring them.

## 0.2.19 - 2026-08-11

### Fixed

- Update the default job image to `osodevops/kafka-backup:v0.15.13`, which
  fixes backups of compacted topics. Previously a fetch that landed on an
  offset compacted away from the tail of a record batch made the backup loop
  on the same batch forever (stalled progress, duplicate segments written to
  storage), and a batch whose records were all compacted away terminated the
  partition backup early while reporting success. Fixes
  [#54](https://github.com/osodevops/strimzi-backup-operator/issues/54).

## 0.2.18 - 2026-07-21

### Fixed

- Update the default job image to `osodevops/kafka-backup:v0.15.12`, which
  labels storage write metrics with the storage backend actually written to
  (`s3`, `azure`, `gcs`, …) instead of a hardcoded `filesystem`, and exposes
  counters under the documented single-`_total` names (for example
  `kafka_backup_records_total` rather than
  `kafka_backup_records_total_total`). Dashboards built on the doubled names
  must move to the documented names. Fixes
  [#50](https://github.com/osodevops/strimzi-backup-operator/issues/50).

## 0.2.17 - 2026-07-21

### Fixed

- Remove the unsupported `spec.backup.encryption` field from the OSS API and
  generated CRDs. Client-side backup encryption is an Enterprise Edition
  capability; exposing it here caused reconciliation to fail instead of
  creating a backup Job. A schema-hidden compatibility guard keeps older
  installed CRDs fail-closed rather than silently running an unencrypted
  backup. Fixes [#48](https://github.com/osodevops/strimzi-backup-operator/issues/48).

## 0.2.16 - 2026-07-19

### Added

- Add an opt-in Helm `metrics.jobPodMonitor` that discovers metrics-enabled `kafka-backup` Job and CronJob pods directly across namespaces. Generated backup and restore containers now declare their configured metrics port and carry a dedicated discovery label. Fixes [#43](https://github.com/osodevops/strimzi-backup-operator/issues/43).
- Add `spec.metrics.keepAliveSeconds` for backup and restore jobs so one-shot
  operations remain scrapeable after completion, and update the default job
  image to the released `osodevops/kafka-backup:v0.15.11` that supports it.
- Consume the runtime cardinality fix and progress metrics from
  `kafka-backup:v0.15.11`: aggregate continuous lag, snapshot target/remaining,
  and explicit unlimited partition series with `maxPartitionLabels: 0`. Fixes
  [#45](https://github.com/osodevops/strimzi-backup-operator/issues/45).

### Fixed

- Always expose `strimzi_backup_operator_build_info` from the operator's `/metrics` endpoint and record reconciliation counts and durations, so a healthy idle operator no longer returns an empty `200` response. Monitoring documentation now distinguishes the operator `ServiceMonitor` on port 9090 from job metrics on port 8080.

## 0.2.15 - 2026-07-19

### Fixed

- Honor `strimzi.io/pause-reconciliation: "true"` on `KafkaBackup` and `KafkaRestore` resources. Paused resources now receive a `ReconciliationPaused` status condition without creating or updating finalizers, ConfigMaps, Jobs, or CronJobs; deletion cleanup remains available for resources that were paused after reconciliation. Fixes [#44](https://github.com/osodevops/strimzi-backup-operator/issues/44).

## 0.2.14 - 2026-07-11

### Fixed

- Force server-side apply for operator-owned scheduled backup CronJobs so `KafkaBackup` changes such as `spec.resources` converge even when another field manager previously claimed parts of the generated pod template. This prevents apply conflicts from leaving the CronJob stale until it is deleted and the backup is reconciled again. Fixes [#41](https://github.com/osodevops/strimzi-backup-operator/issues/41).

## 0.2.13 - 2026-07-10

### Fixed

- Resolve Strimzi `Kafka` and `KafkaUser` resources through the stable `kafka.strimzi.io/v1` API required by Strimzi 1.0 and later. The operator falls back to `v1beta2` on a not-found response so existing installations running older Strimzi releases remain supported. Fixes [#39](https://github.com/osodevops/strimzi-backup-operator/issues/39).

## 0.2.12 - 2026-07-08

### Added

- Add `spec.strimziClusterRef.listener` to `KafkaBackup` and `KafkaRestore` to select the Kafka listener by name, overriding the automatic selection.

### Fixed

- Connect backup and restore jobs through a Kafka listener whose `authentication.type` matches the resource's `spec.authentication`, instead of always preferring the first TLS listener. Jobs using `scram-sha-512` were routed to a mutual-TLS listener when one existed, and the broker rejected the handshake with `CertificateRequired` since SCRAM clients carry no client certificate. Among matching listeners, in-cluster (`internal`, `cluster-ip`) and TLS-encrypted listeners are preferred; the bootstrap address is taken from the Kafka CR status entry for the selected listener. When no listener matches, reconciliation reports a `NoCompatibleListener` condition listing the cluster's listeners rather than generating a config that cannot work. Fixes [#37](https://github.com/osodevops/strimzi-backup-operator/issues/37).

## 0.2.11 - 2026-07-03

### Fixed

- Write `sasl_mechanism: SCRAM-SHA512` (no hyphen before the digits) into generated backup and restore ConfigMaps. The kafka-backup binary's config parser only accepts `SCRAM-SHA512`, so jobs for resources using `authentication.type: scram-sha-512` failed on startup with `unknown variant 'SCRAM-SHA-512'`. Fixes [#35](https://github.com/osodevops/strimzi-backup-operator/issues/35).

## 0.2.10 - 2026-06-12

### Added

- Add `spec.backoffLimit` to `KafkaRestore` and `KafkaBackup` to control pod retries on generated Jobs (including scheduled CronJob runs). Fixes [#31](https://github.com/osodevops/strimzi-backup-operator/issues/31).

### Changed

- Restore Jobs now default to `backoffLimit: 0` (a single attempt, previously 3): restores append to or purge target topics, so retrying a partially completed attempt can duplicate data. Set `spec.backoffLimit` explicitly to opt into retries. Backup Jobs keep the previous default of 3.

### Fixed

- Delete Jobs, CronJobs, and ConfigMaps with explicit Background propagation when a `KafkaBackup`/`KafkaRestore` is deleted. The batch/v1 Job API's legacy default deletion propagation is `Orphan`, which stripped the Job ownerReference from its pods and left Completed pods behind. Fixes [#30](https://github.com/osodevops/strimzi-backup-operator/issues/30).

## 0.2.9 - 2026-06-12

### Fixed

- Stop re-running `KafkaRestore` jobs after completion. A finished restore Job has `active=0`, which the reconciler read as "no job running" and re-created the Job on every 5-minute requeue; Job creation is now gated on the full set of Jobs for the resource (running, succeeded, or failed), and one-shot `KafkaBackup` runs are gated the same way. Fixes [#29](https://github.com/osodevops/strimzi-backup-operator/issues/29).
- Watch backup/restore Jobs from the controllers so `KafkaBackup`/`KafkaRestore` status reflects Job completion or failure within seconds instead of after the next periodic requeue.
- Treat a terminally failed restore Job (backoffLimit exhausted) as terminal: report a `RestoreFailed` condition instead of silently re-creating the Job every requeue. Pod-level retries remain owned by the Job's `backoffLimit`.
- Make status patches idempotent so repeated reconciles no longer rewrite `lastTransitionTime`/`completionTime` with the current wall clock on every pass.

## 0.2.8 - 2026-06-11

### Added

- Add `spec.topics` with include/exclude glob (or `~`-prefixed regex) patterns to `KafkaRestore`, allowing specific topics to be restored from a backup. Fixes [#26](https://github.com/osodevops/strimzi-backup-operator/issues/26).
- Add `spec.template.pod.serviceAccountName` to `KafkaBackup` and `KafkaRestore` so job pods in namespaces other than the operator's can run with a service account that exists there. Fixes [#27](https://github.com/osodevops/strimzi-backup-operator/issues/27).

## 0.2.7 - 2026-06-10

### Fixed

- Propagate `spec.schedule.suspend` to the scheduled backup CronJob so suspending a `KafkaBackup` stops scheduled runs, and report a `BackupSuspended` condition while suspended. Fixes [#24](https://github.com/osodevops/strimzi-backup-operator/issues/24).

## 0.2.6 - 2026-06-03

### Added

- Add `spec.template.pod.hostAliases` support for `KafkaBackup` and `KafkaRestore` job pods. Fixes [#22](https://github.com/osodevops/strimzi-backup-operator/issues/22).

## 0.2.5 - 2026-05-17

### Changed

- Update the default `kafka-backup` job image to `osodevops/kafka-backup:v0.15.6`.

## 0.2.4 - 2026-05-08

### Added

- Add first-class `spec.logging` and `spec.env` support for `KafkaBackup` and `KafkaRestore` job pods. Fixes [#18](https://github.com/osodevops/strimzi-backup-operator/issues/18).

### Changed

- Update the default `kafka-backup` job image to `osodevops/kafka-backup:v0.15.5`.

## 0.2.2 - 2026-04-28

### Fixed

- Use the Helm-rendered operator service account for backup and restore job pods by default, with a `backupJobs.serviceAccountName` override for dedicated job service accounts. Fixes [#14](https://github.com/osodevops/strimzi-backup-operator/issues/14).
- Update the default `kafka-backup` job image to the public `osodevops/kafka-backup:v0.15.3` release. Fixes [#15](https://github.com/osodevops/strimzi-backup-operator/issues/15).
- Apply scheduled backup retention policies by discovering stored backup manifests, preserving `status.backupHistory`, and pruning expired backup objects from S3, Azure Blob Storage, GCS, or filesystem storage. Fixes [#16](https://github.com/osodevops/strimzi-backup-operator/issues/16).

### Changed

- Aligned the operator-generated backup and restore configuration with the current `kafka-backup` v0.15.3 storage layout and image behavior.
