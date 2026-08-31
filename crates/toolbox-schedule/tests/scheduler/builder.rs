use std::{sync::Arc, time::Duration};

use toolbox_cluster::InProcessLockManager;
use toolbox_schedule::{ScheduleError, Scheduler, Trigger};

/// Two jobs sharing a name would share a lock key and silently exclude each
/// other, which is the worst kind of bug: everything looks scheduled.
#[tokio::test]
async fn two_jobs_cannot_share_a_name() {
    let builder = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .job(
            "dup",
            Trigger::fixed_rate(Duration::from_secs(60)),
            Duration::from_secs(5),
            || Box::pin(async { Ok(()) }),
        )
        .unwrap();

    let err = builder
        .job(
            "dup",
            Trigger::fixed_rate(Duration::from_secs(60)),
            Duration::from_secs(5),
            || Box::pin(async { Ok(()) }),
        )
        .unwrap_err();
    assert!(matches!(err, ScheduleError::DuplicateName(_)));
}
