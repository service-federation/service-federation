/// Result of attempting to stop a service by its persisted state.
pub enum StopResult {
    /// Service was successfully stopped.
    Stopped,
    /// Service stop was skipped (e.g., PID reused, no PID/container).
    Skipped(String),
    /// Service stop was attempted but failed.
    Failed,
}

/// Stop a service using only its persisted state (PID/container ID).
///
/// This encapsulates the common pattern shared by `stop_remaining_state_services`
/// and `run_stop_from_state`: given a `ServiceState`, stop whatever is running.
pub async fn stop_service_by_state(
    name: &str,
    state: &service_federation::state::ServiceState,
) -> StopResult {
    if let Some(container_id) = state.container_id.as_deref() {
        if graceful_docker_stop(container_id).await {
            StopResult::Stopped
        } else {
            StopResult::Failed
        }
    } else if let Some(pid) = state.pid {
        if !validate_pid_start_time(pid, state.started_at) {
            StopResult::Skipped(format!("PID {} was reused by another process", pid))
        } else if graceful_process_kill(pid).await {
            StopResult::Stopped
        } else {
            StopResult::Failed
        }
    } else {
        StopResult::Skipped(format!("no PID or container ID for service '{}'", name))
    }
}

/// Remove orphaned Docker containers for a work directory (no orchestrator needed).
///
/// Finds all containers whose name starts with `fed-{hash}-` (where `hash` is
/// derived from `work_dir`) and force-removes them. Returns the number of
/// containers successfully removed.
pub async fn remove_orphan_containers_for_workdir(work_dir: &std::path::Path) -> usize {
    use service_federation::docker::DockerClient;
    use service_federation::service::hash_work_dir;
    use std::time::Duration;

    let client = DockerClient::new();
    let timeout = Duration::from_secs(10);
    let work_dir_hash = hash_work_dir(work_dir);
    let prefix = format!("fed-{}-", work_dir_hash);

    let containers = match client.ps_names(&format!("name=^{}", prefix), timeout).await {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut removed = 0;
    for container in containers {
        if client.rm_force(&container, timeout).await.is_ok() {
            removed += 1;
        }
    }

    removed
}

/// Gracefully stop a Docker container.
///
/// Tries `docker stop` first (sends SIGTERM, waits), then `docker rm -f`.
pub async fn graceful_docker_stop(container_id: &str) -> bool {
    use service_federation::docker::DockerClient;
    use std::time::Duration;

    let client = DockerClient::new();
    let timeout = Duration::from_secs(30);

    // stop_and_remove: docker stop -t 10, then docker rm -f
    client
        .stop_and_remove(container_id, 10, timeout)
        .await
        .unwrap_or(false)
}

/// Re-export from error module for backwards compatibility with command imports.
pub use service_federation::error::validate_pid_start_time;

/// Gracefully kill a process.
///
/// Sends SIGTERM first, waits up to 5 seconds, then sends SIGKILL.
pub async fn graceful_process_kill(pid: u32) -> bool {
    use std::time::Duration;
    use tokio::process::Command;

    // Check if process exists first
    let exists = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if !exists {
        // Process already dead
        return true;
    }

    // Send SIGTERM
    let term_result = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    if term_result.is_err() {
        return false;
    }

    // Wait for process to exit (up to 5 seconds)
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;

        let still_exists = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        if !still_exists {
            return true;
        }
    }

    // Process didn't exit, send SIGKILL
    let kill_result = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    // Wait a bit more for SIGKILL to take effect
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if finally dead
    let dead = !Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    kill_result.is_ok() && dead
}
