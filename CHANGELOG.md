# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

### Fixed

- Use the Helm-rendered operator service account for backup and restore job pods by default, with a `backupJobs.serviceAccountName` override for dedicated job service accounts. Fixes [#14](https://github.com/osodevops/strimzi-backup-operator/issues/14).
- Update the default `kafka-backup` job image to the public `osodevops/kafka-backup:v0.15.3` release. Fixes [#15](https://github.com/osodevops/strimzi-backup-operator/issues/15).
- Apply scheduled backup retention policies by discovering stored backup manifests, preserving `status.backupHistory`, and pruning expired backup objects from S3, Azure Blob Storage, GCS, or filesystem storage. Fixes [#16](https://github.com/osodevops/strimzi-backup-operator/issues/16).

### Changed

- Aligned the operator-generated backup and restore configuration with the current `kafka-backup` v0.15.3 storage layout and image behavior.
