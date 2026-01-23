//! Resource limits configuration.
//!
//! This module contains the [`ResourceLimits`] struct for configuring
//! service resource constraints (memory, CPU, file descriptors, etc.).

use serde::{Deserialize, Serialize};

/// Resource limits for services.
///
/// These limits apply to both Docker and process-based services,
/// though the implementation differs:
///
/// - Docker: Maps to container resource flags (--memory, --cpus, etc.)
/// - Process: Maps to rlimit settings or cgroups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Memory limit (e.g., "512m", "2g", "1024mb")
    /// For Docker: maps to --memory flag
    /// For Process: maps to RLIMIT_AS
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,

    /// Soft memory limit (Docker only, maps to --memory-reservation)
    /// Process will be throttled before hitting hard limit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_reservation: Option<String>,

    /// Memory swap limit (Docker only, maps to --memory-swap)
    /// Total memory + swap limit. Use "0" to disable swap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_swap: Option<String>,

    /// CPU limit as decimal (e.g., "0.5" = 50%, "2.0" = 2 CPUs)
    /// For Docker: maps to --cpus flag
    /// For Process: maps to CPU affinity/cgroup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<String>,

    /// CPU shares (relative weight, default 1024)
    /// For Docker: maps to --cpu-shares
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_shares: Option<u32>,

    /// Maximum number of PIDs (processes/threads)
    /// For Docker: maps to --pids-limit
    /// For Process: maps to RLIMIT_NPROC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,

    /// Maximum number of open file descriptors
    /// For Docker: maps to --ulimit nofile
    /// For Process: maps to RLIMIT_NOFILE
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nofile: Option<u32>,

    /// Whether to strictly enforce resource limits (default: false).
    ///
    /// - **If false** (default): Log a warning and continue if setrlimit fails.
    ///   This allows services to run on systems where setrlimit is restricted
    ///   (e.g., inside containers, macOS with SIP enabled).
    ///
    /// - **If true**: Fail service startup if setrlimit fails.
    ///   Use this in production environments where resource limit enforcement
    ///   is critical for security or stability.
    ///
    /// # Platform Notes
    ///
    /// Some platforms restrict setrlimit calls:
    /// - **Docker containers**: May not allow raising limits beyond container limits
    /// - **macOS with SIP**: System Integrity Protection may restrict limits
    /// - **Unprivileged users**: Can only lower limits, not raise them
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict_limits: bool,
}

/// Helper for serde skip_serializing_if
fn is_false(b: &bool) -> bool {
    !b
}
