use service_federation::Orchestrator;

pub async fn run_script(
    orchestrator: &mut Orchestrator,
    name: &str,
    extra_args: &[String],
    _verbose: bool,
) -> anyhow::Result<()> {
    let available_scripts = orchestrator.list_scripts();

    if available_scripts.is_empty() {
        println!("No scripts defined in configuration");
        return Ok(());
    }

    if !available_scripts.contains(&name.to_string()) {
        eprintln!("Script '{}' not found", name);
        eprintln!("\nAvailable scripts:");
        for script in available_scripts {
            eprintln!("  - {}", script);
        }
        return Err(anyhow::anyhow!("Script not found"));
    }

    // Run interactively with stdin/stdout/stderr passthrough
    // This enables TUI apps, colors, and user interaction
    let status = orchestrator
        .run_script_interactive(name, extra_args)
        .await?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        std::process::exit(code);
    }

    Ok(())
}
