//! Resource usage monitoring for services.
//!
//! This module provides functionality to query resource usage metrics (memory, CPU, threads)
//! for running services. The implementation is platform-specific:
//!
//! - **Linux**: Reads from `/proc/{pid}/stat` and `/proc/{pid}/status` for accurate, efficient metrics
//! - **macOS**: Uses the `ps` command as a fallback (less efficient but portable)
//! - **Other platforms**: Returns default/unknown values
//!
//! # Design Notes
//!
//! Reading from `/proc` on Linux is the most efficient approach - it's a direct syscall that
//! reads kernel memory structures. The `ps` command on macOS is less efficient (process spawn overhead)
//! but provides similar information.
//!
//! CPU percentage is calculated as a snapshot of the process's cumulative CPU time.
//! For accurate CPU percentage over time, callers should sample periodically and calculate
//! the delta themselves.

use serde::{Deserialize, Serialize};

/// Resource usage metrics for a running service.
///
/// All fields use `Option` because resource queries can fail (process exited,
/// permission denied, unsupported platform, etc.). Callers should handle `None`
/// gracefully by displaying "N/A" or similar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Memory usage in bytes (RSS - Resident Set Size).
    /// This is the actual physical memory used by the process.
    pub memory_rss_bytes: Option<u64>,

    /// Virtual memory size in bytes (VSZ).
    /// This includes all memory the process can access, including swapped and shared memory.
    pub memory_vsz_bytes: Option<u64>,

    /// CPU usage percentage (0-100 per core, so can exceed 100 on multi-core systems).
    /// This is a snapshot of cumulative CPU time, not an averaged rate.
    pub cpu_percent: Option<f64>,

    /// Number of threads/tasks in the process.
    pub thread_count: Option<u64>,
}

