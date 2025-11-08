// macOS socket mapper backends

mod libproc;
mod lsof;

pub use libproc::LibprocSocketMapper;
pub use lsof::LsofSocketMapper;

use super::{SocketMapperBackend, SocketMapperInfo};
use crate::backends::BackendPriority;
use anyhow::Result;

/// Detect all available socket mapper backends on macOS
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![
        SocketMapperInfo {
            name: "libproc",
            priority: BackendPriority::Best,
            available: LibprocSocketMapper::is_available(),
        },
        SocketMapperInfo {
            name: "lsof",
            priority: BackendPriority::Good,
            available: LsofSocketMapper::is_available(),
        },
    ]
}

/// Select socket mapper backend for macOS
///
/// Priority order (if no preference specified):
/// 1. libproc (Best - native API, most efficient)
/// 2. lsof (Good - reliable but spawns external process)
pub fn select_socket_mapper(preference: Option<&str>) -> Result<Box<dyn SocketMapperBackend>> {
    if let Some(name) = preference {
        match name {
            "libproc" => Ok(Box::new(LibprocSocketMapper::new()?)),
            "lsof" => Ok(Box::new(LsofSocketMapper::new()?)),
            _ => Err(anyhow::anyhow!("Unknown socket mapper: {}", name)),
        }
    } else {
        // Auto-select: prefer libproc over lsof
        if LibprocSocketMapper::is_available() {
            Ok(Box::new(LibprocSocketMapper::new()?))
        } else if LsofSocketMapper::is_available() {
            Ok(Box::new(LsofSocketMapper::new()?))
        } else {
            Err(anyhow::anyhow!(
                "No socket mapper backends available on macOS"
            ))
        }
    }
}
