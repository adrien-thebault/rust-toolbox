//! The scheduler.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use toolbox_cluster::LockManager;
use tracing::{debug, error, info, warn};

use crate::{
    clock::{Clock, SystemClock},
    error::ScheduleError,
    job::{JobFuture, JobOutcome, Overlap, RunMode, ScheduledJob},
    trigger::Trigger,
};

/// How much longer than its timeout a job's lease is held.
///
/// The lease has to outlive the run, or a second replica takes the lock while
/// the first is still going.
const LEASE_MARGIN: Duration = Duration::from_secs(30);

/// The shortest an exclusive job's lease is held, whatever its timeout.
///
/// A lease that ends when the run does is not exclusion: the next replica to
/// tick takes the lock and runs the **same occurrence** again, so a job runs
/// once per replica instead of once. The lease must cover the occurrence
/// window, which is what ShedLock calls `lockAtLeastFor`.
const MIN_LEASE: Duration = Duration::from_secs(30);

/// A job's mutable state, kept per scheduler instance.
struct JobState {
    /// Set while an occurrence of this job is in flight.
    running: Arc<AtomicBool>,
    /// When this job is next due.
    next_at: DateTime<Utc>,
    /// When it last completed without error, if ever.
    last_success: Option<DateTime<Utc>>,
}

/// Runs registered jobs on a clock.
///
/// **A scheduler is not a job queue.** This is the clock; a queue is a
/// different thing, and they compose: "every night at 3am, email 500 people"
/// is one occurrence here that enqueues 500 jobs there.
pub struct Scheduler {
    /// Every registered job.
    jobs: Vec<ScheduledJob>,
    /// Per-job mutable state, by job name.
    state: HashMap<&'static str, JobState>,
    /// Takes the cluster-wide lease so an occurrence runs once.
    locks: Arc<dyn LockManager>,
    /// The time source the schedule is evaluated against.
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("jobs", &self.jobs.len())
            .finish_non_exhaustive()
    }
}

impl Scheduler {
    /// Start building over a lock manager.
    ///
    /// The lock manager is what makes `Exclusive` mean anything. Under
    /// `DEPLOYMENT=clustered` it must be a shared one, and the deployment guard
    /// is what checks that.
    ///
    /// # Arguments
    ///
    /// * `locks` - The lock manager. It is what makes [`RunMode::Exclusive`]
    ///   mean anything, so under `DEPLOYMENT=clustered` it must be a shared
    ///   adapter.
    #[must_use]
    pub fn builder(locks: Arc<dyn LockManager>) -> SchedulerBuilder {
        SchedulerBuilder {
            jobs: Vec::new(),
            locks,
            clock: Arc::new(SystemClock),
        }
    }
}

impl Scheduler {
    /// Log one line per job with its next fire time.
    ///
    /// Non-negotiable: "what is scheduled, and when does it next run?" must be
    /// answerable from the logs of a process that just started, without
    /// attaching anything to it.
    pub fn log_schedule(&self) {
        info!(jobs = self.jobs.len(), "scheduled jobs");
        for job in &self.jobs {
            let next = self.state.get(job.name).map(|s| s.next_at);
            info!(
                job = job.name,
                trigger = %job.trigger.describe(),
                mode = ?job.mode,
                overlap = ?job.overlap,
                timeout_s = job.timeout.as_secs(),
                next_run_at = ?next,
                "  job"
            );
        }
    }

    /// The registered jobs and their next fire times, for an admin endpoint.
    #[must_use]
    pub fn schedule(&self) -> Vec<JobSummary> {
        self.jobs
            .iter()
            .map(|job| {
                let state = self.state.get(job.name);
                JobSummary {
                    name: job.name,
                    trigger: job.trigger.describe(),
                    mode: job.mode,
                    overlap: job.overlap,
                    next_run_at: state.map(|s| s.next_at),
                    last_success_at: state.and_then(|s| s.last_success),
                }
            })
            .collect()
    }

