# Changelog

All notable changes to this project will be documented in this file.

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
