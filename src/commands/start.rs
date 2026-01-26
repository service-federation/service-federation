use service_federation::{
    config::{Config, ServiceType},
    port::PortConflict,
    service::Status,
    Orchestrator, WatchMode,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub async fn run_start(
    orchestrator: &mut Orchestrator,
    config: &Config,
    services: Vec<String>,
    watch: bool,
    replace: bool,
    dry_run: bool,
    config_path: &std::path::Path,
) -> anyhow::Result<()> {
    let services_to_start = if services.is_empty() {
        // Use entrypoint
        if let Some(ref ep) = config.entrypoint {
            vec![ep.clone()]
        } else if !config.entrypoints.is_empty() {
            config.entrypoints.clone()
        } else {
            println!("No services specified and no entrypoint configured");
            return Ok(());
        }
    } else {
        // Expand tag references (e.g., @backend) into service names
        config.expand_service_selection(&services)
    };

    // Handle dry run mode - show what would happen without starting services
    if dry_run {
        return run_dry_run(orchestrator, config, services_to_start).await;
    }

    // If --replace is set, first stop any fed-managed services gracefully,
    // then kill any remaining external processes occupying required ports
    if replace {
        // First, gracefully stop services from previous sessions
        let stopped_services = stop_previous_session_services(orchestrator).await;
        if stopped_services > 0 {
            println!(
                "Stopped {} service(s) from previous session\n",
                stopped_services
            );
            // Give a moment for ports to be fully released
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Only try to free ports if we didn't stop any fed services
        // (if we did, the ports should be available now, or in TIME_WAIT briefly)
        let mut freed_any = false;
        if stopped_services == 0 {
            let params = orchestrator.get_resolved_parameters();

            for name in orchestrator.get_port_parameter_names() {
                let Some(value) = params.get(name) else {
                    continue;
                };
                let Ok(port) = value.parse::<u16>() else {
                    continue;
                };
                let Some(conflict) = PortConflict::check(port) else {
                    continue;
                };

                print!("Freeing port {} ({})... ", port, name);
                match conflict.free_port() {
                    Ok(msg) => {
                        println!("{}", msg);
                        freed_any = true;
                    }
                    Err(e) => {
                        println!("\x1b[31mfailed: {}\x1b[0m", e);
                    }
                }
            }

            if freed_any {
                println!();
            }
        }
    }

    // Show what we're about to start with their dependencies
    let dep_graph = orchestrator.get_dependency_graph();
    for service in &services_to_start {
        let deps = dep_graph.get_dependencies(service);
        if deps.is_empty() {
            println!("Starting: {}", service);
        } else {
            println!("Starting: {} (with deps: {})", service, deps.join(", "));
        }
    }
    println!();

    // Set up Ctrl+C handler during startup to allow aborting
    let startup_abort = Arc::new(AtomicBool::new(false));
    let startup_abort_clone = startup_abort.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        startup_abort_clone.store(true, Ordering::SeqCst);
    });

    // Track which services we've already started (to avoid duplicate messages)
    let mut started: std::collections::HashSet<String> = std::collections::HashSet::new();

    for service in &services_to_start {
        // Check if user aborted during startup
        if startup_abort.load(Ordering::SeqCst) {
            println!("\n\nStartup aborted. Cleaning up...");
            orchestrator.cleanup().await;
            println!("Cleanup complete");
            return Ok(());
        }

        // Get dependencies for this service
        let deps = orchestrator
            .get_dependency_graph()
            .get_dependencies(service);

        // Start dependencies first (show progress)
        for dep in &deps {
            if !started.contains(dep) {
                print!("  {} (dependency)...", dep);
                std::io::Write::flush(&mut std::io::stdout())?;
                match orchestrator.start(dep).await {
                    Ok(_) => {
                        println!(" ready");
                        started.insert(dep.clone());
                    }
                    Err(e) => {
                        println!(" \x1b[31mfailed\x1b[0m");
                        eprintln!(
                            "\n\x1b[31mError starting dependency '{}': {}\x1b[0m",
                            dep, e
                        );
                        orchestrator.cleanup().await;
                        return Err(e.into());
                    }
                }
            }
        }

        // Start the main service
        if !started.contains(service) {
            print!("  {}...", service);
            std::io::Write::flush(&mut std::io::stdout())?;
            match orchestrator.start(service).await {
                Ok(_) => {
                    println!(" ready");
                    started.insert(service.clone());
                }
                Err(e) => {
                    println!(" \x1b[31mfailed\x1b[0m");
                    eprintln!("\n\x1b[31mError: {}\x1b[0m", e);

                    // If service not found, show available services
                    if e.to_string().contains("Service not found") {
                        let status = orchestrator.get_status().await;
                        if !status.is_empty() {
                            eprintln!("\nAvailable services:");
                            for name in status.keys() {
                                eprintln!("  - {}", name);
                            }
                        }
                        eprintln!(
                            "\nHint: Check your service-federation.yaml or run 'fed validate'"
                        );
                    }

                    orchestrator.cleanup().await;
                    return Err(e.into());
                }
            }
        }
    }

    println!("\nAll services started successfully!");

    // Mark startup complete - enables monitoring to clean up dead services
    orchestrator.mark_startup_complete();

    // Print resolved parameters
    let params = orchestrator.get_resolved_parameters();
    if !params.is_empty() {
        println!("\nResolved parameters:");
        for (key, value) in params {
            println!("  {}: {}", key, value);
        }
    }

    // Print status and check for failing services
    println!("\nService Status:");
    let status = orchestrator.get_status().await;
    let params = orchestrator.get_resolved_parameters();

    // Collect port conflicts for all port parameters
    let mut port_conflicts: Vec<(String, u16, String, Option<u32>)> = Vec::new();
    let has_failing = status.values().any(|s| *s == Status::Failing);

    for (name, stat) in &status {
        let status_str = match stat {
            Status::Running => "Running",
            Status::Healthy => "Healthy",
            Status::Failing => "Failing",
            Status::Stopped => "Stopped",
            Status::Starting => "Starting",
            Status::Stopping => "Stopping",
        };
        println!("  {}: {}", name, status_str);
    }

    // If any services are failing, check ALL port parameters for conflicts
    if has_failing {
        // Collect PIDs of all fed-managed services to filter them out
        let mut managed_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for name in status.keys() {
            if let Ok(Some(pid)) = orchestrator.get_service_pid(name).await {
                managed_pids.insert(pid);
            }
        }

        // Collect names of running/healthy services to match against process names
        // This helps identify when our own services are holding ports
        let running_services: std::collections::HashSet<String> = status
            .iter()
            .filter(|(_, s)| matches!(s, Status::Running | Status::Healthy))
            .map(|(name, _)| name.to_lowercase())
            .collect();

        // Check if any Docker services are running (to skip com.docker.backend as "conflict")
        let has_running_docker_services = orchestrator.has_docker_services()
            && running_services
                .iter()
                .any(|name| orchestrator.is_docker_service(name));

        // Check if any process-based services are running (non-Docker, non-Gradle)
        // These typically run as node/npm/bun/python/java etc.
        let has_running_process_services = running_services
            .iter()
            .any(|name| orchestrator.is_process_service(name));

        for param_name in orchestrator.get_port_parameter_names() {
            let Some(param_value) = params.get(param_name) else {
                continue;
            };
            let Ok(port) = param_value.parse::<u16>() else {
                continue;
            };
            let Some(conflict) = PortConflict::check(port) else {
                continue;
            };

            for process in &conflict.processes {
                // Skip if this is a fed-managed service (by PID)
                if managed_pids.contains(&process.pid) {
                    continue;
                }

                // Skip Docker daemon if we have running Docker services
                // (it holds ports on behalf of containers)
                let name_lower = process.name.to_lowercase();
                if has_running_docker_services
                    && (name_lower.contains("docker") || name_lower.contains("com.docker"))
                {
                    continue;
                }

                // Skip if process name matches a running service name
                // (handles forked processes and containers)
                let matches_service = running_services
                    .iter()
                    .any(|svc| name_lower.contains(svc) || svc.contains(&name_lower));
                if matches_service {
                    continue;
                }

                // Skip common runtime processes if we have running process services
                // (node/npm/bun for JS, python for Python, java for JVM, etc.)
                const COMMON_RUNTIMES: &[&str] = &[
                    "node", "npm", "npx", "bun", "deno", "python", "python3", "java", "gradle",
                    "ruby", "go", "cargo", "rust",
                ];
                if has_running_process_services
                    && COMMON_RUNTIMES.iter().any(|rt| name_lower == *rt)
                {
                    continue;
                }

                port_conflicts.push((
                    param_name.clone(),
                    port,
                    process.name.clone(),
                    Some(process.pid),
                ));
            }
            // Only report unknown if no processes found at all
            if conflict.processes.is_empty() {
                port_conflicts.push((param_name.clone(), port, "unknown".to_string(), None));
            }
        }

        println!();
        if !port_conflicts.is_empty() {
            eprintln!("\x1b[31m⚠️  Port conflicts detected:\x1b[0m");
            for (param_name, port, process_name, pid) in &port_conflicts {
                if let Some(p) = pid {
                    eprintln!(
                        "\x1b[31m  {} (port {}) - occupied by '{}' (PID {})\x1b[0m",
                        param_name, port, process_name, p
                    );
                } else {
                    eprintln!(
                        "\x1b[31m  {} (port {}) - occupied by external process\x1b[0m",
                        param_name, port
                    );
                }
            }
            println!();
            println!("Hint: Run 'fed start --replace' to kill conflicting processes");
            println!("      Or manually stop the external services first");
        } else {
            eprintln!("\x1b[31m⚠️  Some services are failing. Check logs with 'fed logs <service>'\x1b[0m");
        }
    }

    if !watch {
        println!("\nServices running in background");
        println!("  Use 'fed stop' to stop them");
        println!("  Use 'fed tui' for interactive mode");
    } else {
        run_watch_mode(orchestrator, config, config_path).await?;
    }

    Ok(())
}

