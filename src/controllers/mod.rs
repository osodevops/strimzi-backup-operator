pub mod backup;
pub mod restore;

use std::time::Duration;

use futures::stream::{self, Stream, StreamExt};
use tracing::info;

/// Label selector for the Jobs and CronJobs the backup controller owns.
pub const OWNED_BACKUP_SELECTOR: &str = "kafkabackup.com/type=backup";
/// Label selector for the Jobs the restore controller owns.
pub const OWNED_RESTORE_SELECTOR: &str = "kafkabackup.com/type=restore";

/// Delays (each relative to the previous tick) after which every watched
/// resource is reconciled again following controller start-up: once at +5s
/// and once at +60s. This is the safety net for a write that lands after the
/// initial list-driven pass — e.g. the last apply of an operator pod that is
/// still draining during an upgrade (issue #62).
pub const STARTUP_RESYNC_DELAYS: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(55)];

/// Tunables for a controller run, so tests can shorten the timers.
#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    /// Post-start-up resync ticks, cumulative delays. Empty disables them.
    pub startup_resync_delays: &'static [Duration],
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            startup_resync_delays: &STARTUP_RESYNC_DELAYS,
        }
    }
}

/// A stream that yields `()` once per delay (each measured from the previous
/// tick) and then ends. Fed to `Controller::reconcile_all_on`.
pub fn startup_resync_ticks(
    delays: &'static [Duration],
) -> impl Stream<Item = ()> + Send + Sync + 'static {
    stream::iter(delays.iter().copied()).then(|delay| async move {
        tokio::time::sleep(delay).await;
        info!(after = ?delay, "Post-startup reconcile of all resources");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_resync_ticks_yields_once_per_delay_then_ends() {
        static DELAYS: [Duration; 2] = [Duration::from_millis(1), Duration::from_millis(1)];
        let ticks: Vec<()> = startup_resync_ticks(&DELAYS).collect().await;
        assert_eq!(ticks.len(), 2);

        static NONE: [Duration; 0] = [];
        let ticks: Vec<()> = startup_resync_ticks(&NONE).collect().await;
        assert!(ticks.is_empty());
    }

    #[test]
    fn default_run_options_use_the_documented_resync_schedule() {
        assert_eq!(
            RunOptions::default().startup_resync_delays,
            &STARTUP_RESYNC_DELAYS
        );
        let total: Duration = STARTUP_RESYNC_DELAYS.iter().sum();
        assert_eq!(total, Duration::from_secs(60));
    }
}
