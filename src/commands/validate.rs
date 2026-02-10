use crate::output::UserOutput;
use service_federation::Parser as ConfigParser;
use std::path::PathBuf;

pub fn run_validate(config_path: Option<PathBuf>, out: &dyn UserOutput) -> anyhow::Result<()> {
    let parser = ConfigParser::new();
    let config_path = if let Some(path) = config_path {
        path
    } else {
        match parser.find_config_file() {
            Ok(path) => path,
            Err(_) => {
                out.error("Error: No configuration file found");
                out.error(&format!(
                    "\nSearched for service-federation.yaml in:\n  - Current directory: {}\n  - Parent directories up to root",
                    std::env::current_dir()?.display()
                ));
                out.error("\nHint: Run 'fed init' to create a starter configuration");
                return Err(anyhow::anyhow!("Configuration file not found"));
            }
        }
    };

    out.status(&format!("Validating {}...", config_path.display()));

    let config = match parser.load_config(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            out.error("Configuration failed to load");
            out.error(&format!("\nError: {}", e));
            return Err(e.into());
        }
    };

    config.validate()?;

    out.success("Configuration is valid\n");

    // Show summary
    out.status(&format!("Services: {}", config.services.len()));
    for (name, service) in &config.services {
        let service_type = if service.process.is_some() {
            "process"
        } else if service.image.is_some() {
            "docker"
        } else if service.compose_file.is_some() {
            "docker-compose"
        } else if service.gradle_task.is_some() {
            "gradle"
        } else {
            "unknown"
        };
        out.status(&format!("  - {} ({})", name, service_type));
    }

    if !config.parameters.is_empty() {
        out.status(&format!("\nParameters: {}", config.parameters.len()));
        for (name, param) in &config.parameters {
            if let Some(param_type) = &param.param_type {
                out.status(&format!("  - {} (type: {})", name, param_type));
            } else {
                out.status(&format!("  - {} (string)", name));
            }
        }
    }

    if let Some(ref ep) = config.entrypoint {
        out.status(&format!("\nEntrypoint: {}", ep));
    } else if !config.entrypoints.is_empty() {
        out.status(&format!("\nEntrypoints: {}", config.entrypoints.join(", ")));
    }

    Ok(())
}
