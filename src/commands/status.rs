use service_federation::{config::Config, Orchestrator};

pub async fn run_status(
    orchestrator: &Orchestrator,
    config: &Config,
    json: bool,
    tag: Option<String>,
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
                    service_federation::Status::Running => "running",
                    service_federation::Status::Healthy => "healthy",
                    service_federation::Status::Stopped => "stopped",
                    service_federation::Status::Starting => "starting",
                    service_federation::Status::Failing => "failing",
                    service_federation::Status::Stopping => "stopping",
                };
                (
                    name,
                    json!({
                        "status": status_str
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();

        println!("{}", serde_json::to_string_pretty(&status_obj)?);
    } else {
        println!("Service Status:");
        println!("{:-<50}", "");

        if status.is_empty() {
            println!("  No services configured");
        } else {
            for (name, stat) in status {
                let status_icon = match stat {
                    service_federation::Status::Running | service_federation::Status::Healthy => {
                        "+"
                    }
                    service_federation::Status::Stopped => "o",
                    service_federation::Status::Starting => ".",
                    service_federation::Status::Failing => "x",
                    service_federation::Status::Stopping => ".",
                };
                println!("  {} {:<30} {:?}", status_icon, name, stat);
            }
        }
    }

    Ok(())
}
