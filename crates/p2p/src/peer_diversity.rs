use std::collections::HashMap;
use std::net::IpAddr;

/// Enforces peer diversity to resist eclipse attacks.
///
/// Limits the number of peers per IP subnet to prevent an attacker
/// from filling all connection slots with nodes they control.
///
/// Rules:
/// - Max `max_per_subnet16` peers per /16 subnet (e.g., 10.1.*.*)
/// - Max `max_per_subnet8` peers per /8 subnet (e.g., 10.*.*.*)
/// - Reserve `outbound_only_slots` connection slots for outbound connections
pub struct PeerDiversityGuard {
    /// Count of peers per /16 subnet
    subnet16_counts: HashMap<u32, usize>,
    /// Count of peers per /8 subnet
    subnet8_counts: HashMap<u8, usize>,
    /// Max peers per /16 subnet
    max_per_subnet16: usize,
    /// Max peers per /8 subnet
    max_per_subnet8: usize,
    /// Total connected peers
    total_peers: usize,
    /// Max total peers
    max_peers: usize,
    /// Reserved outbound-only slots (attacker cannot fill these)
    outbound_only_slots: usize,
    /// Current inbound peer count
    inbound_count: usize,
}

impl PeerDiversityGuard {
    pub fn new(max_peers: usize) -> Self {
        PeerDiversityGuard {
            subnet16_counts: HashMap::new(),
            subnet8_counts: HashMap::new(),
            max_per_subnet16: 10,
            max_per_subnet8: 25,
            max_peers,
            total_peers: 0,
            outbound_only_slots: 8,
            inbound_count: 0,
        }
    }

    /// Check if a new inbound connection from this IP should be accepted.
    pub fn allow_inbound(&self, ip: IpAddr) -> bool {
        // Check total limit (minus reserved outbound slots)
        let max_inbound = self.max_peers.saturating_sub(self.outbound_only_slots);
        if self.inbound_count >= max_inbound {
            return false;
        }

        self.check_subnet_limits(ip)
    }

    /// Check if a new outbound connection to this IP should be accepted.
    pub fn allow_outbound(&self, ip: IpAddr) -> bool {
        if self.total_peers >= self.max_peers {
            return false;
        }

        self.check_subnet_limits(ip)
    }

