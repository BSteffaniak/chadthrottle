// Windows socket-to-PID mapping backends
//
// This module will provide Windows-specific socket mapping implementations
// using GetExtendedTcpTable/GetExtendedUdpTable APIs or netstat parsing.
//
// For now, this is a stub that returns an empty mapper.

use crate::backends::process::ConnectionMap;
use crate::backends::process::socket_mapper::{SocketMapperBackend, SocketMapperInfo};
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::Result;
use std::collections::HashMap;

/// Stub socket mapper for Windows (to be implemented)
pub struct WindowsStubMapper;

impl SocketMapperBackend for WindowsStubMapper {
    fn name(&self) -> &'static str {
        "windows-stub"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Fallback
    }

    fn is_available() -> bool {
        true // Stub is always "available" but returns empty data
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            ipv4_support: false,
            ipv6_support: false,
            per_process: false,
            per_connection: false,
        }
    }

    fn get_connection_map(&self) -> Result<ConnectionMap> {
        log::warn!("Windows socket-to-PID mapping not yet implemented");
        Ok(ConnectionMap {
            socket_to_pid: HashMap::new(),
            tcp_connections: Vec::new(),
            tcp6_connections: Vec::new(),
            udp_connections: Vec::new(),
            udp6_connections: Vec::new(),
        })
    }
}

/// Detect available socket mapper backends on Windows
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![SocketMapperInfo {
        name: "windows-stub",
        priority: BackendPriority::Fallback,
        available: true,
    }]
}

/// Select best socket mapper backend for Windows
pub fn select_socket_mapper(preference: Option<&str>) -> Box<dyn SocketMapperBackend> {
    if let Some(name) = preference {
        log::warn!(
            "Windows socket mapper preference '{}' ignored - only stub available",
            name
        );
    }
    Box::new(WindowsStubMapper)
}
