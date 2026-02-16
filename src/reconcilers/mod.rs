pub mod backup;
pub mod restore;

/// Finalizer name used by this operator
pub const FINALIZER: &str = "kafkabackup.com/cleanup";

/// Annotation key for manual backup triggers
pub const TRIGGER_ANNOTATION: &str = "kafkabackup.com/trigger";
pub const TRIGGER_VALUE_NOW: &str = "now";

/// Default backup image
pub const DEFAULT_BACKUP_IMAGE: &str = "ghcr.io/osodevops/kafka-backup:latest";
