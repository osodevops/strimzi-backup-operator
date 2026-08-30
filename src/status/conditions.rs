use chrono::Utc;

use crate::crd::common::Condition;
use crate::engine::EngineCheck;

/// Standard condition types following Strimzi conventions
pub const CONDITION_TYPE_READY: &str = "Ready";
pub const CONDITION_TYPE_BACKUP_COMPLETE: &str = "BackupComplete";
pub const CONDITION_TYPE_RESTORE_COMPLETE: &str = "RestoreComplete";
pub const CONDITION_TYPE_SCHEDULED: &str = "Scheduled";
pub const CONDITION_TYPE_ERROR: &str = "Error";
pub const CONDITION_TYPE_RECONCILIATION_PAUSED: &str = "ReconciliationPaused";
/// Whether the resolved engine image meets the operator's minimum supported
/// kafka-backup release. Informational: a Job is created either way.
pub const CONDITION_TYPE_ENGINE_VERSION_SUPPORTED: &str = "EngineVersionSupported";

/// Condition status values
pub const STATUS_TRUE: &str = "True";
pub const STATUS_FALSE: &str = "False";
pub const STATUS_UNKNOWN: &str = "Unknown";

/// Common reason strings
pub const REASON_RECONCILING: &str = "Reconciling";
pub const REASON_BACKUP_RUNNING: &str = "BackupRunning";
pub const REASON_BACKUP_COMPLETED: &str = "BackupCompleted";
pub const REASON_BACKUP_FAILED: &str = "BackupFailed";
pub const REASON_BACKUP_SCHEDULED: &str = "BackupScheduled";
pub const REASON_BACKUP_SUSPENDED: &str = "BackupSuspended";
pub const REASON_RESTORE_RUNNING: &str = "RestoreRunning";
pub const REASON_RESTORE_COMPLETED: &str = "RestoreCompleted";
pub const REASON_RESTORE_FAILED: &str = "RestoreFailed";
pub const REASON_CLUSTER_NOT_FOUND: &str = "ClusterNotFound";
pub const REASON_INVALID_CONFIG: &str = "InvalidConfiguration";
pub const REASON_SECRET_NOT_FOUND: &str = "SecretNotFound";
pub const REASON_RECONCILIATION_PAUSED: &str = "ReconciliationPaused";
pub const REASON_ENGINE_VERSION_SUPPORTED: &str = "EngineVersionSupported";
pub const REASON_ENGINE_VERSION_UNKNOWN: &str = "EngineVersionUnknown";
pub const REASON_ENGINE_OLDER_THAN_MINIMUM: &str = "EngineOlderThanMinimum";

/// Where the compatibility policy is written down.
pub const COMPATIBILITY_DOC_URL: &str =
    "https://github.com/osodevops/strimzi-backup-operator#compatibility";

/// Create a new condition
pub fn new_condition(condition_type: &str, status: &str, reason: &str, message: &str) -> Condition {
    Condition {
        condition_type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(Utc::now()),
    }
}

/// Set or update a condition in a conditions list.
/// If a condition with the same type exists and the status hasn't changed,
/// only the reason and message are updated (preserving lastTransitionTime).
pub fn set_condition(conditions: &mut Vec<Condition>, new_condition: Condition) {
    if let Some(existing) = conditions
        .iter_mut()
        .find(|c| c.condition_type == new_condition.condition_type)
    {
        if existing.status != new_condition.status {
            *existing = new_condition;
        } else {
            existing.reason = new_condition.reason;
            existing.message = new_condition.message;
        }
    } else {
        conditions.push(new_condition);
    }
}

/// Find a condition by type
pub fn find_condition<'a>(
    conditions: &'a [Condition],
    condition_type: &str,
) -> Option<&'a Condition> {
    conditions
        .iter()
        .find(|c| c.condition_type == condition_type)
}

/// Check if a condition is true
pub fn is_condition_true(conditions: &[Condition], condition_type: &str) -> bool {
    find_condition(conditions, condition_type).is_some_and(|c| c.status == STATUS_TRUE)
}

