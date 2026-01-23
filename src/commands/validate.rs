use service_federation::Parser as ConfigParser;
use std::path::PathBuf;

pub fn run_validate(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let parser = ConfigParser::new();
    let config_path = if let Some(path) = config_path {
        path
    } else {
        match parser.find_config_file() {
            Ok(path) => path,
            Err(_) => {
                eprintln!("Error: No configuration file found");
                eprintln!("\nSearched for service-federation.yaml in:");
                eprintln!(
                    "  - Current directory: {}",
                    std::env::current_dir()?.display()
                );
                eprintln!("  - Parent directories up to root");
                eprintln!("\nHint: Run 'fed init' to create a starter configuration");
                return Err(anyhow::anyhow!("Configuration file not found"));
            }
        }
    };

    println!("Validating {}...", config_path.display());

    let config = match parser.load_config(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Configuration failed to load");
            eprintln!("\nError: {}", e);
            return Err(e.into());
        }
    };

    config.validate()?;

    println!("Configuration is valid\n");

    // Show summary
    println!("Services: {}", config.services.len());
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
        println!("  - {} ({})", name, service_type);
    }

    if !config.parameters.is_empty() {
        println!("\nParameters: {}", config.parameters.len());
        for (name, param) in &config.parameters {
            if let Some(param_type) = &param.param_type {
                println!("  - {} (type: {})", name, param_type);
            } else {
                println!("  - {} (string)", name);
            }
        }
    }

    if let Some(ref ep) = config.entrypoint {
        println!("\nEntrypoint: {}", ep);
    } else if !config.entrypoints.is_empty() {
        println!("\nEntrypoints: {}", config.entrypoints.join(", "));
    }

    Ok(())
}
