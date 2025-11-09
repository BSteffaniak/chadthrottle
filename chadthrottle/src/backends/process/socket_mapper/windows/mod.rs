// Windows socket-to-PID mapping backends
//
// This module provides Windows-specific socket mapping implementations
// using GetExtendedTcpTable/GetExtendedUdpTable APIs from IP Helper.

mod iphelper;

pub use iphelper::IpHelperSocketMapper;

use crate::backends::BackendPriority;
use crate::backends::process::socket_mapper::{SocketMapperBackend, SocketMapperInfo};
use anyhow::Result;

/// Detect available socket mapper backends on Windows
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![SocketMapperInfo {
        name: "iphelper",
        priority: BackendPriority::Best,
        available: IpHelperSocketMapper::is_available(),
    }]
}

/// Select socket mapper backend for Windows
///
/// Currently only supports IP Helper API (iphelper).
pub fn select_socket_mapper(preference: Option<&str>) -> Result<Box<dyn SocketMapperBackend>> {
    if let Some(name) = preference {
        match name {
            "iphelper" => Ok(Box::new(IpHelperSocketMapper::new()?)),
            _ => Err(anyhow::anyhow!("Unknown socket mapper: {}", name)),
        }
    } else {
        // Auto-select: IP Helper is the only available backend
        if IpHelperSocketMapper::is_available() {
            Ok(Box::new(IpHelperSocketMapper::new()?))
        } else {
            Err(anyhow::anyhow!(
                "No socket mapper backends available on Windows"
            ))
        }
    }
}
