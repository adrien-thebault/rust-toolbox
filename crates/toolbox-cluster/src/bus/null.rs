//! A bus that discards everything.

use async_trait::async_trait;

use super::{
    BusCapabilities, BusError, BusOrdering, Delivery, EventBus, EventStream, StartPosition, Topic,
};
use crate::{
    deployment::{Adapter, Scope},
    event::CloudEvent,
};

/// A bus that drops everything, for a process that publishes but has no
/// subscriber and no wish to run one.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullBus;

#[async_trait]
impl EventBus for NullBus {
    fn capabilities(&self) -> BusCapabilities {
        BusCapabilities {
            delivery: Delivery::AtMostOnce,
            replay: None,
            ordering: BusOrdering::None,
            max_payload: usize::MAX,
            durable: false,
        }
    }

    async fn publish(&self, _topic: &Topic, _event: CloudEvent) -> Result<(), BusError> {
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &Topic,
        _from: StartPosition,
    ) -> Result<EventStream, BusError> {
        Ok(Box::pin(tokio_stream::empty()))
    }
}

impl Adapter for NullBus {
    fn name(&self) -> &'static str {
        "NullBus"
    }

    fn scope(&self) -> Scope {
        // It has no state, so replicating it changes nothing.
        Scope::Shared
    }
}
