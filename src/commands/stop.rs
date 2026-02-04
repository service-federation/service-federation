use service_federation::{
    config::Config, service::hash_work_dir, state::StateTracker, Orchestrator,
};
use std::path::Path;

use super::lifecycle::{graceful_docker_stop, graceful_process_kill, validate_pid_start_time};

pub async fn run_stop(
    orchestrator: &mut Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    if services.is_empty() {
        println!("Stopping all services...");
        orchestrator.stop_all().await?;

        // If the config changed since services were started, some running services may
        // still exist in state but not in the current config (so stop_all won't see them).
        // Best-effort stop those remaining state-tracked services.
        let extra_stopped = stop_remaining_state_services(orchestrator).await;
        if extra_stopped > 0 {
            println!("Stopped {} additional service(s) from state", extra_stopped);
        }

        // Also remove any orphaned containers (from failed starts, etc.)
        match orchestrator.remove_orphaned_containers().await {
            Ok(count) if count > 0 => {
                println!("Removed {} orphaned container(s)", count);
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: Failed to clean orphaned containers: {}", e);
            }
        }

        // Also remove any orphaned processes (from crashed services, etc.)
        let process_count = orchestrator.remove_orphaned_processes().await;
        if process_count > 0 {
            println!("Killed {} orphaned process(es)", process_count);
        }

        orchestrator.cleanup().await;
    } else {
        // Expand tag references (e.g., @backend) into service names
        let services_to_stop = config.expand_service_selection(&services);

        for service in services_to_stop {
            print!("  Stopping {}...", service);
            match orchestrator.stop(&service).await {
                Ok(_) => println!(" done"),
                Err(e) => println!(" failed ({})", e),
            }
        }
        orchestrator.state_tracker.write().await.save().await?;
    }
    println!("Services stopped");

    Ok(())
}

fn state_status_is_active(status: &str) -> bool {
    matches!(
        status,
        "running" | "healthy" | "starting" | "failing" | "stopping"
    )
}

/// Stop any services that remain in state after config-based stop operations.
///
/// This catches cases where services are running but no longer appear in the
/// current config (e.g., services renamed/removed).
async fn stop_remaining_state_services(orchestrator: &Orchestrator) -> usize {
    use service_federation::state::SqliteStateTracker;

    // Avoid holding the outer RwLock across await by cloning the DB connection.
    let conn = orchestrator.state_tracker.read().await.clone_connection();
    let services = SqliteStateTracker::fetch_services_from_connection(&conn).await;

    if services.is_empty() {
        return 0;
    }

    let mut stopped_names: Vec<String> = Vec::new();

    for (name, state) in services {
        if !state_status_is_active(state.status.as_str()) {
            continue;
        }

        // These services are not necessarily present in the current config.
        // Stop by state (PID/container) and unregister regardless of config.
        print!("  Stopping {} (from state)...", name);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let mut note: Option<String> = None;
        let success = if let Some(container_id) = state.container_id.as_deref() {
            graceful_docker_stop(container_id).await
        } else if let Some(pid) = state.pid {
            if !validate_pid_start_time(pid, state.started_at) {
                // The recorded PID is not safe to signal; drop this state entry.
                note = Some(format!(
                    "skipped (PID {} was reused by another process)",
                    pid
                ));
                true
            } else {
                graceful_process_kill(pid).await
            }
        } else {
            // No PID/container - nothing to stop
            note = Some("skipped (no PID/container)".to_string());
            true
        };

        if success {
            if let Some(note) = note {
                println!(" {}", note);
            } else {
                println!(" done");
            }
            stopped_names.push(name);
        } else {
            println!(" failed");
        }
    }

    if stopped_names.is_empty() {
        return 0;
    }

    let mut tracker = orchestrator.state_tracker.write().await;
    for name in &stopped_names {
        let _ = tracker.unregister_service(name).await;
    }
    let _ = tracker.save().await;

    stopped_names.len()
}

/// Stop services using only the state tracker (no config required).
/// Used when config is invalid but we still need to stop running services.
pub async fn run_stop_from_state(work_dir: &Path, services: Vec<String>) -> anyhow::Result<()> {
    let mut tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    tracker.initialize().await?;

    let all_services = tracker.get_services().await;
    if all_services.is_empty() {
        println!("No services found in state tracker.");
        return Ok(());
    }

    let services_to_stop: Vec<_> = if services.is_empty() {
        all_services.into_iter().collect()
    } else {
        // No tag expansion in fallback mode — use names directly
        all_services
            .into_iter()
            .filter(|(name, _)| services.contains(name))
            .collect()
    };

    println!(
        "Stopping {} service(s) from state tracker...",
        services_to_stop.len()
    );

    for (name, state) in &services_to_stop {
        let is_active = matches!(state.status.as_str(), "running" | "healthy" | "starting");
        if !is_active {
            continue;
        }

        print!("  Stopping {}...", name);

        let mut note: Option<String> = None;
        let stopped = if let Some(container_id) = state.container_id.as_deref() {
            graceful_docker_stop(container_id).await
        } else if let Some(pid) = state.pid {
            if !validate_pid_start_time(pid, state.started_at) {
                note = Some(format!(
                    "skipped (PID {} was reused by another process)",
                    pid
                ));
                true
            } else {
                graceful_process_kill(pid).await
            }
        } else {
            note = Some("skipped (no PID/container)".to_string());
            true
        };

        if stopped {
            if let Some(note) = note {
                println!(" {}", note);
            } else {
                println!(" done");
            }
        } else {
            println!(" failed");
        }
    }

    // Update state:
    // - stop all: clear entire DB
    // - stop subset: only unregister the named services (leave others intact)
    if services.is_empty() {
        tracker.clear().await?;
    } else {
        for (name, _) in &services_to_stop {
            let _ = tracker.unregister_service(name).await;
        }
        tracker.save().await?;
    }

    // Also remove any orphaned containers not in state DB
    let work_dir_hash = hash_work_dir(work_dir);
    let prefix = format!("fed-{}-", work_dir_hash);

    let output = tokio::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name=^{}", prefix),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .await;

    if let Ok(output) = output {
        if output.status.success() {
            let containers: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let mut removed = 0;
            for container in containers {
                let rm_output = tokio::process::Command::new("docker")
                    .args(["rm", "-f", &container])
                    .output()
                    .await;

                if let Ok(out) = rm_output {
                    if out.status.success() {
                        removed += 1;
                    }
                }
            }

            if removed > 0 {
                println!("Removed {} orphaned container(s)", removed);
            }
        }
    }

    println!("Services stopped");

    Ok(())
}
