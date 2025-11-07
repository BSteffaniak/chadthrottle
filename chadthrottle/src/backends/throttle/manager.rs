// ThrottleManager coordinates upload and download throttling backends

use super::{
    BackendInfo, DownloadThrottleBackend, UploadThrottleBackend, detect_download_backends,
    detect_upload_backends,
};
use crate::backends::ActiveThrottle;
use crate::process::ThrottleLimit;
use anyhow::Result;
use std::collections::HashMap;

/// Manages throttling by coordinating separate upload and download backends
pub struct ThrottleManager {
    upload_backend: Option<Box<dyn UploadThrottleBackend>>,
    download_backend: Option<Box<dyn DownloadThrottleBackend>>,
}

impl ThrottleManager {
    /// Create a new ThrottleManager with the given backends
    pub fn new(
        upload_backend: Option<Box<dyn UploadThrottleBackend>>,
        download_backend: Option<Box<dyn DownloadThrottleBackend>>,
    ) -> Self {
        Self {
            upload_backend,
            download_backend,
        }
    }

    /// Get names of active backends
    pub fn backend_names(&self) -> (Option<String>, Option<String>) {
        (
            self.upload_backend.as_ref().map(|b| b.name().to_string()),
            self.download_backend.as_ref().map(|b| b.name().to_string()),
        )
    }

    /// Get comprehensive backend information for UI display
    pub fn get_backend_info(
        &self,
        preferred_upload: Option<String>,
        preferred_download: Option<String>,
    ) -> BackendInfo {
        BackendInfo {
            active_upload: self.upload_backend.as_ref().map(|b| b.name().to_string()),
            active_download: self.download_backend.as_ref().map(|b| b.name().to_string()),
            available_upload: detect_upload_backends()
                .into_iter()
                .map(|b| (b.name.to_string(), b.priority, b.available))
                .collect(),
            available_download: detect_download_backends()
                .into_iter()
                .map(|b| (b.name.to_string(), b.priority, b.available))
                .collect(),
            preferred_upload,
            preferred_download,
            upload_capabilities: self.upload_backend.as_ref().map(|b| b.capabilities()),
            download_capabilities: self.download_backend.as_ref().map(|b| b.capabilities()),
        }
    }

    /// Apply throttle to a process (upload and/or download)
    pub fn throttle_process(
        &mut self,
        pid: i32,
        process_name: String,
        limit: &ThrottleLimit,
    ) -> Result<()> {
        let mut applied_any = false;

        // Apply upload throttle if specified AND backend available
        if let Some(upload_limit) = limit.upload_limit {
            if let Some(ref mut backend) = self.upload_backend {
                backend.throttle_upload(pid, process_name.clone(), upload_limit)?;
                applied_any = true;
            } else {
                log::error!("⚠️  Upload throttling requested but no backend available");
                log::error!(
                    "    Install 'tc' (traffic control) and enable cgroups to use upload throttling."
                );
            }
        }

        // Apply download throttle if specified AND backend available
        if let Some(download_limit) = limit.download_limit {
            if let Some(ref mut backend) = self.download_backend {
                backend.throttle_download(pid, process_name, download_limit)?;
                applied_any = true;
            } else {
                log::error!("⚠️  Download throttling requested but no backend available");
                log::error!("    Enable the 'ifb' kernel module to use download throttling.");
                log::error!("    See IFB_SETUP.md for instructions.");
            }
        }

        if !applied_any && (limit.upload_limit.is_some() || limit.download_limit.is_some()) {
            return Err(anyhow::anyhow!("No throttling backends available"));
        }

        Ok(())
    }

    /// Remove all throttles from a process
    pub fn remove_throttle(&mut self, pid: i32) -> Result<()> {
        // Remove upload throttle if backend available
        if let Some(ref mut backend) = self.upload_backend {
            backend.remove_upload_throttle(pid)?;
        }

        // Remove download throttle if backend available
        if let Some(ref mut backend) = self.download_backend {
            backend.remove_download_throttle(pid)?;
        }

        Ok(())
    }

    /// Get throttle information for a process
    pub fn get_throttle(&self, pid: i32) -> Option<ActiveThrottle> {
        let upload_limit = self
            .upload_backend
            .as_ref()
            .and_then(|b| b.get_upload_throttle(pid));
        let download_limit = self
            .download_backend
            .as_ref()
            .and_then(|b| b.get_download_throttle(pid));

        if upload_limit.is_some() || download_limit.is_some() {
            Some(ActiveThrottle {
                pid,
                process_name: String::new(), // Would need to track this separately
                upload_limit,
                download_limit,
            })
        } else {
            None
        }
    }

    /// Check if download throttling is available
    pub fn is_download_throttling_available(&self) -> bool {
        self.download_backend.is_some()
    }

    /// Get all active throttles (merged from upload and download backends)
    pub fn get_all_throttles(&self) -> HashMap<i32, ActiveThrottle> {
        let mut throttles: HashMap<i32, ActiveThrottle> = HashMap::new();

        // Collect upload throttles
        if let Some(ref backend) = self.upload_backend {
            for (pid, upload_limit) in backend.get_all_throttles() {
                throttles.entry(pid).or_insert_with(|| ActiveThrottle {
                    pid,
                    process_name: String::new(),
                    upload_limit: Some(upload_limit),
                    download_limit: None,
                });
                if let Some(throttle) = throttles.get_mut(&pid) {
                    throttle.upload_limit = Some(upload_limit);
                }
            }
        }

        // Collect download throttles
        if let Some(ref backend) = self.download_backend {
            for (pid, download_limit) in backend.get_all_throttles() {
                throttles.entry(pid).or_insert_with(|| ActiveThrottle {
                    pid,
                    process_name: String::new(),
                    upload_limit: None,
                    download_limit: Some(download_limit),
                });
                if let Some(throttle) = throttles.get_mut(&pid) {
                    throttle.download_limit = Some(download_limit);
                }
            }
        }

        throttles
    }

    /// Cleanup all throttles
    pub fn cleanup(&mut self) -> Result<()> {
        if let Some(ref mut backend) = self.upload_backend {
            backend.cleanup()?;
        }

        if let Some(ref mut backend) = self.download_backend {
            backend.cleanup()?;
        }

        Ok(())
    }
}

impl Drop for ThrottleManager {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
