use once_cell::sync::Lazy;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};

pub struct P2PMetrics {
    pub peer_count: IntCounter,
    pub message_rate: Histogram,
    pub bandwidth_bytes: Histogram,
}

impl P2PMetrics {
    fn new() -> Self {
        P2PMetrics {
            peer_count: register_int_counter!(
                "aether_p2p_peer_updates",
                "Number of peer table updates"
            )
            .expect("register peer_count"),
            message_rate: register_histogram!(
                "aether_p2p_message_rate",
                "Messages per second observed"
            )
            .expect("register message_rate"),
            bandwidth_bytes: register_histogram!(
                "aether_p2p_bandwidth_bytes",
                "Bandwidth utilisation in bytes"
            )
            .expect("register bandwidth"),
        }
    }
}

pub static P2P_METRICS: Lazy<P2PMetrics> = Lazy::new(P2PMetrics::new);
