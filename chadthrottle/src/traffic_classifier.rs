//! Traffic categorization utilities (internet vs local network)
//!
//! This module provides cross-platform IP address classification.
//! All monitor backends use this to categorize traffic consistently.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Traffic category - Internet vs Local network
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficCategory {
    /// Internet/WAN traffic (public IPs)
    Internet,
    /// Local/LAN traffic (private IPs, loopback, link-local)
    Local,
}

/// Determines if an IP address represents local/private network traffic
///
/// Local traffic includes:
/// - IPv4: RFC 1918 private ranges, loopback, link-local, etc.
/// - IPv6: Loopback, link-local, unique local addresses
pub fn is_local_traffic(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_local_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_local_ipv6(ipv6),
    }
}

fn is_local_ipv4(ip: &Ipv4Addr) -> bool {
    ip.is_private()           // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        || ip.is_loopback()   // 127.0.0.0/8
        || ip.is_link_local() // 169.254.0.0/16
        || ip.is_broadcast()  // 255.255.255.255
        || ip.is_documentation() // Test networks
        || ip.is_unspecified() // 0.0.0.0
}

fn is_local_ipv6(ip: &Ipv6Addr) -> bool {
    ip.is_loopback()                  // ::1
        || ip.is_unicast_link_local() // fe80::/10
        || ip.is_unspecified()        // ::
        || is_unique_local_ipv6(ip) // fc00::/7
}

/// Check if IPv6 address is in Unique Local Address range (fc00::/7)
/// This is IPv6's equivalent to RFC 1918 private addresses
fn is_unique_local_ipv6(ip: &Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

/// Categorize traffic based on remote IP address
///
/// This is the main entry point for monitor backends to use.
pub fn categorize_traffic(remote_ip: &IpAddr) -> TrafficCategory {
    if is_local_traffic(remote_ip) {
        TrafficCategory::Local
    } else {
        TrafficCategory::Internet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_private() {
        assert_eq!(
            categorize_traffic(&"192.168.1.1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"10.0.0.1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"172.16.0.1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"172.31.255.255".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv4_loopback() {
        assert_eq!(
            categorize_traffic(&"127.0.0.1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"127.255.255.255".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv4_link_local() {
        assert_eq!(
            categorize_traffic(&"169.254.1.1".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv4_internet() {
        assert_eq!(
            categorize_traffic(&"8.8.8.8".parse().unwrap()),
            TrafficCategory::Internet
        );
        assert_eq!(
            categorize_traffic(&"1.1.1.1".parse().unwrap()),
            TrafficCategory::Internet
        );
        assert_eq!(
            categorize_traffic(&"140.82.112.4".parse().unwrap()),
            TrafficCategory::Internet
        );
    }

    #[test]
    fn test_ipv6_loopback() {
        assert_eq!(
            categorize_traffic(&"::1".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv6_link_local() {
        assert_eq!(
            categorize_traffic(&"fe80::1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"fe80::1cd4:a0ff:fed4:aa2a".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv6_unique_local() {
        assert_eq!(
            categorize_traffic(&"fc00::1".parse().unwrap()),
            TrafficCategory::Local
        );
        assert_eq!(
            categorize_traffic(&"fd00::1".parse().unwrap()),
            TrafficCategory::Local
        );
    }

    #[test]
    fn test_ipv6_internet() {
        assert_eq!(
            categorize_traffic(&"2001:4860:4860::8888".parse().unwrap()),
            TrafficCategory::Internet
        );
        assert_eq!(
            categorize_traffic(&"2606:4700:4700::1111".parse().unwrap()),
            TrafficCategory::Internet
        );
    }
}
