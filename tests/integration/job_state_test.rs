use k8s_openapi::api::batch::v1::{Job, JobCondition, JobStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kafka_backup_operator::jobs::job_state::{
    classify_jobs, should_create_backup_job, should_create_restore_job, JobsState,
};

fn job(name: &str, status: Option<JobStatus>) -> Job {
    Job {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        },
        spec: None,
        status,
    }
}

fn condition(type_: &str, status: &str) -> JobCondition {
    JobCondition {
        type_: type_.to_string(),
        status: status.to_string(),
        ..Default::default()
    }
}

fn active_job(name: &str) -> Job {
    job(
        name,
        Some(JobStatus {
            active: Some(1),
            ..Default::default()
        }),
    )
}

fn succeeded_job(name: &str) -> Job {
    job(
        name,
        Some(JobStatus {
            succeeded: Some(1),
            conditions: Some(vec![condition("Complete", "True")]),
            ..Default::default()
        }),
    )
}

fn failed_job(name: &str) -> Job {
    job(
        name,
        Some(JobStatus {
            failed: Some(4),
            conditions: Some(vec![condition("Failed", "True")]),
            ..Default::default()
        }),
    )
}

#[test]
fn test_no_jobs_allows_creation() {
    let state = classify_jobs(&[]);
    assert_eq!(state, JobsState::NoJobs);
    assert!(should_create_restore_job(&state));
    assert!(should_create_backup_job(&state, false));
}

#[test]
fn test_active_job_blocks_creation() {
    let state = classify_jobs(&[active_job("r-1")]);
    assert_eq!(state, JobsState::InProgress);
    assert!(!should_create_restore_job(&state));
    assert!(!should_create_backup_job(&state, false));
}

/// A Job that was just created has no meaningful status yet (the Job
/// controller has not set `active`). It must still count as in progress —
/// otherwise a reconcile racing Job-controller status propagation creates a
/// duplicate.
#[test]
fn test_pending_job_without_status_blocks_creation() {
    let state = classify_jobs(&[job("r-1", None)]);
    assert_eq!(state, JobsState::InProgress);
    assert!(!should_create_restore_job(&state));
}

#[test]
fn test_pending_job_with_empty_status_blocks_creation() {
    let state = classify_jobs(&[job("r-1", Some(JobStatus::default()))]);
    assert_eq!(state, JobsState::InProgress);
    assert!(!should_create_restore_job(&state));
}

/// Issue #29: a completed Job has `active=0`, which the operator used to read
/// as "no job running" — re-creating the restore Job on every requeue.
#[test]
fn test_succeeded_job_blocks_creation() {
    let state = classify_jobs(&[succeeded_job("r-1")]);
    assert_eq!(
        state,
        JobsState::Succeeded {
            job_name: "r-1".to_string()
        }
    );
    assert!(!should_create_restore_job(&state));
    assert!(!should_create_backup_job(&state, false));
}

/// `succeeded > 0` alone (no conditions yet) must already count as success.
#[test]
fn test_succeeded_count_without_conditions_blocks_creation() {
    let state = classify_jobs(&[job(
        "r-1",
        Some(JobStatus {
            succeeded: Some(1),
            ..Default::default()
        }),
    )]);
    assert_eq!(
        state,
        JobsState::Succeeded {
            job_name: "r-1".to_string()
        }
    );
    assert!(!should_create_restore_job(&state));
}

/// A terminally failed restore must not be silently re-run every requeue;
/// pod-level retries are owned by the Job's backoffLimit.
#[test]
fn test_failed_job_blocks_creation() {
    let state = classify_jobs(&[failed_job("r-1")]);
    assert_eq!(
        state,
        JobsState::Failed {
            job_name: "r-1".to_string()
        }
    );
    assert!(!should_create_restore_job(&state));
    assert!(!should_create_backup_job(&state, false));
}

/// `failed > 0` while the Job is still retrying within its backoffLimit (no
/// `Failed` condition) is not terminal.
#[test]
fn test_retrying_job_is_in_progress_not_failed() {
    let state = classify_jobs(&[job(
        "r-1",
        Some(JobStatus {
            failed: Some(2),
            active: Some(1),
            ..Default::default()
        }),
    )]);
    assert_eq!(state, JobsState::InProgress);
}

/// A stray duplicate may still be running next to an already-succeeded Job
/// (state left behind by pre-fix operator versions). Success wins: the
/// restore is complete.
#[test]
fn test_succeeded_takes_precedence_over_active() {
    let state = classify_jobs(&[succeeded_job("r-1"), active_job("r-2")]);
    assert_eq!(
        state,
        JobsState::Succeeded {
            job_name: "r-1".to_string()
        }
    );
    assert!(!should_create_restore_job(&state));
}

/// A failed Job plus a newer active run (manual re-trigger) reports running.
#[test]
fn test_active_takes_precedence_over_failed() {
    let state = classify_jobs(&[failed_job("b-1"), active_job("b-2")]);
    assert_eq!(state, JobsState::InProgress);
}

/// Manual trigger annotation requests a fresh backup run after a terminal
/// Job, but never stacks onto an active one.
#[test]
fn test_backup_trigger_allows_rerun_after_terminal_states() {
    let succeeded = classify_jobs(&[succeeded_job("b-1")]);
    assert!(should_create_backup_job(&succeeded, true));

    let failed = classify_jobs(&[failed_job("b-1")]);
    assert!(should_create_backup_job(&failed, true));

    let in_progress = classify_jobs(&[active_job("b-1")]);
    assert!(!should_create_backup_job(&in_progress, true));
}
