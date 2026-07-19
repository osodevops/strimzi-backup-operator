# Changelog

All notable changes to this project will be documented in this file.

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
