// Linux socket mapper backends

mod procfs;
pub use procfs::ProcfsSocketMapper;

use super::{SocketMapperBackend, SocketMapperInfo};
use crate::backends::BackendPriority;
use anyhow::Result;

/// Detect all available socket mapper backends on Linux
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![SocketMapperInfo {
        name: "procfs",
        priority: BackendPriority::Best,
        available: ProcfsSocketMapper::is_available(),
    }]
}

/// Select socket mapper backend for Linux
///
/// Currently only procfs is available on Linux, but this function
/// follows the same pattern as other platforms for consistency.
pub fn select_socket_mapper(preference: Option<&str>) -> Result<Box<dyn SocketMapperBackend>> {
    if let Some(name) = preference {
        match name {
            "procfs" => Ok(Box::new(ProcfsSocketMapper::new()?)),
            _ => Err(anyhow::anyhow!("Unknown socket mapper: {}", name)),
        }
    } else {
        // Auto-select (only one option on Linux currently)
        Ok(Box::new(ProcfsSocketMapper::new()?))
    }
}
