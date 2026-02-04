use service_federation::{
    config::{Config, ServiceType},
    parameter::PortResolutionReason,
    port::PortConflict,
    service::Status,
    Orchestrator, WatchMode,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::lifecycle::{graceful_docker_stop, graceful_process_kill, validate_pid_start_time};

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

    // Print startup messages from the resolved config (templates substituted)
    print_startup_messages(orchestrator.get_config(), &started);

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

    // Brief delay to let processes bind ports and potentially fail with EADDRINUSE.
    // Then use active status check to detect processes that crashed after spawn.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
        }

        // Show per-service failure details
        let failing_services: Vec<&String> = status
            .iter()
            .filter(|(_, s)| **s == Status::Failing)
            .map(|(name, _)| name)
            .collect();

        if !failing_services.is_empty() {
            eprintln!("\x1b[31m⚠️  Failing services:\x1b[0m");
            for name in &failing_services {
                eprintln!("\x1b[31m  ✗ {}\x1b[0m", name);
                if let Some(error) = orchestrator.get_last_error(name).await {
                    for line in error.lines() {
                        eprintln!("    {}", line);
                    }
                } else if let Ok(logs) = orchestrator.get_logs(name, Some(5)).await {
                    if !logs.is_empty() {
                        eprintln!("    Recent logs:");
                        for line in &logs {
                            eprintln!("      {}", line);
                        }
                    }
                }
            }
            eprintln!();
            eprintln!("Use 'fed logs <service>' for full logs");
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

/// Print startup messages from services in a Unicode box.
///
/// Collects `startup_message` from started services, sorts entrypoint messages
/// last, and renders them in a bordered box.
fn print_startup_messages(config: &Config, started: &std::collections::HashSet<String>) {
    // Collect (service_name, message) pairs for started services
    let mut messages: Vec<(&str, &str)> = Vec::new();
    for (name, service) in &config.services {
        if started.contains(name) {
            if let Some(ref msg) = service.startup_message {
                messages.push((name, msg));
            }
        }
    }

    if messages.is_empty() {
        return;
    }

    // Determine which services are entrypoints
    let entrypoints: std::collections::HashSet<&str> = {
        let mut set = std::collections::HashSet::new();
        if let Some(ref ep) = config.entrypoint {
            set.insert(ep.as_str());
        }
        for ep in &config.entrypoints {
            set.insert(ep.as_str());
        }
        set
    };

    // Stable sort: non-entrypoints first (preserve insertion order), entrypoints last
    messages.sort_by_key(|(name, _)| entrypoints.contains(name));

    // Calculate box width (max message length + 2 for padding)
    let max_len = messages.iter().map(|(_, msg)| msg.len()).max().unwrap_or(0);
    let box_width = max_len + 2; // 1 space padding on each side

    let horizontal = "─".repeat(box_width);

    println!();
    println!("╭{}╮", horizontal);
    for (i, (_, msg)) in messages.iter().enumerate() {
        if i > 0 {
            println!("├{}┤", horizontal);
        }
        println!("│ {:width$} │", msg, width = max_len);
    }
    println!("╰{}╯", horizontal);
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

    // Clone state tracker, cancellation token, and services for force quit cleanup
    let state_tracker_clone = orchestrator.state_tracker.clone();
    let cancel_token_clone = orchestrator.child_token();
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
                        cancel_token_clone.cancel();

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

    // 4. Check for port conflicts using resolution tracking
    // Release port listeners first so our own listeners don't appear as conflicts.
    // Safe in dry-run since we never start services.
    orchestrator.release_port_listeners();

    println!("\nPort availability:");
    let port_resolutions = orchestrator.get_port_resolutions();
    let mut conflicts_found = false;
    if port_resolutions.is_empty() {
        println!("  No port parameters detected");
    } else {
        for resolution in port_resolutions {
            match &resolution.reason {
                PortResolutionReason::DefaultAvailable | PortResolutionReason::SessionCached => {
                    // Check if port is still available (it might have been taken since resolution)
                    if let Some(conflict) = PortConflict::check(resolution.resolved_port) {
                        conflicts_found = true;
                        println!(
                            "  [CONFLICT] Port {} ({}):",
                            resolution.resolved_port, resolution.param_name
                        );
                        if conflict.processes.is_empty() {
                            println!("    - Port in use by unknown process");
                        } else {
                            for process in &conflict.processes {
                                println!("    - '{}' (PID {})", process.name, process.pid);
                            }
                        }
                    } else {
                        println!(
                            "  [OK] Port {} ({}) is available",
                            resolution.resolved_port, resolution.param_name
                        );
                    }
                }
                PortResolutionReason::ConflictAutoResolved {
                    default_port,
                    conflict_pid,
                    conflict_process,
                } => {
                    conflicts_found = true;
                    let process_info = match (conflict_pid, conflict_process) {
                        (Some(pid), Some(name)) => format!("'{}' (PID {})", name, pid),
                        (Some(pid), None) => format!("PID {}", pid),
                        _ => "unknown process".to_string(),
                    };
                    println!(
                        "  [CONFLICT] Default port {} ({}) occupied by {} - resolved to {}",
                        default_port, resolution.param_name, process_info, resolution.resolved_port
                    );
                }
                PortResolutionReason::Random => {
                    println!(
                        "  [OK] Port {} ({}) randomly allocated",
                        resolution.resolved_port, resolution.param_name
                    );
                }
            }
        }
        if !conflicts_found {
            println!("  All {} port(s) available", port_resolutions.len());
        }
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
