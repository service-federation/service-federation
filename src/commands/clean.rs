use service_federation::{config::Config, Orchestrator};

pub async fn run_clean(
    orchestrator: &Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    let services_to_clean = if services.is_empty() {
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

    println!("\nAll clean commands completed successfully.");

    Ok(())
}