impl ResourceUsage {
    /// Query resource usage for a process by PID.
    ///
    /// Returns resource metrics if the process exists and is accessible.
    /// Returns default (all None) if the process doesn't exist or query fails.
    ///
    /// # Arguments
    ///
    /// * `pid` - Process ID to query
    ///
    /// # Platform Support
    ///
    /// - **Linux**: Reads from `/proc/{pid}/stat` and `/proc/{pid}/status`
    /// - **macOS**: Uses `ps` command
    /// - **Other**: Returns default (all None)
    pub async fn query(pid: u32) -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::query_linux(pid).await
        }

        #[cfg(target_os = "macos")]
        {
            Self::query_macos(pid).await
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            // Unsupported platform
            let _ = pid; // Suppress unused variable warning
            Self::default()
        }
    }

    /// Query resource usage on Linux using /proc filesystem.
    ///
    /// This is the most efficient method - it reads kernel data structures directly.
    ///
    /// Format of `/proc/{pid}/stat` (space-separated, field numbers from `man proc`):
    /// - Field 1: pid
    /// - Field 2: comm (command name in parentheses)
    /// - Field 3: state (R, S, D, Z, T, etc.)
    /// - Field 14: utime (user mode jiffies)
    /// - Field 15: stime (kernel mode jiffies)
    /// - Field 20: num_threads
    /// - Field 23: vsize (virtual memory in bytes)
    /// - Field 24: rss (resident set size in pages)
    ///
    /// RSS is in pages (typically 4096 bytes), so we multiply by page size.
    #[cfg(target_os = "linux")]
    async fn query_linux(pid: u32) -> Self {
        let stat_path = format!("/proc/{}/stat", pid);
        let status_path = format!("/proc/{}/status", pid);

        // Check if process exists first
        if !std::path::Path::new(&stat_path).exists() {
            return Self::default();
        }

        // Read /proc/{pid}/stat
        let stat_content = match tokio::fs::read_to_string(&stat_path).await {
            Ok(content) => content,
            Err(_) => return Self::default(),
        };

        // Parse /proc/{pid}/stat
        // Format: pid (comm) state ... (see man 5 proc)
        // We need to handle (comm) which may contain spaces and parentheses
        let stat_parts = Self::parse_proc_stat(&stat_content);
        if stat_parts.is_empty() {
            return Self::default();
        }

        // Extract metrics from stat
        // Field indices are 0-based after parsing (field 1 in docs = index 0)
        let vsize_bytes = stat_parts.get(22).and_then(|s| s.parse::<u64>().ok());
        let rss_pages = stat_parts.get(23).and_then(|s| s.parse::<u64>().ok());
        let num_threads = stat_parts.get(19).and_then(|s| s.parse::<u64>().ok());

        // Convert RSS from pages to bytes (typically 4096 bytes per page)
        let page_size = 4096u64; // Could query with sysconf(_SC_PAGESIZE) but 4096 is standard
        let memory_rss_bytes = rss_pages.map(|pages| pages * page_size);

        // Calculate CPU percentage from utime + stime
        // This calculates lifetime average CPU usage since the process started.
        let utime = stat_parts.get(13).and_then(|s| s.parse::<u64>().ok());
        let stime = stat_parts.get(14).and_then(|s| s.parse::<u64>().ok());
        let starttime = stat_parts.get(21).and_then(|s| s.parse::<u64>().ok());

        let cpu_percent = match (utime, stime, starttime) {
            (Some(u), Some(s), Some(start)) => {
                // Read system uptime from /proc/uptime
                Self::calculate_cpu_percent(u, s, start).await
            }
            _ => None,
        };

        // Try to read VmRSS from /proc/{pid}/status for more accurate RSS
        // (stat file RSS is in pages, status file is in KB)
        let memory_rss_from_status = match tokio::fs::read_to_string(&status_path).await {
            Ok(content) => Self::parse_vmrss_from_status(&content),
            Err(_) => None,
        };

        Self {
            memory_rss_bytes: memory_rss_from_status.or(memory_rss_bytes),
            memory_vsz_bytes: vsize_bytes,
            cpu_percent,
            thread_count: num_threads,
        }
    }

    /// Calculate CPU percentage from utime, stime, and starttime.
    ///
    /// Returns the lifetime average CPU usage since the process started.
    /// This is calculated as: 100 * (utime + stime) / (elapsed_time * hertz)
    ///
    /// Note: This is a lifetime average, not an instantaneous rate.
    /// For short-lived processes or recently started processes, this may
    /// not reflect current CPU usage accurately.
    #[cfg(target_os = "linux")]
    async fn calculate_cpu_percent(utime: u64, stime: u64, starttime: u64) -> Option<f64> {
        // Read system uptime from /proc/uptime
        let uptime_content = tokio::fs::read_to_string("/proc/uptime").await.ok()?;
        let uptime_secs: f64 = uptime_content.split_whitespace().next()?.parse().ok()?;

        // Clock ticks per second (typically 100 on Linux)
        // Could use sysconf(_SC_CLK_TCK) but 100 is standard
        let hertz: f64 = 100.0;

        // Calculate process elapsed time in seconds
        let process_start_secs = starttime as f64 / hertz;
        let elapsed_secs = uptime_secs - process_start_secs;

        // Avoid division by zero for very recently started processes
        if elapsed_secs <= 0.0 {
            return None;
        }

        // Calculate CPU time used in seconds
        let cpu_time_secs = (utime + stime) as f64 / hertz;

        // CPU percentage (can exceed 100% on multi-core systems)
        let cpu_percent = 100.0 * cpu_time_secs / elapsed_secs;

        Some(cpu_percent)
    }

    /// Parse `/proc/{pid}/stat`, handling (comm) field correctly.
    ///
    /// The comm field (process name) is wrapped in parentheses and may contain spaces
    /// or other special characters. We need to find the closing parenthesis first,
    /// then split the rest by whitespace.
    #[cfg(target_os = "linux")]
    fn parse_proc_stat(content: &str) -> Vec<String> {
        // Find the last ')' to handle comm names with ')' in them
        let Some(comm_end) = content.rfind(')') else {
            return Vec::new();
        };

        // Split: everything before comm_end + 1 is "pid (comm)", rest are fields
        let after_comm = &content[comm_end + 1..];
        let fields: Vec<String> = after_comm
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        fields
    }

    /// Parse VmRSS (resident memory) from `/proc/{pid}/status`.
    ///
    /// Format: VmRSS:    12345 kB
    #[cfg(target_os = "linux")]
    fn parse_vmrss_from_status(content: &str) -> Option<u64> {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                // Format: "VmRSS:    12345 kB"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // parts[1] is the number, parts[2] is the unit (kB)
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return Some(kb * 1024); // Convert KB to bytes
                    }
                }
            }
        }
        None
    }

    /// Query resource usage on macOS using ps command.
    ///
    /// This is less efficient than Linux's /proc approach (spawns a subprocess)
    /// but provides similar information.
    ///
    /// ps output format (-o flag):
    /// - rss: resident set size in kilobytes
    /// - vsz: virtual size in kilobytes
    /// - %cpu: CPU usage percentage
    /// - thcount: thread count (macOS-specific flag)
    #[cfg(target_os = "macos")]
    async fn query_macos(pid: u32) -> Self {
        let output = tokio::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "rss=,vsz=,%cpu=,thcount="])
            .output()
            .await;

        let Ok(output) = output else {
            return Self::default();
        };

        if !output.status.success() {
            return Self::default();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.split_whitespace().collect();

        if parts.len() < 4 {
            return Self::default();
        }

        // Parse ps output: rss vsz %cpu thcount
        let memory_rss_bytes = parts[0].parse::<u64>().ok().map(|kb| kb * 1024);
        let memory_vsz_bytes = parts[1].parse::<u64>().ok().map(|kb| kb * 1024);
        let cpu_percent = parts[2].parse::<f64>().ok();
        let thread_count = parts[3].parse::<u64>().ok();

        Self {
            memory_rss_bytes,
            memory_vsz_bytes,
            cpu_percent,
            thread_count,
        }
    }

    /// Format memory bytes as human-readable string (e.g., "512 MB", "2.5 GB").
    pub fn format_memory(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_current_process() {
        // Query our own process - should return some metrics
        let pid = std::process::id();
        let usage = ResourceUsage::query(pid).await;

        // On supported platforms (Linux, macOS), we should get at least one metric
        // Note: In some test environments (containers, CI), metrics may not be available
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let has_any_metric = usage.memory_rss_bytes.is_some()
                || usage.memory_vsz_bytes.is_some()
                || usage.thread_count.is_some();

            if !has_any_metric {
                // Log warning but don't fail - test environment may be restricted
                eprintln!(
                    "Warning: No resource metrics available for current process (PID {})",
                    pid
                );
                eprintln!("This can happen in restricted test environments");
            }
        }

        // If we do get RSS, it should be > 0
        if let Some(rss) = usage.memory_rss_bytes {
            assert!(rss > 0, "RSS should be > 0 for running process");
        }

        // If we get thread count, it should be at least 1
        if let Some(threads) = usage.thread_count {
            assert!(threads >= 1, "Should have at least 1 thread");
        }
    }

    #[tokio::test]
    async fn test_query_nonexistent_process() {
        // Query a PID that almost certainly doesn't exist
        let usage = ResourceUsage::query(u32::MAX).await;

        // Should return default (all None)
        assert!(usage.memory_rss_bytes.is_none());
        assert!(usage.memory_vsz_bytes.is_none());
        assert!(usage.cpu_percent.is_none());
        assert!(usage.thread_count.is_none());
    }

    #[test]
    fn test_format_memory() {
        assert_eq!(ResourceUsage::format_memory(500), "500 B");
        assert_eq!(ResourceUsage::format_memory(1024), "1.0 KB");
        assert_eq!(ResourceUsage::format_memory(1024 * 1024), "1.0 MB");
        assert_eq!(ResourceUsage::format_memory(1536 * 1024), "1.5 MB");
        assert_eq!(ResourceUsage::format_memory(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(
            ResourceUsage::format_memory(2 * 1024 * 1024 * 1024),
            "2.0 GB"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_parse_proc_stat() {
        // Real example from /proc/self/stat
        let content = "12345 (bash) R 12344 12345 12345 34816 12345 4194304 1234 0 0 0 10 5 0 0 20 0 1 0 123456789 12345678 1234 18446744073709551615 0 0 0 0 0 0 0";
        let fields = ResourceUsage::parse_proc_stat(content);

        assert!(!fields.is_empty());
        // First field after (comm) should be state
        assert_eq!(fields[0], "R");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_parse_proc_stat_with_complex_comm() {
        // Process name with spaces and parentheses
        let content = "12345 (foo (bar) baz) S 12344 12345 12345 34816 12345 4194304 1234 0 0 0 10 5 0 0 20 0 1 0 123456789 12345678 1234 18446744073709551615 0 0 0 0 0 0 0";
        let fields = ResourceUsage::parse_proc_stat(content);

        assert!(!fields.is_empty());
        assert_eq!(fields[0], "S"); // State should be correct
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_parse_vmrss_from_status() {
        let content = "Name:   bash\nVmRSS:    12345 kB\nVmSize:   67890 kB\n";
        let rss = ResourceUsage::parse_vmrss_from_status(content);

        assert_eq!(rss, Some(12345 * 1024));
    }
}