    /// Run every job whose time has come, once.
    ///
    /// This is what a test drives, and what the run loop calls.
    ///
    /// # Errors
    /// [`ScheduleError`] when a trigger cannot produce its next occurrence.
    pub async fn tick_once(&mut self) -> Result<Vec<(&'static str, JobOutcome)>, ScheduleError> {
        let now = to_utc(self.clock.now());
        let mut outcomes = Vec::new();

        for index in 0..self.jobs.len() {
            let due = self
                .state
                .get(self.jobs[index].name)
                .is_some_and(|s| s.next_at <= now);
            if !due {
                continue;
            }
            let name = self.jobs[index].name;
            let outcome = self.run_job(index).await?;

            if let Some(state) = self.state.get_mut(name) {
                state.next_at = self.jobs[index].trigger.next_after(now)?;
                if outcome == JobOutcome::Succeeded {
                    state.last_success = Some(now);
                }
            }
            outcomes.push((name, outcome));
        }
        Ok(outcomes)
    }

    /// Run one job by name, whether or not it is due.
    ///
    /// What `POST /admin/jobs/{name}/run` calls. It pays for itself the first
    /// time you need to re-run last night's failed job without a deploy.
    ///
    /// # Arguments
    ///
    /// * `name` - The job to run. Unknown names are an error rather than a
    ///   silent no-op, because this is reached from an admin endpoint.
    ///
    /// # Errors
    /// [`ScheduleError::NotFound`] when there is no such job.
    pub async fn run_now(&mut self, name: &str) -> Result<JobOutcome, ScheduleError> {
        let index = self
            .jobs
            .iter()
            .position(|j| j.name == name)
            .ok_or_else(|| ScheduleError::NotFound(name.to_owned()))?;
        self.run_job(index).await
    }

    /// Run one registered job: take the lock if it is exclusive, apply the
    /// overlap policy, then time the body.
    ///
    /// # Arguments
    ///
    /// * `index` - Where the job sits in the registration order, which is how
    ///   its mutable state is addressed.
    async fn run_job(&mut self, index: usize) -> Result<JobOutcome, ScheduleError> {
        let now = to_utc(self.clock.now());
        // Hold the lock until the *next* occurrence is due, so no other
        // replica can run this one.
        let until_next = self.jobs[index]
            .trigger
            .next_after(now)
            .ok()
            .and_then(|next| (next - now).to_std().ok())
            .unwrap_or(MIN_LEASE);

        let job = &self.jobs[index];
        let name = job.name;

        let running = self.state.get(name).map_or_else(
            || Arc::new(AtomicBool::new(false)),
            |s| Arc::clone(&s.running),
        );

        // Overlap first: it is a local question, and asking the lock manager
        // about a job this replica is already running would be a round trip to
        // learn something we know.
        if job.overlap == Overlap::Skip && running.swap(true, Ordering::SeqCst) {
            warn!(
                job = name,
                "the previous run is still going; skipping this occurrence"
            );
            record(name, JobOutcome::Overlapped, Duration::ZERO);
            return Ok(JobOutcome::Overlapped);
        }

        // Long enough to outlive the run, and long enough to cover the window
        // until the next occurrence. The second is what makes it exclusion
        // rather than mutual exclusion during the run only.
        let lease = (job.timeout + LEASE_MARGIN).max(until_next).max(MIN_LEASE);
        let guard = match job.mode {
            RunMode::Local => None,
            RunMode::Exclusive => {
                let taken = self
                    .locks
                    .try_lock(&lock_key(name), lease)
                    .await
                    .map_err(|e| ScheduleError::Lock(e.to_string()))?;

                let Some(guard) = taken else {
                    // Not an error: another replica is doing it, which is the
                    // whole point.
                    debug!(job = name, "another replica holds this job's lock");
                    running.store(false, Ordering::SeqCst);
                    record(name, JobOutcome::Skipped, Duration::ZERO);
                    return Ok(JobOutcome::Skipped);
                };
                Some(guard)
            }
        };

        let body = Arc::clone(&job.body);
        let timeout = job.timeout;
        let started = std::time::Instant::now();

        let outcome = match tokio::time::timeout(timeout, body()).await {
            Ok(Ok(())) => JobOutcome::Succeeded,
            Ok(Err(e)) => {
                error!(job = name, error = %e, "a scheduled job failed");
                JobOutcome::Failed
            }
            Err(_) => {
                error!(
                    job = name,
                    timeout_s = timeout.as_secs(),
                    "a scheduled job timed out"
                );
                JobOutcome::TimedOut
            }
        };

        running.store(false, Ordering::SeqCst);

        // Held rather than released: the lease expiring is what lets the next
        // occurrence run, and releasing here would let another replica run
        // *this* one.
        if let Some(guard) = guard {
            guard.keep();
        }

        record(name, outcome, started.elapsed());
        Ok(outcome)
    }

