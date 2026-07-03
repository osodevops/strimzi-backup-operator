pub mod backup_config;
pub mod logging_config;
pub mod restore_config;
pub mod secrets;
pub mod storage_config;

/// SASL mechanism value for SCRAM-SHA-512 as the kafka-backup binary's config
/// parser expects it: `SCRAM-SHA512`, with no hyphen before the digits. The
/// binary rejects the IANA spelling `SCRAM-SHA-512` with "unknown variant"
/// (issue #35).
pub(crate) const SASL_MECHANISM_SCRAM_SHA512: &str = "SCRAM-SHA512";
