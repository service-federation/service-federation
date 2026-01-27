use crate::config::{Config, HealthCheckType};
use crate::dependency::{ExternalServiceExpander, Graph};
use crate::error::{Error, Result};
use crate::healthcheck::{CommandChecker, DockerCommandChecker, HealthChecker, HttpChecker};
use crate::parameter::Resolver;
use crate::service::{OutputMode, ServiceManager, Status};
use crate::state::{ServiceState, StateTracker};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
// Using tokio::sync::RwLock for async-aware locking
// Previous implementation used parking_lot::RwLock which required HashMap removal pattern
// to avoid blocking tokio threads across .await points.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Default timeout for service startup operations (2 minutes)
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

/// Default timeout for service stop operations (30 seconds)
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Type alias for the shared service registry
type ServiceRegistry = HashMap<String, Arc<tokio::sync::Mutex<Box<dyn ServiceManager>>>>;
/// Type alias for the shared, async-safe service registry
type SharedServiceRegistry = Arc<tokio::sync::RwLock<ServiceRegistry>>;
/// Type alias for the health checker registry
type HealthCheckerRegistry = HashMap<String, Box<dyn HealthChecker>>;
/// Type alias for the shared health checker registry
type SharedHealthCheckerRegistry = Arc<tokio::sync::RwLock<HealthCheckerRegistry>>;

/// The central coordinator for managing service lifecycles in Service Federation.
///
/// The Orchestrator is responsible for:
/// - Starting services in dependency-aware order
/// - Stopping services (and their dependents) gracefully
/// - Monitoring service health and restarting failed services
/// - Managing port allocation and TOCTOU race prevention
/// - Running install commands and scripts
///
/// # Concurrency Model
///
/// The Orchestrator is designed for safe concurrent access:
/// - Most methods take `&self` instead of `&mut self`
/// - Interior mutability is used via `RwLock`, `Mutex`, and atomic types
/// - A `CancellationToken` allows graceful cancellation of in-progress operations
/// - Operations can be cancelled by calling `cancel_operations()`
///
/// # Lock Ordering (to prevent deadlocks)
///
/// When acquiring multiple locks, always acquire in this order:
/// 1. `services` (RwLock)
/// 2. `health_checkers` (RwLock)
/// 3. `state_tracker` (RwLock)
/// 4. Individual service `Mutex`es (from services map)
///
/// # Example
///
/// ```no_run
/// use service_federation::{Config, Orchestrator};
///
/// # async fn example() -> Result<(), service_federation::Error> {
/// let config = Config::default(); // Load from YAML in practice
/// let mut orchestrator = Orchestrator::new(config, std::path::PathBuf::from(".")).await?;
/// orchestrator.initialize().await?;
/// orchestrator.start_all().await?;
///
/// // When shutting down:
/// orchestrator.cleanup().await;
/// # Ok(())
/// # }
/// ```
///
/// # Service Lifecycle
///
/// 1. **Initialization**: Load config, resolve parameters, create service managers
/// 2. **Start**: Start services in parallel groups respecting dependencies
/// 3. **Monitor**: Continuously check health and restart failed services
/// 4. **Stop**: Stop services in reverse dependency order
pub struct Orchestrator {
    pub(super) config: Config,
    /// Original unresolved config - used by isolated to create child orchestrators
    /// that can re-resolve templates with fresh port allocations.
    original_config: Option<Config>,
    resolver: Resolver,
    dep_graph: Graph,
    pub(super) services: SharedServiceRegistry,
    pub(super) health_checkers: SharedHealthCheckerRegistry,
    work_dir: PathBuf,
    pub state_tracker: Arc<tokio::sync::RwLock<StateTracker>>,
    namespace: String,
    /// Output mode for process services.
    pub(super) output_mode: OutputMode,
    pub monitoring_shutdown: Arc<tokio::sync::Notify>,
    active_profiles: Vec<String>,
    pub(super) monitoring_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub(super) startup_complete: Arc<AtomicBool>,
    /// Track if port listeners have been released (to prevent TOCTOU races)
    port_listeners_released: AtomicBool,
    /// Cancellation token for in-progress operations.
    /// Call `cancel_operations()` to cancel all ongoing start/stop operations.
    cancellation_token: CancellationToken,
    /// Timeout for service startup operations
    pub startup_timeout: Duration,
    /// Timeout for service stop operations
    pub stop_timeout: Duration,
    /// Guard to ensure cleanup runs exactly once
    cleanup_started: AtomicBool,
}

impl Orchestrator {
    /// Create a builder for constructing an `Orchestrator` with a fluent API.
    ///
    /// This is the preferred way to create an orchestrator as it automatically
    /// calls `initialize()` and provides a cleaner API.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use service_federation::{Config, Orchestrator};
    /// use service_federation::service::OutputMode;
    ///
    /// # async fn example() -> Result<(), service_federation::Error> {
    /// let config = Config::default();
    /// let orchestrator = Orchestrator::builder()
    ///     .config(config)
    ///     .output_mode(OutputMode::Captured)
    ///     .build()
    ///     .await?;
    /// // initialize() is called automatically
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> crate::orchestrator::OrchestratorBuilder {
        crate::orchestrator::OrchestratorBuilder::new()
    }

    /// Create a new orchestrator from configuration
    pub async fn new(config: Config, work_dir: PathBuf) -> Result<Self> {
        // Store original config for isolated child orchestrators
        let original_config = Some(config.clone());
        Ok(Self {
            config,
            original_config,
            resolver: Resolver::new(),
            dep_graph: Graph::new(),
            services: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            health_checkers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            state_tracker: Arc::new(tokio::sync::RwLock::new(
                StateTracker::new(work_dir.clone()).await?,
            )),
            work_dir,
            namespace: "root".to_string(),
            output_mode: OutputMode::default(),
            monitoring_shutdown: Arc::new(tokio::sync::Notify::new()),
            active_profiles: Vec::new(),
            monitoring_task: Arc::new(tokio::sync::Mutex::new(None)),
            startup_complete: Arc::new(AtomicBool::new(false)),
            port_listeners_released: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            cleanup_started: AtomicBool::new(false),
        })
    }

    /// Create a nested orchestrator with a namespace (for external services)
    pub async fn new_with_namespace(config: Config, namespace: String, work_dir: PathBuf) -> Result<Self> {
        // Store original config for isolated child orchestrators
        let original_config = Some(config.clone());
        Ok(Self {
            config,
            original_config,
            resolver: Resolver::new(),
            dep_graph: Graph::new(),
            services: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            health_checkers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            state_tracker: Arc::new(tokio::sync::RwLock::new(
                StateTracker::new(work_dir.clone()).await?,
            )),
            work_dir,
            namespace,
            output_mode: OutputMode::default(),
            monitoring_shutdown: Arc::new(tokio::sync::Notify::new()),
            active_profiles: Vec::new(),
            monitoring_task: Arc::new(tokio::sync::Mutex::new(None)),
            startup_complete: Arc::new(AtomicBool::new(false)),
            port_listeners_released: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            cleanup_started: AtomicBool::new(false),
        })
    }

