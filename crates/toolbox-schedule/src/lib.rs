//! Cluster-safe scheduled tasks.
//!
//! It encodes three defaults that are otherwise chosen by accident -
//! exactly-one-replica rather than every replica, skip rather than overlap on
//! an overrun, and a **mandatory** timeout with a leased lock.
//!
//! # A scheduler is not a job queue
//!
//! This is the clock. A queue is a different thing, and they compose: "every
//! night at 3am, email 500 people" is one occurrence here that enqueues 500
//! jobs there. By the time you need the queue you already have `OutboxBus`,
//! `LockManager` and this, and a `FOR UPDATE SKIP LOCKED` worker over the
//! outbox table shares all three.
//!
//! # What Spring actually gave you
//!
//! `@Scheduled` fires on **every** instance. If your experience of it felt
//! cluster-safe, that was ShedLock. So the translation here is not "find the
//! `@Scheduled` equivalent" - it is "build the thing Spring needed a second
//! library for", which is [`toolbox_cluster::LockManager`], already there.
//!
//! # One caveat about the injected clock
//!
//! [`Scheduler`] reads time from the [`clock::Clock`] port, but a lock
//! manager's leases are measured against the wall clock. Under
//! [`clock::ManualClock`] the two disagree: advancing the manual clock past an
//! occurrence does not age a held lease. That is correct in production, where
//! both are real, and it means a test using `ManualClock` should assert on
//! outcomes rather than on a lease having expired.
//!
//! # Its own crate, deliberately
//!
//! `Scheduler` composes the cluster traits; it is not one of them, and it has
//! exactly one implementation. What makes it a crate is feature unification: a
//! `schedule` feature on `toolbox-cluster` would put croner and the metrics
//! facade into every gateway in a mixed workspace, including ones that
//! schedule nothing.

pub mod clock;
pub mod error;
pub mod job;
pub mod scheduler;
pub mod trigger;

pub use clock::{Clock, ManualClock, SystemClock};
pub use error::ScheduleError;
pub use job::{JobFuture, JobOutcome, JobResult, Overlap, RunMode, ScheduledJob};
pub use scheduler::{JobSummary, Scheduler, SchedulerBuilder, lock_key};
pub use trigger::{Trigger, parse_cron};