/// Create a Ready=True condition
pub fn ready(reason: &str, message: &str) -> Condition {
    new_condition(CONDITION_TYPE_READY, STATUS_TRUE, reason, message)
}

/// Create a Ready=False condition
pub fn not_ready(reason: &str, message: &str) -> Condition {
    new_condition(CONDITION_TYPE_READY, STATUS_FALSE, reason, message)
}

/// Create the condition used while the Strimzi pause annotation is enabled.
pub fn reconciliation_paused() -> Condition {
    new_condition(
        CONDITION_TYPE_RECONCILIATION_PAUSED,
        STATUS_TRUE,
        REASON_RECONCILIATION_PAUSED,
        "Reconciliation is paused by annotation",
    )
}

/// Create an error condition (sets Ready=False and adds Error condition)
pub fn error_conditions(reason: &str, message: &str) -> Vec<Condition> {
    vec![
        not_ready(reason, message),
        new_condition(CONDITION_TYPE_ERROR, STATUS_TRUE, reason, message),
    ]
}

/// The `EngineVersionSupported` condition for a resolved engine image.
pub fn engine_version_condition(check: &EngineCheck, image: &str) -> Condition {
    match check {
        EngineCheck::Supported(version) => new_condition(
            CONDITION_TYPE_ENGINE_VERSION_SUPPORTED,
            STATUS_TRUE,
            REASON_ENGINE_VERSION_SUPPORTED,
            &format!("{image} ({version}) is a supported kafka-backup release"),
        ),
        EngineCheck::Unknown => new_condition(
            CONDITION_TYPE_ENGINE_VERSION_SUPPORTED,
            STATUS_TRUE,
            REASON_ENGINE_VERSION_UNKNOWN,
            &format!(
                "{image} does not carry a kafka-backup release tag; the compatibility policy \
                 cannot be applied and the image is used as given"
            ),
        ),
        EngineCheck::Unsupported { found, min } => new_condition(
            CONDITION_TYPE_ENGINE_VERSION_SUPPORTED,
            STATUS_FALSE,
            REASON_ENGINE_OLDER_THAN_MINIMUM,
            &format!(
                "{image} ({found}) is older than the minimum supported kafka-backup {min}; \
                 options this engine does not know are ignored, see {COMPATIBILITY_DOC_URL}"
            ),
        ),
    }
}

/// Conditions that describe the resource's configuration rather than the
/// outcome of the current reconcile, and therefore survive every status
/// update that replaces the outcome conditions.
fn is_sticky(condition: &Condition) -> bool {
    condition.condition_type == CONDITION_TYPE_ENGINE_VERSION_SUPPORTED
}

/// Replace the outcome conditions (`Ready`, `Error`, …) with `new`, keeping
/// any sticky conditions already present.
pub fn replace_conditions(conditions: &mut Vec<Condition>, new: Vec<Condition>) {
    let sticky: Vec<Condition> = conditions.drain(..).filter(is_sticky).collect();
    *conditions = new;
    conditions.extend(sticky);
}