async fn run_watch_mode(
    orchestrator: &mut Orchestrator,
    config: &Config,
    config_path: &std::path::Path,
) -> anyhow::Result<()> {
    println!("\nServices running with watch mode enabled");
    println!("  Files will be monitored for changes. Press Ctrl+C to stop...");

    // Set up watch mode
    let work_dir = if let Some(parent) = config_path.parent() {
        if parent.as_os_str().is_empty() {
            std::env::current_dir()?
        } else {
            parent.to_path_buf()
        }
    } else {
        std::env::current_dir()?
    };

    let mut watch_mode = match WatchMode::new(config, &work_dir) {
        Ok(wm) => {
            println!("  Watching for file changes...");
            Some(wm)
        }
        Err(e) => {
            eprintln!("Failed to start watch mode: {}", e);
            eprintln!("  Continuing without file watching...");
            None
        }
    };

    // Install signal handler for SIGINT (Ctrl+C) and SIGTERM
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let force_quit = Arc::new(AtomicBool::new(false));
    let force_quit_clone = force_quit.clone();

    // Clone state tracker, monitoring shutdown, and services for force quit cleanup
    let state_tracker_clone = orchestrator.state_tracker.clone();
    let monitoring_shutdown_clone = orchestrator.monitoring_shutdown.clone();
    let services_clone = orchestrator.get_services_arc();

    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};

        // Set up signal handlers, logging warnings if they fail
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("Failed to create SIGINT handler: {}", e);
                None
            }
        };
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("Failed to create SIGTERM handler: {}", e);
                None
            }
        };

        // If neither signal handler works, just wait forever (process can still be killed)
        if sigint.is_none() && sigterm.is_none() {
            tracing::warn!(
                "No signal handlers available - process can only be terminated externally"
            );
            std::future::pending::<()>().await;
            return;
        }

        let mut signal_count = 0;
        loop {
            tokio::select! {
                _ = async {
                    if let Some(ref mut s) = sigint {
                        s.recv().await
                    } else {
                        std::future::pending::<Option<()>>().await
                    }
                } => {
                    signal_count += 1;

                    if signal_count == 1 {
                        println!("\n\nStopping services... (Press Ctrl+C again to force quit)");
                        shutdown_tx.send(()).await.ok();
                    } else {
                        println!("\n\nForce quitting...");
                        force_quit_clone.store(true, Ordering::SeqCst);

                        // Kill all running services before exit
                        let services_map = services_clone.read().await;
                        for (_, service_arc) in services_map.iter() {
                            if let Ok(mut manager) = service_arc.try_lock() {
                                let _ = manager.kill().await;
                            }
                        }
                        drop(services_map);

                        // Save state tracker before exit
                        if let Err(e) = state_tracker_clone.write().await.save().await {
                            eprintln!("Failed to save state: {}", e);
                        }

                        // Signal monitoring task to shut down
                        monitoring_shutdown_clone.notify_waiters();

                        std::process::exit(130);
                    }
                }
                _ = async {
                    if let Some(ref mut s) = sigterm {
                        s.recv().await
                    } else {
                        std::future::pending::<Option<()>>().await
                    }
                } => {
                    println!("\n\nReceived SIGTERM, stopping services gracefully...");
                    shutdown_tx.send(()).await.ok();
                    break;
                }
            }
        }
    });

    // Main event loop: watch for file changes or shutdown signal
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                break;
            }
            event = async {
                if let Some(ref mut wm) = watch_mode {
                    wm.next_event().await
                } else {
                    std::future::pending::<Option<service_federation::watch::FileChangeEvent>>().await
                }
            } => {
                if let Some(event) = event {
                    println!("\nFile change detected in service '{}': {} file(s) changed",
                        event.service_name, event.changed_paths.len());
                    println!("  Restarting {}...", event.service_name);

                    // Stop the service
                    match orchestrator.stop(&event.service_name).await {
                        Ok(_) => {
                            match orchestrator.start(&event.service_name).await {
                                Ok(_) => {
                                    println!("  {} restarted successfully", event.service_name);
                                }
                                Err(e) => {
                                    eprintln!("  Failed to start {}: {}", event.service_name, e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("  Failed to stop {}: {}", event.service_name, e);
                        }
                    }
                }
            }
        }
    }

    // Perform cleanup if not force quitting
    if !force_quit.load(Ordering::SeqCst) {
        orchestrator.cleanup().await;
        println!("All services stopped");
    }

    Ok(())
}

