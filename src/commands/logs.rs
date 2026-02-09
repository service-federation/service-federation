use crate::output::UserOutput;
use service_federation::{Error as FedError, Orchestrator};

pub async fn run_logs(
    orchestrator: &Orchestrator,
    service: &str,
    tail: Option<usize>,
    follow: bool,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    if follow {
        out.status(&format!(
            "Following logs for {} (Press Ctrl+C to stop):",
            service
        ));
        out.status(&format!("{:-<50}", ""));

        let mut last_line_count = 0;

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            shutdown_tx.send(()).await.ok();
        });

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    out.status("\nStopped following logs");
                    break;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    match orchestrator.get_logs(service, None).await {
                        Ok(logs) => {
                            if logs.len() > last_line_count {
                                for line in logs.iter().skip(last_line_count) {
                                    out.status(line);
                                }
                                last_line_count = logs.len();
                            }
                        }
                        Err(e) => {
                            out.error(&format!("Error getting logs: {}", e));
                            if matches!(e, FedError::ServiceNotFound(_)) {
                                let status = orchestrator.get_status().await;
                                if !status.is_empty() {
                                    out.error("\nAvailable services:");
                                    for name in status.keys() {
                                        out.error(&format!("  - {}", name));
                                    }
                                }
                            }
                            return Err(e.into());
                        }
                    }
                }
            }
        }
    } else {
        match orchestrator.get_logs(service, tail).await {
            Ok(logs) => {
                if logs.is_empty() {
                    out.status(&format!("No logs available for service '{}'", service));
                } else {
                    out.status(&format!("Logs for {}:", service));
                    out.status(&format!("{:-<50}", ""));
                    for line in logs {
                        out.status(&line);
                    }
                }
            }
            Err(e) => {
                out.error(&format!("Error getting logs: {}", e));
                if matches!(e, FedError::ServiceNotFound(_)) {
                    let status = orchestrator.get_status().await;
                    if !status.is_empty() {
                        out.error("\nAvailable services:");
                        for name in status.keys() {
                            out.error(&format!("  - {}", name));
                        }
                    }
                }
                return Err(e.into());
            }
        }
    }

    Ok(())
}
