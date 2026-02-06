use crate::service::Status;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Lock file format - persisted state of running services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    /// PID of the fed process that created this lock
    pub fed_pid: u32,

    /// Working directory when services were started
    pub work_dir: String,

    /// When this orchestration session started
    pub started_at: DateTime<Utc>,

    /// All running services with their state
    pub services: HashMap<String, ServiceState>,

    /// All allocated ports (to prevent conflicts)
    pub allocated_ports: Vec<u16>,
}

impl LockFile {
    pub fn new(work_dir: String) -> Self {
        Self {
            fed_pid: std::process::id(),
            work_dir,
            started_at: Utc::now(),
            services: HashMap::new(),
            allocated_ports: Vec::new(),
        }
    }
}

/// Persisted state for a single service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    /// Fully qualified service name (with namespace)
    pub id: String,

    /// Current status
    pub status: Status,

    /// Service type (process, docker, external, etc.)
    pub service_type: String,

    /// Process ID (if applicable)
    pub pid: Option<u32>,

    /// Docker container ID (if applicable)
    pub container_id: Option<String>,

    /// Port allocations for this service
    pub port_allocations: HashMap<String, u16>,

    /// When this service was started
    pub started_at: DateTime<Utc>,

    /// If this is from an external service, the repo info
    pub external_repo: Option<String>,

    /// Namespace this service belongs to (root, or external service name)
    pub namespace: String,

    /// Number of times this service has been restarted
    #[serde(default)]
    pub restart_count: u32,

    /// When the service was last restarted (if ever)
    #[serde(default)]
    pub last_restart_at: Option<DateTime<Utc>>,

    /// Number of consecutive failures (resets on successful health check)
    #[serde(default)]
    pub consecutive_failures: u32,

    /// Resolved startup message template (for display after start and in status)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_message: Option<String>,
}

impl ServiceState {
    pub fn new(id: String, service_type: String, namespace: String) -> Self {
        Self {
            id,
            status: Status::Starting,
            service_type,
            pid: None,
            container_id: None,
            port_allocations: HashMap::new(),
            started_at: Utc::now(),
            external_repo: None,
            namespace,
            restart_count: 0,
            last_restart_at: None,
            consecutive_failures: 0,
            startup_message: None,
        }
    }

    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    pub fn with_container_id(mut self, container_id: String) -> Self {
        self.container_id = Some(container_id);
        self
    }

    pub fn with_external_repo(mut self, repo: String) -> Self {
        self.external_repo = Some(repo);
        self
    }
}
