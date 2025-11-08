// macOS socket mapper backends

mod lsof;
pub use lsof::LsofSocketMapper;

use super::{SocketMapperBackend, SocketMapperInfo};
use crate::backends::BackendPriority;
use anyhow::Result;

/// Detect all available socket mapper backends on macOS
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![
        SocketMapperInfo {
            name: "lsof",
            priority: BackendPriority::Good,
            available: LsofSocketMapper::is_available(),
        },
        // Future: libproc backend with BackendPriority::Best
    ]
}

/// Select socket mapper backend for macOS
///
/// Priority order (if no preference specified):
/// 1. libproc (future - native API, most efficient)
/// 2. lsof (current - reliable but spawns external process)
pub fn select_socket_mapper(preference: Option<&str>) -> Result<Box<dyn SocketMapperBackend>> {
    if let Some(name) = preference {
        match name {
            "lsof" => Ok(Box::new(LsofSocketMapper::new()?)),
            // Future: "libproc" => Ok(Box::new(LibprocSocketMapper::new()?)),
            _ => Err(anyhow::anyhow!("Unknown socket mapper: {}", name)),
        }
    } else {
        // Auto-select best available backend
        // For now, just lsof; in future, prefer libproc if available
        if LsofSocketMapper::is_available() {
            Ok(Box::new(LsofSocketMapper::new()?))
        } else {
            Err(anyhow::anyhow!(
                "No socket mapper backends available on macOS"
            ))
        }
    }
}
