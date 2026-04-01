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

        let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 1));

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
