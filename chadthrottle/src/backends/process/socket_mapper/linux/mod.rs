// Linux socket mapper backends

mod libproc;
mod procfs;

pub use libproc::LibprocSocketMapper;
pub use procfs::ProcfsSocketMapper;

use super::{SocketMapperBackend, SocketMapperInfo};
use crate::backends::BackendPriority;
use anyhow::Result;

/// Detect all available socket mapper backends on Linux
pub fn detect_socket_mappers() -> Vec<SocketMapperInfo> {
    vec![
        SocketMapperInfo {
            name: "procfs",
            priority: BackendPriority::Best,
            available: ProcfsSocketMapper::is_available(),
        },
        SocketMapperInfo {
            name: "libproc",
            priority: BackendPriority::Good,
            available: LibprocSocketMapper::is_available(),
        },
    ]
}

/// Select socket mapper backend for Linux
///
/// Priority order (if no preference specified):
/// 1. procfs (Best - manual optimized implementation)
/// 2. libproc (Good - crate wrapper around procfs)
pub fn select_socket_mapper(preference: Option<&str>) -> Result<Box<dyn SocketMapperBackend>> {
    if let Some(name) = preference {
        match name {
            "procfs" => Ok(Box::new(ProcfsSocketMapper::new()?)),
            "libproc" => Ok(Box::new(LibprocSocketMapper::new()?)),
            _ => Err(anyhow::anyhow!("Unknown socket mapper: {}", name)),
        }
    } else {
        // Auto-select: prefer procfs (manual optimized) over libproc (wrapper)
        if ProcfsSocketMapper::is_available() {
            Ok(Box::new(ProcfsSocketMapper::new()?))
        } else if LibprocSocketMapper::is_available() {
            Ok(Box::new(LibprocSocketMapper::new()?))
        } else {
            Err(anyhow::anyhow!(
                "No socket mapper backends available on Linux"
            ))
        }
    }
}
