use crate::output::UserOutput;
use service_federation::{Error as FedError, Orchestrator};

pub async fn run_script(
    orchestrator: &mut Orchestrator,
    name: &str,
    extra_args: &[String],
    _verbose: bool,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    let available_scripts = orchestrator.list_scripts();

    if available_scripts.is_empty() {
        out.status("No scripts defined in configuration");
        return Ok(());
    }

    if !available_scripts.contains(&name.to_string()) {
        out.warning("Available scripts:");
        for script in &available_scripts {
            out.warning(&format!("  - {}", script));
        }
        return Err(FedError::ScriptNotFound(name.to_string()).into());
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
