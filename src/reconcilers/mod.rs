pub mod backup;
pub mod restore;

/// Finalizer name used by this operator
pub const FINALIZER: &str = "kafkabackup.com/cleanup";

/// Annotation key for manual backup triggers
pub const TRIGGER_ANNOTATION: &str = "kafkabackup.com/trigger";
pub const TRIGGER_VALUE_NOW: &str = "now";

/// Strimzi-compatible annotation for temporarily suppressing reconciliation.
pub const PAUSE_RECONCILIATION_ANNOTATION: &str = "strimzi.io/pause-reconciliation";

/// Return whether a custom resource has explicitly paused reconciliation.
pub fn is_reconciliation_paused<K: kube::ResourceExt>(resource: &K) -> bool {
    resource
        .annotations()
        .get(PAUSE_RECONCILIATION_ANNOTATION)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

/// Default backup image. Pinned to a public, current kafka-backup release so
/// backup/restore job behavior is deterministic and the image is anonymously
/// pullable by Kubernetes.
pub const DEFAULT_BACKUP_IMAGE: &str = "osodevops/kafka-backup:v0.15.11";

/// Environment variable used by the Helm chart to pass the service account that
/// backup/restore job pods should run as.
pub const JOB_SERVICE_ACCOUNT_ENV: &str = "BACKUP_JOB_SERVICE_ACCOUNT";

/// Fallback used outside Helm. The chart sets `BACKUP_JOB_SERVICE_ACCOUNT` to
/// its rendered service account name, which handles custom release names and
/// `serviceAccount.name` overrides.
pub const DEFAULT_JOB_SERVICE_ACCOUNT: &str = "strimzi-backup-operator";

/// Delete parameters for cleaning up resources owned by a backup/restore CR
/// (Jobs, CronJobs, ConfigMaps) when the CR is deleted.
///
/// Background propagation must be explicit: the batch/v1 Job API's legacy
/// server-side default is `Orphan`, which strips the Job ownerReference from
/// its pods instead of deleting them, leaving Completed pods behind.
pub fn cleanup_delete_params() -> kube::api::DeleteParams {
    kube::api::DeleteParams::background()
}

pub fn job_service_account_name() -> Option<String> {
    let value = std::env::var(JOB_SERVICE_ACCOUNT_ENV)
        .unwrap_or_else(|_| DEFAULT_JOB_SERVICE_ACCOUNT.to_string());
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