/// Run in dry-run mode: show what would happen without starting services.
///
/// This displays:
/// 1. Services to start (with their dependencies)
/// 2. Start order (topological sort)
/// 3. Resolved parameters
/// 4. Port conflict detection
/// 5. Environment variables per service (with secrets masked)
/// 6. Resource limits
/// 7. Validation summary
async fn run_dry_run(
    orchestrator: &Orchestrator,
    config: &Config,
    services_to_start: Vec<String>,
) -> anyhow::Result<()> {
    println!("=== Dry Run Mode ===\n");

    let dep_graph = orchestrator.get_dependency_graph();

    // 1. Show services that would be started with their dependencies
    println!("Services to start:");
    for service in &services_to_start {
        let deps = dep_graph.get_dependencies(service);
        if deps.is_empty() {
            println!("  - {}", service);
        } else {
            println!("  - {} (depends on: {})", service, deps.join(", "));
        }
    }

    // 2. Calculate and show start order (topological sort of all services to start)
    // Collect all services including dependencies
    let mut all_services: Vec<String> = Vec::new();
    for service in &services_to_start {
        let deps = dep_graph.get_dependencies(service);
        for dep in deps {
            if !all_services.contains(&dep) {
                all_services.push(dep);
            }
        }
        if !all_services.contains(service) {
            all_services.push(service.clone());
        }
    }

    println!("\nStart order:");
    for (i, service) in all_services.iter().enumerate() {
        let service_config = config.services.get(service);
        let service_type = service_config
            .map(|s| s.service_type())
            .unwrap_or(ServiceType::Undefined);
        println!("  {}. {} ({:?})", i + 1, service, service_type);
    }

    // 3. Show resolved parameters
    let params = orchestrator.get_resolved_parameters();
    if !params.is_empty() {
        println!("\nResolved parameters:");
        // Sort parameters for consistent output
        let mut sorted_params: Vec<_> = params.iter().collect();
        sorted_params.sort_by_key(|(k, _)| *k);
        for (key, value) in sorted_params {
            println!("  {}: {}", key, value);
        }
    }

    // 4. Check for port conflicts
    println!("\nPort availability:");
    let mut conflicts_found = false;
    let port_params = orchestrator.get_port_parameter_names();

    for name in port_params {
        let Some(value) = params.get(name) else {
            continue;
        };
        let Ok(port) = value.parse::<u16>() else {
            continue;
        };

        if let Some(conflict) = PortConflict::check(port) {
            conflicts_found = true;
            println!("  [CONFLICT] Port {} ({}):", port, name);
            if conflict.processes.is_empty() {
                println!("    - Port in use by unknown process");
            } else {
                for process in &conflict.processes {
                    println!("    - '{}' (PID {})", process.name, process.pid);
                }
            }
        } else {
            println!("  [OK] Port {} ({}) is available", port, name);
        }
    }
    if !conflicts_found && port_params.is_empty() {
        println!("  No port parameters detected");
    } else if !conflicts_found {
        println!("  All {} port(s) available", port_params.len());
    }

    // 5. Show environment variables per service (mask sensitive values)
    println!("\nService configuration:");
    for service_name in &all_services {
        if let Some(service_config) = config.services.get(service_name) {
            println!("  {}:", service_name);

            // Show service type
            let service_type = service_config.service_type();
            println!("    type: {:?}", service_type);

            // Show process command or image
            if let Some(ref process) = service_config.process {
                println!("    command: {}", process);
            }
            if let Some(ref image) = service_config.image {
                println!("    image: {}", image);
            }
            if let Some(ref gradle_task) = service_config.gradle_task {
                println!("    gradle_task: {}", gradle_task);
            }

            // Show working directory if set
            if let Some(ref cwd) = service_config.cwd {
                println!("    cwd: {}", cwd);
            }

            // Show health check if configured
            if let Some(ref healthcheck) = service_config.healthcheck {
                let timeout = healthcheck.get_timeout();
                match healthcheck.get_http_url() {
                    Some(url) => {
                        println!("    healthcheck: HTTP GET {} (timeout: {:?})", url, timeout)
                    }
                    None => {
                        if let Some(cmd) = healthcheck.get_command() {
                            println!(
                                "    healthcheck: command '{}' (timeout: {:?})",
                                cmd, timeout
                            );
                        }
                    }
                }
            }

            // Show environment variables with masked secrets
            if !service_config.environment.is_empty() {
                println!("    environment:");
                let mut sorted_env: Vec<_> = service_config.environment.iter().collect();
                sorted_env.sort_by_key(|(k, _)| *k);
                for (key, value) in sorted_env {
                    let display_value = mask_sensitive_value(key, value);
                    println!("      {}: {}", key, display_value);
                }
            }

            // Show restart policy if configured
            if let Some(ref restart) = service_config.restart {
                println!("    restart: {:?}", restart);
            }
        }
    }

    // 6. Show resource limits
    println!("\nResource limits:");
    let mut any_limits = false;
    for service_name in &all_services {
        if let Some(service_config) = config.services.get(service_name) {
            if let Some(ref resources) = service_config.resources {
                any_limits = true;
                println!("  {}:", service_name);
                if let Some(ref mem) = resources.memory {
                    println!("    memory: {}", mem);
                }
                if let Some(ref cpus) = resources.cpus {
                    println!("    cpus: {}", cpus);
                }
                if let Some(nofile) = resources.nofile {
                    println!("    nofile: {}", nofile);
                }
                if let Some(pids) = resources.pids {
                    println!("    pids: {}", pids);
                }
            }
        }
    }
    if !any_limits {
        println!("  No resource limits configured");
    }

    // 7. Validation summary
    println!("\n=== Validation Summary ===");
    println!("  Configuration: OK (parsed successfully)");
    println!("  Services to start: {}", all_services.len());
    if conflicts_found {
        println!("  Port conflicts: DETECTED (use --replace to kill conflicting processes)");
    } else {
        println!("  Port conflicts: None");
    }

    println!("\n=== Dry run complete ===");
    println!("Run without --dry-run to actually start services");

    Ok(())
}

