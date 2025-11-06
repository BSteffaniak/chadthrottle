// Bandwidth history tracking for graphing

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of history samples to keep (e.g., 60 samples = 1 minute at 1Hz)
const MAX_HISTORY_SAMPLES: usize = 60;

/// A single bandwidth measurement sample
#[derive(Debug, Clone)]
pub struct BandwidthSample {
    pub timestamp: u64,     // Unix timestamp in seconds
    pub download_rate: u64, // bytes per second
    pub upload_rate: u64,   // bytes per second
}

/// Bandwidth history for a single process
#[derive(Debug, Clone)]
pub struct ProcessHistory {
    pub pid: i32,
    pub process_name: String,
    pub samples: VecDeque<BandwidthSample>,
}

impl ProcessHistory {
    pub fn new(pid: i32, process_name: String) -> Self {
        Self {
            pid,
            process_name,
            samples: VecDeque::with_capacity(MAX_HISTORY_SAMPLES),
        }
    }

    /// Add a new sample, removing old ones if we exceed the limit
    pub fn add_sample(&mut self, download_rate: u64, upload_rate: u64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let sample = BandwidthSample {
            timestamp,
            download_rate,
            upload_rate,
        };

        self.samples.push_back(sample);

        // Remove old samples if we exceed the limit
        while self.samples.len() > MAX_HISTORY_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// Get the maximum download rate in history
    pub fn max_download_rate(&self) -> u64 {
        self.samples
            .iter()
            .map(|s| s.download_rate)
            .max()
            .unwrap_or(0)
    }

    /// Get the maximum upload rate in history
    pub fn max_upload_rate(&self) -> u64 {
        self.samples
            .iter()
            .map(|s| s.upload_rate)
            .max()
            .unwrap_or(0)
    }

    /// Get the average download rate
    pub fn avg_download_rate(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|s| s.download_rate).sum();
        sum / self.samples.len() as u64
    }

    /// Get the average upload rate
    pub fn avg_upload_rate(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|s| s.upload_rate).sum();
        sum / self.samples.len() as u64
    }

    /// Get samples as vectors for graphing
    pub fn get_graph_data(&self) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
        let download_data: Vec<(f64, f64)> = self
            .samples
            .iter()
            .enumerate()
            .map(|(i, s)| (i as f64, s.download_rate as f64))
            .collect();

        let upload_data: Vec<(f64, f64)> = self
            .samples
            .iter()
            .enumerate()
            .map(|(i, s)| (i as f64, s.upload_rate as f64))
            .collect();

        (download_data, upload_data)
    }
}

/// Global history tracker for all processes
#[derive(Debug)]
pub struct HistoryTracker {
    pub histories: HashMap<i32, ProcessHistory>,
}

impl HistoryTracker {
    pub fn new() -> Self {
        Self {
            histories: HashMap::new(),
        }
    }

    /// Update history for a process
    pub fn update(&mut self, pid: i32, process_name: String, download_rate: u64, upload_rate: u64) {
        let history = self
            .histories
            .entry(pid)
            .or_insert_with(|| ProcessHistory::new(pid, process_name.clone()));

        // Update process name in case it changed
        history.process_name = process_name;
        history.add_sample(download_rate, upload_rate);
    }

    /// Get history for a specific process
    pub fn get_history(&self, pid: i32) -> Option<&ProcessHistory> {
        self.histories.get(&pid)
    }

    /// Get mutable history for a specific process
    pub fn get_history_mut(&mut self, pid: i32) -> Option<&mut ProcessHistory> {
        self.histories.get_mut(&pid)
    }

    /// Remove history for a process (when it exits)
    pub fn remove(&mut self, pid: i32) {
        self.histories.remove(&pid);
    }

    /// Clear all histories
    pub fn clear(&mut self) {
        self.histories.clear();
    }

    /// Get the number of processes being tracked
    pub fn len(&self) -> usize {
        self.histories.len()
    }

    /// Check if tracking any processes
    pub fn is_empty(&self) -> bool {
        self.histories.is_empty()
    }
}

impl Default for HistoryTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_history() {
        let mut history = ProcessHistory::new(1234, "test".to_string());

        history.add_sample(1000, 500);
        history.add_sample(2000, 1000);
        history.add_sample(1500, 750);

        assert_eq!(history.samples.len(), 3);
        assert_eq!(history.max_download_rate(), 2000);
        assert_eq!(history.max_upload_rate(), 1000);
        assert_eq!(history.avg_download_rate(), 1500);
        assert_eq!(history.avg_upload_rate(), 750);
    }

    #[test]
    fn test_history_limit() {
        let mut history = ProcessHistory::new(1234, "test".to_string());

        // Add more than MAX_HISTORY_SAMPLES
        for i in 0..(MAX_HISTORY_SAMPLES + 10) {
            history.add_sample(i as u64, i as u64);
        }

        assert_eq!(history.samples.len(), MAX_HISTORY_SAMPLES);
    }
}