    fn check_subnet_limits(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                let subnet16 = ((octets[0] as u32) << 8) | (octets[1] as u32);
                let subnet8 = octets[0];

                // Check /16 limit
                let count16 = self.subnet16_counts.get(&subnet16).copied().unwrap_or(0);
                if count16 >= self.max_per_subnet16 {
                    return false;
                }

                // Check /8 limit
                let count8 = self.subnet8_counts.get(&subnet8).copied().unwrap_or(0);
                if count8 >= self.max_per_subnet8 {
                    return false;
                }

                true
            }
            IpAddr::V6(_) => {
                // For IPv6, allow by default (subnet analysis is more complex)
                self.total_peers < self.max_peers
            }
        }
    }

    /// Record a new peer connection.
    pub fn on_peer_connected(&mut self, ip: IpAddr, is_inbound: bool) {
        self.total_peers += 1;
        if is_inbound {
            self.inbound_count += 1;
        }

        if let IpAddr::V4(ipv4) = ip {
            let octets = ipv4.octets();
            let subnet16 = ((octets[0] as u32) << 8) | (octets[1] as u32);
            let subnet8 = octets[0];

            *self.subnet16_counts.entry(subnet16).or_insert(0) += 1;
            *self.subnet8_counts.entry(subnet8).or_insert(0) += 1;
        }
    }

    /// Record a peer disconnection.
    pub fn on_peer_disconnected(&mut self, ip: IpAddr, is_inbound: bool) {
        self.total_peers = self.total_peers.saturating_sub(1);
        if is_inbound {
            self.inbound_count = self.inbound_count.saturating_sub(1);
        }

        if let IpAddr::V4(ipv4) = ip {
            let octets = ipv4.octets();
            let subnet16 = ((octets[0] as u32) << 8) | (octets[1] as u32);
            let subnet8 = octets[0];

            if let Some(count) = self.subnet16_counts.get_mut(&subnet16) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.subnet16_counts.remove(&subnet16);
                }
            }
            if let Some(count) = self.subnet8_counts.get_mut(&subnet8) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.subnet8_counts.remove(&subnet8);
                }
            }
        }
    }

    pub fn total_peers(&self) -> usize {
        self.total_peers
    }

    pub fn inbound_count(&self) -> usize {
        self.inbound_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_allows_diverse_peers() {
        let guard = PeerDiversityGuard::new(125);

        // Different /16 subnets — should all be allowed
        assert!(guard.allow_inbound(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 1))));
        assert!(guard.allow_inbound(IpAddr::V4(Ipv4Addr::new(10, 2, 0, 1))));
        assert!(guard.allow_inbound(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))));
    }

    #[test]
    fn test_blocks_same_subnet16_over_limit() {
        let mut guard = PeerDiversityGuard::new(125);

        // Fill up 10.1.0.0/16
        for i in 0..10u8 {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, i));
            assert!(guard.allow_inbound(ip));
            guard.on_peer_connected(ip, true);
        }

        // 11th from same /16 should be blocked
        let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 11));
        assert!(
            !guard.allow_inbound(ip),
            "should block 11th peer from same /16"
        );

        // Different /16 should still be allowed
        let ip = IpAddr::V4(Ipv4Addr::new(10, 2, 0, 1));
        assert!(guard.allow_inbound(ip));
    }

    #[test]
    fn test_outbound_slots_reserved() {
        let mut guard = PeerDiversityGuard::new(20);
        // max_peers=20, outbound_only_slots=8, so max_inbound=12

        // Fill 12 inbound peers from diverse subnets
        for i in 0..12u8 {
            let ip = IpAddr::V4(Ipv4Addr::new(i + 1, 1, 0, 1));
            assert!(guard.allow_inbound(ip));
            guard.on_peer_connected(ip, true);
        }

        // 13th inbound should be blocked (reserved for outbound)
        let ip = IpAddr::V4(Ipv4Addr::new(100, 1, 0, 1));
        assert!(
            !guard.allow_inbound(ip),
            "should block inbound when outbound slots reserved"
        );

        // But outbound should still be allowed
        assert!(guard.allow_outbound(ip), "outbound should still be allowed");
    }

    #[test]
    fn test_disconnect_frees_slot() {
        let mut guard = PeerDiversityGuard::new(125);

        // Fill /16 to limit
        for i in 0..10u8 {
            let peer_ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, i));
            guard.on_peer_connected(peer_ip, true);
        }

        assert!(!guard.allow_inbound(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 99))));

        // Disconnect one
        guard.on_peer_disconnected(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0)), true);

        // Now should be allowed again
        assert!(guard.allow_inbound(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 99))));
    }

    #[test]
    fn test_connect_disconnect_invariant() {
        let mut guard = PeerDiversityGuard::new(125);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 1));
        guard.on_peer_connected(ip, true);
        assert_eq!(guard.total_peers(), 1);
        assert_eq!(guard.inbound_count(), 1);
        guard.on_peer_disconnected(ip, true);
        assert_eq!(guard.total_peers(), 0);
        assert_eq!(guard.inbound_count(), 0);
    }

    #[test]
    fn test_subnet8_limit() {
        let mut guard = PeerDiversityGuard::new(200);

        // Fill 25 peers across different /16s but same /8 (10.*)
        for i in 0..25u8 {
            let ip = IpAddr::V4(Ipv4Addr::new(10, i, 0, 1));
            guard.on_peer_connected(ip, true);
        }

        // 26th from same /8 should be blocked
        let ip = IpAddr::V4(Ipv4Addr::new(10, 100, 0, 1));
        assert!(
            !guard.allow_inbound(ip),
            "should block when /8 limit reached"
        );

        // Different /8 should work
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1));
        assert!(guard.allow_inbound(ip));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::net::{IpAddr, Ipv4Addr};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn total_peers_never_exceeds_max(max_peers in 10usize..50, num_connects in 0usize..100) {
            let mut guard = PeerDiversityGuard::new(max_peers);
            for i in 0..num_connects {
                // Use diverse IPs to avoid subnet limits
                let a = ((i / 256) % 256) as u8;
                let b = (i % 256) as u8;
                let ip = IpAddr::V4(Ipv4Addr::new(a.wrapping_add(1), b.wrapping_add(1), 0, 1));
                if guard.allow_outbound(ip) {
                    guard.on_peer_connected(ip, false);
                }
            }
            prop_assert!(guard.total_peers() <= max_peers);
        }

        #[test]
        fn inbound_respects_outbound_reservation(max_peers in 20usize..60, num_connects in 0usize..100) {
            let mut guard = PeerDiversityGuard::new(max_peers);
            let max_inbound = max_peers.saturating_sub(8); // outbound_only_slots = 8
            for i in 0..num_connects {
                let a = ((i / 256) % 256) as u8;
                let b = (i % 256) as u8;
                let ip = IpAddr::V4(Ipv4Addr::new(a.wrapping_add(1), b.wrapping_add(1), 0, 1));
                if guard.allow_inbound(ip) {
                    guard.on_peer_connected(ip, true);
                }
            }
            prop_assert!(guard.inbound_count() <= max_inbound);
        }

        #[test]
        fn connect_disconnect_conserves_count(connects in 1usize..30, disconnects_ratio in 0.0f64..1.0) {
            let mut guard = PeerDiversityGuard::new(200);
            let mut ips = Vec::new();
            for i in 0..connects {
                let ip = IpAddr::V4(Ipv4Addr::new(((i+1) % 255) as u8, ((i/255 + 1) % 255) as u8, 0, 1));
                guard.on_peer_connected(ip, true);
                ips.push(ip);
            }
            prop_assert_eq!(guard.total_peers(), connects);

            let num_disc = (connects as f64 * disconnects_ratio) as usize;
            for ip in ips.iter().take(num_disc) {
                guard.on_peer_disconnected(*ip, true);
            }
            prop_assert_eq!(guard.total_peers(), connects - num_disc);
            prop_assert_eq!(guard.inbound_count(), connects - num_disc);
        }

        #[test]
        fn subnet16_limit_enforced(num_from_same in 0usize..20) {
            let guard_max = 200;
            let mut guard = PeerDiversityGuard::new(guard_max);
            // All from 10.1.0.0/16
            let mut accepted = 0usize;
            for i in 0..num_from_same {
                let ip = IpAddr::V4(Ipv4Addr::new(10, 1, (i / 256) as u8, (i % 256) as u8));
                if guard.allow_inbound(ip) {
                    guard.on_peer_connected(ip, true);
                    accepted += 1;
                }
            }
            prop_assert!(accepted <= 10, "max_per_subnet16 is 10, got {}", accepted);
        }

        #[test]
        fn ipv6_allowed_within_total_limit(num_peers in 0usize..30) {
            let max = 20usize;
            let mut guard = PeerDiversityGuard::new(max);
            let max_inbound = max.saturating_sub(8);
            let mut accepted = 0;
            for i in 0..num_peers {
                let ip = IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, i as u16));
                if guard.allow_inbound(ip) {
                    guard.on_peer_connected(ip, true);
                    accepted += 1;
                }
            }
            prop_assert!(accepted <= max_inbound);
        }

        #[test]
        fn disconnect_never_underflows(connects in 0usize..10, extra_disconnects in 0usize..5) {
            let mut guard = PeerDiversityGuard::new(200);
            let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 1));
            for _ in 0..connects {
                guard.on_peer_connected(ip, true);
            }
            for _ in 0..(connects + extra_disconnects) {
                guard.on_peer_disconnected(ip, true);
            }
            // saturating_sub should prevent underflow
            prop_assert_eq!(guard.total_peers(), 0);
            prop_assert_eq!(guard.inbound_count(), 0);
        }
    }
}
