use chrono::Utc;

use crate::crd::common::Condition;

/// Standard condition types following Strimzi conventions
pub const CONDITION_TYPE_READY: &str = "Ready";
pub const CONDITION_TYPE_BACKUP_COMPLETE: &str = "BackupComplete";
pub const CONDITION_TYPE_RESTORE_COMPLETE: &str = "RestoreComplete";
pub const CONDITION_TYPE_SCHEDULED: &str = "Scheduled";
pub const CONDITION_TYPE_ERROR: &str = "Error";

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
pub const REASON_RESTORE_RUNNING: &str = "RestoreRunning";
pub const REASON_RESTORE_COMPLETED: &str = "RestoreCompleted";
pub const REASON_RESTORE_FAILED: &str = "RestoreFailed";
pub const REASON_CLUSTER_NOT_FOUND: &str = "ClusterNotFound";
pub const REASON_INVALID_CONFIG: &str = "InvalidConfiguration";
pub const REASON_SECRET_NOT_FOUND: &str = "SecretNotFound";

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

/// Create an error condition (sets Ready=False and adds Error condition)
pub fn error_conditions(reason: &str, message: &str) -> Vec<Condition> {
    vec![
        not_ready(reason, message),
        new_condition(CONDITION_TYPE_ERROR, STATUS_TRUE, reason, message),
    ]
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
    fn test_is_condition_true() {
        let conditions = vec![ready("Test", "test")];
        assert!(is_condition_true(&conditions, CONDITION_TYPE_READY));
        assert!(!is_condition_true(&conditions, CONDITION_TYPE_ERROR));
    }
}
