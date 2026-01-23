use service_federation::{config::Config, Orchestrator};

pub async fn run_stop(
    orchestrator: &mut Orchestrator,
    config: &Config,
    services: Vec<String>,
) -> anyhow::Result<()> {
    if services.is_empty() {
        println!("Stopping all services...");
        orchestrator.stop_all().await?;
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
