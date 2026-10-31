use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Cheap runtime diagnostics. Snapshots are observational and are not a
/// transactional consistency boundary across fields.
#[derive(Default)]
pub struct Metrics {
    mapping_count: AtomicUsize,
    events_seen: AtomicUsize,
    events_matched: AtomicUsize,
    queue_overflows: AtomicUsize,
    actions_emitted: AtomicUsize,
    injection_failures: AtomicUsize,
    hook_active: AtomicBool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetricsSnapshot {
    pub mapping_count: usize,
    pub events_seen: usize,
    pub events_matched: usize,
    pub queue_overflows: usize,
    pub actions_emitted: usize,
    pub injection_failures: usize,
    pub hook_active: bool,
}

impl Metrics {
    pub fn set_mapping_count(&self, count: usize) {
        self.mapping_count.store(count, Ordering::Relaxed);
    }

    pub fn record_event_seen(&self) {
        self.events_seen.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_event_matched(&self) {
        self.events_matched.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_queue_overflow(&self) {
        self.queue_overflows.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_action_emitted(&self) {
        self.actions_emitted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_injection_failure(&self) {
        self.injection_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_hook_active(&self, active: bool) {
        self.hook_active.store(active, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            mapping_count: self.mapping_count.load(Ordering::Relaxed),
            events_seen: self.events_seen.load(Ordering::Relaxed),
            events_matched: self.events_matched.load(Ordering::Relaxed),
            queue_overflows: self.queue_overflows.load(Ordering::Relaxed),
            actions_emitted: self.actions_emitted.load(Ordering::Relaxed),
            injection_failures: self.injection_failures.load(Ordering::Relaxed),
            hook_active: self.hook_active.load(Ordering::Relaxed),
        }
    }
}
