use k8s_openapi::api::batch::v1::Job;

/// Aggregate state of the Jobs spawned for a backup/restore CR.
///
/// Derived from the full list of Jobs matching the CR's label selector so
/// the reconciler can decide whether another Job may be created. A completed
/// or failed Job must block re-creation just like a running one: backup and
/// restore runs are one-shot, and pod-level retries are owned by the Job's
/// `backoffLimit`, not the reconciler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobsState {
    /// No Jobs exist for this resource.
    NoJobs,
    /// At least one Job is running or has not reached a terminal state yet.
    InProgress,
    /// A Job completed successfully.
    Succeeded { job_name: String },
    /// A Job failed terminally (exhausted its backoffLimit).
    Failed { job_name: String },
}

/// Classify the Jobs belonging to one CR into an aggregate state.
///
/// Precedence: Succeeded > InProgress > Failed. A succeeded Job means the
/// operation completed even if a stray duplicate is still running; a failed
/// Job with a newer active run (manual re-trigger) reports running.
pub fn classify_jobs(jobs: &[Job]) -> JobsState {
    if let Some(job) = jobs.iter().find(|j| job_succeeded(j)) {
        return JobsState::Succeeded {
            job_name: job.metadata.name.clone().unwrap_or_default(),
        };
    }
    // Any non-terminal Job counts as in progress, including a just-created
    // Job whose status the Job controller has not populated yet.
    if jobs.iter().any(|j| !job_failed(j)) {
        return JobsState::InProgress;
    }
    if let Some(job) = jobs.iter().find(|j| job_failed(j)) {
        return JobsState::Failed {
            job_name: job.metadata.name.clone().unwrap_or_default(),
        };
    }
    JobsState::NoJobs
}

/// A Job succeeded if it reports a `Complete=True` condition or any
/// succeeded pods.
pub fn job_succeeded(job: &Job) -> bool {
    let Some(status) = job.status.as_ref() else {
        return false;
    };
    status.succeeded.unwrap_or(0) > 0 || has_condition(job, "Complete")
}

/// A Job failed terminally only when it reports a `Failed=True` condition
/// (backoffLimit exhausted or deadline exceeded). `status.failed > 0` alone
/// counts pod retries still owned by the Job controller and is not terminal.
pub fn job_failed(job: &Job) -> bool {
    has_condition(job, "Failed")
}

fn has_condition(job: &Job, condition_type: &str) -> bool {
    job.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .is_some_and(|conds| {
            conds
                .iter()
                .any(|c| c.type_ == condition_type && c.status == "True")
        })
}

/// Whether a restore Job may be created. Restores are strictly one-shot:
/// any existing Job — running, succeeded, or failed — suppresses creation.
pub fn should_create_restore_job(state: &JobsState) -> bool {
    matches!(state, JobsState::NoJobs)
}

/// Whether a one-shot backup Job may be created. A manual trigger
/// (`kafkabackup.com/trigger=now`) requests a fresh run, so it bypasses the
/// "a terminal Job already exists" gate — but never stacks onto an active Job.
pub fn should_create_backup_job(state: &JobsState, manually_triggered: bool) -> bool {
    match state {
        JobsState::InProgress => false,
        JobsState::NoJobs => true,
        JobsState::Succeeded { .. } | JobsState::Failed { .. } => manually_triggered,
    }
}
