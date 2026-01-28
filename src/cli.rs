use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fed")]
#[command(about = "Service Federation - Orchestrate complex service dependencies")]
pub struct Cli {
    /// Config file path (defaults to service-federation.yaml)
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Working directory
    #[arg(short, long)]
    pub workdir: Option<PathBuf>,

    /// Environment for variable resolution (development, staging, production)
    #[arg(short, long, default_value = "development")]
    pub env: String,

    /// Active profiles for conditional service startup (can be repeated)
    #[arg(short, long)]
    pub profile: Vec<String>,

    /// Offline mode: skip git operations and use only cached packages
    #[arg(long)]
    pub offline: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start services
    Start {
        /// Services to start (defaults to entrypoint)
        services: Vec<String>,

        /// Watch for file changes and auto-restart services (runs in foreground)
        #[arg(short, long)]
        watch: bool,

        /// Kill any external processes occupying required ports before starting
        #[arg(long)]
        replace: bool,

        /// Output mode for process services.
        /// - file: Background mode, logs to files (default for start)
        /// - captured: Interactive mode, logs to memory (default for watch/tui)
        /// - passthrough: Pass-through mode, inherit stdio (for testing/CI)
        #[arg(long, value_name = "MODE")]
        output: Option<String>,

        /// Preview what would happen without actually starting services
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop services
    Stop {
        /// Services to stop (defaults to all)
        services: Vec<String>,
    },
    /// Restart services
    Restart {
        /// Services to restart (defaults to all)
        services: Vec<String>,
    },
    /// Show service status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Filter services by tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Show service logs
    Logs {
        /// Service name
        service: String,
        /// Number of lines to show
        #[arg(short = 'n', long)]
        tail: Option<usize>,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
    /// Launch interactive TUI
    Tui {
        /// Watch for file changes and auto-restart services
        #[arg(short, long)]
        watch: bool,
    },
    /// Run a script defined in service-federation.yaml
    Run {
        /// Script name
        name: String,
        /// Arguments to pass to the script (after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run install commands for services
    Install {
        /// Services to install (defaults to all services with install field)
        services: Vec<String>,
    },
    /// Run clean commands for services
    Clean {
        /// Services to clean (defaults to all services with clean field)
        services: Vec<String>,
    },
    /// Run build commands for services
    Build {
        /// Services to build (defaults to all services with build field)
        services: Vec<String>,
    },
    /// Manage sessions
    #[command(subcommand)]
    Session(SessionCommands),
    /// Manage package cache
    #[command(subcommand)]
    Package(PackageCommands),
    /// Manage port allocations
    #[command(subcommand)]
    Ports(PortsCommands),
    /// Initialize a new service-federation.yaml config
    Init {
        /// Output file path
        #[arg(short, long, default_value = "service-federation.yaml")]
        output: PathBuf,
        /// Overwrite existing file
        #[arg(short, long)]
        force: bool,
    },
    /// Validate configuration without starting services
    Validate,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
    },
    /// Check system requirements (Docker, Gradle, etc.)
    Doctor,
    /// Show resource usage for all services
    Top {
        /// Refresh interval in seconds
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },
    /// Debug commands for inspecting internal state
    #[command(subcommand)]
    Debug(DebugCommands),

    /// Run a script by name (shorthand for `fed run <script>`)
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Show full state tracker contents
    State {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show port allocations
    Ports {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show circuit breaker state for a service
    CircuitBreaker {
        /// Service name
        service: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Start a new session
    Start {
        /// Custom session ID (optional)
        #[arg(long)]
        id: Option<String>,
    },
    /// End the current session
    End,
    /// List all sessions
    List,
    /// Cleanup orphaned sessions
    Cleanup {
        /// Skip confirmation prompt
        #[arg(long, short)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum PackageCommands {
    /// List cached packages
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Refresh (re-fetch) a cached package
    Refresh {
        /// Package source (e.g., github:org/repo or git+ssh://...)
        /// If not specified, refreshes all packages in current config
        package: Option<String>,
    },
    /// Clear the entire package cache
    Clear {
        /// Skip confirmation prompt
        #[arg(long, short)]
        force: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum PortsCommands {
    /// List current port allocations [default when no subcommand given]
    #[command(alias = "ls")]
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Allocate fresh random ports for all port parameters
    Randomize {
        /// Skip confirmation, auto-stop running services
        #[arg(long, short)]
        force: bool,
    },
    /// Clear port allocations (next start uses defaults)
    Reset {
        /// Skip confirmation, auto-stop running services
        #[arg(long, short)]
        force: bool,
    },
}

impl Default for PortsCommands {
    fn default() -> Self {
        PortsCommands::List { json: false }
    }
}
