//! The deployment guard.
//!
//! "does this adapter work on more than one replica?" is a question every
//! stateful component answers, and the answer is discovered in production
//! unless something asks at startup. This asks.

use std::collections::BTreeMap;

use tracing::warn;

/// How many replicas of this process are running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deployment {
    /// Exactly one. Local adapters are fine.
    Single,
    /// More than one, so anything holding state in process memory is either
    /// broken or degraded.
    Clustered {
        /// Identifies this replica in logs and lock ownership.
        instance_id: String,
    },
}

impl Deployment {
    /// Whether more than one replica is running.
    #[must_use]
    pub fn is_clustered(&self) -> bool {
        matches!(self, Self::Clustered { .. })
    }

    /// This replica's id, when clustered.
    #[must_use]
    pub fn instance_id(&self) -> Option<&str> {
        match self {
            Self::Clustered { instance_id } => Some(instance_id),
            Self::Single => None,
        }
    }
}

/// Whether an adapter's state is shared across replicas, and how badly it
/// fails when it is not.
///
/// Two severities rather than one, because not every local adapter is equally
/// wrong. An in-process event bus under three replicas means a subscriber
/// never sees two thirds of the events - that is broken. Per-process rate
/// limiting under three replicas means three times the intended allowance -
/// that is degraded. Refusing to boot for the second kind would make the guard
/// something people switch off, which costs you the cases where it is right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Safe on any number of replicas.
    Shared,
    /// Single-replica only; correctness breaks otherwise. Refuses to start.
    Local,
    /// Single-replica only, but the failure is degradation rather than
    /// breakage. Warns loudly and starts.
    LocalDegraded {
        /// What degrades, in one line, shown in the warning.
        note: &'static str,
    },
}

/// A stateful component that declares whether it can be replicated.
pub trait Adapter {
    /// The name shown in the startup error or warning.
    fn name(&self) -> &'static str;
    /// Whether its state is shared across replicas.
    fn scope(&self) -> Scope;
    /// The environment variable to change, and what to change it to.
    ///
    /// This is what turns "something is wrong" into "here is the fix", so it
    /// is worth filling in even when it feels obvious.
    fn remedy(&self) -> Option<&'static str> {
        None
    }
}

/// Why a process refused to start.
#[derive(Debug, thiserror::Error)]
#[error("{count} adapter(s) are single-replica only, but DEPLOYMENT=clustered:\n{detail}")]
pub struct DeploymentError {
    /// How many adapters failed.
    pub count: usize,
    /// One block per failing adapter, naming what breaks and what to change.
    pub detail: String,
    /// The failing adapter names, for a test or a metric.
    pub adapters: Vec<&'static str>,
}

/// Refuse to start when a `Local` adapter is running under `Clustered`, and
/// warn for every `LocalDegraded` one.
///
/// Called by `serve_http` and `serve_grpc`, which is what makes it impossible
/// to forget.
///
/// # Arguments
///
/// * `deployment` - How many replicas are running, which is the only thing that
///   makes a local adapter wrong.
/// * `adapters` - Every stateful component this process wired up. One `Local`
///   entry is enough to refuse the start.
///
/// # Errors
/// [`DeploymentError`] listing every adapter whose correctness breaks.
pub fn check_deployment(
    deployment: &Deployment,
    adapters: &[&dyn Adapter],
) -> Result<(), DeploymentError> {
    if !deployment.is_clustered() {
        return Ok(());
    }

    let mut broken: BTreeMap<&'static str, Option<&'static str>> = BTreeMap::new();
    for adapter in adapters {
        match adapter.scope() {
            Scope::Shared => {}
            Scope::LocalDegraded { note } => {
                warn!(
                    adapter = adapter.name(),
                    remedy = adapter.remedy().unwrap_or("none"),
                    "`{}` is per-process, but DEPLOYMENT=clustered: {note}",
                    adapter.name(),
                );
            }
            Scope::Local => {
                broken.insert(adapter.name(), adapter.remedy());
            }
        }
    }

    if broken.is_empty() {
        return Ok(());
    }

    let detail = broken
        .iter()
        .map(|(name, remedy)| {
            let remedy = remedy.unwrap_or("use an adapter whose state is shared across replicas");
            format!("  adapter `{name}` is single-replica only\n    --> {remedy}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    Err(DeploymentError {
        count: broken.len(),
        detail,
        adapters: broken.into_keys().collect(),
    })
}