    /// Get the working directory for services
    pub fn work_dir(&self) -> &std::path::Path {
        &self.work_dir
    }

    /// Release port listeners once, just before services start.
    /// This minimizes the TOCTOU race window between port allocation and service bind.
    ///
    /// Uses atomic compare_exchange to ensure idempotency - only releases once
    /// even if called from multiple concurrent start operations.
    fn release_port_listeners_once(&self) {
        // Use compare_exchange to ensure we only release once
        if self
            .port_listeners_released
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.resolver.release_port_listeners();
        }
    }

    /// Cancel all in-progress operations.
    ///
    /// This will cause any ongoing `start`, `stop`, or `start_all` operations
    /// to return `Error::Cancelled`. The cancellation is cooperative - operations
    /// check the cancellation token at key points and exit gracefully.
    ///
    /// After cancellation, operations may leave services in a partially started
    /// or stopped state. Call `stop_all()` after cancellation to ensure cleanup.
    pub fn cancel_operations(&self) {
        self.cancellation_token.cancel();
    }

    /// Check if operations have been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Reset the cancellation token for new operations.
    ///
    /// This should be called after handling a cancellation if you want to
    /// perform new operations on the orchestrator.
    pub fn reset_cancellation(&mut self) {
        self.cancellation_token = CancellationToken::new();
    }

    /// Get a child cancellation token for use in spawned tasks.
    ///
    /// Child tokens are automatically cancelled when the parent is cancelled.
    pub fn child_token(&self) -> CancellationToken {
        self.cancellation_token.child_token()
    }

    /// Set the working directory for services
    pub async fn set_work_dir(&mut self, dir: PathBuf) -> Result<()> {
        self.work_dir = dir.clone();
        // Create new state tracker for the target directory.
        // Note: Both state trackers may briefly hold locks if directories differ,
        // but this is fine since they're different lock files. If they're the same
        // directory, the lock acquisition will gracefully handle the conflict.
        self.state_tracker = Arc::new(tokio::sync::RwLock::new(StateTracker::new(dir).await?));
        Ok(())
    }

    /// Set the output mode for process services.
    ///
    /// - `OutputMode::File`: Background mode, logs to files (used by `fed start`)
    /// - `OutputMode::Captured`: Interactive mode, logs to memory (used by TUI, `fed start -w`)
    /// - `OutputMode::Passthrough`: Pass-through mode, inherit stdio (used for testing, CI/CD)
    pub fn set_output_mode(&mut self, mode: OutputMode) {
        self.output_mode = mode;
    }

    /// Enable auto-resolve mode for port conflicts (use in TUI mode to avoid interactive prompts)
    pub fn set_auto_resolve_conflicts(&mut self, auto_resolve: bool) {
        self.resolver.set_auto_resolve_conflicts(auto_resolve);
    }

    /// Enable randomized port allocation.
    ///
    /// Skips the session port cache and allocates fresh random ports for all
    /// port-type parameters. Also enables auto-resolve to avoid interactive
    /// conflict prompts. Useful for running a second instance of the same
    /// project in a different worktree.
    pub fn set_randomize_ports(&mut self, randomize: bool) {
        self.resolver.set_isolated_mode(randomize);
        self.resolver.set_auto_resolve_conflicts(randomize);
    }

    /// Set active profiles for service filtering
    pub fn with_profiles(mut self, profiles: Vec<String>) -> Self {
        self.active_profiles = profiles;
        self
    }

    /// Initialize the orchestrator for service management.
    ///
    /// This must be called after creating the orchestrator and before starting any services.
    /// Initialization performs the following steps:
    ///
    /// 1. Load any existing state from the lock file
    /// 2. Clean up orphaned Docker containers from previous sessions
    /// 3. Resolve configuration parameters (including port allocation)
    /// 4. Expand external service dependencies
    /// 5. Apply service profiles filtering
    /// 6. Build the dependency graph
    /// 7. Create service managers for all configured services
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - State file is corrupted
    /// - Parameter resolution fails (e.g., port conflict)
    /// - External dependency cannot be loaded
    /// - Circular dependencies are detected
    /// - Service configuration is invalid
    pub async fn initialize(&mut self) -> Result<()> {
        // Initialize state tracker (loads existing lock file if present)
        self.state_tracker.write().await.initialize().await?;

        // Cleanup orphaned Docker containers from dead sessions
        match crate::service::DockerService::cleanup_orphaned_containers().await {
            Ok(removed) if removed > 0 => {
                tracing::info!("Cleaned up {} orphaned container(s) on startup", removed);
            }
            Ok(_) => {} // No orphaned containers
            Err(e) => {
                tracing::warn!("Failed to cleanup orphaned containers: {}", e);
                // Continue initialization even if cleanup fails
            }
        }

        // First pass: resolve parent parameters only (not services yet)
        // This allows us to use resolved parameter values when expanding external services
        self.resolver.resolve_parameters(&mut self.config)?;

        // Expand external services using resolved parent parameters
        let expander =
            ExternalServiceExpander::new(&self.config, &self.resolver, self.work_dir.clone());
        self.config = expander.expand().await?;

        // Set work directory for .env file resolution
        self.resolver.set_work_dir(&self.work_dir);

        // Second pass: full config resolution including external services
        let resolved = self.resolver.resolve_config(&self.config)?;
        self.config = resolved;

        // Track allocated ports in state
        for port in self.resolver.get_allocated_ports() {
            self.state_tracker.write().await.track_port(port).await;
        }

        // NOTE: Port listeners are held until services actually start.
        // This prevents TOCTOU race conditions where another process could
        // steal the port between allocation and service bind.
        // Listeners are released in start_all() or start_service().

        // Filter services based on active profiles (Docker Compose semantics):
        // - Services without profiles: always included
        // - Services with profiles: only included if at least one profile is active
        // Borrow active_profiles before the mutable borrow of services
        let active_profiles = &self.active_profiles;
        self.config.services.retain(|_, service| {
            // Services without profiles are always included
            if service.profiles.is_empty() {
                return true;
            }

            // Services with profiles are only included if at least one
            // of their profiles is active
            service.profiles.iter().any(|p| active_profiles.contains(p))
        });

        // Build dependency graph
        self.build_dependency_graph()?;

        // Create services
        self.create_services().await?;

        // Create health checkers
        self.create_health_checkers().await;

        // Save initial state
        self.state_tracker.write().await.save().await?;

        // Start background monitoring (if not in detached mode)
        self.start_monitoring().await;

        Ok(())
    }

    /// Build the dependency graph from config
    fn build_dependency_graph(&mut self) -> Result<()> {
        self.dep_graph = Graph::new();

        // Add all services as nodes
        for name in self.config.services.keys() {
            self.dep_graph.add_node(name.clone());
        }

        // Add dependency edges
        for (name, service) in &self.config.services {
            for dep in &service.depends_on {
                self.dep_graph
                    .add_edge(name.clone(), dep.service_name().to_string());
            }
        }

        // Check for circular dependencies (topological_sort returns the cycle if one exists)
        self.dep_graph.topological_sort()?;

        Ok(())
    }

    /// Create health checkers for services
    async fn create_health_checkers(&mut self) {
        for (name, service) in &self.config.services {
            if let Some(ref healthcheck) = service.healthcheck {
                // Use configured timeout or default (5 seconds)
                let timeout = healthcheck.get_timeout();

                let checker: Box<dyn HealthChecker> = match healthcheck.health_check_type() {
                    HealthCheckType::Http => {
                        if let Some(url) = healthcheck.get_http_url() {
                            // Use shared HTTP client to prevent file descriptor exhaustion
                            // when running many services with HTTP health checks
                            match HttpChecker::with_shared_client(url.to_string(), timeout) {
                                Ok(checker) => Box::new(checker),
                                Err(e) => {
                                    tracing::warn!(
                                        "Skipping invalid healthcheck URL for service '{}': {}",
                                        name,
                                        e
                                    );
                                    continue;
                                }
                            }
                        } else {
                            continue;
                        }
                    }
                    HealthCheckType::Command => {
                        if let Some(cmd) = healthcheck.get_command() {
                            // Docker services: run healthcheck inside container
                            // Process/Gradle services: run healthcheck on host
                            if service.image.is_some() {
                                // Docker service - use docker exec
                                let container_name = format!("fed-{}", name);
                                Box::new(DockerCommandChecker::new(
                                    container_name,
                                    cmd.to_string(),
                                    timeout,
                                ))
                            } else {
                                // Process/Gradle service - run on host
                                Box::new(CommandChecker::new(
                                    "bash".to_string(),
                                    vec!["-c".to_string(), cmd.to_string()],
                                    timeout,
                                ))
                            }
                        } else {
                            continue;
                        }
                    }
                    HealthCheckType::None => continue,
                };

                self.health_checkers
                    .write()
                    .await
                    .insert(name.clone(), checker);
            }
        }
    }

    /// Wait for a service to become healthy (used by script dependencies).
    /// Returns Ok(()) when healthy, or Err after timeout.
    pub async fn wait_for_healthy(&self, service_name: &str, timeout: Duration) -> Result<()> {
        let health_checkers = self.health_checkers.read().await;
        let checker = match health_checkers.get(service_name) {
            Some(c) => c,
            None => {
                // No healthcheck configured - consider it healthy after a brief moment
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Ok(());
            }
        };

        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(500);

        loop {
            if start.elapsed() > timeout {
                return Err(Error::Config(format!(
                    "Service '{}' did not become healthy within {:?}",
                    service_name, timeout
                )));
            }

            match checker.check().await {
                Ok(true) => {
                    tracing::debug!("Service '{}' is healthy", service_name);
                    return Ok(());
                }
                Ok(false) => {
                    tracing::debug!("Service '{}' not healthy yet, waiting...", service_name);
                }
                Err(e) => {
                    tracing::debug!(
                        "Service '{}' health check failed: {}, retrying...",
                        service_name,
                        e
                    );
                }
            }

            tokio::time::sleep(check_interval).await;
        }
    }

    /// Force run install command for a service (clears install state first).
    pub async fn run_install(&self, service_name: &str) -> Result<()> {
        let lifecycle =
            crate::orchestrator::ServiceLifecycleCommands::new(&self.config, &self.work_dir);
        lifecycle.run_install(service_name).await
    }

    /// Run install command for a service if needed.
    async fn run_install_if_needed(&self, service_name: &str) -> Result<()> {
        let lifecycle =
            crate::orchestrator::ServiceLifecycleCommands::new(&self.config, &self.work_dir);
        lifecycle.run_install_if_needed(service_name).await
    }

    /// Run clean command for a service.
    ///
    /// This will:
    /// 1. Run the user-defined clean command (if present)
    /// 2. Remove any Docker volumes associated with the service
    pub async fn run_clean(&self, service_name: &str) -> Result<()> {
        let lifecycle =
            crate::orchestrator::ServiceLifecycleCommands::new(&self.config, &self.work_dir);
        lifecycle.run_clean(service_name).await
    }

    /// Run build command for a service.
    pub async fn run_build(&self, service_name: &str) -> Result<()> {
        let lifecycle =
            crate::orchestrator::ServiceLifecycleCommands::new(&self.config, &self.work_dir);
        lifecycle.run_build(service_name).await
    }

    /// Start a specific service and its dependencies.
    ///
    /// This method is cancellable via `cancel_operations()` and respects
    /// the `startup_timeout` setting. Dependencies are started first in
    /// topological order.
    ///
    /// # Cancellation
    ///
    /// If cancelled during startup, returns `Error::Cancelled`. Services that
    /// have already started will remain running - call `stop_all()` to clean up.
    ///
    /// # Timeout
    ///
    /// Each service has `startup_timeout` to complete startup. If exceeded,
    /// returns `Error::Timeout`.
    pub async fn start(&self, service_name: &str) -> Result<()> {
        // Check for early cancellation
        if self.cancellation_token.is_cancelled() {
            return Err(Error::Cancelled(service_name.to_string()));
        }

        // Release port listeners just before starting services.
        // This minimizes the TOCTOU race window.
        self.release_port_listeners_once();

        // Get services to start in order
        let deps = self.dep_graph.get_dependencies(service_name);

        // Start dependencies first
        for dep in deps {
            // Check for cancellation before each dependency
            if self.cancellation_token.is_cancelled() {
                return Err(Error::Cancelled(dep.clone()));
            }
            self.start_service_with_timeout(&dep).await?;
        }

        // Start the requested service
        self.start_service_with_timeout(service_name).await
    }

    /// Start a service with timeout and cancellation support.
    async fn start_service_with_timeout(&self, name: &str) -> Result<()> {
        let cancel_token = self.cancellation_token.clone();
        let timeout = self.startup_timeout;

        tokio::select! {
            biased;

            _ = cancel_token.cancelled() => {
                Err(Error::Cancelled(name.to_string()))
            }

            result = tokio::time::timeout(timeout, self.start_service(name)) => {
                match result {
                    Ok(inner_result) => inner_result,
                    Err(_elapsed) => Err(Error::Timeout(name.to_string())),
                }
            }
        }
    }

    /// Start a single service
    async fn start_service(&self, name: &str) -> Result<()> {
        self.start_service_impl(name)
            .instrument(tracing::info_span!("start_service", service.name = %name))
            .await
    }

    /// Implementation of start_service (separate to allow instrumentation)
    async fn start_service_impl(&self, name: &str) -> Result<()> {
        // Get Arc clone of manager
        let manager_arc = {
            let services = self.services.read().await;
            if let Some(arc) = services.get(name) {
                Arc::clone(arc)
            } else {
                // Enhanced debugging for the "Service not found" paradox
                let available: Vec<_> = services.keys().collect();
                tracing::error!(
                    "Service '{}' not found in HashMap. Available services: {:?}. \
                     This should not happen - the service might have been removed during startup.",
                    name,
                    available
                );
                return Err(Error::ServiceNotFound(name.to_string()));
            }
        };

        // Check if already running (deduplication) and check for cancellation
        // IMPORTANT: We check cancellation inside the lock to prevent TOCTOU race
        // where cancellation happens between check and acquiring the lock
        {
            let manager = manager_arc.lock().await;

            // Check cancellation while holding the lock - this is the critical fix for TOCTOU
            if self.cancellation_token.is_cancelled() {
                tracing::debug!(
                    "start_service: cancellation detected inside lock for '{}'",
                    name
                );
                return Err(Error::Cancelled(name.to_string()));
            }

            let status = manager.status();
            if status == Status::Running || status == Status::Healthy {
                return Ok(());
            }
        }

        // Determine service type
        let service_type = if let Some(service_config) = self.config.services.get(name) {
            format!("{:?}", service_config.service_type())
        } else {
            "unknown".to_string()
        };

        // Atomically register service in state tracker before starting.
        // This prevents TOCTOU race conditions where multiple threads try to start the same service.
        // SQLite's ACID transactions ensure only one thread succeeds in registration.
        //
        // Note: The state tracker lock is released before actual service startup.
        // This is intentional - we use SQLite's atomic INSERT OR IGNORE pattern
        // for concurrency control, not the lock itself.
        let service_state =
            ServiceState::new(name.to_string(), service_type, self.namespace.clone());

        if !self
            .state_tracker
            .write()
            .await
            .register_service(service_state)
            .await?
        {
            // Service was already registered by another thread, bail out
            return Ok(());
        }

        // Run install step if needed
        self.run_install_if_needed(name)
            .instrument(tracing::info_span!("install_if_needed"))
            .await?;

        // Check cancellation before the expensive start operation
        if self.cancellation_token.is_cancelled() {
            tracing::debug!(
                "start_service: cancellation detected before start for '{}'",
                name
            );
            // Unregister since we registered but won't start
            self.state_tracker
                .write()
                .await
                .unregister_service(name)
                .await;
            return Err(Error::Cancelled(name.to_string()));
        }

        // Start the service
        // Lock the mutex and call start()
        // Service stays in HashMap during entire operation - no race conditions!
        async {
            let mut manager = manager_arc.lock().await;

            // Final cancellation check inside the lock
            if self.cancellation_token.is_cancelled() {
                tracing::debug!(
                    "start_service: cancellation detected inside start lock for '{}'",
                    name
                );
                // Don't call unregister here - it's called below in cleanup
                return Err(Error::Cancelled(name.to_string()));
            }

            manager.start().await.map_err(|e| {
                // Unregister on failure - note we can't await here, so use blocking
                // This is safe because we're in an error path
                Error::ServiceStartFailed(name.to_string(), e.to_string())
            })?;
            Ok(())
        }
        .instrument(tracing::info_span!("spawn_service"))
        .await?;

        // Unregister on failure if needed
        // (moved outside the lock to avoid holding it during state tracker operations)

        // Update state to running and save atomically
        // IMPORTANT: Hold write lock across all updates AND save to prevent race conditions
        // when multiple services start concurrently
        async {
            let mut tracker = self.state_tracker.write().await;
            tracker.update_service_status(name, "running").await?;

            // Store PID if available (for process services)
            // Store container ID if available (for docker services)
            {
                let manager = manager_arc.lock().await;
                if let Some(pid) = manager.get_pid() {
                    tracker.update_service_pid(name, pid).await?;
                }
                if let Some(container_id) = manager.get_container_id() {
                    tracker
                        .update_service_container_id(name, container_id)
                        .await?;
                }
                // Store port mappings if available (for docker services)
                let port_mappings = manager.get_port_mappings().await;
                if !port_mappings.is_empty() {
                    tracker
                        .update_service_port_mappings(name, port_mappings)
                        .await?;
                }
            }

            // Save while holding the write lock to ensure atomicity
            tracker.save().await?;
            Result::<()>::Ok(())
        }
        .instrument(tracing::info_span!("update_state"))
        .await?;

        Ok(())
    }

    /// Start all services respecting dependencies.
    ///
    /// Services are started in parallel groups based on dependency order.
    /// All services in a group are started concurrently, but groups are
    /// processed sequentially.
    ///
    /// # Cancellation
    ///
    /// This method is cancellable via `cancel_operations()`. If cancelled,
    /// the current group will complete but subsequent groups will not start.
    pub async fn start_all(&self) -> Result<()> {
        // Check for early cancellation
        if self.cancellation_token.is_cancelled() {
            return Err(Error::Cancelled("start_all".to_string()));
        }

        // Release port listeners just before starting services.
        // This minimizes the TOCTOU race window.
        self.release_port_listeners_once();

        // Get parallel groups
        let groups = self.dep_graph.get_parallel_groups()?;

        // Start each group sequentially, but services within each group start concurrently
        for group in groups {
            // Check for cancellation before each group
            if self.cancellation_token.is_cancelled() {
                return Err(Error::Cancelled("start_all".to_string()));
            }

            // Start all services in this group concurrently with timeout
            let futures: Vec<_> = group
                .iter()
                .map(|service_name| self.start_service_with_timeout(service_name))
                .collect();

            // Wait for all services in the group to start
            let results = futures::future::join_all(futures).await;

            // Collect any errors
            let errors: Vec<Error> = results.into_iter().filter_map(|r| r.err()).collect();

            // If any service failed, return aggregated errors
            if !errors.is_empty() {
                if errors.len() == 1 {
                    return Err(errors.into_iter().next().expect("errors not empty"));
                } else {
                    return Err(Error::Multiple(errors));
                }
            }
        }

        Ok(())
    }

    /// Stop a service and its dependents.
    ///
    /// Dependents are stopped first in reverse dependency order to ensure
    /// services don't lose their dependencies while running.
    ///
    /// # Cancellation
    ///
    /// This method respects cancellation but will attempt to stop services
    /// even if cancelled - a cancelled stop is less dangerous than a
    /// cancelled start. Returns `Error::Cancelled` after completing stops.
    pub async fn stop(&self, service_name: &str) -> Result<()> {
        // Get all dependents
        let dependents = self.get_all_dependents(service_name);

        let was_cancelled = self.cancellation_token.is_cancelled();

        // Stop dependents first (in reverse order)
        for dependent in dependents.iter().rev() {
            self.stop_service_with_timeout(dependent).await?;
        }

        // Stop the requested service
        self.stop_service_with_timeout(service_name).await?;

        // Report cancellation after completing the stop
        if was_cancelled {
            return Err(Error::Cancelled(service_name.to_string()));
        }

        Ok(())
    }

    /// Stop a service with timeout support.
    async fn stop_service_with_timeout(&self, name: &str) -> Result<()> {
        let timeout = self.stop_timeout;

        match tokio::time::timeout(timeout, self.stop_service(name)).await {
            Ok(result) => result,
            Err(_elapsed) => {
                tracing::warn!("Stop timeout exceeded for service '{}', forcing stop", name);
                // Even on timeout, we try to continue - the service might be stuck
                // but we don't want to leave other services hanging
                Ok(())
            }
        }
    }

    /// Get all transitive dependents of a service
    fn get_all_dependents(&self, service_name: &str) -> Vec<String> {
        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();

        self.traverse_dependents(service_name, &mut visited, &mut result);

        result
    }

    fn traverse_dependents(
        &self,
        service_name: &str,
        visited: &mut std::collections::HashSet<String>,
        result: &mut Vec<String>,
    ) {
        if visited.contains(service_name) {
            return;
        }
        visited.insert(service_name.to_string());

        let dependents = self.dep_graph.get_dependents(service_name);
        for dependent in dependents {
            self.traverse_dependents(&dependent, visited, result);
            result.push(dependent);
        }
    }

    /// Stop a single service
    async fn stop_service(&self, name: &str) -> Result<()> {
        self.stop_service_impl(name)
            .instrument(tracing::info_span!("stop_service", service.name = %name))
            .await
    }

    /// Implementation of stop_service (separate to allow instrumentation)
    async fn stop_service_impl(&self, name: &str) -> Result<()> {
        // Get Arc clone of manager
        let manager_arc = {
            let services = self.services.read().await;
            if let Some(arc) = services.get(name) {
                Arc::clone(arc)
            } else {
                // Service not found, nothing to stop
                return Ok(());
            }
        };

        // Lock and stop the service
        async {
            let mut manager = manager_arc.lock().await;
            if manager.status() == Status::Stopped {
                return Result::<()>::Ok(());
            }
            manager.stop().await?;
            Result::<()>::Ok(())
        }
        .instrument(tracing::info_span!("stop_service_manager"))
        .await?;

        // Unregister from state tracker
        async {
            let mut tracker = self.state_tracker.write().await;
            tracker.unregister_service(name).await;
            tracker.save().await?;
            Result::<()>::Ok(())
        }
        .instrument(tracing::info_span!("unregister_state"))
        .await?;

        Ok(())
    }

    /// Stop all running services.
    ///
    /// Services are stopped in reverse topological order to ensure
    /// dependents are stopped before their dependencies.
    ///
    /// Unlike `start` methods, stop operations continue even if cancelled
    /// to ensure proper cleanup.
    pub async fn stop_all(&self) -> Result<()> {
        // Get services in reverse topological order
        let order = self.dep_graph.topological_sort()?;

        let mut errors = Vec::new();

        // Stop in reverse order with timeout
        for name in order.iter().rev() {
            if let Err(e) = self.stop_service_with_timeout(name).await {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(Error::Multiple(errors));
        }

        Ok(())
    }

    /// Restart all services in dependency-aware order.
    ///
    /// Stops all services first (in reverse dependency order), then starts
    /// them all (in dependency order).
    ///
    /// # Cancellation
    ///
    /// The stop phase will complete even if cancelled, but the start phase
    /// can be cancelled.
    pub async fn restart_all(&self) -> Result<()> {
        // First, stop all services in reverse dependency order
        self.stop_all().await?;

        // Check for cancellation before starting
        if self.cancellation_token.is_cancelled() {
            return Err(Error::Cancelled("restart_all".to_string()));
        }

        // Then, start all services in dependency order
        self.start_all().await?;

        Ok(())
    }

    /// Get status of all services
    /// Also triggers health checks for running services to detect exits promptly
    pub async fn get_status(&self) -> HashMap<String, Status> {
        let services = self.services.read().await;
        let mut result = HashMap::new();
        for (name, arc) in services.iter() {
            let manager = arc.lock().await;
            let status = manager.status();

            // For running services, trigger a health check to detect exits
            // The health check is cached (500ms TTL) so this is inexpensive
            if status == Status::Running || status == Status::Healthy {
                // Health check updates status internally if process has died
                let _ = manager.health().await;
            }

            // Get status again after potential health check update
            result.insert(name.clone(), manager.status());
        }
        result
    }

    /// Get a service manager
    pub async fn get_service(&self, name: &str) -> Option<Status> {
        let services = self.services.read().await;
        match services.get(name) {
            Some(arc) => {
                let manager = arc.lock().await;
                Some(manager.status())
            }
            None => None,
        }
    }

    /// Get the dependency graph
    pub fn get_dependency_graph(&self) -> &Graph {
        &self.dep_graph
    }

    /// Get a reference to resolved parameters.
    ///
    /// This is more efficient than [`Self::get_resolved_parameters_owned`] when you only
    /// need to read the parameters without taking ownership.
    pub fn get_resolved_parameters(&self) -> &HashMap<String, String> {
        self.resolver.get_resolved_parameters()
    }

    /// Get an owned copy of resolved parameters.
    ///
    /// Use [`Self::get_resolved_parameters`] instead if you only need to read the parameters.
    pub fn get_resolved_parameters_owned(&self) -> HashMap<String, String> {
        self.resolver.get_resolved_parameters().clone()
    }

    /// Get names of parameters with `type: port`.
    pub fn get_port_parameter_names(&self) -> &[String] {
        self.resolver.get_port_parameter_names()
    }

    /// Release port listeners so that port conflict checks can detect external processes.
    ///
    /// In dry-run mode, the resolver holds TcpListeners on resolved ports to prevent TOCTOU races.
    /// These must be released before checking for conflicts, otherwise the conflict checker
    /// detects our own listeners as "conflicts".
    pub fn release_port_listeners(&self) {
        self.release_port_listeners_once();
    }

    /// Check if any services in the config are Docker-based.
    pub fn has_docker_services(&self) -> bool {
        self.config
            .services
            .values()
            .any(|svc| svc.image.is_some())
    }

    /// Check if a specific service is Docker-based (has an image).
    pub fn is_docker_service(&self, name: &str) -> bool {
        self.config
            .services
            .get(name)
            .map(|svc| svc.image.is_some())
            .unwrap_or(false)
    }

    /// Check if a specific service is process-based (has a process command).
    pub fn is_process_service(&self, name: &str) -> bool {
        self.config
            .services
            .get(name)
            .map(|svc| svc.process.is_some())
            .unwrap_or(false)
    }

    /// Get cloned services Arc for external access (e.g., signal handlers)
    pub fn get_services_arc(&self) -> SharedServiceRegistry {
        Arc::clone(&self.services)
    }

    /// Get service logs
    pub async fn get_logs(&self, service_name: &str, tail: Option<usize>) -> Result<Vec<String>> {
        let manager_arc = {
            let services = self.services.read().await;
            if let Some(arc) = services.get(service_name) {
                Arc::clone(arc)
            } else {
                return Err(Error::ServiceNotFound(service_name.to_string()));
            }
        };

        let manager = manager_arc.lock().await;
        manager.logs(tail).await
    }

    /// Get service PID (if applicable)
    pub async fn get_service_pid(&self, service_name: &str) -> Result<Option<u32>> {
        let manager_arc = {
            let services = self.services.read().await;
            if let Some(arc) = services.get(service_name) {
                Arc::clone(arc)
            } else {
                return Err(Error::ServiceNotFound(service_name.to_string()));
            }
        };

        let manager = manager_arc.lock().await;
        Ok(manager.get_pid())
    }

    /// Run a script by name
    pub async fn run_script(&self, script_name: &str) -> Result<std::process::Output> {
        let script = self
            .config
            .scripts
            .get(script_name)
            .ok_or_else(|| Error::ScriptNotFound(script_name.to_string()))?
            .clone();

        // Resolve dependencies - can be services or other scripts (SF-00035)
        // Note: We handle script deps by running them via run_script_interactive
        // since run_script can't easily be made recursive without boxing
        for dep in &script.depends_on {
            if self.config.scripts.contains_key(dep) {
                // Dependency is a script - run it via interactive runner
                // (which handles recursion via Box::pin)
                self.run_script_interactive(dep, &[]).await?;
            } else {
                // Dependency is a service - start it if not running
                if !self.is_service_running(dep).await {
                    self.start(dep).await?;
                }
            }
        }

        // Resolve script at execution time (scripts are not pre-resolved to support isolated)
        let params = self.resolver.get_resolved_parameters().clone();
        let resolved_env = self
            .resolver
            .resolve_environment(&script.environment, &params)
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to resolve environment for script '{}': {}",
                    script_name, e
                ))
            })?;

        let resolved_script_cmd = self
            .resolver
            .resolve_template_shell_safe(&script.script, &params)
            .map_err(|e| {
                Error::Config(format!("Failed to resolve script '{}': {}", script_name, e))
            })?;

        // Determine working directory
        let work_dir = if let Some(ref cwd) = script.cwd {
            std::path::Path::new(cwd).to_path_buf()
        } else {
            self.work_dir.clone()
        };

        // Build command - handle both single commands and shell scripts
        let mut command = if cfg!(target_os = "windows") {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/C", &resolved_script_cmd]);
            cmd
        } else {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(["-c", &resolved_script_cmd]);
            cmd
        };

        command.current_dir(&work_dir);
        command.envs(&resolved_env);

        // Execute with timeout - default 5 minutes to prevent hanging
        let timeout = std::time::Duration::from_secs(300);
        let output_result = tokio::time::timeout(timeout, command.output()).await;

        let output = match output_result {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(Error::Config(format!(
                    "Failed to execute script '{}': {}",
                    script_name, e
                )));
            }
            Err(_) => {
                return Err(Error::Config(format!(
                    "Script '{}' exceeded timeout of {} seconds",
                    script_name,
                    timeout.as_secs()
                )));
            }
        };

        Ok(output)
    }

    /// Run a script interactively with stdin/stdout/stderr passthrough.
    /// This is suitable for interactive TUIs like jest --watch.
    /// Returns only the exit status since output goes directly to terminal.
    ///
    /// Extra arguments are appended to the script command with proper shell escaping.
    /// Example: `fed test -- -t "specific test"` passes `-t "specific test"` to the script.
    ///
    /// If the script has `isolated: true`, it runs in an isolated context
    /// with fresh port allocations and isolated service instances.
    pub async fn run_script_interactive(
        &self,
        script_name: &str,
        extra_args: &[String],
    ) -> Result<std::process::ExitStatus> {
        let script = self
            .config
            .scripts
            .get(script_name)
            .ok_or_else(|| Error::ScriptNotFound(script_name.to_string()))?
            .clone();

        if script.isolated {
            // Execute in isolated context with fresh port allocations
            self.run_script_isolated(script_name, &script, extra_args)
                .await
        } else {
            // Execute in shared context (existing behavior)
            self.run_script_shared(script_name, &script, extra_args)
                .await
        }
    }

    /// Run a script in shared context (reuses session ports and services).
    async fn run_script_shared(
        &self,
        script_name: &str,
        script: &crate::config::Script,
        extra_args: &[String],
    ) -> Result<std::process::ExitStatus> {
        // Resolve dependencies - can be services or other scripts (SF-00035)
        for dep in &script.depends_on {
            if self.config.scripts.contains_key(dep) {
                // Dependency is a script - run it recursively
                // Note: Circular dependencies are caught by graph validation during config parsing
                // Use Box::pin to enable async recursion (avoids infinite future size)
                Box::pin(self.run_script_interactive(dep, &[])).await?;
            } else {
                // Dependency is a service - start it if not running
                if !self.is_service_running(dep).await {
                    self.start(dep).await?;
                }
            }
        }

        // Resolve script at execution time (scripts are not pre-resolved to support isolated)
        let params = self.resolver.get_resolved_parameters().clone();
        let resolved_env = self
            .resolver
            .resolve_environment(&script.environment, &params)
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to resolve environment for script '{}': {}",
                    script_name, e
                ))
            })?;

        let resolved_script = self
            .resolver
            .resolve_template_shell_safe(&script.script, &params)
            .map_err(|e| {
                Error::Config(format!("Failed to resolve script '{}': {}", script_name, e))
            })?;

        // Create resolved script for execution
        let resolved = crate::config::Script {
            cwd: script.cwd.clone(),
            depends_on: vec![], // Already resolved above
            environment: resolved_env,
            script: resolved_script,
            isolated: false,
        };

        // Execute the script command
        Self::execute_script_command(&resolved, extra_args, &self.work_dir).await
    }

    /// Run a script in isolated context with fresh port allocations (SF-00034).
    ///
    /// Creates a child orchestrator that:
    /// 1. Allocates fresh random ports for all port-type parameters
    /// 2. Starts dependencies with the new ports
    /// 3. Runs the script in isolation
    /// 4. Cleans up all services after completion
    async fn run_script_isolated(
        &self,
        script_name: &str,
        _script: &crate::config::Script, // Unused - we get the script from original_config
        extra_args: &[String],
    ) -> Result<std::process::ExitStatus> {
        tracing::info!(
            "Running script '{}' in isolated context (isolated: true)",
            script_name
        );

        // Use the ORIGINAL unresolved config so the child can re-resolve templates
        // with fresh port allocations. If original_config is None (shouldn't happen),
        // fall back to cloning the resolved config.
        let child_config = self
            .original_config
            .clone()
            .unwrap_or_else(|| self.config.clone());

        // Get the script from the original config (has unresolved templates)
        let original_script = child_config
            .scripts
            .get(script_name)
            .ok_or_else(|| Error::ScriptNotFound(script_name.to_string()))?
            .clone();

        let mut child_orchestrator = Orchestrator::new(child_config, self.work_dir.clone()).await?;
        child_orchestrator.output_mode = self.output_mode;

        // Enable isolated mode to skip session port cache and allocate fresh ports
        child_orchestrator.resolver.set_isolated_mode(true);

        // Clear the state tracker before initialization so the child doesn't restore
        // old container IDs from previous runs. Each isolated run should start fresh.
        child_orchestrator
            .state_tracker
            .write()
            .await
            .clear()
            .await?;

        // Initialize child orchestrator (allocates fresh ports due to isolated mode)
        child_orchestrator.initialize().await?;

        // Run script dependencies in the child context
        // Script-to-script dependencies run in parent context (they don't need isolation
        // unless they also have isolated)
        let mut service_deps = Vec::new();
        for dep in &original_script.depends_on {
            if self.config.scripts.contains_key(dep) {
                // Script dependency: run in parent context
                // (nested scripts get their own isolation if they have isolated)
                Box::pin(self.run_script_interactive(dep, &[])).await?;
            } else {
                // Service dependency: start in child context with isolated ports
                child_orchestrator.start(dep).await?;
                service_deps.push(dep.clone());
            }
        }

        // Wait for service dependencies to become healthy
        // This is important for isolated scripts that need DB connections etc.
        for dep in &service_deps {
            child_orchestrator
                .wait_for_healthy(dep, Duration::from_secs(60))
                .await?;
        }

        // Get the child's resolved parameters for the script environment
        let child_params = child_orchestrator
            .resolver
            .get_resolved_parameters()
            .clone();

        // Re-resolve the script's environment and command with child parameters
        // Using original_script which has unresolved templates like {{DB_PORT}}
        let resolved_env = child_orchestrator
            .resolver
            .resolve_environment(&original_script.environment, &child_params)
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to resolve environment for script '{}': {}",
                    script_name, e
                ))
            })?;

        let resolved_script_cmd = child_orchestrator
            .resolver
            .resolve_template_shell_safe(&original_script.script, &child_params)
            .map_err(|e| {
                Error::Config(format!("Failed to resolve script '{}': {}", script_name, e))
            })?;

        // Create a modified script with resolved environment and command
        let isolated_script = crate::config::Script {
            cwd: original_script.cwd.clone(),
            depends_on: vec![], // Already resolved
            environment: resolved_env,
            script: resolved_script_cmd,
            isolated: false, // Don't recurse
        };

        // Execute script in child context
        let result =
            Self::execute_script_command(&isolated_script, extra_args, &self.work_dir).await;

        // Cleanup child orchestrator (stops all services started in isolation)
        tracing::debug!("Cleaning up isolated context for script '{}'", script_name);
        child_orchestrator.cleanup().await;

        // Log which ports were used (helpful for debugging)
        if !child_params.is_empty() {
            let port_params: Vec<_> = child_params
                .iter()
                .filter(|(k, _)| k.to_uppercase().contains("PORT"))
                .collect();
            if !port_params.is_empty() {
                tracing::debug!("Isolated ports released: {:?}", port_params);
            }
        }

        result
    }

    /// Execute a script command with the given environment.
    /// This is the common script execution logic shared by both shared and isolated contexts.
    async fn execute_script_command(
        script: &crate::config::Script,
        extra_args: &[String],
        work_dir: &std::path::Path,
    ) -> Result<std::process::ExitStatus> {
        use crate::parameter::shell_escape;
        use std::process::Stdio;

        // Determine working directory
        let script_work_dir = if let Some(ref cwd) = script.cwd {
            std::path::Path::new(cwd).to_path_buf()
        } else {
            work_dir.to_path_buf()
        };

        // Check if script uses positional parameters ($@, $*, $1, $2, etc.)
        // If not, we'll auto-append arguments to the command for convenience
        let uses_positional_params = Self::script_uses_positional_params(&script.script);

        // Build command with inherited stdio for full TTY passthrough
        let mut command = if cfg!(target_os = "windows") {
            let mut cmd = tokio::process::Command::new("cmd");
            if extra_args.is_empty() {
                cmd.args(["/C", &script.script]);
            } else {
                // Windows: append args to the command
                let escaped_args: Vec<String> =
                    extra_args.iter().map(|arg| shell_escape(arg)).collect();
                let trimmed_script = script.script.trim_end();
                let full_script = format!("{} {}", trimmed_script, escaped_args.join(" "));
                cmd.args(["/C", &full_script]);
            }
            cmd
        } else {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c");

            if extra_args.is_empty() {
                // No arguments - just run the script
                cmd.arg(&script.script);
            } else if uses_positional_params {
                // Script uses $@, $1, etc. - pass args as positional parameters
                // Use: sh -c 'script' script_name arg1 arg2
                // This makes $@, $1, $2, etc. work properly in the script
                // The first arg after the script becomes $0 (shown in error messages)
                cmd.arg(&script.script);
                cmd.arg("script"); // $0 - appears in error messages
                cmd.args(extra_args); // $1, $2, ... and $@
            } else {
                // Script doesn't use positional params - auto-append args to command
                // This allows `script: prisma` to work with `fed prisma generate`
                let escaped_args: Vec<String> =
                    extra_args.iter().map(|arg| shell_escape(arg)).collect();
                let trimmed_script = script.script.trim_end();
                let full_script = format!("{} {}", trimmed_script, escaped_args.join(" "));
                cmd.arg(&full_script);
            }
            cmd
        };

        command.current_dir(&script_work_dir);
        command.envs(&script.environment);

        // Inherit stdio for interactive use - enables TUI, colors, and user input
        command.stdin(Stdio::inherit());
        command.stdout(Stdio::inherit());
        command.stderr(Stdio::inherit());

        // Spawn and wait for completion (no timeout for interactive scripts)
        let mut child = command
            .spawn()
            .map_err(|e| Error::Config(format!("Failed to spawn script: {}", e)))?;

        let status = child
            .wait()
            .await
            .map_err(|e| Error::Config(format!("Failed to wait for script: {}", e)))?;

        Ok(status)
    }

    /// Check if a script uses positional parameters ($@, $*, $1, $2, etc.)
    /// Used to decide whether to pass args via positional params or auto-append.
    fn script_uses_positional_params(script: &str) -> bool {
        // Match common positional parameter patterns:
        // $@ $* $1 $2 ... $9 ${1} ${@} ${*} ${10} etc.
        // Also match "$@" "$*" etc. (quoted forms)
        let patterns = [
            "$@", "$*", "$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8", "$9", "${@}", "${*}",
        ];

        for pattern in patterns {
            if script.contains(pattern) {
                return true;
            }
        }

        // Check for ${N} where N is a number (handles $10, $11, etc.)
        // Simple check: look for ${ followed by a digit
        let bytes = script.as_bytes();
        for i in 0..bytes.len().saturating_sub(2) {
            if bytes[i] == b'$' && bytes[i + 1] == b'{' {
                if let Some(&next) = bytes.get(i + 2) {
                    if next.is_ascii_digit() {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if a service is running
    async fn is_service_running(&self, service_name: &str) -> bool {
        let services = self.services.read().await;
        match services.get(service_name) {
            Some(arc) => {
                let manager = arc.lock().await;
                let status = manager.status();
                status == Status::Running || status == Status::Healthy
            }
            None => false,
        }
    }

    /// Get list of available scripts
    pub fn list_scripts(&self) -> Vec<String> {
        self.config.scripts.keys().cloned().collect()
    }

    /// Mark startup as complete - allows monitoring to clean up dead services
    /// Called after all initial services have been started to prevent race conditions
    pub fn mark_startup_complete(&self) {
        self.startup_complete.store(true, Ordering::SeqCst);
        tracing::debug!("Startup complete - monitoring cleanup enabled");
    }

    /// Cleanup resources.
    ///
    /// This method:
    /// 1. Ensures cleanup runs exactly once (using atomic guard)
    /// 2. Signals monitoring loop to shut down
    /// 3. Waits for monitoring task to finish (with timeout)
    /// 4. Stops all running services
    /// 5. Releases port listeners
    /// 6. Clears the lock file if all services are stopped
    ///
    /// Can be called with `&self` for concurrent access patterns.
    /// Multiple concurrent calls are safe - only the first will execute.
    pub async fn cleanup(&self) {
        // Use compare_exchange to ensure cleanup runs exactly once
        if self
            .cleanup_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::debug!("Cleanup already in progress or completed, skipping");
            return;
        }

        // Cancel all in-progress operations first
        // This ensures any running start/stop operations bail out quickly
        tracing::debug!("Cleanup: canceling in-progress operations");
        self.cancellation_token.cancel();

        // Signal monitoring loop to shut down
        tracing::debug!("Cleanup: signaling monitoring shutdown");
        self.monitoring_shutdown.notify_waiters();

        // Wait for monitoring task to finish (with timeout to avoid hanging)
        {
            let mut task_opt = self.monitoring_task.lock().await;
            if let Some(handle) = task_opt.take() {
                // Drop the guard before awaiting the handle
                drop(task_opt);
                // Wait for the task with a timeout to prevent indefinite hanging
                // No sleep needed - the cancel + notify should cause immediate response
                tracing::debug!("Cleanup: waiting for monitoring task");
                match tokio::time::timeout(Duration::from_secs(5), handle).await {
                    Ok(_) => tracing::debug!("Cleanup: monitoring task completed"),
                    Err(_) => tracing::warn!("Cleanup: monitoring task join timed out, continuing"),
                }
            }
        }

        tracing::debug!("Cleanup: stopping all services");
        let _ = self.stop_all().await;
        tracing::debug!("Cleanup: releasing port listeners");
        // Use shared cleanup since we only have &self
        self.resolver.cleanup_shared();

        // Clear lock file if all services are stopped
        tracing::debug!("Cleanup: checking state tracker");
        if self
            .state_tracker
            .read()
            .await
            .get_services()
            .await
            .is_empty()
        {
            let _ = self.state_tracker.write().await.clear().await;
        }
        tracing::debug!("Cleanup: complete");
    }
}

impl Drop for Orchestrator {
    fn drop(&mut self) {
        // Signal monitoring task to shut down (best-effort)
        // Note: This is a synchronous drop, so we can't await the task to finish
        // For proper cleanup with awaiting monitoring task completion,
        // call cleanup() explicitly before dropping
        self.monitoring_shutdown.notify_waiters();
    }
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("namespace", &self.namespace)
            .field("work_dir", &self.work_dir)
            .field("output_mode", &self.output_mode)
            .field("active_profiles", &self.active_profiles)
            .field("startup_timeout", &self.startup_timeout)
            .field("stop_timeout", &self.stop_timeout)
            .field(
                "startup_complete",
                &self.startup_complete.load(Ordering::Relaxed),
            )
            .field(
                "port_listeners_released",
                &self.port_listeners_released.load(Ordering::Relaxed),
            )
            .field(
                "cleanup_started",
                &self.cleanup_started.load(Ordering::Relaxed),
            )
            .field("is_cancelled", &self.is_cancelled())
            .field("resolver", &"<resolver>")
            .field("state_tracker", &"<state_tracker>")
            .field("services", &"<async>")
            .field("health_checkers", &"<async>")
            .field("monitoring_shutdown", &"<notify>")
            .field("monitoring_task", &"<async>")
            .field("cancellation_token", &"<token>")
            .field("config", &self.config)
            .field("dep_graph", &self.dep_graph)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// Test that concurrent cleanup calls are handled safely - only one executes
    #[tokio::test]
    async fn test_concurrent_cleanup_guard() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();
        let orchestrator = Orchestrator::new(config, temp_dir.path().to_path_buf()).await.unwrap();

        // Verify cleanup_started is initially false
        assert!(!orchestrator.cleanup_started.load(Ordering::SeqCst));

        // Run cleanup multiple times concurrently
        let orch = Arc::new(orchestrator);
        let orch1 = Arc::clone(&orch);
        let orch2 = Arc::clone(&orch);
        let orch3 = Arc::clone(&orch);

        let (r1, r2, r3) = tokio::join!(orch1.cleanup(), orch2.cleanup(), orch3.cleanup());

        // All should complete without panic (one executes, others skip)
        let _ = (r1, r2, r3);

        // Verify cleanup_started is now true
        assert!(orch.cleanup_started.load(Ordering::SeqCst));
    }

    /// Test that cleanup runs exactly once even with sequential calls
    #[tokio::test]
    async fn test_cleanup_runs_once() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();
        let orchestrator = Orchestrator::new(config, temp_dir.path().to_path_buf()).await.unwrap();
        let orch = Arc::new(orchestrator);

        // First cleanup should execute
        orch.cleanup().await;
        assert!(orch.cleanup_started.load(Ordering::SeqCst));

        // Second cleanup should be skipped (no-op)
        orch.cleanup().await;

        // Should still be true (no reset)
        assert!(orch.cleanup_started.load(Ordering::SeqCst));
    }

    /// Test that cleanup completes within a reasonable timeout
    #[tokio::test]
    async fn test_cleanup_does_not_hang() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();
        let orchestrator = Orchestrator::new(config, temp_dir.path().to_path_buf()).await.unwrap();
        let orch = Arc::new(orchestrator);

        // Cleanup should complete within 10 seconds even in worst case
        let result = tokio::time::timeout(Duration::from_secs(10), orch.cleanup()).await;

        // Should complete without timeout
        assert!(result.is_ok(), "Cleanup should complete within timeout");
    }

    /// Test that cleanup cancels the cancellation token
    #[tokio::test]
    async fn test_cleanup_cancels_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Config::default();
        let orchestrator = Orchestrator::new(config, temp_dir.path().to_path_buf()).await.unwrap();

        // Before cleanup, cancellation should not be set
        assert!(!orchestrator.is_cancelled());

        let orch = Arc::new(orchestrator);
        orch.cleanup().await;

        // After cleanup, cancellation should be set
        assert!(orch.is_cancelled());
    }
}
