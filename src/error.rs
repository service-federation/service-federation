// Allow unused_assignments at module level because thiserror's generated code
// for struct variants triggers false positive warnings - the fields ARE used
// in the Display impl but rustc's lint pass doesn't see this.
#![allow(unused_assignments)]

use miette::Diagnostic;
use std::io;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Service not found: {0}")]
    #[diagnostic(
        code(fed::service::not_found),
        help(
            "Check available services with `fed status` or validate your service-federation.yaml"
        )
    )]
    ServiceNotFound(String),

    #[error("Parameter not found: {0}")]
    #[diagnostic(
        code(fed::parameter::not_found),
        help("Declare the parameter in your service-federation.yaml:\n\nparameters:\n  {0}:\n    default: \"value\"")
    )]
    ParameterNotFound(String),

    #[error("Circular dependency detected: {}", .0.join(" -> "))]
    #[diagnostic(
        code(fed::dependency::circular),
        help("Services cannot depend on each other in a cycle. Review the depends_on fields")
    )]
    CircularDependency(Vec<String>),

    #[error("Invalid parameter value for '{name}': {reason}")]
    #[diagnostic(
        code(fed::parameter::invalid),
        help("Check the parameter type and constraints in your service-federation.yaml")
    )]
    InvalidParameter { name: String, reason: String },

    #[error("Service '{0}' is not running")]
    #[diagnostic(
        code(fed::service::not_running),
        help("Start the service with: fed start {0}")
    )]
    ServiceNotRunning(String),

    #[error("Service '{0}' failed to start: {1}")]
    #[diagnostic(
        code(fed::service::start_failed),
        help("Check the service logs with `fed logs {0}`\nVerify the command exists and is executable")
    )]
    ServiceStartFailed(String, String),

    #[error("Service '{0}' health check failed: {1}")]
    #[diagnostic(
        code(fed::service::health_check_failed),
        help("Check the service logs with `fed logs {0}`\nVerify the healthcheck URL/command is correct in your config")
    )]
    HealthCheckFailed(String, String),

    #[error("Port allocation failed: {0}")]
    #[diagnostic(
        code(fed::port::allocation_failed),
        help("Try using 'type: port' parameters which auto-fallback to available ports")
    )]
    PortAllocation(String),

    #[error("Port {port} is in use{}",
        .process_name.as_ref()
            .zip(.pid.as_ref())
            .map(|(name, pid)| format!(" by process '{}' (PID {})", name, pid))
            .unwrap_or_default()
    )]
    #[diagnostic(
        code(fed::port::conflict),
        help("Find what's using the port with: lsof -i :{port} (macOS/Linux) or netstat -ano | findstr :{port} (Windows)\nTo free the port, stop the conflicting process or use a different port in your config")
    )]
    PortConflict {
        port: u16,
        pid: Option<u32>,
        process_name: Option<String>,
    },

    #[error("{service} – port {port} already allocated")]
    #[diagnostic(
        code(fed::docker::port_conflict),
        help("To stop existing processes and containers run:\n\n    fed start --replace")
    )]
    DockerPortConflict { service: String, port: u16 },

    #[error("Operation aborted by user")]
    Aborted,

    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Template resolution error: {0}")]
    TemplateResolution(String),

    #[error("Timeout waiting for service '{0}'")]
    #[diagnostic(
        code(fed::service::timeout),
        help(
            "The service may be slow to start. Check logs with `fed logs {0}` or increase timeout"
        )
    )]
    Timeout(String),

    #[error("Operation cancelled for service '{0}'")]
    Cancelled(String),

    #[error("Multiple errors occurred:\n{}", .0.iter().map(|e| format!("  - {}", e)).collect::<Vec<_>>().join("\n"))]
    Multiple(Vec<Error>),

    #[error("Invalid configuration: {0}")]
    #[diagnostic(
        code(fed::config::validation),
        help("Run `fed validate` for detailed validation errors")
    )]
    Validation(String),

    #[error("Package error: {0}")]
    Package(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Invalid package source: {0}")]
    InvalidPackageSource(String),

    #[error("Package authentication failed: {0}")]
    PackageAuthFailed(String),

    #[error("Package version not found: {0}")]
    PackageVersionNotFound(String),

    #[error("Package cache error: {0}")]
    PackageCache(String),

    #[error("Package merge conflict: {0}")]
    PackageMergeConflict(String),

    #[error("Circular package dependency detected")]
    CircularPackageDependency,

    #[error("Diamond dependency detected for package '{identity}':\n{conflicts_formatted}")]
    #[diagnostic(
        code(fed::package::diamond_dependency),
        help("Pin all packages to use the same version of '{identity}', or restructure your dependencies to avoid the conflict")
    )]
    DiamondDependency {
        identity: String,
        conflicts_formatted: String,
    },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    #[diagnostic(
        code(fed::database::error),
        help("See recovery suggestions based on the specific error type")
    )]
    Database(#[from] tokio_rusqlite::Error),

    #[error("Invalid PID {pid}: {reason}")]
    InvalidPid { pid: u32, reason: String },

    #[error("Session error: {0}")]
    #[diagnostic(
        code(fed::session::error),
        help(
            "List sessions with `fed session list` or start a new session with `fed session start`"
        )
    )]
    Session(String),

    #[error("Docker Compose error: {0}")]
    #[diagnostic(
        code(fed::docker::compose),
        help("Verify Docker is running with `docker ps` and check docker-compose.yml syntax")
    )]
    DockerCompose(String),

    #[error("Script not found: {0}")]
    #[diagnostic(
        code(fed::script::not_found),
        help("List available scripts in the 'scripts:' section of your service-federation.yaml")
    )]
    ScriptNotFound(String),

    #[error("Environment file '{env_file}' sets undeclared parameter '{name}': declare it in service-federation.yaml or remove from .env")]
    #[diagnostic(
        code(fed::env::undeclared),
        help("Add parameter declaration:\n\nparameters:\n  {name}:\n    default: \"\"")
    )]
    UndeclaredEnvVariable { name: String, env_file: String },

    #[error("Script '{name}' exited with code {exit_code}")]
    #[diagnostic(code(fed::script::failed))]
    ScriptFailed { name: String, exit_code: i32 },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Returns a helpful suggestion for resolving this error, if available.
    pub fn suggestion(&self) -> Option<String> {
        match self {
            Error::ServiceNotFound(name) => Some(format!(
                "Did you mean to run 'fed start {}' first? Or check 'fed status' for running services.",
                name
            )),
            Error::ServiceNotRunning(name) => Some(format!(
                "Start the service with: fed start {}",
                name
            )),
            Error::PortConflict { port, pid, process_name } => {
                let kill_hint = match (pid, process_name) {
                    (Some(p), Some(name)) => format!("To free the port, stop '{}' (PID {}) or use a different port.", name, p),
                    (Some(p), None) => format!("To free the port, kill PID {} or use a different port.", p),
                    _ => "To use a different port, add 'default: <port>' to your parameter or let fed auto-allocate.".to_string(),
                };
                Some(format!(
                    "Port {} is already in use. {}",
                    port, kill_hint
                ))
            }
            Error::PortAllocation(msg) => Some(format!(
                "Port allocation failed: {}. Try using 'type: port' parameters which auto-fallback to available ports.",
                msg
            )),
            Error::DockerPortConflict { port, .. } => Some(format!(
                "Port {} is in use by another container or process.\n\nTo stop existing processes and containers run:\n\n    fed start --replace",
                port
            )),
            Error::CircularDependency(path) => Some(format!(
                "Services cannot depend on each other in a cycle. Review the depends_on fields for: {}",
                path.join(", ")
            )),
            Error::HealthCheckFailed(service, _) => Some(format!(
                "Check the service logs with: fed logs {}\nAlso verify the healthcheck URL/command is correct in your config.",
                service
            )),
            Error::Session(msg) if msg.contains("not found") => Some(
                "Start a session with: fed session start --id <name>".to_string()
            ),
            Error::Config(_) | Error::Validation(_) => Some(
                "Validate your config with: fed validate".to_string()
            ),
            Error::ScriptNotFound(name) => Some(format!(
                "Available scripts are defined in the 'scripts:' section of your config. Did you mean a different name than '{}'?",
                name
            )),
            Error::UndeclaredEnvVariable { name, .. } => Some(format!(
                "Add a parameter declaration for '{}' in your service-federation.yaml:\n\nparameters:\n  {}:\n    default: \"\"",
                name, name
            )),
            Error::Database(e) => {
                // Match on specific SQLite error codes for targeted recovery hints
                let err_str = e.to_string();
                if err_str.contains("database is locked") || err_str.contains("SQLITE_BUSY") {
                    Some(
                        "Another fed instance may be running. Use `fed stop` to stop all services, or check for stale lock files:\n  rm .fed/.lock\n\nIf the issue persists, find and stop other fed processes.".to_string()
                    )
                } else if err_str.contains("disk I/O error") || err_str.contains("SQLITE_IOERR") {
                    Some(
                        "Database disk I/O error detected. Check:\n  1. Disk space: df -h\n  2. File permissions on .fed/ directory\n  3. File system health\n\nIf the issue persists, backup and remove .fed/ directory.".to_string()
                    )
                } else if err_str.contains("database disk image is malformed") || err_str.contains("SQLITE_CORRUPT") {
                    Some(
                        "Database corrupted. Backup the database and try recovery:\n  cp .fed/lock.db .fed/lock.db.backup\n  sqlite3 .fed/lock.db '.recover' | sqlite3 .fed/lock.db.recovered\n\nIf recovery fails, remove .fed/ directory as a last resort:\n  rm -rf .fed/".to_string()
                    )
                } else {
                    Some(
                        "Database error encountered. If the issue persists, you can reset fed's state by running:\n  rm -rf .fed/\n\nThis will remove all service state and port allocations. Running services will need to be restarted.".to_string()
                    )
                }
            }
            _ => None,
        }
    }

    /// Formats the error with its suggestion (if any) for user-friendly display.
    pub fn with_suggestion(&self) -> String {
        match self.suggestion() {
            Some(suggestion) => format!("{}\n\nHint: {}", self, suggestion),
            None => self.to_string(),
        }
    }
}

