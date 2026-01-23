use service_federation::Orchestrator;

pub async fn run_port(orchestrator: &Orchestrator, service: &str) -> anyhow::Result<()> {
    let params = orchestrator.get_resolved_parameters();

    // Look for a parameter that looks like it's for this service's port
    let service_upper = service.to_uppercase().replace("-", "_");
    let possible_keys = vec![
        format!("{}_PORT", service_upper),
        format!("{}_HTTP_PORT", service_upper),
        format!("{}_HTTPS_PORT", service_upper),
        format!("PORT_{}", service_upper),
        "PORT".to_string(),
    ];

    for key in possible_keys {
        if let Some(value) = params.get(&key) {
            println!("{}", value);
            return Ok(());
        }
    }

    // If no port parameter found, check service environment
    let status = orchestrator.get_status().await;
    if !status.contains_key(service) {
        eprintln!("Service '{}' not found", service);
        if !status.is_empty() {
            eprintln!("\nAvailable services:");
            for name in status.keys() {
                eprintln!("  - {}", name);
            }
        }
        return Err(anyhow::anyhow!("Service not found"));
    }

    eprintln!("No port parameter found for service '{}'", service);
    eprintln!("\nAvailable parameters:");
    for (key, value) in params {
        eprintln!("  {}: {}", key, value);
    }

    Err(anyhow::anyhow!("Port not found"))
}
