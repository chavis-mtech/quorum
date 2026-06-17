//! EventSink adapter — distributes LiveEvent via tokio broadcast (for WebSocket subscribers)

use tokio::sync::broadcast;

use crate::domain::models::LiveEvent;
use crate::domain::ports::EventSink;

#[derive(Clone)]
pub struct BroadcastSink {
    tx: broadcast::Sender<LiveEvent>,
}

impl BroadcastSink {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.tx.subscribe()
    }
}

impl EventSink for BroadcastSink {
    fn publish(&self, event: &LiveEvent) {
        // send failure (no subscribers) is harmless
        let _ = self.tx.send(event.clone());
    }
}
