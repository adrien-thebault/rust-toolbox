//! Log and trace initialisation.
//!
//! `tracing_subscriber::fmt()` is one line, but JSON output, per-module
//! filtering and a `-v`/`-q` pair are the three things you need on the first
//! day you ship, and together they are the >10 lines of setup that the first
//! question asks for. Invoked identically in every binary.

use std::str::FromStr;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

/// How log lines are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable, multi-line. For a terminal.
    #[default]
    Pretty,
    /// Human-readable, one line per event.
    Compact,
    /// One JSON object per event. For a log shipper.
    Json,
}

impl FromStr for LogFormat {
    type Err = TelemetryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "compact" => Ok(Self::Compact),
            "json" => Ok(Self::Json),
            other => Err(TelemetryError::UnknownFormat(other.to_owned())),
        }
    }
}

/// Why telemetry could not be initialised.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TelemetryError {
    /// `LOG_FORMAT` named something that does not exist.
    #[error("unknown log format `{0}`; expected pretty, compact or json")]
    UnknownFormat(String),
    /// The filter directive did not parse.
    #[error("invalid log filter: {0}")]
    Filter(String),
    /// A subscriber was already installed.
    #[error("telemetry was already initialised")]
    AlreadyInitialised,
}

/// The standard logging arguments, identical in every binary.
#[cfg(feature = "clap")]
#[derive(Debug, Clone, clap::Args)]
pub struct TelemetryArgs {
    /// Raise the log level. Repeat for more: `-v` is debug, `-vv` is trace.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Lower the log level. Repeat for less: `-q` is warn, `-qq` is error.
    #[arg(short = 'q', long = "quiet", action = clap::ArgAction::Count, global = true)]
    pub quiet: u8,

    /// How log lines are rendered.
    #[arg(long, env = "LOG_FORMAT", default_value = "pretty")]
    pub log_format: String,

    /// A `tracing` filter directive, overriding `-v`/`-q` entirely.
    #[arg(long, env = "RUST_LOG")]
    pub log_filter: Option<String>,
}

#[cfg(feature = "clap")]
impl TelemetryArgs {
    /// The level `-v`/`-q` select, before `log_filter` overrides it.
    #[must_use]
    pub fn level(&self) -> tracing::Level {
        match i16::from(self.verbose) - i16::from(self.quiet) {
            i16::MIN..=-2 => tracing::Level::ERROR,
            -1 => tracing::Level::WARN,
            0 => tracing::Level::INFO,
            1 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE,
        }
    }

    /// Install the subscriber.
    ///
    /// # Errors
    /// [`TelemetryError`] when the format or filter is invalid, or a
    /// subscriber was already installed.
    pub fn init(&self) -> Result<TelemetryGuard, TelemetryError> {
        let format: LogFormat = self.log_format.parse()?;
        let filter = match &self.log_filter {
            Some(directive) => {
                EnvFilter::try_new(directive).map_err(|e| TelemetryError::Filter(e.to_string()))?
            }
            None => EnvFilter::new(self.level().to_string().to_ascii_lowercase()),
        };
        init(format, filter)
    }
}

/// Install a subscriber directly, for a binary that does not use clap.
///
/// # Arguments
///
/// * `format` - How to render lines: readable at a terminal, JSON for a
///   collector.
/// * `filter` - The per-module level filter, usually built from `RUST_LOG`.
///
/// # Errors
/// [`TelemetryError::AlreadyInitialised`] when one is already installed.
pub fn init(format: LogFormat, filter: EnvFilter) -> Result<TelemetryGuard, TelemetryError> {
    let registry = tracing_subscriber::registry().with(filter);
    let installed = match format {
        LogFormat::Pretty => registry
            .with(tracing_subscriber::fmt::layer().pretty())
            .try_init(),
        LogFormat::Compact => registry
            .with(tracing_subscriber::fmt::layer().compact())
            .try_init(),
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .try_init(),
    };
    installed.map_err(|_| TelemetryError::AlreadyInitialised)?;
    Ok(TelemetryGuard { _private: () })
}

/// Held for the process's lifetime. Dropping it flushes any exporter.
#[derive(Debug)]
pub struct TelemetryGuard {
    /// Blocks construction outside this module.
    _private: (),
}