/// Validates and converts a u32 PID to nix::unistd::Pid safely.
/// Returns Err for PID 0 (process group), PID 1 (init), or values > i32::MAX.
pub fn validate_pid(pid: u32, service_name: &str) -> Result<nix::unistd::Pid> {
    if pid == 0 {
        return Err(Error::InvalidPid {
            pid,
            reason: format!(
                "PID 0 is invalid for service '{}' (refers to process group, not a process)",
                service_name
            ),
        });
    }
    if pid == 1 {
        return Err(Error::InvalidPid {
            pid,
            reason: format!(
                "refusing to operate on PID 1 (init) for service '{}'",
                service_name
            ),
        });
    }
    if pid > i32::MAX as u32 {
        return Err(Error::InvalidPid {
            pid,
            reason: format!(
                "PID {} exceeds i32::MAX for service '{}', cannot convert safely",
                pid, service_name
            ),
        });
    }
    Ok(nix::unistd::Pid::from_raw(pid as i32))
}

/// Same as validate_pid but allows PID 1 check to be skipped for existence checks.
/// Use validate_pid for signal operations; use this for read-only checks.
pub fn validate_pid_for_check(pid: u32) -> Option<nix::unistd::Pid> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    Some(nix::unistd::Pid::from_raw(pid as i32))
}