/// Whether `existing` already says what `new` says (same status, reason and
/// message), so a status patch can be skipped.
pub fn condition_matches(existing: Option<&Condition>, new: &Condition) -> bool {
    existing.is_some_and(|c| {
        c.status == new.status && c.reason == new.reason && c.message == new.message
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_condition_adds_new() {
        let mut conditions = vec![];
        let cond = ready("Test", "test message");
        set_condition(&mut conditions, cond);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].condition_type, CONDITION_TYPE_READY);
        assert_eq!(conditions[0].status, STATUS_TRUE);
    }

    #[test]
    fn test_set_condition_updates_existing_same_status() {
        let mut conditions = vec![ready("OldReason", "old message")];
        let original_time = conditions[0].last_transition_time;
        let new_cond = ready("NewReason", "new message");
        set_condition(&mut conditions, new_cond);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].reason.as_deref(), Some("NewReason"));
        assert_eq!(conditions[0].message.as_deref(), Some("new message"));
        // Transition time should be preserved
        assert_eq!(conditions[0].last_transition_time, original_time);
    }

    #[test]
    fn test_set_condition_updates_existing_different_status() {
        let mut conditions = vec![ready("OldReason", "old message")];
        let new_cond = not_ready("NewReason", "new message");
        set_condition(&mut conditions, new_cond);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].status, STATUS_FALSE);
    }

    #[test]
    fn test_find_condition() {
        let conditions = vec![
            ready("Test", "ready"),
            new_condition(CONDITION_TYPE_ERROR, STATUS_FALSE, "NoError", "no error"),
        ];
        let found = find_condition(&conditions, CONDITION_TYPE_READY);
        assert!(found.is_some());
        assert_eq!(found.unwrap().status, STATUS_TRUE);

        let not_found = find_condition(&conditions, "NonExistent");
        assert!(not_found.is_none());
    }

    #[test]
    fn engine_condition_unsupported_is_false_with_reason_and_message() {
        let check = crate::engine::check_engine_version("osodevops/kafka-backup:v0.15.3");
        let cond = engine_version_condition(&check, "osodevops/kafka-backup:v0.15.3");
        assert_eq!(cond.condition_type, CONDITION_TYPE_ENGINE_VERSION_SUPPORTED);
        assert_eq!(cond.status, STATUS_FALSE);
        assert_eq!(
            cond.reason.as_deref(),
            Some(REASON_ENGINE_OLDER_THAN_MINIMUM)
        );
        let message = cond.message.unwrap();
        assert!(message.contains("v0.15.3"), "{message}");
        assert!(message.contains("v0.16.0"), "{message}");
        assert!(message.contains(COMPATIBILITY_DOC_URL), "{message}");
    }

    #[test]
    fn engine_condition_unknown_is_true_with_unknown_reason() {
        let check = crate::engine::check_engine_version("osodevops/kafka-backup:latest");
        let cond = engine_version_condition(&check, "osodevops/kafka-backup:latest");
        assert_eq!(cond.status, STATUS_TRUE);
        assert_eq!(cond.reason.as_deref(), Some(REASON_ENGINE_VERSION_UNKNOWN));
    }

    #[test]
    fn engine_condition_supported_is_true() {
        let check = crate::engine::check_engine_version("osodevops/kafka-backup:v0.19.1");
        let cond = engine_version_condition(&check, "osodevops/kafka-backup:v0.19.1");
        assert_eq!(cond.status, STATUS_TRUE);
        assert_eq!(
            cond.reason.as_deref(),
            Some(REASON_ENGINE_VERSION_SUPPORTED)
        );
    }

    #[test]
    fn replace_conditions_keeps_the_engine_condition_after_the_outcome_conditions() {
        let engine = engine_version_condition(
            &crate::engine::check_engine_version("x:v0.19.1"),
            "x:v0.19.1",
        );
        let mut conditions = vec![ready("Old", "old"), engine.clone()];
        replace_conditions(&mut conditions, error_conditions("Boom", "boom"));
        assert_eq!(conditions.len(), 3);
        assert_eq!(conditions[0].condition_type, CONDITION_TYPE_READY);
        assert_eq!(conditions[1].condition_type, CONDITION_TYPE_ERROR);
        assert_eq!(
            conditions[2].condition_type,
            CONDITION_TYPE_ENGINE_VERSION_SUPPORTED
        );
        assert_eq!(
            conditions[2].last_transition_time,
            engine.last_transition_time
        );
    }

    #[test]
    fn condition_matches_compares_status_reason_and_message() {
        let a = ready("R", "m");
        let mut b = ready("R", "m");
        assert!(condition_matches(Some(&a), &b));
        b.message = Some("other".into());
        assert!(!condition_matches(Some(&a), &b));
        assert!(!condition_matches(None, &a));
    }

    #[test]
    fn test_is_condition_true() {
        let conditions = vec![ready("Test", "test")];
        assert!(is_condition_true(&conditions, CONDITION_TYPE_READY));
        assert!(!is_condition_true(&conditions, CONDITION_TYPE_ERROR));
    }
}
