//! Address classification shared by the cgroup-SKB programs and host tests.
//!
//! Cgroup-SKB data starts at the IP header, not an Ethernet header. Classify the
//! remote endpoint: source on ingress, destination on egress. "Local" means
//! private, loopback, link-local, unspecified, or IPv4 limited-broadcast space;
//! it is not a routing-table or physical-network locality determination.

use crate::{TRAFFIC_TYPE_ALL, TRAFFIC_TYPE_INTERNET, TRAFFIC_TYPE_LOCAL};

/// Decide whether an IP packet belongs to the configured traffic class.
///
/// `read` copies packet bytes from an offset relative to the network header.
/// Unknown versions and truncated headers are throttled conservatively.
#[inline(always)]
pub fn should_throttle_ip_packet(
    traffic_type: u8,
    ingress: bool,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> bool {
    if traffic_type == TRAFFIC_TYPE_ALL {
        return true;
    }

    let mut version = [0u8; 1];
    if !read(0, &mut version) {
        return true;
    }
    let is_local = match version[0] >> 4 {
        4 => {
            if version[0] & 0x0f < 5 {
                return true;
            }
            let mut header = [0u8; 20];
            if !read(0, &mut header) {
                return true;
            }
            let offset = if ingress { 12 } else { 16 };
            is_ipv4_local(&[
                header[offset],
                header[offset + 1],
                header[offset + 2],
                header[offset + 3],
            ])
        }
        6 => {
            let mut header = [0u8; 40];
            if !read(0, &mut header) {
                return true;
            }
            let offset = if ingress { 8 } else { 24 };
            let mut address = [0u8; 16];
            address.copy_from_slice(&header[offset..offset + 16]);
            is_ipv6_local(&address)
        }
        _ => return true,
    };

    match traffic_type {
        TRAFFIC_TYPE_INTERNET => !is_local,
        TRAFFIC_TYPE_LOCAL => is_local,
        _ => true,
    }
}

#[inline(always)]
fn is_ipv4_local(ip: &[u8; 4]) -> bool {
    ip[0] == 10
        || ip[0] == 127
        || (ip[0] == 169 && ip[1] == 254)
        || (ip[0] == 172 && ip[1] >= 16 && ip[1] <= 31)
        || (ip[0] == 192 && ip[1] == 168)
        || *ip == [0, 0, 0, 0]
        || *ip == [255, 255, 255, 255]
}

#[inline(always)]
fn is_ipv6_local(ip: &[u8; 16]) -> bool {
    (ip[0] == 0xfe && (ip[1] & 0xc0) == 0x80)
        || (ip[0] & 0xfe) == 0xfc
        || *ip == [0; 16]
        || *ip == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(packet: &[u8], ingress: bool, traffic_type: u8) -> bool {
        should_throttle_ip_packet(traffic_type, ingress, |offset, out| {
            let Some(bytes) = packet.get(offset..offset + out.len()) else {
                return false;
            };
            out.copy_from_slice(bytes);
            true
        })
    }

    #[test]
    fn ipv4_uses_remote_endpoint_in_each_direction() {
        for remote in [[8, 8, 8, 8], [192, 168, 1, 2], [172, 32, 0, 1]] {
            for ingress in [false, true] {
                let mut packet = [0u8; 20];
                packet[0] = 0x45;
                let (source, destination) = if ingress {
                    (remote, [10, 0, 0, 1])
                } else {
                    ([10, 0, 0, 1], remote)
                };
                packet[12..16].copy_from_slice(&source);
                packet[16..20].copy_from_slice(&destination);
                assert_eq!(
                    classify(&packet, ingress, TRAFFIC_TYPE_LOCAL),
                    is_ipv4_local(&remote)
                );
                assert_eq!(
                    classify(&packet, ingress, TRAFFIC_TYPE_INTERNET),
                    !is_ipv4_local(&remote)
                );
            }
        }
    }

    #[test]
    fn ipv6_is_not_misread_as_ipv4() {
        for remote in [
            [
                0x20, 1, 0x48, 0x60, 0x48, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0x88, 0x88,
            ],
            [0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        ] {
            for ingress in [false, true] {
                let mut packet = [0u8; 40];
                packet[0] = 0x60;
                let offset = if ingress { 8 } else { 24 };
                packet[offset..offset + 16].copy_from_slice(&remote);
                assert_eq!(
                    classify(&packet, ingress, TRAFFIC_TYPE_LOCAL),
                    is_ipv6_local(&remote)
                );
                assert_eq!(
                    classify(&packet, ingress, TRAFFIC_TYPE_INTERNET),
                    !is_ipv6_local(&remote)
                );
            }
        }
    }

    #[test]
    fn local_address_boundaries() {
        for ip in [
            [10, 0, 0, 1],
            [127, 1, 2, 3],
            [169, 254, 1, 2],
            [172, 16, 0, 1],
            [172, 31, 255, 255],
            [192, 168, 0, 1],
            [0; 4],
            [255; 4],
        ] {
            assert!(is_ipv4_local(&ip));
        }
        for ip in [
            [172, 15, 255, 255],
            [172, 32, 0, 0],
            [192, 169, 0, 1],
            [8, 8, 8, 8],
        ] {
            assert!(!is_ipv4_local(&ip));
        }
        assert!(is_ipv6_local(&[0; 16]));
        let mut loopback = [0; 16];
        loopback[15] = 1;
        assert!(is_ipv6_local(&loopback));
        loopback[15] = 2;
        assert!(!is_ipv6_local(&loopback));
    }

    #[test]
    fn malformed_packets_are_conservatively_throttled() {
        for packet in [
            &[][..],
            &[0x45][..],
            &[0x60][..],
            &[0x30; 40][..],
            &[0x44; 20][..],
        ] {
            for ingress in [false, true] {
                for traffic_type in [
                    TRAFFIC_TYPE_ALL,
                    TRAFFIC_TYPE_LOCAL,
                    TRAFFIC_TYPE_INTERNET,
                    255,
                ] {
                    assert!(classify(packet, ingress, traffic_type));
                }
            }
        }
    }
}
