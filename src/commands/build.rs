use service_federation::{config::Config, Orchestrator};

pub async fn run_build(
    orchestrator: &Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    let services_to_build = if services.is_empty() {
        config
            .services
            .iter()
            .filter(|(_, svc)| svc.build.is_some())
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    } else {
        services
    };

    if services_to_build.is_empty() {
        println!("No services with build field found");
        return Ok(());
    }

    println!(
        "Running build for services: {}",
        services_to_build.join(", ")
    );

    for service in &services_to_build {
        println!("\n[build] {}", service);
        if let Err(e) = orchestrator.run_build(service).await {
            println!("[build] {} failed: {}", service, e);
            return Err(e.into());
        }
    }

    println!("\nAll builds completed successfully.");

    Ok(())
}
