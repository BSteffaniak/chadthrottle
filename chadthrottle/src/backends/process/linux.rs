// Linux-specific process utilities using procfs

use super::socket_mapper::{SocketMapperBackend, select_socket_mapper};
use super::{ConnectionMap, ProcessEntry, ProcessUtils};
use anyhow::Result;
use procfs::process::all_processes;

/// Linux process utilities with pluggable socket mapping
pub struct LinuxProcessUtils {
    socket_mapper: Box<dyn SocketMapperBackend>,
}

impl LinuxProcessUtils {
    pub fn new() -> Self {
        Self::with_socket_mapper(None)
    }

    /// Create with a specific socket mapper backend
    pub fn with_socket_mapper(backend_name: Option<&str>) -> Self {
        let socket_mapper = select_socket_mapper(backend_name)
            .expect("Failed to initialize socket mapper on Linux");

        log::debug!("Using socket mapper backend: {}", socket_mapper.name());

        Self { socket_mapper }
    }
}

impl ProcessUtils for LinuxProcessUtils {
    fn get_process_name(&self, pid: i32) -> Result<String> {
        std::fs::read_to_string(format!("/proc/{}/comm", pid))
            .map(|s| s.trim().to_string())
            .or_else(|_| Ok(format!("PID {}", pid)))
    }

    fn process_exists(&self, pid: i32) -> bool {
        procfs::process::Process::new(pid).is_ok()
    }

    fn get_all_processes(&self) -> Result<Vec<ProcessEntry>> {
        let all_procs = all_processes()?;
        let mut entries = Vec::new();

        for proc_result in all_procs {
            if let Ok(process) = proc_result {
                let pid = process.pid();
                let name = if let Ok(stat) = process.stat() {
                    stat.comm
                } else {
                    format!("PID {}", pid)
                };
                entries.push(ProcessEntry { pid, name });
            }
        }

        Ok(entries)
    }

    fn get_connection_map(&self) -> Result<ConnectionMap> {
        // Delegate to pluggable socket mapper backend
        self.socket_mapper.get_connection_map()
    }
}
