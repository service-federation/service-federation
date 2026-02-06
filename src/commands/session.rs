use crate::output::UserOutput;
use crate::cli::SessionCommands;
use service_federation::session::Session;
use service_federation::{Orchestrator, Parser as ConfigParser};
use std::path::PathBuf;

pub async fn run_session(
    session_cmd: &SessionCommands,
    workdir: Option<PathBuf>,
    profiles: Vec<String>,
    out: &dyn UserOutput,
) -> anyhow::Result<()> {
    let workdir = if let Some(wd) = workdir {
        wd
    } else {
        std::env::current_dir()?
    };

    match session_cmd {
        SessionCommands::Start { id } => {
            let session = Session::create(id.clone(), workdir.clone())?;

            // Write .fed/session file in the workspace (use workdir, not current_dir)
            Session::write_session_file_for_dir(session.id(), &workdir)?;

            out.success(&format!("Session started: {}", session.id()));
            out.status(&format!("  Workspace: {}", workdir.display()));
            out.status("  Session file: .fed/session");
            out.blank();
            out.status("The session is now active in this directory.");
            out.status("All 'fed' commands will use this session automatically.");
            out.blank();
            out.status("To use this session elsewhere:");
            out.status(&format!("  export FED_SESSION={}", session.id()));
            out.blank();
            out.status("To end this session:");
            out.status("  fed session end");
        }
        SessionCommands::End => {
            // Try to detect session from env var or .fed/session file
            let session_id = if let Ok(id) = std::env::var("FED_SESSION") {
                id
            } else if let Some(id) = Session::read_session_file_for_dir(&workdir)? {
                id
            } else {
                return Err(anyhow::anyhow!(
                    "No active session found.\n\
                    Either set FED_SESSION environment variable or run this command\n\
                    in a directory with a .fed/session file."
                ));
            };

            let session = Session::load(&session_id)?;

            out.status(&format!("Ending session: {}", session.id()));

            // Stop all services in this session
            out.status("  Stopping services...");

            // Load config from session workspace
            let config_path = match ConfigParser::find_config_in_dir(session.workspace()) {
                Ok(path) => path,
                Err(_) => {
                    out.status("  Config file not found in workspace, skipping service cleanup");
                    out.status("  You may need to manually stop any running services");

                    // Remove .fed/session file if it exists (in the session's workspace)
                    if let Err(e) = Session::remove_session_file_for_dir(session.workspace()) {
                        tracing::debug!("Could not remove session file: {}", e);
                    }

                    // Delete session data
                    session.delete()?;

                    out.success("Session ended");

                    if std::env::var("FED_SESSION").is_ok() {
                        out.blank();
                        out.status("To clear the environment variable, run:");
                        out.status("  unset FED_SESSION");
                    }

                    return Ok(());
                }
            };

            let parser = ConfigParser::new();
            let config = parser.load_config(&config_path)?;

            // Create orchestrator to stop services
            let mut orchestrator =
                Orchestrator::new(config.clone(), session.workspace().to_path_buf())
                    .await?
                    .with_profiles(profiles);
            orchestrator.initialize().await?;

            // Stop all services
            if let Err(e) = orchestrator.stop_all().await {
                out.status(&format!("  Error stopping services: {}", e));
            }

            // Clean up orchestrator resources
            orchestrator.cleanup().await;

            // Remove .fed/session file if it exists (in the session's workspace)
            if let Err(e) = Session::remove_session_file_for_dir(session.workspace()) {
                tracing::debug!("Could not remove session file: {}", e);
            }

            // Delete session data
            session.delete()?;

            out.success("Session ended");

            // Only show unset message if FED_SESSION is set
            if std::env::var("FED_SESSION").is_ok() {
                out.blank();
                out.status("To clear the environment variable, run:");
                out.status("  unset FED_SESSION");
            }
        }
        SessionCommands::List => {
            let sessions = Session::list_all()?;

            if sessions.is_empty() {
                out.status("No sessions found");
                return Ok(());
            }

            out.status("Sessions:");
            out.status(&format!("{:-<80}", ""));
            out.status(&format!(
                "{:<15} {:<12} {:<40} Created",
                "ID", "Status", "Workspace"
            ));
            out.status(&format!("{:-<80}", ""));

            for metadata in sessions {
                let status_str = match metadata.status {
                    service_federation::session::SessionStatus::Active => "active",
                    service_federation::session::SessionStatus::Ended => "ended",
                };

                let created = chrono::DateTime::<chrono::Local>::from(metadata.created_at);
                let created_str = created.format("%Y-%m-%d %H:%M").to_string();

                out.status(&format!(
                    "{:<15} {:<12} {:<40} {}",
                    metadata.id,
                    status_str,
                    metadata.workspace.display().to_string(),
                    created_str
                ));
            }
        }
        SessionCommands::Cleanup { force } => {
            let sessions = Session::list_all()?;
            let mut orphaned = Vec::new();

            for metadata in sessions {
                if let Ok(session) = Session::load(&metadata.id) {
                    if !session.is_shell_alive() {
                        orphaned.push(session);
                    }
                }
            }

            if orphaned.is_empty() {
                out.status("No orphaned sessions found");
                return Ok(());
            }

            out.status(&format!(
                "Found {} orphaned session(s):",
                orphaned.len()
            ));
            for session in &orphaned {
                out.status(&format!(
                    "  - {} (workspace: {})",
                    session.id(),
                    session.workspace().display()
                ));
            }

            let should_delete = if *force {
                true
            } else {
                out.blank();
                out.progress("Remove these sessions? [y/N] ");

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                input.trim().to_lowercase() == "y"
            };

            if should_delete {
                for session in orphaned {
                    out.progress(&format!("  Removing {}...", session.id()));
                    session.delete()?;
                    out.finish_progress(" done");
                }
                out.success("\nCleanup complete");
            } else {
                out.status("Cancelled");
            }
        }
    }

    Ok(())
}
