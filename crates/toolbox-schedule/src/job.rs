//! What a job is, and how it is run.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use crate::trigger::Trigger;

/// What a job's body returns.
pub type JobResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// A job's body.
pub type JobFuture = Pin<Box<dyn Future<Output = JobResult> + Send>>;

/// Where a job may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    /// Exactly one replica runs each occurrence, decided by the lock manager.
    ///
    /// The default, and the thing Spring's `@Scheduled` does **not** do -
    /// `@Scheduled` fires on every instance, and anyone whose experience of it
    /// felt cluster-safe was using ShedLock.
    #[default]
    Exclusive,
    /// Every replica runs it. Correct for a cache refresh that is per-process
    /// by nature, wrong for anything that writes.
    Local,
}

/// What to do when an occurrence arrives while the previous run is still going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overlap {
    /// Do not start a second run.
    ///
    /// The default. A `FixedRate` job that occasionally overruns its period is
    /// how runs start corrupting each other.
    #[default]
    Skip,
    /// Start it anyway.
    Concurrent,
}

/// A registered job.
pub struct ScheduledJob {
    /// The name, unique within a scheduler, used for the lock key and metrics.
    pub name: &'static str,
    /// When it fires.
    pub trigger: Trigger,
    /// Where it may run.
    pub mode: RunMode,
    /// What to do about an overrun.
    pub overlap: Overlap,
    /// How long a run may take.
    ///
    /// **Mandatory.** A hung run holding an exclusive lease means the job
    /// silently never runs again anywhere, which is the characteristic
    /// scheduled-job failure and is invisible until somebody asks why the
    /// nightly report stopped.
    pub timeout: Duration,
    /// The body.
    pub body: Arc<dyn Fn() -> JobFuture + Send + Sync>,
}

impl std::fmt::Debug for ScheduledJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledJob")
            .field("name", &self.name)
            .field("trigger", &self.trigger.describe())
            .field("mode", &self.mode)
            .field("overlap", &self.overlap)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    /// It completed.
    Succeeded,
    /// Its body returned an error.
    Failed,
    /// It exceeded its timeout and was abandoned.
    TimedOut,
    /// Another replica held the lock.
    Skipped,
    /// The previous run was still going and `Overlap::Skip` is set.
    Overlapped,
}

impl JobOutcome {
    /// The label this outcome is counted under.
    #[must_use]
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
            Self::Overlapped => "overlapped",
        }
    }

    /// Whether the job actually ran here.
    #[must_use]
    pub fn ran(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::TimedOut)
    }
}
