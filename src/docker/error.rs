use std::fmt;
use std::time::Duration;

/// Structured error type for Docker CLI operations.
///
/// Replaces the previous `Error::Docker(String)` with machine-actionable variants
/// that enable better error messages and programmatic error handling.
#[derive(Debug)]
pub enum DockerError {
    /// Docker command timed out.
    Timeout { command: String, timeout: Duration },

    /// Docker command ran but returned non-zero exit.
    CommandFailed {
        command: String,
        stderr: String,
        exit_code: Option<i32>,
    },

    /// Docker binary couldn't be executed (not in PATH, permission denied).
    ExecFailed {
        command: String,
        source: std::io::Error,
    },

    /// Container doesn't exist (parsed from "No such container" stderr).
    ContainerNotFound { container: String },

    /// Docker daemon not responding.
    DaemonUnavailable,
}

impl DockerError {
    /// Create a timeout error.
    pub fn timeout(cmd: impl Into<String>, dur: Duration) -> Self {
        DockerError::Timeout {
            command: cmd.into(),
            timeout: dur,
        }
    }

    /// Create a command-failed error from an `std::process::Output`.
    pub fn failed(cmd: impl Into<String>, output: &std::process::Output) -> Self {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        DockerError::CommandFailed {
            command: cmd.into(),
            stderr,
            exit_code: output.status.code(),
        }
    }

    /// Create a command-failed error from a stderr string and optional exit code.
    pub fn cmd_failed(
        cmd: impl Into<String>,
        stderr: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        DockerError::CommandFailed {
            command: cmd.into(),
            stderr: stderr.into(),
            exit_code,
        }
    }

    /// Create an exec-failed error (binary not found / permission denied).
    pub fn exec_failed(cmd: impl Into<String>, err: std::io::Error) -> Self {
        DockerError::ExecFailed {
            command: cmd.into(),
            source: err,
        }
    }
}

impl fmt::Display for DockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DockerError::Timeout { command, timeout } => {
                write!(
                    f,
                    "Timed out running '{}' (exceeded {} seconds)",
                    command,
                    timeout.as_secs()
                )
            }
            DockerError::CommandFailed {
                command,
                stderr,
                exit_code,
            } => {
                if let Some(code) = exit_code {
                    write!(f, "'{}' failed (exit code {}): {}", command, code, stderr)
                } else {
                    write!(f, "'{}' failed: {}", command, stderr)
                }
            }
            DockerError::ExecFailed { command, source } => {
                write!(f, "Failed to execute '{}': {}", command, source)
            }
            DockerError::ContainerNotFound { container } => {
                write!(f, "No such container: {}", container)
            }
            DockerError::DaemonUnavailable => {
                write!(f, "Docker daemon is not responding")
            }
        }
    }
}

impl std::error::Error for DockerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DockerError::ExecFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}
