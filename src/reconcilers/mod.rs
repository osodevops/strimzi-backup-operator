pub mod backup;
pub mod restore;

/// Finalizer name used by this operator
pub const FINALIZER: &str = "kafkabackup.com/cleanup";

/// Annotation key for manual backup triggers
pub const TRIGGER_ANNOTATION: &str = "kafkabackup.com/trigger";
pub const TRIGGER_VALUE_NOW: &str = "now";

/// Default backup image. Pinned to a specific version so behaviour is deterministic —
/// v0.13.5 is the first release that activates incremental one-shot backups from an
/// `offset_storage` block alone (see kafka-backup PR #92).
pub const DEFAULT_BACKUP_IMAGE: &str = "ghcr.io/osodevops/kafka-backup:v0.13.5";
