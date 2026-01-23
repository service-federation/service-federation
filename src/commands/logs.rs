use service_federation::Orchestrator;

pub async fn run_logs(
    orchestrator: &Orchestrator,
    service: &str,
    tail: Option<usize>,
    follow: bool,
) -> anyhow::Result<()> {
    if follow {
        println!("Following logs for {} (Press Ctrl+C to stop):", service);
        println!("{:-<50}", "");

        let mut last_line_count = 0;

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            shutdown_tx.send(()).await.ok();
        });

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    println!("\nStopped following logs");
                    break;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    match orchestrator.get_logs(service, None).await {
                        Ok(logs) => {
                            if logs.len() > last_line_count {
                                for line in logs.iter().skip(last_line_count) {
                                    println!("{}", line);
                                }
                                last_line_count = logs.len();
                            }
                        }
                        Err(e) => {
                            eprintln!("Error getting logs: {}", e);
                            if e.to_string().contains("Service not found") {
                                let status = orchestrator.get_status().await;
                                if !status.is_empty() {
                                    eprintln!("\nAvailable services:");
                                    for name in status.keys() {
                                        eprintln!("  - {}", name);
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
                    println!("No logs available for service '{}'", service);
                } else {
                    println!("Logs for {}:", service);
                    println!("{:-<50}", "");
                    for line in logs {
                        println!("{}", line);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting logs: {}", e);
                if e.to_string().contains("Service not found") {
                    let status = orchestrator.get_status().await;
                    if !status.is_empty() {
                        eprintln!("\nAvailable services:");
                        for name in status.keys() {
                            eprintln!("  - {}", name);
                        }
                    }
                }
                return Err(e.into());
            }
        }
    }

    Ok(())
}