/// Mask sensitive environment variable values.
///
/// Returns "***" for values whose keys contain sensitive keywords,
/// otherwise returns the original value.
fn mask_sensitive_value(key: &str, value: &str) -> String {
    let key_lower = key.to_lowercase();
    let sensitive_patterns = [
        "secret",
        "password",
        "token",
        "api_key",
        "apikey",
        "private_key",
        "privatekey",
        "auth",
        "credential",
    ];

    for pattern in &sensitive_patterns {
        if key_lower.contains(pattern) {
            return "***".to_string();
        }
    }

    value.to_string()
}

/// Stop services from a previous fed session gracefully.
///
/// This is called by `--replace` to cleanly stop fed-managed services
/// before killing any remaining external processes.
///
/// Returns the number of services stopped.
async fn stop_previous_session_services(orchestrator: &Orchestrator) -> usize {
    use service_federation::state::SqliteStateTracker;

    // Clone the database connection while briefly holding the read lock.
    // This avoids holding the RwLock across the async database query,
    // which could cause contention with health monitoring or status checks.
    let conn = orchestrator.state_tracker.read().await.clone_connection();
    // Lock released here - the cloned connection is internally thread-safe

    let services = SqliteStateTracker::fetch_services_from_connection(&conn).await;

    if services.is_empty() {
        return 0;
    }

    let mut stopped = 0;

    for (name, state) in &services {
        // Skip services that aren't running
        if state.status != "running" && state.status != "healthy" {
            continue;
        }

        print!("Stopping {} ({})... ", name, state.service_type);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let success = if let Some(ref container_id) = state.container_id {
            // Docker service - stop and remove container
            graceful_docker_stop(container_id).await
        } else if let Some(pid) = state.pid {
            // Validate PID hasn't been reused by checking process start time
            if !validate_pid_start_time(pid, state.started_at) {
                println!("skipped (PID {} was reused by another process)", pid);
                continue;
            }
            // Process service - graceful kill
            graceful_process_kill(pid).await
        } else {
            // No PID or container - nothing to stop
            println!("skipped (no PID/container)");
            continue;
        };

        if success {
            println!("stopped");
            stopped += 1;
        } else {
            println!("\x1b[33mfailed\x1b[0m");
        }
    }

    // Clear the state tracker so we start fresh
    if stopped > 0 {
        if let Err(e) = orchestrator.state_tracker.write().await.clear().await {
            tracing::warn!("Failed to clear state after stopping services: {}", e);
        }
    }

    stopped
}

