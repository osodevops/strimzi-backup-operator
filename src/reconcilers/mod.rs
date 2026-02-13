pub mod backup;
pub mod restore;

/// Finalizer name used by this operator
pub const FINALIZER: &str = "backup.strimzi.io/cleanup";

/// Annotation key for manual backup triggers
pub const TRIGGER_ANNOTATION: &str = "backup.strimzi.io/trigger";
pub const TRIGGER_VALUE_NOW: &str = "now";

/// Default backup image
pub const DEFAULT_BACKUP_IMAGE: &str = "ghcr.io/osodevops/kafka-backup:latest";
