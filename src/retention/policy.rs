use chrono::{Duration, Utc};
use tracing::{info, warn};

use crate::crd::common::BackupHistoryEntry;
use crate::crd::kafka_backup::RetentionSpec;

/// Evaluate which backups should be pruned based on the retention policy
pub fn evaluate_retention(
    history: &[BackupHistoryEntry],
    retention: &RetentionSpec,
) -> Vec<String> {
    let mut to_prune = Vec::new();

    if history.is_empty() {
        return to_prune;
    }

    // Sort by start_time descending (newest first)
    let mut sorted: Vec<&BackupHistoryEntry> = history.iter().collect();
    sorted.sort_by(|a, b| b.start_time.cmp(&a.start_time));

    // Apply max_backups limit
    if let Some(max_backups) = retention.max_backups {
        if sorted.len() > max_backups as usize {
            for entry in &sorted[max_backups as usize..] {
                if !to_prune.contains(&entry.id) {
                    info!(backup_id = %entry.id, "Marking for pruning (exceeds maxBackups)");
                    to_prune.push(entry.id.clone());
                }
            }
        }
    }

    // Apply max_age limit
    if let Some(max_age) = &retention.max_age {
        if let Some(duration) = parse_duration(max_age) {
            let cutoff = Utc::now() - duration;
            for entry in &sorted {
                if entry.start_time < cutoff && !to_prune.contains(&entry.id) {
                    info!(
                        backup_id = %entry.id,
                        start_time = %entry.start_time,
                        "Marking for pruning (exceeds maxAge)"
                    );
                    to_prune.push(entry.id.clone());
                }
            }
        } else {
            warn!(max_age = %max_age, "Failed to parse maxAge duration");
        }
    }

    to_prune
}

/// Parse a duration string like "30d", "720h", "4w"
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let value: i64 = num_str.parse().ok()?;

    match unit {
        "s" => Some(Duration::seconds(value)),
        "m" => Some(Duration::minutes(value)),
        "h" => Some(Duration::hours(value)),
        "d" => Some(Duration::days(value)),
        "w" => Some(Duration::weeks(value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::common::BackupStatus;
    use chrono::{Duration, Utc};

    fn make_entry(id: &str, days_ago: i64) -> BackupHistoryEntry {
        BackupHistoryEntry {
            id: id.to_string(),
            status: BackupStatus::Completed,
            start_time: Utc::now() - Duration::days(days_ago),
            completion_time: None,
            size_bytes: None,
            topics_backed_up: None,
            partitions_backed_up: None,
        }
    }

    #[test]
    fn test_prune_by_max_backups() {
        let history = vec![
            make_entry("backup-1", 5),
            make_entry("backup-2", 4),
            make_entry("backup-3", 3),
            make_entry("backup-4", 2),
            make_entry("backup-5", 1),
        ];

        let retention = RetentionSpec {
            max_backups: Some(3),
            max_age: None,
            prune_on_schedule: true,
        };

        let to_prune = evaluate_retention(&history, &retention);
        assert_eq!(to_prune.len(), 2);
        assert!(to_prune.contains(&"backup-1".to_string()));
        assert!(to_prune.contains(&"backup-2".to_string()));
    }

    #[test]
    fn test_prune_by_max_age() {
        let history = vec![
            make_entry("backup-old", 45),
            make_entry("backup-recent", 5),
            make_entry("backup-newest", 1),
        ];

        let retention = RetentionSpec {
            max_backups: None,
            max_age: Some("30d".to_string()),
            prune_on_schedule: true,
        };

        let to_prune = evaluate_retention(&history, &retention);
        assert_eq!(to_prune.len(), 1);
        assert!(to_prune.contains(&"backup-old".to_string()));
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30d"), Some(Duration::days(30)));
        assert_eq!(parse_duration("720h"), Some(Duration::hours(720)));
        assert_eq!(parse_duration("4w"), Some(Duration::weeks(4)));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("invalid"), None);
    }
}
