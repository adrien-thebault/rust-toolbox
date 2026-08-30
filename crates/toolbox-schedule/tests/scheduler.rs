use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use toolbox_cluster::{InProcessLockManager, LockManager};
use toolbox_schedule::{
    JobOutcome, ManualClock, Overlap, RunMode, ScheduleError, Scheduler, Trigger, lock_key,
};

/// A job that counts how many times it ran.
fn counting(counter: Arc<AtomicUsize>) -> impl Fn() -> toolbox_schedule::JobFuture + Send + Sync {
    move || {
        let counter = Arc::clone(&counter);
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// **The test that keeps `Exclusive` honest.** Three schedulers sharing one
/// lock manager, each ticked once: the job runs exactly once, not three times.
#[tokio::test]
async fn three_schedulers_sharing_a_lock_manager_run_a_job_exactly_once() {
    let locks: Arc<dyn LockManager> = Arc::new(InProcessLockManager::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ManualClock::new());

    let mut schedulers = Vec::new();
    for _ in 0..3 {
        let scheduler = Scheduler::builder(Arc::clone(&locks))
            .clock(clock.clone())
            .job(
                "nightly",
                Trigger::fixed_rate(Duration::from_secs(60)),
                Duration::from_secs(5),
                counting(Arc::clone(&counter)),
            )
            .unwrap()
            .build()
            .unwrap();
        schedulers.push(scheduler);
    }

    // Move past the first occurrence, then tick every replica.
    clock.advance(Duration::from_secs(61));
    let mut ran = 0;
    let mut skipped = 0;
    for scheduler in &mut schedulers {
        for (_, outcome) in scheduler.tick_once().await.unwrap() {
            match outcome {
                JobOutcome::Succeeded => ran += 1,
                JobOutcome::Skipped => skipped += 1,
                other => panic!("unexpected outcome {other:?}"),
            }
        }
    }

    assert_eq!(counter.load(Ordering::SeqCst), 1, "the job body ran once");
    assert_eq!(ran, 1, "one replica won");
    assert_eq!(skipped, 2, "the other two stood down without erroring");
}

/// Local is for work that is per-process by nature - a cache refresh - and is
/// wrong for anything that writes.
#[tokio::test]
async fn a_local_job_runs_on_every_replica() {
    let locks: Arc<dyn LockManager> = Arc::new(InProcessLockManager::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ManualClock::new());

    let mut schedulers = Vec::new();
    for _ in 0..3 {
        schedulers.push(
            Scheduler::builder(Arc::clone(&locks))
                .clock(clock.clone())
                .job(
                    "refresh-cache",
                    Trigger::fixed_rate(Duration::from_secs(60)),
                    Duration::from_secs(5),
                    counting(Arc::clone(&counter)),
                )
                .unwrap()
                .mode(RunMode::Local)
                .build()
                .unwrap(),
        );
    }

    clock.advance(Duration::from_secs(61));
    for scheduler in &mut schedulers {
        scheduler.tick_once().await.unwrap();
    }
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_job_does_not_run_before_it_is_due() {
    let counter = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ManualClock::new());
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(clock.clone())
        .job(
            "later",
            Trigger::fixed_rate(Duration::from_secs(3600)),
            Duration::from_secs(5),
            counting(Arc::clone(&counter)),
        )
        .unwrap()
        .build()
        .unwrap();

    clock.advance(Duration::from_secs(60));
    assert!(scheduler.tick_once().await.unwrap().is_empty());
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    clock.advance(Duration::from_secs(3600));
    assert_eq!(scheduler.tick_once().await.unwrap().len(), 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

/// A hung run holding an exclusive lease means the job silently never runs
/// again anywhere. The timeout is mandatory for exactly that reason.
#[tokio::test(start_paused = true)]
async fn a_job_that_overruns_its_timeout_is_abandoned() {
    let clock = Arc::new(ManualClock::new());
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(clock.clone())
        .job(
            "hangs",
            Trigger::fixed_rate(Duration::from_secs(60)),
            Duration::from_millis(50),
            || {
                Box::pin(async {
                    std::future::pending::<()>().await;
                    Ok(())
                })
            },
        )
        .unwrap()
        .build()
        .unwrap();

    clock.advance(Duration::from_secs(61));
    let outcomes = scheduler.tick_once().await.unwrap();
    assert_eq!(outcomes, [("hangs", JobOutcome::TimedOut)]);
}

#[tokio::test]
async fn a_failing_job_is_reported_rather_than_swallowed() {
    let clock = Arc::new(ManualClock::new());
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(clock.clone())
        .job(
            "fails",
            Trigger::fixed_rate(Duration::from_secs(60)),
            Duration::from_secs(5),
            || Box::pin(async { Err("nope".into()) }),
        )
        .unwrap()
        .build()
        .unwrap();

    clock.advance(Duration::from_secs(61));
    assert_eq!(
        scheduler.tick_once().await.unwrap(),
        [("fails", JobOutcome::Failed)]
    );
}

/// A `FixedRate` job that occasionally overruns its period is how runs start
/// corrupting each other, so skipping is the default.
///
/// Uses `RunMode::Local` on purpose: overlap is a *local* question, answered
/// by the running flag before the lock manager is consulted at all, and an
/// exclusive job's held lease would mask it.
#[tokio::test]
async fn an_overrunning_job_does_not_start_a_second_run_by_default() {
    let clock = Arc::new(ManualClock::new());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let started = Arc::new(AtomicUsize::new(0));

    let (g, s) = (Arc::clone(&gate), Arc::clone(&started));
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(clock.clone())
        .job(
            "slow",
            Trigger::fixed_rate(Duration::from_secs(60)),
            Duration::from_secs(30),
            move || {
                let (g, s) = (Arc::clone(&g), Arc::clone(&s));
                Box::pin(async move {
                    s.fetch_add(1, Ordering::SeqCst);
                    let _ = g.acquire().await;
                    Ok(())
                })
            },
        )
        .unwrap()
        .mode(RunMode::Local)
        .overlap(Overlap::Skip)
        .build()
        .unwrap();

    // The first run blocks on the gate, held open across the second tick.
    clock.advance(Duration::from_secs(61));
    let first = tokio::spawn({
        // Ticking in a task so the blocked body does not block the test.
        async move {
            let outcomes = scheduler.tick_once().await.unwrap();
            (scheduler, outcomes)
        }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    gate.add_permits(1);
    let (mut scheduler, outcomes) = first.await.unwrap();
    assert_eq!(outcomes, [("slow", JobOutcome::Succeeded)]);

    // And a second occurrence after it finished runs normally.
    clock.advance(Duration::from_secs(61));
    gate.add_permits(1);
    assert_eq!(
        scheduler.tick_once().await.unwrap(),
        [("slow", JobOutcome::Succeeded)]
    );
    assert_eq!(started.load(Ordering::SeqCst), 2);
}

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

/// Pays for itself the first time you need to re-run last night's failed job
/// without a deploy.
#[tokio::test]
async fn a_job_can_be_run_on_demand_even_when_not_due() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(Arc::new(ManualClock::new()))
        .job(
            "nightly",
            Trigger::cron("0 3 * * *").unwrap(),
            Duration::from_secs(5),
            counting(Arc::clone(&counter)),
        )
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        scheduler.run_now("nightly").await.unwrap(),
        JobOutcome::Succeeded
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn running_an_unknown_job_names_it() {
    let mut scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(Arc::new(ManualClock::new()))
        .build()
        .unwrap();
    let err = scheduler.run_now("nope").await.unwrap_err();
    assert!(matches!(err, ScheduleError::NotFound(_)));
    assert!(err.to_string().contains("nope"));
}

/// "What is scheduled, and when does it next run?" must be answerable from the
/// logs of a process that just started.
#[tokio::test]
async fn the_schedule_is_inspectable() {
    let scheduler = Scheduler::builder(Arc::new(InProcessLockManager::new()))
        .clock(Arc::new(ManualClock::new()))
        .job(
            "nightly",
            Trigger::cron("0 3 * * *").unwrap(),
            Duration::from_secs(300),
            || Box::pin(async { Ok(()) }),
        )
        .unwrap()
        .build()
        .unwrap();

    let summary = scheduler.schedule();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].name, "nightly");
    assert!(summary[0].trigger.contains("0 3 * * *"));
    assert_eq!(summary[0].mode, RunMode::Exclusive, "exclusive by default");
    assert_eq!(summary[0].overlap, Overlap::Skip, "skip by default");
    assert!(summary[0].next_run_at.is_some());
}

/// Prefixed so a job called `migrations` cannot collide with anything else
/// taking a lock in the same store.
#[test]
fn lock_keys_are_namespaced() {
    assert_eq!(lock_key("nightly"), "toolbox:job:nightly");
    assert_ne!(lock_key("migrations"), "toolbox_migrations");
}

/// The lease is held past the end of the run, so the next replica to tick
/// cannot redo the same occurrence. Releasing on completion is what made the
/// three-scheduler test fail with a count of three.
#[tokio::test]
async fn an_exclusive_lease_outlives_the_run_it_guarded() {
    let locks: Arc<dyn LockManager> = Arc::new(InProcessLockManager::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ManualClock::new());

    let mut scheduler = Scheduler::builder(Arc::clone(&locks))
        .clock(clock.clone())
        .job(
            "nightly",
            Trigger::fixed_rate(Duration::from_secs(3600)),
            Duration::from_secs(5),
            counting(Arc::clone(&counter)),
        )
        .unwrap()
        .build()
        .unwrap();

    clock.advance(Duration::from_secs(3601));
    scheduler.tick_once().await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // The lock is still held even though the run finished.
    assert!(
        locks
            .try_lock(&lock_key("nightly"), Duration::from_secs(1))
            .await
            .unwrap()
            .is_none(),
        "the lease covers the occurrence window, not just the run"
    );
}
