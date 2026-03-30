use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingEvent {
    pub timestamp: u64,
    pub job_id: String,
    pub provider_id: String,
    pub score: f64,
}

#[derive(Debug, Default)]
pub struct RouterMetrics {
    routed_jobs: u64,
    recent_events: VecDeque<RoutingEvent>,
    max_events: usize,
}

impl RouterMetrics {
    pub fn new(max_events: usize) -> Self {
        Self {
            routed_jobs: 0,
            recent_events: VecDeque::new(),
            max_events,
        }
    }

    pub fn record(&mut self, job_id: String, provider_id: String, score: f64) {
        self.routed_jobs += 1;
        let event = RoutingEvent {
            timestamp: now_unix_secs(),
            job_id,
            provider_id,
            score,
        };
        self.recent_events.push_back(event);
        while self.recent_events.len() > self.max_events {
            self.recent_events.pop_front();
        }
    }

    pub fn routed_jobs(&self) -> u64 {
        self.routed_jobs
    }

    pub fn recent_events(&self) -> &VecDeque<RoutingEvent> {
        &self.recent_events
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_event_buffer() {
        let mut metrics = RouterMetrics::new(2);
        metrics.record("job1".into(), "provider1".into(), 0.9);
        metrics.record("job2".into(), "provider2".into(), 0.8);
        metrics.record("job3".into(), "provider3".into(), 0.7);

        assert_eq!(metrics.routed_jobs(), 3);
        assert_eq!(metrics.recent_events().len(), 2);
        assert_eq!(metrics.recent_events().front().unwrap().job_id, "job2");
    }
}
