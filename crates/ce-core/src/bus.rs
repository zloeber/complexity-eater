//! In-process fanout for workspace events.
//!
//! Publication uses `try_send` on a bounded channel. A slow subscriber sets a
//! lag flag and is discarded from the live sequence; the next receive reports
//! [`Delivery::Lagged`] and the caller reloads authoritative state. One
//! subscriber cannot block another, and publication does not wait on the
//! consumer. Dropping a [`Subscription`] removes it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::events::Event;

pub const SUBSCRIBER_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct EventFilter {
    types: Option<Vec<String>>,
}

impl EventFilter {
    pub fn all() -> Self {
        Self { types: None }
    }

    pub fn only(types: &[&str]) -> Self {
        Self {
            types: Some(types.iter().map(|item| (*item).to_string()).collect()),
        }
    }

    fn allows(&self, event_type: &str) -> bool {
        self.types
            .as_ref()
            .is_none_or(|types| types.iter().any(|item| item == event_type))
    }
}

#[derive(Debug)]
pub enum Delivery {
    Event(Event),
    Lagged,
}

struct Subscriber {
    id: u64,
    tx: SyncSender<Event>,
    lagged: Arc<AtomicBool>,
    filter: EventFilter,
}

struct BusInner {
    next_id: AtomicU64,
    subscribers: Mutex<Vec<Subscriber>>,
}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<BusInner>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BusInner {
                next_id: AtomicU64::new(1),
                subscribers: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn subscribe(&self, filter: EventFilter, capacity: usize) -> Subscription {
        let (tx, rx) = mpsc::sync_channel(capacity.max(1));
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let lagged = Arc::new(AtomicBool::new(false));
        self.inner
            .subscribers
            .lock()
            .expect("event bus")
            .push(Subscriber {
                id,
                tx,
                lagged: Arc::clone(&lagged),
                filter,
            });
        Subscription {
            id,
            rx,
            lagged,
            bus: Arc::clone(&self.inner),
        }
    }

    pub fn publish(&self, event: Event) {
        let mut subscribers = self.inner.subscribers.lock().expect("event bus");
        subscribers.retain(|subscriber| {
            if !subscriber.filter.allows(&event.event_type) {
                return true;
            }
            match subscriber.tx.try_send(event.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    subscriber.lagged.store(true, Ordering::Release);
                    true
                }
                Err(TrySendError::Disconnected(_)) => false,
            }
        });
    }

    pub fn subscriber_count(&self) -> usize {
        self.inner.subscribers.lock().expect("event bus").len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Subscription {
    id: u64,
    rx: Receiver<Event>,
    lagged: Arc<AtomicBool>,
    bus: Arc<BusInner>,
}

impl Subscription {
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Delivery, RecvTimeoutError> {
        if self.take_lag() {
            return Ok(Delivery::Lagged);
        }
        match self.rx.recv_timeout(timeout) {
            Ok(event) => {
                if self.take_lag() {
                    Ok(Delivery::Lagged)
                } else {
                    Ok(Delivery::Event(event))
                }
            }
            Err(err) => Err(err),
        }
    }

    fn take_lag(&self) -> bool {
        if self.lagged.swap(false, Ordering::AcqRel) {
            while self.rx.try_recv().is_ok() {}
            true
        } else {
            false
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut subscribers) = self.bus.subscribers.lock() {
            subscribers.retain(|subscriber| subscriber.id != self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use serde_json::json;

    use super::*;
    use crate::events::{compose_event, PROPOSAL_CREATED};

    fn sample(id: i64, event_type: &str) -> Event {
        compose_event(
            id,
            event_type,
            "2026-10-12T00:00:00.000000+00:00",
            json!({"workspace_id": "workspace_a", "proposal_id": format!("proposal_{id}")}),
        )
    }

    #[test]
    fn two_subscribers_each_receive_a_published_event() {
        let bus = EventBus::new();
        let first = bus.subscribe(EventFilter::all(), 4);
        let second = bus.subscribe(EventFilter::all(), 4);
        bus.publish(sample(1, PROPOSAL_CREATED));
        for sub in [&first, &second] {
            match sub.recv_timeout(Duration::from_secs(1)).unwrap() {
                Delivery::Event(event) => assert_eq!(event.event_id, 1),
                Delivery::Lagged => panic!("unexpected lag"),
            }
        }
    }

    #[test]
    fn a_filter_skips_unrelated_events() {
        let bus = EventBus::new();
        let sub = bus.subscribe(EventFilter::only(&[PROPOSAL_CREATED]), 4);
        bus.publish(sample(1, "lease.acquired"));
        bus.publish(sample(2, PROPOSAL_CREATED));
        match sub.recv_timeout(Duration::from_secs(1)).unwrap() {
            Delivery::Event(event) => assert_eq!(event.event_id, 2),
            Delivery::Lagged => panic!("unexpected lag"),
        }
    }

    #[test]
    fn a_full_subscriber_does_not_block_publication() {
        let bus = EventBus::new();
        let slow = bus.subscribe(EventFilter::all(), 1);
        let fast = bus.subscribe(EventFilter::all(), 64);
        bus.publish(sample(1, PROPOSAL_CREATED));
        let started = Instant::now();
        for id in 2..12 {
            bus.publish(sample(id, PROPOSAL_CREATED));
        }
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(matches!(
            slow.recv_timeout(Duration::from_millis(200)).unwrap(),
            Delivery::Lagged
        ));
        let mut seen = Vec::new();
        while let Ok(Delivery::Event(event)) = fast.recv_timeout(Duration::from_millis(50)) {
            seen.push(event.event_id);
        }
        assert!(seen.contains(&1));
        assert!(seen.len() > 1);
    }

    #[test]
    fn dropping_a_subscriber_removes_it() {
        let bus = EventBus::new();
        let sub = bus.subscribe(EventFilter::all(), 1);
        assert_eq!(bus.subscriber_count(), 1);
        drop(sub);
        assert_eq!(bus.subscriber_count(), 0);
        bus.publish(sample(1, PROPOSAL_CREATED));
        assert_eq!(bus.subscriber_count(), 0);
    }
}
