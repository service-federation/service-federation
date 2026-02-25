use crate::cli::IsolateCommands;
use crate::output::UserOutput;
use service_federation::state::StateTracker;
use service_federation::{Orchestrator, Parser as ConfigParser};
use std::path::PathBuf;

pub async fn run_isolate(
    cmd: &IsolateCommands,
    workdir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    let work_dir = super::ports::resolve_work_dir(workdir, config_path.as_deref())?;

    match cmd {
        IsolateCommands::Enable { force } => enable(&work_dir, config_path, *force, out).await,
        IsolateCommands::Disable { force } => disable(&work_dir, *force, out).await,
        IsolateCommands::Status => status(&work_dir, out).await,
        IsolateCommands::Rotate { force } => rotate(&work_dir, config_path, *force, out).await,
    }
}

async fn enable(
    work_dir: &std::path::Path,
    config_path: Option<PathBuf>,
    force: bool,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    // Check if already isolated
    let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    let (enabled, existing_id) = tracker.get_isolation_mode().await;
    if enabled {
        out.status(&format!(
            "Isolation mode is already enabled (ID: {}).",
            existing_id.as_deref().unwrap_or("unknown")
        ));
        out.status("Use `fed isolate rotate` to re-roll ports and ID.\n");
        return Ok(());
    }
    drop(tracker);

    super::ports::ensure_services_stopped(work_dir, force, out).await?;

    // Load config
    let parser = ConfigParser::new();
    let resolved_config = if let Some(path) = config_path {
        path
    } else {
        parser.find_config_file()?
    };
    let config = parser.load_config(&resolved_config)?;
    config.validate()?;

    // Create orchestrator with randomized ports and initialize to resolve them
    let mut orchestrator = Orchestrator::new(config, work_dir.to_path_buf()).await?;
    orchestrator.set_work_dir(work_dir.to_path_buf()).await?;
    orchestrator.set_randomize_ports(true);
    orchestrator.initialize().await?;

    // Generate isolation ID and persist
    let isolation_id = format!("iso-{:08x}", rand::random::<u32>());
    let tracker = orchestrator.state_tracker.read().await;
    tracker
        .set_isolation_mode(true, Some(isolation_id.clone()))
        .await?;

    // Display allocated ports
    let ports = tracker.get_global_port_allocations().await;
    drop(tracker);

    out.status("\nIsolation mode enabled.");
    out.status(&format!("Isolation ID: {}", isolation_id));
    out.status("\nAllocated ports:");

    let mut sorted: Vec<_> = ports.iter().collect();
    sorted.sort_by_key(|(_, port)| *port);
    for (param, port) in &sorted {
        out.status(&format!("  {:>5}  {}", port, param));
    }
    out.blank();
    out.status("Services will use unique container names and these ports.");
    out.status("Use `fed isolate disable` to return to defaults.\n");

    Ok(())
}

async fn disable(
    work_dir: &std::path::Path,
    force: bool,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    // Check if isolation is currently enabled
    {
        let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
        let (enabled, _) = tracker.get_isolation_mode().await;
        if !enabled {
            out.status("Isolation mode is already disabled.\n");
            return Ok(());
        }
    }

    super::ports::ensure_services_stopped(work_dir, force, out).await?;

    // Clear port resolutions
    let mut tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    tracker.initialize().await?;
    tracker.clear_port_resolutions().await?;

    // Clear session port cache (same pattern as reset_ports)
    if let Ok(Some(mut session)) =
        service_federation::session::Session::current_for_workdir(Some(work_dir))
    {
        if let Err(e) = session.clear_ports() {
            tracing::warn!("Failed to clear session port cache: {}", e);
        }
    }

    // Clear isolation mode
    tracker.clear_isolation_mode().await?;

    out.success(
        "Isolation mode disabled. Next `fed start` will use default ports and shared containers.\n",
    );

    Ok(())
}

async fn status(work_dir: &std::path::Path, out: &dyn UserOutput) -> anyhow::Result<()> {
    let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    let (enabled, isolation_id) = tracker.get_isolation_mode().await;

    out.status("\nIsolation Status");
    out.status("================\n");

    if enabled {
        out.status("  Mode:         enabled");
        out.status(&format!(
            "  Isolation ID: {}",
            isolation_id.as_deref().unwrap_or("unknown")
        ));

        // Show port allocations
        let ports = tracker.get_global_port_allocations().await;
        if !ports.is_empty() {
            out.status("\n  Port allocations:");
            let mut sorted: Vec<_> = ports.iter().collect();
            sorted.sort_by_key(|(_, port)| *port);
            for (param, port) in &sorted {
                out.status(&format!("    {:>5}  {}", port, param));
            }
        }
    } else {
        out.status("  Mode: disabled");
        out.status("\n  Using default ports and shared container names.");
        out.status("  Run `fed isolate enable` to activate isolation.");
    }

    out.blank();

    Ok(())
}

async fn rotate(
    work_dir: &std::path::Path,
    config_path: Option<PathBuf>,
    force: bool,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    // Check isolation is currently enabled
    let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    let (enabled, _) = tracker.get_isolation_mode().await;
    if !enabled {
        anyhow::bail!("Isolation mode is not enabled. Run `fed isolate enable` first.");
    }
    drop(tracker);

    super::ports::ensure_services_stopped(work_dir, force, out).await?;

    // Load config
    let parser = ConfigParser::new();
    let resolved_config = if let Some(path) = config_path {
        path
    } else {
        parser.find_config_file()?
    };
    let config = parser.load_config(&resolved_config)?;
    config.validate()?;

    // Create orchestrator with randomized ports and initialize to resolve new ports
    let mut orchestrator = Orchestrator::new(config, work_dir.to_path_buf()).await?;
    orchestrator.set_work_dir(work_dir.to_path_buf()).await?;
    orchestrator.set_randomize_ports(true);
    orchestrator.initialize().await?;

    // Generate new isolation ID and persist
    let isolation_id = format!("iso-{:08x}", rand::random::<u32>());
    let tracker = orchestrator.state_tracker.read().await;
    tracker
        .set_isolation_mode(true, Some(isolation_id.clone()))
        .await?;

    // Display new ports
    let ports = tracker.get_global_port_allocations().await;
    drop(tracker);

    out.status("\nIsolation rotated.");
    out.status(&format!("New isolation ID: {}", isolation_id));
    out.status("\nNew port allocations:");

    let mut sorted: Vec<_> = ports.iter().collect();
    sorted.sort_by_key(|(_, port)| *port);
    for (param, port) in &sorted {
        out.status(&format!("  {:>5}  {}", port, param));
    }
    out.blank();
    out.status("Use `fed isolate status` to view current allocations.\n");

    Ok(())
}
