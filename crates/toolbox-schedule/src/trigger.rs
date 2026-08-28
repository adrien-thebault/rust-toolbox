//! When a job fires.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::error::ScheduleError;

/// When a job runs.
#[derive(Debug, Clone)]
pub enum Trigger {
    /// A cron expression, **in UTC**.
    ///
    /// Named timezones are deliberately not supported. A wall-clock schedule
    /// needs a policy for the spring-forward hour that does not exist and the
    /// autumn hour that happens twice, and that policy is never the one the
    /// caller expected. Write the expression in UTC and accept that a job
    /// pinned to local 3am drifts by an hour twice a year, or run two
    /// expressions.
    Cron {
        /// The expression. Five-field Unix or six-field Quartz, detected
        /// automatically.
        expr: String,
    },
    /// A fixed gap between the **end** of one run and the start of the next.
    ///
    /// Cannot overlap by construction, which is why it needs no `Overlap`.
    FixedDelay {
        /// The gap.
        delay: Duration,
        /// How long to wait before the first run.
        initial: Duration,
    },
    /// A fixed gap between the **starts** of successive runs.
    ///
    /// Can overlap if a run outlasts the period, which is what `Overlap`
    /// decides.
    FixedRate {
        /// The period.
        period: Duration,
        /// How long to wait before the first run.
        initial: Duration,
    },
}

impl Trigger {
    /// A cron trigger in UTC, validated now rather than at the first fire.
    ///
    /// # Arguments
    ///
    /// * `expr` - The expression, in UTC. Five fields are Unix, six are Quartz
    ///   with seconds, and which one it is is detected.
    ///
    /// # Errors
    /// [`ScheduleError::Cron`] when the expression does not parse.
    pub fn cron(expr: impl Into<String>) -> Result<Self, ScheduleError> {
        let expr = expr.into();
        // Parsed at registration so a typo is a startup failure rather than a
        // job that silently never runs.
        parse_cron(&expr)?;
        Ok(Self::Cron { expr })
    }

    /// Every `delay`, measured from the end of the previous run.
    ///
    /// # Arguments
    ///
    /// * `delay` - The gap between the end of one run and the start of the
    ///   next. It cannot overlap by construction.
    #[must_use]
    pub fn fixed_delay(delay: Duration) -> Self {
        Self::FixedDelay {
            delay,
            initial: delay,
        }
    }

    /// Every `period`, measured from the start of the previous run.
    ///
    /// # Arguments
    ///
    /// * `period` - The gap between two starts. A run that outlasts it is what
    ///   the overlap policy is for.
    #[must_use]
    pub fn fixed_rate(period: Duration) -> Self {
        Self::FixedRate {
            period,
            initial: period,
        }
    }

    /// Wait `initial` before the first run.
    ///
    /// # Arguments
    ///
    /// * `initial` - How long to wait before the first run, so a fleet
    ///   restarting together does not run every job at once.
    #[must_use]
    pub fn after(mut self, initial: Duration) -> Self {
        match &mut self {
            Self::FixedDelay { initial: i, .. } | Self::FixedRate { initial: i, .. } => {
                *i = initial;
            }
            Self::Cron { .. } => {}
        }
        self
    }

    /// The next instant this fires strictly after `after`.
    ///
    /// # Arguments
    ///
    /// * `after` - The instant to search from, exclusive. Passing the previous
    ///   fire is what stops a job firing twice for one occurrence.
    ///
    /// # Errors
    /// [`ScheduleError::Cron`] when the expression cannot produce another time.
    pub fn next_after(&self, after: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError> {
        match self {
            Self::Cron { expr } => next_cron(expr, after),
            Self::FixedDelay { delay, .. } | Self::FixedRate { period: delay, .. } => Ok(after
                + chrono::Duration::from_std(*delay)
                    .unwrap_or_else(|_| chrono::Duration::hours(1))),
        }
    }

    /// A one-line description for the schedule table logged at startup.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Cron { expr } => format!("cron `{expr}` (UTC)"),
            Self::FixedDelay { delay, .. } => format!("every {delay:?} after the previous run"),
            Self::FixedRate { period, .. } => format!("every {period:?}"),
        }
    }
}

/// Parse a cron expression, accepting five-field Unix and six-field Quartz.
///
/// Seconds are optional and detected, which is why croner is the crate here: a
/// project migrating from Spring writes six fields, one writing Unix cron
/// writes five, and both have to work.
///
/// # Arguments
///
/// * `expr` - The expression to parse, in UTC.
///
/// # Errors
/// [`ScheduleError::Cron`] when the expression does not parse.
pub fn parse_cron(expr: &str) -> Result<croner::Cron, ScheduleError> {
    croner::parser::CronParser::builder()
        .seconds(croner::parser::Seconds::Optional)
        .build()
        .parse(expr)
        .map_err(|e| ScheduleError::Cron {
            expr: expr.to_owned(),
            reason: e.to_string(),
        })
}

/// The next fire strictly after `after`, in UTC.
///
/// # Arguments
///
/// * `expr` - The cron expression.
/// * `after` - The instant to search from, exclusive. Passing the previous
///   fire is what stops one occurrence firing twice.
///
/// # Errors
/// [`ScheduleError::Cron`] when the expression cannot produce another time.
fn next_cron(expr: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError> {
    parse_cron(expr)?
        .find_next_occurrence(&after, false)
        .map_err(|e| ScheduleError::Cron {
            expr: expr.to_owned(),
            reason: e.to_string(),
        })
}