/// Gracefully stop a Docker container.
///
/// Tries `docker stop` first (sends SIGTERM, waits), then `docker rm -f`.
async fn graceful_docker_stop(container_id: &str) -> bool {
    use std::process::Command;

    // Try graceful stop first (SIGTERM with 10 second timeout)
    let stop_result = Command::new("docker")
        .args(["stop", "-t", "10", container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = stop_result {
        if status.success() {
            // Remove the stopped container
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            return true;
        }
    }

    // Fallback: force remove
    let rm_result = Command::new("docker")
        .args(["rm", "-f", container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    rm_result.map(|s| s.success()).unwrap_or(false)
}

/// Check if a PID belongs to the expected process by comparing start times.
///
/// Returns true if the PID appears valid (start time matches or cannot be determined),
/// false if the PID was clearly reused by a different process.
///
/// This is a defensive check against PID reuse - if a service's PID was recorded
/// but the process died and a new process reused the same PID, we don't want to
/// accidentally kill an unrelated process.
fn validate_pid_start_time(pid: u32, expected_start: chrono::DateTime<chrono::Utc>) -> bool {
    #[cfg(target_os = "linux")]
    {
        // On Linux, read /proc/<pid>/stat to get process start time
        let stat_path = format!("/proc/{}/stat", pid);
        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
            // The stat file format has the process name in parens which can contain spaces,
            // so we find the closing paren and parse from there
            if let Some(close_paren) = stat.rfind(')') {
                let fields: Vec<&str> = stat[close_paren + 2..].split_whitespace().collect();
                // Field 20 (0-indexed after the first two fields) is starttime
                if let Some(&starttime_str) = fields.get(19) {
                    if let Ok(starttime_jiffies) = starttime_str.parse::<u64>() {
                        // Get system boot time and jiffies per second to convert
                        // For now, use a simpler heuristic: if the process started
                        // significantly before our expected time, it's probably reused
                        let now = chrono::Utc::now();
                        let expected_age = now.signed_duration_since(expected_start);

                        // If the expected service is very old (>24h), be lenient
                        if expected_age.num_hours() > 24 {
                            return true;
                        }

                        // Get uptime to calculate approximate process start
                        if let Ok(uptime_str) = std::fs::read_to_string("/proc/uptime") {
                            if let Some(uptime_secs_str) = uptime_str.split_whitespace().next() {
                                if let Ok(uptime_secs) = uptime_secs_str.parse::<f64>() {
                                    // Assume 100 jiffies per second (common default)
                                    let jiffies_per_sec: u64 = 100;
                                    let process_age_secs =
                                        uptime_secs - (starttime_jiffies as f64 / jiffies_per_sec as f64);

                                    // If process started more than 60 seconds before our expected time,
                                    // it's likely a different process that reused the PID
                                    let expected_age_secs = expected_age.num_seconds() as f64;
                                    let time_diff = (process_age_secs - expected_age_secs).abs();

                                    if time_diff > 60.0 {
                                        tracing::warn!(
                                            "PID {} appears to be reused: process age {:.0}s vs expected {:.0}s",
                                            pid, process_age_secs, expected_age_secs
                                        );
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use ps to get process start time
        use std::process::Command;
        if let Ok(output) = Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
        {
            if output.status.success() {
                let lstart = String::from_utf8_lossy(&output.stdout);
                // Parse the lstart format: "Mon Jan  1 12:00:00 2024"
                // Use chrono to parse this
                let lstart = lstart.trim();
                if !lstart.is_empty() {
                    // Try to parse with chrono
                    if let Ok(process_start) =
                        chrono::NaiveDateTime::parse_from_str(lstart, "%a %b %e %H:%M:%S %Y")
                    {
                        // Convert to UTC (assuming local time from ps)
                        let process_start_utc = process_start.and_utc();
                        let time_diff = (process_start_utc - expected_start).num_seconds().abs();

                        // Allow 60 seconds tolerance for timing differences
                        if time_diff > 60 {
                            tracing::warn!(
                                "PID {} appears to be reused: process started at {} vs expected {}",
                                pid, process_start_utc, expected_start
                            );
                            return false;
                        }
                    }
                }
            }
        }
    }

    // Default: trust the PID if we can't determine start time
    true
}

/// Gracefully kill a process.
///
/// Sends SIGTERM first, waits up to 5 seconds, then sends SIGKILL.
async fn graceful_process_kill(pid: u32) -> bool {
    use std::process::Command;
    use std::time::Duration;

    // Check if process exists first
    let exists = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
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
        .status();

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
        .status();

    // Wait a bit more for SIGKILL to take effect
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if finally dead
    let dead = !Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if kill_result.is_ok() && dead {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_sensitive_value_secrets() {
        // Should mask secrets
        assert_eq!(mask_sensitive_value("API_SECRET", "my-secret"), "***");
        assert_eq!(mask_sensitive_value("secret_key", "value"), "***");
        assert_eq!(mask_sensitive_value("MY_SECRET_VALUE", "hidden"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_passwords() {
        assert_eq!(mask_sensitive_value("PASSWORD", "pass123"), "***");
        assert_eq!(mask_sensitive_value("db_password", "dbpass"), "***");
        assert_eq!(mask_sensitive_value("USER_PASSWORD", "userpass"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_tokens() {
        assert_eq!(mask_sensitive_value("AUTH_TOKEN", "token123"), "***");
        assert_eq!(mask_sensitive_value("access_token", "abc"), "***");
        assert_eq!(mask_sensitive_value("REFRESH_TOKEN", "xyz"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_api_keys() {
        assert_eq!(mask_sensitive_value("API_KEY", "key123"), "***");
        assert_eq!(mask_sensitive_value("APIKEY", "key456"), "***");
        assert_eq!(mask_sensitive_value("my_api_key", "key789"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_auth() {
        assert_eq!(mask_sensitive_value("AUTH_HEADER", "bearer xxx"), "***");
        assert_eq!(mask_sensitive_value("OAUTH_TOKEN", "oauth123"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_credentials() {
        assert_eq!(mask_sensitive_value("CREDENTIAL", "cred123"), "***");
        assert_eq!(mask_sensitive_value("aws_credentials", "xxx"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_private_keys() {
        assert_eq!(mask_sensitive_value("PRIVATE_KEY", "-----BEGIN"), "***");
        assert_eq!(mask_sensitive_value("privatekey", "key"), "***");
    }

    #[test]
    fn test_mask_sensitive_value_non_sensitive() {
        // Non-sensitive values should NOT be masked
        assert_eq!(
            mask_sensitive_value("DATABASE_URL", "postgres://localhost"),
            "postgres://localhost"
        );
        assert_eq!(mask_sensitive_value("PORT", "8080"), "8080");
        assert_eq!(mask_sensitive_value("NODE_ENV", "production"), "production");
        assert_eq!(mask_sensitive_value("DEBUG", "true"), "true");
    }

    #[test]
    fn test_mask_sensitive_value_case_insensitive() {
        // Should be case insensitive
        assert_eq!(mask_sensitive_value("password", "pass"), "***");
        assert_eq!(mask_sensitive_value("PASSWORD", "pass"), "***");
        assert_eq!(mask_sensitive_value("Password", "pass"), "***");
        assert_eq!(mask_sensitive_value("PaSsWoRd", "pass"), "***");
    }
}
