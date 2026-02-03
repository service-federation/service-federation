use service_federation::{config::Config, service::hash_work_dir, state::StateTracker, Orchestrator};
use std::path::Path;

pub async fn run_stop(
    orchestrator: &mut Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    if services.is_empty() {
        println!("Stopping all services...");
        orchestrator.stop_all().await?;

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

        match state.service_type.as_str() {
            "Docker" => {
                if let Some(container_id) = &state.container_id {
                    match tokio::process::Command::new("docker")
                        .args(["rm", "-f", container_id])
                        .output()
                        .await
                    {
                        Ok(output) if output.status.success() => println!(" done"),
                        Ok(output) => {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            println!(" failed (docker rm: {})", stderr.trim());
                        }
                        Err(e) => println!(" failed ({})", e),
                    }
                } else {
                    println!(" skipped (no container ID)");
                }
            }
            _ => {
                // Process, Gradle, or other PID-based services
                if let Some(pid) = state.pid {
                    use nix::sys::signal::{self, Signal};
                    use nix::unistd::Pid;
                    let nix_pid = Pid::from_raw(pid as i32);
                    // Send SIGTERM first
                    match signal::kill(nix_pid, Signal::SIGTERM) {
                        Ok(()) => {
                            // Wait briefly for graceful shutdown
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            // Check if still alive, SIGKILL if needed
                            if signal::kill(nix_pid, None).is_ok() {
                                let _ = signal::kill(nix_pid, Signal::SIGKILL);
                            }
                            println!(" done");
                        }
                        Err(_) => {
                            // Process already gone
                            println!(" done (already stopped)");
                        }
                    }
                } else {
                    println!(" skipped (no PID)");
                }
            }
        }
    }

    // Clear all stopped services from state
    tracker.clear().await?;

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
