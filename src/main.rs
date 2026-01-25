mod cli;
mod commands;

use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};
use service_federation::{Error as FedError, Orchestrator, OutputMode, Parser as ConfigParser};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Check if the error is a service_federation::Error and add suggestions
        if let Some(fed_error) = e.downcast_ref::<FedError>() {
            eprintln!("Error: {}", fed_error);
            if let Some(suggestion) = fed_error.suggestion() {
                eprintln!("\nHint: {}", suggestion);
            }
        } else {
            eprintln!("Error: {:#}", e);
        }
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    // CRITICAL: Detect circular dependency before doing anything else
    if let Ok(parent_service) = std::env::var("FED_SPAWNED_BY_SERVICE") {
        eprintln!("Error: Circular dependency detected!");
        eprintln!();
        eprintln!(
            "  Service '{}' invoked 'fed', which would create an infinite loop.",
            parent_service
        );
        eprintln!();
        eprintln!("  Detected call chain:");
        eprintln!("    fed start -> {} (process) -> fed", parent_service);
        eprintln!();
        eprintln!("  Fix: Change the process command in service-federation.yaml to run");
        eprintln!("  the actual application directly instead of invoking 'fed'.");
        eprintln!();
        eprintln!("  Example: Instead of 'npm run dev' where dev runs 'fed start',");
        eprintln!("  use 'npx next dev' or the direct command.");
        std::process::exit(1);
    }

    // Check if help is requested - we'll add scripts after standard help
    let args: Vec<String> = std::env::args().collect();
    let wants_help =
        args.iter().any(|a| a == "--help" || a == "-h") || (args.len() == 2 && args[1] == "help");

    if wants_help {
        // Print standard help first
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        cmd.print_help().ok();
        println!();

        // Try to load config to show available scripts
        let parser = ConfigParser::new();
        if let Ok(config_path) = parser.find_config_file() {
            if let Ok(config) = parser.load_config(&config_path) {
                if !config.scripts.is_empty() {
                    println!();
                    println!("Scripts (run with `fed <script>` or `fed run <script>`):");
                    let mut script_names: Vec<_> = config.scripts.keys().collect();
                    script_names.sort();
                    for name in script_names {
                        println!("  {}", name);
                    }
                }
            }
        }
        return Ok(());
    }

    let cli = Cli::parse();

    // Initialize tracing
    let is_tui = matches!(cli.command, Commands::Tui { .. });
    init_tracing(is_tui)?;

    // Auto-cleanup orphaned sessions on startup
    use service_federation::session::Session;
    if let Err(e) = Session::auto_cleanup_orphaned() {
        tracing::warn!("Failed to cleanup orphaned sessions: {}", e);
    }

    // Handle commands that don't need orchestrator first
    match &cli.command {
        Commands::Init { output, force } => {
            return commands::run_init(output, *force);
        }
        Commands::Validate => {
            return commands::run_validate(cli.config.clone());
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
            return Ok(());
        }
        Commands::Doctor => {
            return commands::run_doctor().await;
        }
        Commands::Session(session_cmd) => {
            return commands::run_session(session_cmd, cli.workdir.clone(), cli.profile.clone())
                .await;
        }
        Commands::Package(package_cmd) => {
            return commands::run_package(package_cmd)
                .await
                .map_err(|e| anyhow::anyhow!(e));
        }
        Commands::Debug(debug_cmd) => {
            // Debug commands only need config and work_dir
            let parser = ConfigParser::new();
            let config_path = if let Some(path) = cli.config.clone() {
                path
            } else {
                parser.find_config_file()?
            };
            let config = parser.load_config(&config_path)?;

            let work_dir = if let Some(workdir) = cli.workdir {
                workdir
            } else {
                // Use config file's directory as working directory
                if let Some(parent) = config_path.parent() {
                    if parent.as_os_str().is_empty() {
                        std::env::current_dir()?
                    } else {
                        parent.to_path_buf()
                    }
                } else {
                    std::env::current_dir()?
                }
            };

            let debug_command = match debug_cmd {
                cli::DebugCommands::State { .. } => commands::DebugCommand::State,
                cli::DebugCommands::Ports { .. } => commands::DebugCommand::Ports,
                cli::DebugCommands::CircuitBreaker { service, .. } => {
                    commands::DebugCommand::CircuitBreaker {
                        service: service.clone(),
                    }
                }
            };

            let json = match debug_cmd {
                cli::DebugCommands::State { json } => *json,
                cli::DebugCommands::Ports { json } => *json,
                cli::DebugCommands::CircuitBreaker { json, .. } => *json,
            };

            return commands::run_debug(debug_command, &config, work_dir, json)
                .await
                .map_err(|e| anyhow::anyhow!(e));
        }
        _ => {}
    }

    // Load configuration
    let parser = ConfigParser::new();
    let config_path = if let Some(path) = cli.config.clone() {
        path
    } else {
        parser.find_config_file()?
    };

    // Load config with package resolution (uses cache-only in offline mode)
    let config = parser
        .load_config_with_packages_offline(&config_path, cli.offline)
        .await?;
    config.validate()?;

    // Create orchestrator with profiles
    let mut orchestrator = Orchestrator::new(config.clone())
        .await?
        .with_profiles(cli.profile.clone());

    if let Some(workdir) = cli.workdir {
        orchestrator.set_work_dir(workdir).await?;
    } else {
        // Use config file's directory as working directory
        if let Some(parent) = config_path.parent() {
            if parent.as_os_str().is_empty() {
                orchestrator.set_work_dir(std::env::current_dir()?).await?;
            } else {
                orchestrator.set_work_dir(parent.to_path_buf()).await?;
            }
        } else {
            orchestrator.set_work_dir(std::env::current_dir()?).await?;
        }
    }

    // Set output mode BEFORE initializing
    // File mode (background) disables the monitoring task since we don't need it for:
    // - Start without watch (we exit immediately after starting)
    // - Stop (we're stopping services, not monitoring them)
    // - Restart (same as start without watch)
    // - Status/Logs (read-only operations)
    let output_mode = match &cli.command {
        Commands::Start {
            watch,
            output,
            dry_run,
            ..
        } => {
            // Dry run doesn't need any output mode setup since we won't start services
            if *dry_run {
                OutputMode::Captured
            } else if let Some(mode) = output {
                // If explicit output mode is provided, use it
                match mode.as_str() {
                    "file" => OutputMode::File,
                    "captured" => OutputMode::Captured,
                    "passthrough" => OutputMode::Passthrough,
                    _ => {
                        eprintln!(
                            "Invalid output mode: '{}'. Use 'file', 'captured', or 'passthrough'.",
                            mode
                        );
                        std::process::exit(1);
                    }
                }
            } else if *watch {
                // Watch mode defaults to captured (interactive)
                OutputMode::Captured
            } else {
                // Start without watch defaults to file (background)
                OutputMode::File
            }
        }
        Commands::Stop { .. }
        | Commands::Restart { .. }
        | Commands::Status { .. }
        | Commands::Logs { .. } => OutputMode::File,
        Commands::Tui { .. } => OutputMode::Captured,
        _ => OutputMode::Captured,
    };
    orchestrator.set_output_mode(output_mode);

    // For script commands, check if the script has isolated: true
    // If so, set auto_resolve_conflicts to avoid prompts for ports that will be re-allocated anyway
    let is_isolated_script = match &cli.command {
        Commands::Run { name, .. } => config
            .scripts
            .get(name)
            .map(|s| s.isolated)
            .unwrap_or(false),
        Commands::External(args) if !args.is_empty() => config
            .scripts
            .get(&args[0])
            .map(|s| s.isolated)
            .unwrap_or(false),
        _ => false,
    };
    if is_isolated_script {
        orchestrator.set_auto_resolve_conflicts(true);
    }

    // Initialize orchestrator
    orchestrator.initialize().await?;

    match cli.command {
        Commands::Start {
            services,
            watch,
            replace,
            output: _,
            dry_run,
        } => {
            commands::run_start(
                &mut orchestrator,
                &config,
                services,
                watch,
                replace,
                dry_run,
                &config_path,
            )
            .await?;
        }
        Commands::Stop { services } => {
            commands::run_stop(&mut orchestrator, &config, services).await?;
        }
        Commands::Restart { services } => {
            commands::run_restart(&mut orchestrator, &config, services).await?;
        }
        Commands::Status { json, tag } => {
            commands::run_status(&orchestrator, &config, json, tag).await?;
        }
        Commands::Logs {
            service,
            tail,
            follow,
        } => {
            commands::run_logs(&orchestrator, &service, tail, follow).await?;
        }
        Commands::Tui { watch } => {
            commands::run_tui(orchestrator, watch, Some(&config)).await?;
        }
        Commands::Run { name, args } => {
            // Strip leading "--" from args (clap captures it literally)
            let extra_args: Vec<String> = args.into_iter().skip_while(|arg| arg == "--").collect();
            commands::run_script(&mut orchestrator, &name, &extra_args, false).await?;
        }
        Commands::External(args) => {
            // Handle `fed <script>` shorthand - first arg is the script name
            // Extra args after script name are passed to the script
            if args.is_empty() {
                anyhow::bail!("No script name provided");
            }
            let script_name = &args[0];

            // Extract extra arguments (everything after the script name)
            // Skip leading "--" since clap's external_subcommand captures it literally
            // e.g., `fed test -- -t auth` gives args = ["test", "--", "-t", "auth"]
            let extra_args: Vec<String> = args
                .iter()
                .skip(1)
                .skip_while(|arg| *arg == "--")
                .cloned()
                .collect();

            let available_scripts = orchestrator.list_scripts();
            if available_scripts.contains(script_name) {
                commands::run_script(&mut orchestrator, script_name, &extra_args, false).await?;
            } else {
                eprintln!("Unknown command or script: '{}'", script_name);
                eprintln!("\nAvailable scripts:");
                for script in available_scripts {
                    eprintln!("  - {}", script);
                }
                eprintln!("\nRun 'fed --help' for available commands.");
                std::process::exit(1);
            }
        }
        Commands::Install { services } => {
            commands::run_install(&orchestrator, &config, services).await?;
        }
        Commands::Clean { services } => {
            commands::run_clean(&orchestrator, &config, services).await?;
        }
        Commands::Build { services } => {
            commands::run_build(&orchestrator, &config, services).await?;
        }
        Commands::Top { interval } => {
            commands::run_top(&orchestrator, interval).await?;
        }
        Commands::Port { service } => {
            commands::run_port(&orchestrator, &service).await?;
        }
        // These are handled earlier
        Commands::Session(_)
        | Commands::Package(_)
        | Commands::Init { .. }
        | Commands::Validate
        | Commands::Completions { .. }
        | Commands::Doctor
        | Commands::Debug(_) => {
            unreachable!("These commands should have been handled earlier");
        }
    }

    Ok(())
}

fn init_tracing(is_tui: bool) -> anyhow::Result<()> {
    if is_tui {
        // For TUI mode, write logs to a file
        let log_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".fed")
            .join("logs");
        std::fs::create_dir_all(&log_dir)?;

        let log_path = log_dir.join("tui.log");
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_writer(std::sync::Mutex::new(log_file))
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .init();
    }

    Ok(())
}
