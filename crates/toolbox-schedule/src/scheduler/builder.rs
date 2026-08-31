//! Collecting jobs before the scheduler starts.

use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use toolbox_cluster::LockManager;

use super::{JobState, Scheduler, to_utc};
use crate::{
    clock::{Clock, SystemClock},
    error::ScheduleError,
    job::{Job, JobFuture, Overlap, RunMode},
    trigger::Trigger,
};

/// Collects jobs before the scheduler starts.
pub struct SchedulerBuilder {
    /// The jobs collected so far.
    jobs: Vec<Job>,
    /// The lock manager the scheduler will use.
    locks: Arc<dyn LockManager>,
    /// The clock the scheduler will use.
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for SchedulerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerBuilder")
            .field(
                "jobs",
                &self.jobs.iter().map(|j| j.name).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl SchedulerBuilder {
    /// Start with the system clock and no jobs.
    pub(super) fn new(locks: Arc<dyn LockManager>) -> Self {
        Self {
            jobs: Vec::new(),
            locks,
            clock: Arc::new(SystemClock),
        }
    }

    /// Drive the scheduler from a different clock.
    ///
    /// The reason `Clock` is a trait: with `ManualClock`, a test asserts a
    /// year of cron fires without taking a year, or a second.
    ///
    /// # Arguments
    ///
    /// * `clock` - The clock the scheduler reads and sleeps on. Every trigger
    ///   is evaluated against it, so a manual one drives the whole loop.
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Register a job.
    ///
    /// # Arguments
    ///
    /// * `name` - The job's name. It is also its lock key and its metric label,
    ///   so it must be unique and stable.
    /// * `trigger` - When it fires.
    /// * `timeout` - How long one run may take. Mandatory: it is also what
    ///   sizes the lock lease, so there is no correct default.
    /// * `body` - What to run. It is called once per occurrence and must be
    ///   safe to call again.
    ///
    /// # Errors
    /// [`ScheduleError::DuplicateName`] when the name is taken - two jobs
    /// sharing a name would share a lock key and silently exclude each other.
    pub fn job<F>(
        mut self,
        name: &'static str,
        trigger: Trigger,
        timeout: Duration,
        body: F,
    ) -> Result<Self, ScheduleError>
    where
        F: Fn() -> JobFuture + Send + Sync + 'static,
    {
        if self.jobs.iter().any(|j| j.name == name) {
            return Err(ScheduleError::DuplicateName(name.to_owned()));
        }
        self.jobs.push(Job {
            name,
            trigger,
            mode: RunMode::default(),
            overlap: Overlap::default(),
            timeout,
            body: Arc::new(body),
        });
        Ok(self)
    }

    /// Change the last-registered job's run mode.
    ///
    /// # Arguments
    ///
    /// * `mode` - Whether the job runs on every replica or on exactly one.
    #[must_use]
    pub fn mode(mut self, mode: RunMode) -> Self {
        if let Some(job) = self.jobs.last_mut() {
            job.mode = mode;
        }
        self
    }

    /// Change the last-registered job's overlap policy.
    ///
    /// # Arguments
    ///
    /// * `overlap` - What to do when an occurrence arrives while the previous
    ///   run is still going.
    #[must_use]
    pub fn overlap(mut self, overlap: Overlap) -> Self {
        if let Some(job) = self.jobs.last_mut() {
            job.overlap = overlap;
        }
        self
    }

    /// Build the scheduler and log the resolved schedule.
    ///
    /// # Errors
    /// [`ScheduleError`] when a trigger cannot produce a first occurrence.
    pub fn build(self) -> Result<Scheduler, ScheduleError> {
        let now = to_utc(self.clock.now());
        let mut state = HashMap::new();

        for job in &self.jobs {
            state.insert(
                job.name,
                JobState {
                    running: Arc::new(AtomicBool::new(false)),
                    next_at: job.trigger.next_after(now)?,
                    last_success: None,
                },
            );
        }

        let scheduler = Scheduler {
            jobs: self.jobs,
            state,
            locks: self.locks,
            clock: self.clock,
        };
        scheduler.log_schedule();
        Ok(scheduler)
    }
}
