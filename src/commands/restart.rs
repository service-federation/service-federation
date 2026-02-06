use crate::output::UserOutput;
use service_federation::{config::Config, Orchestrator};

pub async fn run_restart(
    orchestrator: &mut Orchestrator,
    config: &Config,
    services: Vec<String>,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    if services.is_empty() {
        out.status("Restarting all services in dependency-aware order...");
        orchestrator.restart_all().await?;
        out.success("All services restarted successfully!");
    } else {
        // Expand tag references (e.g., @backend) into service names
        let services_to_restart = config.expand_service_selection(&services);

        out.status(&format!(
            "Restarting services: {}",
            services_to_restart.join(", ")
        ));

        // Phase 1: Stop all services, tracking successes and failures
        let mut stopped_successfully: Vec<String> = Vec::new();
        let mut stop_errors: Vec<(String, String)> = Vec::new();

        for service in &services_to_restart {
            out.progress(&format!("  Stopping {}...", service));
            match orchestrator.stop(service).await {
                Ok(_) => {
                    out.finish_progress(" done");
                    stopped_successfully.push(service.clone());
                }
                Err(e) => {
                    out.finish_progress(&format!(" failed ({})", e));
                    stop_errors.push((service.clone(), e.to_string()));
                }
            }
        }

        // Phase 2: Only start services that were successfully stopped
        let mut start_errors: Vec<(String, String)> = Vec::new();

        for service in &stopped_successfully {
            out.progress(&format!("  Starting {}...", service));
            match orchestrator.start(service).await {
                Ok(_) => out.finish_progress(" done"),
                Err(e) => {
                    out.finish_progress(&format!(" failed ({})", e));
                    start_errors.push((service.clone(), e.to_string()));
                }
            }
        }

        // Phase 3: Report summary
        let has_errors = !stop_errors.is_empty() || !start_errors.is_empty();

        if has_errors {
            out.blank();

            if !stop_errors.is_empty() {
                out.status("Failed to stop:");
                for (service, error) in &stop_errors {
                    out.status(&format!("  - {}: {}", service, error));
                }
            }

            if !start_errors.is_empty() {
                out.status("Failed to start:");
                for (service, error) in &start_errors {
                    out.status(&format!("  - {}: {}", service, error));
                }
            }

            // Return aggregated error
            let error_count = stop_errors.len() + start_errors.len();
            return Err(anyhow::anyhow!(
                "{} service(s) failed to restart",
                error_count
            ));
        }

        out.blank();
        out.success("Services restarted successfully!");
    }

    Ok(())
}
