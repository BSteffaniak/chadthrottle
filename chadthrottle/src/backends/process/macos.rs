// macOS-specific process utilities

use super::{ConnectionEntry, ConnectionMap, ProcessEntry, ProcessUtils};
use anyhow::Result;
use std::collections::HashMap;
use sysinfo::{Pid, System};

/// macOS process utilities using sysinfo and system APIs
pub struct MacOSProcessUtils;

impl MacOSProcessUtils {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessUtils for MacOSProcessUtils {
    fn get_process_name(&self, pid: i32) -> Result<String> {
        let sys = System::new_all();
        let pid_obj = Pid::from_u32(pid as u32);

        sys.process(pid_obj)
            .map(|p| p.name().to_str().unwrap_or("unknown").to_string())
            .ok_or_else(|| anyhow::anyhow!("Process {} not found", pid))
    }

    fn process_exists(&self, pid: i32) -> bool {
        let sys = System::new_all();
        let pid_obj = Pid::from_u32(pid as u32);
        sys.process(pid_obj).is_some()
    }

    fn get_all_processes(&self) -> Result<Vec<ProcessEntry>> {
        let sys = System::new_all();

        let entries = sys
            .processes()
            .iter()
            .map(|(pid, proc)| ProcessEntry {
                pid: pid.as_u32() as i32,
                name: proc.name().to_str().unwrap_or("unknown").to_string(),
            })
            .collect();

        Ok(entries)
    }

    fn get_connection_map(&self) -> Result<ConnectionMap> {
        // TODO: Implement socket-to-PID mapping for macOS
        // For now, return empty map to allow compilation
        // This will be implemented in Phase 2
        log::warn!("macOS socket-to-PID mapping not yet implemented");

        Ok(ConnectionMap {
            socket_to_pid: HashMap::new(),
            tcp_connections: Vec::new(),
            tcp6_connections: Vec::new(),
            udp_connections: Vec::new(),
            udp6_connections: Vec::new(),
        })
    }
}
