use crate::cli::PortsCommands;
use service_federation::state::StateTracker;
use service_federation::{Orchestrator, Parser as ConfigParser};
use std::io::Write;
use std::path::PathBuf;

pub async fn run_ports(
    cmd: &PortsCommands,
    workdir: Option<PathBuf>,
    config_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let work_dir = resolve_work_dir(workdir, config_path.as_deref())?;

    match cmd {
        PortsCommands::List { json } => list_ports(&work_dir, *json).await,
        PortsCommands::Randomize { force } => randomize_ports(&work_dir, config_path, *force).await,
        PortsCommands::Reset { force } => reset_ports(&work_dir, *force).await,
    }
}

fn resolve_work_dir(
    workdir: Option<PathBuf>,
    config_path: Option<&std::path::Path>,
) -> anyhow::Result<PathBuf> {
    if let Some(w) = workdir {
        return Ok(w);
    }
    // Try to derive from config path
    let parser = ConfigParser::new();
    let resolved_config = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        parser.find_config_file()?
    };
    if let Some(parent) = resolved_config.parent() {
        if parent.as_os_str().is_empty() {
            Ok(std::env::current_dir()?)
        } else {
            Ok(parent.to_path_buf())
        }
    } else {
        Ok(std::env::current_dir()?)
    }
}

async fn list_ports(work_dir: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    let ports = tracker.get_global_port_allocations().await;

    if json {
        println!("{}", serde_json::to_string_pretty(&ports)?);
    } else {
        println!("\nPort Allocations");
        println!("================\n");

        if ports.is_empty() {
            println!("No ports are currently allocated.");
            println!("Ports are allocated on `fed start` or `fed ports randomize`.\n");
            return Ok(());
        }

        let mut sorted: Vec<_> = ports.iter().collect();
        sorted.sort_by_key(|(_, port)| *port);

        for (param, port) in &sorted {
            println!("  {:>5}  {}", port, param);
        }
        println!();
    }

    Ok(())
}

async fn randomize_ports(
    work_dir: &std::path::Path,
    config_path: Option<PathBuf>,
    force: bool,
) -> anyhow::Result<()> {
    ensure_services_stopped(work_dir, force).await?;

    // Load config
    let parser = ConfigParser::new();
    let resolved_config = if let Some(path) = config_path {
        path
    } else {
        parser.find_config_file()?
    };
    let config = parser.load_config(&resolved_config)?;
    config.validate()?;

    // Create orchestrator, enable randomization, initialize to resolve ports
    let mut orchestrator = Orchestrator::new(config, work_dir.to_path_buf()).await?;
    orchestrator.set_work_dir(work_dir.to_path_buf()).await?;
    orchestrator.set_randomize_ports(true);
    orchestrator.initialize().await?;

    // Read back the persisted ports
    let tracker = orchestrator.state_tracker.read().await;
    let ports = tracker.get_global_port_allocations().await;
    drop(tracker);

    println!("\nRandomized port allocations:");
    let mut sorted: Vec<_> = ports.iter().collect();
    sorted.sort_by_key(|(_, port)| *port);
    for (param, port) in &sorted {
        println!("  {:>5}  {}", port, param);
    }
    println!();
    println!("Ports persisted. Next `fed start` will use these allocations.");
    println!("Use `fed ports reset` to clear and return to defaults.\n");

    Ok(())
}

async fn reset_ports(work_dir: &std::path::Path, force: bool) -> anyhow::Result<()> {
    ensure_services_stopped(work_dir, force).await?;

    let mut tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    tracker.initialize().await?;
    tracker.clear_port_resolutions().await?;

    println!("Port allocations cleared. Next `fed start` will use default ports.\n");

    Ok(())
}

/// Ensure no services are running. With --force, auto-stop them.
/// Without --force, prompt the user.
async fn ensure_services_stopped(work_dir: &std::path::Path, force: bool) -> anyhow::Result<()> {
    let tracker = StateTracker::new(work_dir.to_path_buf()).await?;
    let services = tracker.get_services().await;

    let running: Vec<_> = services
        .iter()
        .filter(|(_, state)| matches!(state.status.as_str(), "running" | "healthy" | "starting"))
        .map(|(name, _)| name.clone())
        .collect();

    if running.is_empty() {
        return Ok(());
    }

    if force {
        println!("Stopping {} running service(s)...", running.len());
        // Use the state-tracker-based stop (no config needed)
        super::run_stop_from_state(work_dir, vec![]).await?;
    } else {
        println!("The following services are running: {}", running.join(", "));
        print!("Stop them to continue? [y/N] ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            anyhow::bail!("Aborted: services must be stopped first");
        }

        super::run_stop_from_state(work_dir, vec![]).await?;
    }

    Ok(())
}
