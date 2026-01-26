use super::Orchestrator;
use crate::config::Config;
use crate::error::Result;
use crate::service::OutputMode;
use std::path::PathBuf;
use std::time::Duration;

/// Builder for constructing an `Orchestrator` with a fluent API.
///
/// This builder pattern makes orchestrator initialization less error-prone by:
/// - Ensuring `initialize()` is called automatically
/// - Providing clear, chainable configuration methods
/// - Validating configuration before construction
///
/// # Example
///
/// ```no_run
/// use service_federation::{Config, Orchestrator};
/// use service_federation::service::OutputMode;
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), service_federation::Error> {
/// let config = Config::default();
/// let orchestrator = Orchestrator::builder()
///     .config(config)
///     .work_dir(PathBuf::from("."))
///     .output_mode(OutputMode::Captured)
///     .auto_resolve_conflicts(true)
///     .build()
///     .await?;
/// // initialize() is called automatically
/// # Ok(())
/// # }
/// ```
pub struct OrchestratorBuilder {
    config: Option<Config>,
    work_dir: Option<PathBuf>,
    output_mode: OutputMode,
    auto_resolve_conflicts: bool,
    profiles: Vec<String>,
    startup_timeout: Option<Duration>,
    stop_timeout: Option<Duration>,
}

impl OrchestratorBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            config: None,
            work_dir: None,
            output_mode: OutputMode::default(),
            auto_resolve_conflicts: false,
            profiles: Vec::new(),
            startup_timeout: None,
            stop_timeout: None,
        }
    }

    /// Set the configuration.
    ///
    /// This is required to build the orchestrator.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the working directory for services.
    ///
    /// If not set, defaults to the current directory (".").
    pub fn work_dir(mut self, dir: PathBuf) -> Self {
        self.work_dir = Some(dir);
        self
    }

    /// Set the output mode for process services.
    ///
    /// - `OutputMode::File`: Background mode, logs to files
    /// - `OutputMode::Captured`: Interactive mode, logs to memory (default)
    /// - `OutputMode::Passthrough`: Pass-through mode, inherit stdio
    pub fn output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Enable auto-resolve mode for port conflicts.
    ///
    /// When enabled, port conflicts are resolved automatically without prompting.
    /// This is useful in TUI mode to avoid blocking on interactive prompts.
    pub fn auto_resolve_conflicts(mut self, auto_resolve: bool) -> Self {
        self.auto_resolve_conflicts = auto_resolve;
        self
    }

    /// Set active profiles for service filtering.
    ///
    /// Only services matching at least one of these profiles will be started.
    /// If no profiles are set, all services are included.
    pub fn profiles(mut self, profiles: Vec<String>) -> Self {
        self.profiles = profiles;
        self
    }

    /// Set the startup timeout for service operations.
    ///
    /// If not set, uses the default timeout (2 minutes).
    pub fn startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = Some(timeout);
        self
    }

    /// Set the stop timeout for service operations.
    ///
    /// If not set, uses the default timeout (30 seconds).
    pub fn stop_timeout(mut self, timeout: Duration) -> Self {
        self.stop_timeout = Some(timeout);
        self
    }

    /// Build the orchestrator and initialize it.
    ///
    /// This method performs the following steps:
    /// 1. Validates that required fields are set
    /// 2. Creates the orchestrator instance
    /// 3. Applies optional configuration (work_dir, output_mode, etc.)
    /// 4. Calls `initialize()` automatically
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Config is not set
    /// - Initialization fails (see [`Orchestrator::initialize`])
    pub async fn build(self) -> Result<Orchestrator> {
        // Validate required fields
        let config = self
            .config
            .ok_or_else(|| crate::error::Error::Validation("config is required".to_string()))?;

        // Create orchestrator
        let mut orchestrator = if self.profiles.is_empty() {
            Orchestrator::new(config).await?
        } else {
            Orchestrator::new(config)
                .await?
                .with_profiles(self.profiles)
        };

        // Apply optional configuration
        if let Some(dir) = self.work_dir {
            orchestrator.set_work_dir(dir).await?;
        }

        orchestrator.set_output_mode(self.output_mode);
        orchestrator.set_auto_resolve_conflicts(self.auto_resolve_conflicts);

        if let Some(timeout) = self.startup_timeout {
            orchestrator.startup_timeout = timeout;
        }

        if let Some(timeout) = self.stop_timeout {
            orchestrator.stop_timeout = timeout;
        }

        // Initialize automatically
        orchestrator.initialize().await?;

        Ok(orchestrator)
    }
}

impl Default for OrchestratorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builder_requires_config() {
        let result = OrchestratorBuilder::new().build().await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("config"));
        }
    }

    #[tokio::test]
    async fn test_builder_creates_orchestrator() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let config = Config::default();
        let result = OrchestratorBuilder::new()
            .config(config)
            .work_dir(temp_dir.path().to_path_buf())
            .build()
            .await;
        assert!(result.is_ok(), "Builder failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_builder_fluent_api() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let config = Config::default();
        let result = OrchestratorBuilder::new()
            .config(config)
            .work_dir(temp_dir.path().to_path_buf())
            .output_mode(OutputMode::Captured)
            .auto_resolve_conflicts(true)
            .build()
            .await;
        assert!(result.is_ok(), "Builder failed: {:?}", result.err());
    }
}
