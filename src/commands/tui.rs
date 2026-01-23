use service_federation::{config::Config, Orchestrator};

pub async fn run_tui(
    orchestrator: Orchestrator,
    watch: bool,
    config: Option<&Config>,
) -> anyhow::Result<()> {
    if watch {
        service_federation::tui::run_with_watch(orchestrator, true, config).await?;
    } else {
        service_federation::tui::run(orchestrator).await?;
    }

    Ok(())
}
