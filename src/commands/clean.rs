use service_federation::{config::Config, Orchestrator};

pub async fn run_clean(
    orchestrator: &Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    let cleaning_all = services.is_empty();

    // When cleaning all, first remove any orphaned containers
    // (from failed starts, etc.) so volumes can be freed
    if cleaning_all {
        match orchestrator.remove_orphaned_containers().await {
            Ok(count) if count > 0 => {
                println!("Removed {} orphaned container(s)", count);
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: Failed to clean orphaned containers: {}", e);
            }
        }
    }

    let services_to_clean = if cleaning_all {
        // Include services that have either a clean command or Docker volumes
        config
            .services
            .iter()
            .filter(|(_, svc)| svc.clean.is_some() || !svc.volumes.is_empty())
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    } else {
        services
    };

    if services_to_clean.is_empty() {
        println!("No services with clean field or Docker volumes found");
        return Ok(());
    }

    println!(
        "Running clean for services: {}",
        services_to_clean.join(", ")
    );

    for service in &services_to_clean {
        println!("\n[clean] {}", service);
        if let Err(e) = orchestrator.run_clean(service).await {
            println!("[clean] {} failed: {}", service, e);
            return Err(e.into());
        }
    }

    // When cleaning all services, also clear persisted port allocations
    // (from `fed ports randomize`). Partial cleans leave port state intact.
    if cleaning_all {
        orchestrator
            .state_tracker
            .write()
            .await
            .clear_port_resolutions()
            .await?;
    }

    println!("\nAll clean commands completed successfully.");

    Ok(())
}
