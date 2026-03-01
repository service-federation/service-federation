use crate::output::UserOutput;
use fed::{config::Config, Orchestrator};

pub async fn run_status(
    orchestrator: &Orchestrator,
    config: &Config,
    json: bool,
    tag: Option<String>,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    let mut status = orchestrator.get_status().await;

    // Filter by tag if specified
    if let Some(tag_filter) = tag {
        let services_with_tag = config.services_with_tag(&tag_filter);
        status.retain(|name, _| services_with_tag.contains(name));
    }

    if json {
        use serde_json::json;

        let status_obj = status
            .into_iter()
            .map(|(name, stat)| {
                let status_str = match stat {
                    fed::Status::Running => "running",
                    fed::Status::Healthy => "healthy",
                    fed::Status::Stopped => "stopped",
                    fed::Status::Starting => "starting",
                    fed::Status::Failing => "failing",
                    fed::Status::Stopping => "stopping",
                };
                (
                    name,
                    json!({
                        "status": status_str
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();

        out.status(&serde_json::to_string_pretty(&status_obj)?);
    } else {
        out.status("Service Status:");
        out.status(&format!("{:-<50}", ""));

        if status.is_empty() {
            out.status("  No services configured");
        } else {
            for (name, stat) in status {
                let status_icon = match stat {
                    fed::Status::Running | fed::Status::Healthy => "+",
                    fed::Status::Stopped => "o",
                    fed::Status::Starting => ".",
                    fed::Status::Failing => "x",
                    fed::Status::Stopping => ".",
                };
                out.status(&format!("  {} {:<30} {:?}", status_icon, name, stat));
            }
        }
    }

    Ok(())
}
