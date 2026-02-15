//! Orphan detection and cleanup for containers and processes.
//!
//! This module contains the `OrphanCleaner` struct which encapsulates all orphan
//! detection and cleanup logic that was previously part of the main Orchestrator.
//! Extracting these operations improves separation of concerns and keeps the
//! orchestrator core focused on service coordination.

use std::time::Duration;

use crate::config::ServiceType;
use crate::docker::DockerError;
use crate::error::Result;
use crate::service::Status;

use super::core::Orchestrator;

/// Timeout for Docker list/inspect operations during orphan detection.
const DOCKER_LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// Short-lived helper for detecting and removing orphaned containers and processes.
///
/// Constructed on-demand from an `Orchestrator` reference. Orphan methods on
/// `Orchestrator` delegate here after constructing an `OrphanCleaner`.
pub(super) struct OrphanCleaner<'a> {
    orchestrator: &'a Orchestrator,
}

impl<'a> OrphanCleaner<'a> {
    pub fn new(orchestrator: &'a Orchestrator) -> Self {
        Self { orchestrator }
    }

    /// Detect Docker containers for this project that aren't tracked in state DB.
    ///
    /// Returns a list of container names that match this project's naming pattern
    /// (fed-{work_dir_hash}-*) but aren't found in the state tracker.
    ///
    /// This is a conservative detection - we don't auto-remove, just warn.
    pub async fn detect_untracked_containers(&self) -> Result<Vec<String>> {
        use crate::service::{hash_work_dir, sanitize_container_name_component};
        use std::collections::HashSet;

        let work_dir_hash = hash_work_dir(&self.orchestrator.work_dir);
        let prefix = format!("fed-{}-", work_dir_hash);

        // List all containers matching our project prefix
        let output = tokio::time::timeout(
            DOCKER_LIST_TIMEOUT,
            tokio::process::Command::new("docker")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    &format!("name=^{}", prefix),
                    "--format",
                    "{{.Names}}",
                ])
                .output(),
        )
        .await
        .map_err(|_| DockerError::timeout("docker ps", DOCKER_LIST_TIMEOUT))?
        .map_err(|e| DockerError::exec_failed("docker ps", e))?;

        if !output.status.success() {
            return Err(DockerError::failed("docker ps", &output).into());
        }

        let all_containers: HashSet<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if all_containers.is_empty() {
            return Ok(Vec::new());
        }

        // Get tracked services from state DB
        let state = self.orchestrator.state_tracker.read().await;
        let tracked_services = state.get_services().await;

        // Build set of expected container names for tracked Docker services
        let tracked_containers: HashSet<String> = tracked_services
            .iter()
            .filter(|(_, svc)| svc.service_type == ServiceType::Docker)
            .map(|(name, _)| {
                let sanitized = sanitize_container_name_component(name);
                format!("{}{}", prefix, sanitized)
            })
            .collect();

        // Find containers that exist but aren't tracked
        let orphans: Vec<String> = all_containers
            .difference(&tracked_containers)
            .cloned()
            .collect();

        Ok(orphans)
    }

    /// Detect orphaned processes - PIDs in state that are still running
    /// but the service is marked as stopped/stale.
    ///
    /// Returns a list of (service_name, pid) tuples for orphaned processes.
    pub async fn detect_orphan_processes(&self) -> Vec<(String, u32)> {
        let state = self.orchestrator.state_tracker.read().await;
        let services = state.get_services().await;
        let mut orphans = Vec::new();

        for (name, svc) in services {
            // Only check services with PIDs that are marked as stopped/stale
            if let Some(pid) = svc.pid {
                let is_stopped = matches!(svc.status, Status::Stopped | Status::Failing);

                if is_stopped && super::core::is_pid_alive(pid) {
                    orphans.push((name, pid));
                }
            }
        }

        orphans
    }

    /// Remove orphaned containers for this project.
    ///
    /// Finds containers matching `fed-{work_dir_hash}-*` that aren't tracked
    /// in the state database and removes them with `docker rm -f`.
    ///
    /// Returns the number of containers removed.
    pub async fn remove_orphaned_containers(&self) -> Result<usize> {
        let orphans = self.detect_untracked_containers().await?;

        if orphans.is_empty() {
            return Ok(0);
        }

        let mut removed = 0;
        for container in &orphans {
            tracing::info!("Removing orphaned container: {}", container);
            let output = tokio::time::timeout(
                DOCKER_LIST_TIMEOUT,
                tokio::process::Command::new("docker")
                    .args(["rm", "-f", container])
                    .output(),
            )
            .await
            .map_err(|_| DockerError::timeout("docker rm", DOCKER_LIST_TIMEOUT))?
            .map_err(|e| DockerError::exec_failed("docker rm", e))?;

            if output.status.success() {
                removed += 1;
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Ignore "No such container" - already gone
                if !stderr.contains("No such container") {
                    tracing::warn!(
                        "Failed to remove orphaned container '{}': {}",
                        container,
                        stderr.trim()
                    );
                }
            }
        }

        Ok(removed)
    }

    /// Remove orphaned processes for this project.
    ///
    /// Finds processes with PIDs in state DB that are still running
    /// but the service is marked as stopped. Kills them with SIGKILL.
    ///
    /// Returns the number of processes killed.
    pub async fn remove_orphaned_processes(&self) -> usize {
        let orphans = self.detect_orphan_processes().await;

        if orphans.is_empty() {
            return 0;
        }

        let mut killed = 0;
        for (name, pid) in &orphans {
            tracing::info!("Killing orphaned process: {} (PID {})", name, pid);

            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::{getpgid, Pid};

                let nix_pid = Pid::from_raw(*pid as i32);

                // Use killpg() for process group leaders so child processes
                // are also cleaned up, matching ProcessService::stop() behavior.
                let pgid = getpgid(Some(nix_pid)).ok();
                let is_group_leader = pgid == Some(nix_pid);

                let send_signal = |sig: Signal| -> bool {
                    if is_group_leader {
                        signal::killpg(nix_pid, sig).is_ok()
                    } else {
                        signal::kill(nix_pid, sig).is_ok()
                    }
                };

                let is_alive = || -> bool {
                    if is_group_leader {
                        signal::killpg(nix_pid, None).is_ok()
                    } else {
                        signal::kill(nix_pid, None).is_ok()
                    }
                };

                // Try SIGTERM first
                if send_signal(Signal::SIGTERM) {
                    // Wait briefly for graceful shutdown
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                    // If still alive, SIGKILL
                    if is_alive() {
                        let _ = send_signal(Signal::SIGKILL);
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    killed += 1;
                }
            }

            #[cfg(not(unix))]
            {
                tracing::warn!("Cannot kill orphaned process on non-Unix platform");
            }
        }

        // Clear the PIDs from state
        if killed > 0 {
            let mut state = self.orchestrator.state_tracker.write().await;
            for (name, _) in &orphans {
                if let Err(e) = state.unregister_service(name).await {
                    tracing::warn!(
                        "Failed to unregister orphaned service '{}' from state: {}",
                        name,
                        e
                    );
                }
            }
            if let Err(e) = state.save().await {
                tracing::warn!("Failed to save state after orphan cleanup: {}", e);
            }
        }

        killed
    }
}
