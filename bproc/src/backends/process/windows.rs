// Windows ProcessUtils implementation using sysinfo and IP Helper API
//
// This provides process enumeration and socket-to-PID mapping for Windows.

use super::socket_mapper::{SocketMapperBackend, select_socket_mapper};
use super::{ConnectionMap, ProcessEntry, ProcessUtils};
use anyhow::Result;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, System};

pub struct WindowsProcessUtils {
    socket_mapper: Box<dyn SocketMapperBackend>,
    socket_mapper_name: String,
    // Cached System instance - refreshed periodically instead of creating new one each call
    // This is the KEY optimization: System::new_all() is extremely expensive (10-20ms per call)
    // By caching it, we go from 50 calls × 10-20ms = 500-1000ms to 1 call × 10-20ms = 10-20ms
    cached_system: Arc<Mutex<System>>,
}

impl WindowsProcessUtils {
    pub fn new() -> Self {
        Self::with_socket_mapper(None)
    }

    pub fn with_socket_mapper(socket_mapper_preference: Option<&str>) -> Self {
        let socket_mapper = select_socket_mapper(socket_mapper_preference)
            .expect("Failed to initialize socket mapper on Windows");

        let socket_mapper_name = socket_mapper.name().to_string();
        log::debug!("Using socket mapper backend: {}", socket_mapper_name);

        // Create System instance once and cache it
        let cached_system = Arc::new(Mutex::new(System::new_all()));
        log::debug!("Created cached System instance for WindowsProcessUtils");

        Self {
            socket_mapper,
            socket_mapper_name,
            cached_system,
        }
    }

    /// Refresh the cached System instance
    /// This should be called periodically (e.g., once per update cycle) instead of
    /// creating a new System for every process_exists() call
    pub fn refresh_system_cache(&self) {
        if let Ok(mut sys) = self.cached_system.lock() {
            sys.refresh_all();
        }
    }

    /// Get the name of the socket mapper backend
    pub fn socket_mapper_name(&self) -> &str {
        &self.socket_mapper_name
    }

    /// Get capabilities of the socket mapper backend
    pub fn socket_mapper_capabilities(&self) -> crate::backends::BackendCapabilities {
        self.socket_mapper.capabilities()
    }
}

impl ProcessUtils for WindowsProcessUtils {
    fn get_process_name(&self, pid: i32) -> Result<String> {
        // Use cached System instance instead of creating new one
        let sys = self.cached_system.lock().unwrap();
        let pid_obj = Pid::from_u32(pid as u32);

        sys.process(pid_obj)
            .map(|p| p.name().to_str().unwrap_or("unknown").to_string())
            .ok_or_else(|| anyhow::anyhow!("Process {} not found", pid))
    }

    fn process_exists(&self, pid: i32) -> bool {
        // CRITICAL OPTIMIZATION: Use cached System instance!
        // Before: System::new_all() called ~50 times = 500-1000ms
        // After: Use cached System, refreshed once per cycle = ~10-20ms total
        let sys = self.cached_system.lock().unwrap();
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
        // Delegate to pluggable socket mapper backend
        self.socket_mapper.get_connection_map()
    }

    fn refresh_caches(&self) {
        // Refresh the cached System instance
        // This is called once per update cycle instead of creating new System for each process_exists() call
        self.refresh_system_cache();
    }
}