    /// Run until cancelled, sleeping on the clock between ticks.
    ///
    /// # Arguments
    ///
    /// * `tick` - How often to wake and look for due jobs. It is the
    ///   granularity of every trigger, so a one-minute tick cannot fire a
    ///   per-second cron.
    ///
    /// # Errors
    /// [`ScheduleError`] from a trigger that cannot produce its next time.
    pub async fn run(&mut self, tick: Duration) -> Result<(), ScheduleError> {
        loop {
            self.tick_once().await?;
            self.clock.sleep(tick).await;
        }
    }
}

/// One job's state, for an admin endpoint or a startup log.
#[derive(Debug, Clone)]
pub struct JobSummary {
    /// The job's name.
    pub name: &'static str,
    /// A one-line description of its trigger.
    pub trigger: String,
    /// Where it may run.
    pub mode: RunMode,
    /// What it does about an overrun.
    pub overlap: Overlap,
    /// When it next fires, as this replica sees it.
    pub next_run_at: Option<DateTime<Utc>>,
    /// When it last succeeded here.
    pub last_success_at: Option<DateTime<Utc>>,
}

/// The lock key for a job.
///
/// Prefixed so a job called `migrations` cannot collide with anything else
/// taking a lock in the same store.
///
/// # Arguments
///
/// * `name` - The job name, prefixed so a job called `migrations` cannot
///   collide with anything else taking a lock in the same store.
#[must_use]
pub fn lock_key(name: &str) -> String {
    format!("toolbox:job:{name}")
}

/// Emit the three metrics that make a scheduled job observable.
///
/// The `metrics` crate is a facade: with no recorder installed these compile
/// to nothing, so this crate depends on no exporter and a binary that wants
/// Prometheus installs one in about fifteen lines.
///
/// # Arguments
///
/// * `name` - The job the metrics are labelled with.
/// * `outcome` - How the run ended, which becomes the counter's second label.
/// * `elapsed` - How long the body took, recorded as a histogram.
fn record(name: &'static str, outcome: JobOutcome, elapsed: Duration) {
    metrics::counter!("job_runs_total", "job" => name, "outcome" => outcome.as_label())
        .increment(1);

    if outcome.ran() {
        metrics::histogram!("job_duration_seconds", "job" => name).record(elapsed.as_secs_f64());
    }
    if outcome == JobOutcome::Succeeded {
        // The only metric that catches "the job silently stopped", which is the
        // characteristic scheduled-job failure. Alert on
        // time() - job_last_success_timestamp.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        metrics::gauge!("job_last_success_timestamp", "job" => name).set(now);
    }
}

/// A `SystemTime` from the clock as the `DateTime<Utc>` the triggers work
/// in.
///
/// # Arguments
///
/// * `t` - The instant the clock reported. A time before the epoch clamps
///   rather than panicking.
fn to_utc(t: std::time::SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(t)
}

/// Collects jobs before the scheduler starts.
pub struct SchedulerBuilder {
    /// The jobs collected so far.
    jobs: Vec<ScheduledJob>,
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
        self.jobs.push(ScheduledJob {
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
