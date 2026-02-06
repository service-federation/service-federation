use crate::output::UserOutput;
use service_federation::{config::Config, Orchestrator};

pub async fn run_install(
    orchestrator: &Orchestrator,
    config: &Config,
    services: Vec<String>,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    let services_to_install = if services.is_empty() {
        config
            .services
            .iter()
            .filter(|(_, svc)| svc.install.is_some())
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    } else {
        services
    };

    if services_to_install.is_empty() {
        out.status("No services with install field found");
        return Ok(());
    }

    out.status(&format!(
        "Running install for services: {}",
        services_to_install.join(", ")
    ));

    for service in &services_to_install {
        out.status(&format!("\n[install] {}", service));
        if let Err(e) = orchestrator.run_install(service).await {
            out.status(&format!("[install] {} failed: {}", service, e));
            return Err(e.into());
        }
    }

    out.success("\nAll installs completed successfully.");

    Ok(())
}
