use crate::error::{validate_pid_for_check, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: String,
    pub created_at: SystemTime,
    pub workspace: PathBuf,
    pub shell_pid: Option<u32>,
    pub status: SessionStatus,
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Ended,
}

/// Port allocation info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortAllocation {
    pub parameter_name: String,
    pub port: u16,
    pub allocated_at: SystemTime,
}

/// Session manager
#[derive(Debug)]
pub struct Session {
    metadata: SessionMetadata,
    ports: HashMap<String, u16>,
    session_dir: PathBuf,
}

impl Session {
    /// Get the sessions root directory
    fn sessions_root() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| Error::Config("Could not determine home directory".to_string()))?;
        Ok(home.join(".fed").join("sessions"))
    }

    /// Get the directory for a specific session
    fn session_dir(session_id: &str) -> Result<PathBuf> {
        Ok(Self::sessions_root()?.join(session_id))
    }

    /// Generate a new random session ID with cryptographically secure randomness
    pub fn generate_id() -> String {
        use rand::Rng;

        let mut rng = rand::thread_rng();
        // Generate two 64-bit random numbers for 128 bits total entropy
        let high: u64 = rng.gen();
        let low: u64 = rng.gen();

        format!("sess-{:016x}{:016x}", high, low)
    }

    /// Validate session ID format
    pub fn validate_id(id: &str) -> Result<()> {
        if id.is_empty() {
            return Err(Error::Session("Session ID cannot be empty".to_string()));
        }

        if !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::Config(format!(
                "Session ID '{}' contains invalid characters. Only alphanumeric, '-', and '_' allowed.",
                id
            )));
        }

        if id.len() > 64 {
            return Err(Error::Config(format!(
                "Session ID '{}' is too long (max 64 characters)",
                id
            )));
        }

        Ok(())
    }

    /// Create a new session
    pub fn create(id: Option<String>, workspace: PathBuf) -> Result<Self> {
        let session_id = id.unwrap_or_else(Self::generate_id);
        Self::validate_id(&session_id)?;

        let session_dir = Self::session_dir(&session_id)?;

        // Ensure parent directory exists (sessions root)
        let sessions_root = Self::sessions_root()?;
        fs::create_dir_all(&sessions_root)
            .map_err(|e| Error::Session(format!("Failed to create sessions directory: {}", e)))?;

        // Atomically create session directory - fails if already exists (prevents TOCTOU race)
        match fs::create_dir(&session_dir) {
            Ok(_) => { /* success */ }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::Config(format!(
                    "Session '{}' already exists. Use 'fed session end {}' to cleanup first.",
                    session_id, session_id
                )));
            }
            Err(e) => {
                return Err(Error::Session(format!(
                    "Failed to create session directory: {}",
                    e
                )));
            }
        }

        let metadata = SessionMetadata {
            id: session_id.clone(),
            created_at: SystemTime::now(),
            workspace: workspace.canonicalize().unwrap_or(workspace),
            shell_pid: Self::get_parent_shell_pid(),
            status: SessionStatus::Active,
        };

        let session = Self {
            metadata,
            ports: HashMap::new(),
            session_dir,
        };

        // Create logs directory
        session.ensure_logs_dir()?;

        // Save initial state
        session.save()?;

        Ok(session)
    }

    /// Load an existing session
    pub fn load(session_id: &str) -> Result<Self> {
        Self::validate_id(session_id)?;
        let session_dir = Self::session_dir(session_id)?;

        if !session_dir.exists() {
            return Err(Error::Session(format!(" '{}' not found", session_id)));
        }

        let metadata = Self::load_metadata(&session_dir)?;
        let ports = Self::load_ports(&session_dir)?;

        Ok(Self {
            metadata,
            ports,
            session_dir,
        })
    }

    /// Get the current session from FED_SESSION env var or .fed/session file
    pub fn current() -> Result<Option<Self>> {
        Self::current_for_workdir(None)
    }

    /// Get the current session, checking the specified workdir for .fed/session file.
    ///
    /// This is the workdir-aware version of `current()`. Use this when you have
    /// a specific working directory (e.g., from `-w` flag) that may differ from
    /// the process's current directory.
    ///
    /// Priority:
    /// 1. FED_SESSION env var (always takes precedence)
    /// 2. .fed/session file in the specified workdir (if provided)
    /// 3. .fed/session file in current directory (fallback)
    pub fn current_for_workdir(workdir: Option<&Path>) -> Result<Option<Self>> {
        // First check FED_SESSION env var (takes precedence)
        if let Ok(session_id) = std::env::var("FED_SESSION") {
            return Ok(Some(Self::load(&session_id)?));
        }

        // Check workdir first if provided
        if let Some(dir) = workdir {
            if let Some(session_id) = Self::read_session_file_for_dir(dir)? {
                return Ok(Some(Self::load(&session_id)?));
            }
        }

        // Fall back to .fed/session file in current directory
        if let Some(session_id) = Self::read_session_file()? {
            return Ok(Some(Self::load(&session_id)?));
        }

        Ok(None)
    }

    /// Read session ID from .fed/session file in current directory
    pub fn read_session_file() -> Result<Option<String>> {
        let current_dir = std::env::current_dir()
            .map_err(|e| Error::Session(format!("Failed to get current directory: {}", e)))?;
        let fed_dir = current_dir.join(".fed");
        let session_file = fed_dir.join("session");

        // Directly try to read the file - avoid TOCTOU race
        let contents = match fs::read_to_string(&session_file) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(Error::Session(format!(
                    "Failed to read .fed/session: {}",
                    e
                )))
            }
        };

        let contents = contents.trim().to_string();
        if contents.is_empty() {
            return Ok(None);
        }

        Self::validate_id(&contents)?;
        Ok(Some(contents))
    }

    /// Read session ID from .fed/session file in a specific directory.
    ///
    /// This is the directory-aware version of `read_session_file()`.
    pub fn read_session_file_for_dir(dir: &Path) -> Result<Option<String>> {
        let fed_dir = dir.join(".fed");
        let session_file = fed_dir.join("session");

        // Directly try to read the file - avoid TOCTOU race
        let contents = match fs::read_to_string(&session_file) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(Error::Session(format!(
                    "Failed to read {}: {}",
                    session_file.display(),
                    e
                )))
            }
        };

        let contents = contents.trim().to_string();
        if contents.is_empty() {
            return Ok(None);
        }

        Self::validate_id(&contents)?;
        Ok(Some(contents))
    }

    /// Write session ID to .fed/session file in current directory
    pub fn write_session_file(session_id: &str) -> Result<()> {
        Self::validate_id(session_id)?;

        let current_dir = std::env::current_dir()
            .map_err(|e| Error::Session(format!("Failed to get current directory: {}", e)))?;
        let fed_dir = current_dir.join(".fed");

        // Create .fed directory if it doesn't exist
        fs::create_dir_all(&fed_dir)
            .map_err(|e| Error::Session(format!("Failed to create .fed directory: {}", e)))?;

        let session_file = fed_dir.join("session");

        fs::write(&session_file, format!("{}\n", session_id))
            .map_err(|e| Error::Session(format!("Failed to write .fed/session: {}", e)))?;

        Ok(())
    }

    /// Write session ID to .fed/session file in a specific directory.
    ///
    /// This is the directory-aware version of `write_session_file()`.
    pub fn write_session_file_for_dir(session_id: &str, dir: &Path) -> Result<()> {
        Self::validate_id(session_id)?;

        let fed_dir = dir.join(".fed");

        // Create .fed directory if it doesn't exist
        fs::create_dir_all(&fed_dir)
            .map_err(|e| Error::Session(format!("Failed to create .fed directory: {}", e)))?;

        let session_file = fed_dir.join("session");

        fs::write(&session_file, format!("{}\n", session_id)).map_err(|e| {
            Error::Session(format!("Failed to write {}: {}", session_file.display(), e))
        })?;

        Ok(())
    }

    /// Remove .fed/session file from current directory
    pub fn remove_session_file() -> Result<()> {
        let current_dir = std::env::current_dir()
            .map_err(|e| Error::Session(format!("Failed to get current directory: {}", e)))?;
        Self::remove_session_file_for_dir(&current_dir)
    }

    /// Remove .fed/session file from a specific directory.
    ///
    /// This is the directory-aware version of `remove_session_file()`.
    pub fn remove_session_file_for_dir(dir: &Path) -> Result<()> {
        let session_file = dir.join(".fed").join("session");

        // Directly try to remove the file - avoid TOCTOU race
        // Ignore NotFound errors since the goal is to ensure file doesn't exist
        match fs::remove_file(&session_file) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Config(format!(
                "Failed to remove {}: {}",
                session_file.display(),
                e
            ))),
        }
    }

    /// Auto-cleanup orphaned sessions (silent)
    /// Returns the number of sessions cleaned up
    pub fn auto_cleanup_orphaned() -> Result<usize> {
        let sessions = Self::list_all()?;
        let mut cleaned_count = 0;

        for metadata in sessions {
            if let Ok(session) = Self::load(&metadata.id) {
                if !session.is_shell_alive() {
                    // Clean up orphaned session
                    if let Err(e) = session.delete() {
                        tracing::warn!("Failed to delete orphaned session {}: {}", metadata.id, e);
                    }
                    cleaned_count += 1;
                }
            }
        }

        Ok(cleaned_count)
    }

    /// List all sessions
    pub fn list_all() -> Result<Vec<SessionMetadata>> {
        let sessions_root = Self::sessions_root()?;

        if !sessions_root.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in fs::read_dir(sessions_root)
            .map_err(|e| Error::Session(format!("Failed to read sessions directory: {}", e)))?
        {
            let entry = entry
                .map_err(|e| Error::Session(format!("Failed to read session entry: {}", e)))?;

            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Ok(metadata) = Self::load_metadata(&entry.path()) {
                    sessions.push(metadata);
                }
            }
        }

        // Sort by created_at descending (newest first)
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(sessions)
    }

    /// Save session state
    pub fn save(&self) -> Result<()> {
        Self::save_metadata(&self.session_dir, &self.metadata)?;
        Self::save_ports(&self.session_dir, &self.ports)?;
        Ok(())
    }

    /// Delete session data
    pub fn delete(&self) -> Result<()> {
        fs::remove_dir_all(&self.session_dir)
            .map_err(|e| Error::Session(format!("Failed to delete session directory: {}", e)))
    }

    /// Get session ID
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    /// Get session workspace
    pub fn workspace(&self) -> &Path {
        &self.metadata.workspace
    }

    /// Get the logs directory for this session
    pub fn logs_dir(&self) -> PathBuf {
        self.session_dir.join("logs")
    }

    /// Get log file path for a specific service
    /// Sanitizes service name for filesystem safety
    pub fn log_file_path(&self, service_name: &str) -> PathBuf {
        let sanitized =
            sanitize_service_name_for_path(service_name).unwrap_or_else(|_| "invalid".to_string());
        self.logs_dir().join(format!("{}.log", sanitized))
    }

    /// Create logs directory if it doesn't exist
    pub fn ensure_logs_dir(&self) -> Result<()> {
        let logs_dir = self.logs_dir();
        if !logs_dir.exists() {
            fs::create_dir_all(&logs_dir)
                .map_err(|e| Error::Config(format!("Failed to create logs directory: {}", e)))?;
        }
        Ok(())
    }

    /// Get allocated port for a parameter
    pub fn get_port(&self, parameter_name: &str) -> Option<u16> {
        self.ports.get(parameter_name).copied()
    }

    /// Allocate and save a port
    pub fn save_port(&mut self, parameter_name: String, port: u16) -> Result<()> {
        self.ports.insert(parameter_name, port);
        Self::save_ports(&self.session_dir, &self.ports)
    }

    /// Check if a service has been installed
    pub fn is_installed(&self, service_name: &str) -> bool {
        let installed_dir = self.session_dir.join("installed");
        let sanitized =
            sanitize_service_name_for_path(service_name).unwrap_or_else(|_| "invalid".to_string());
        let marker_file = installed_dir.join(sanitized);
        marker_file.exists()
    }

    /// Mark a service as installed
    pub fn mark_installed(&self, service_name: &str) -> Result<()> {
        let installed_dir = self.session_dir.join("installed");
        fs::create_dir_all(&installed_dir)
            .map_err(|e| Error::Config(format!("Failed to create installed directory: {}", e)))?;

        let sanitized = sanitize_service_name_for_path(service_name)?;
        let marker_file = installed_dir.join(sanitized);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();

        fs::write(&marker_file, timestamp.to_string())
            .map_err(|e| Error::Config(format!("Failed to create install marker: {}", e)))?;

        Ok(())
    }

    /// Clear install state for a service
    pub fn clear_installed(&self, service_name: &str) -> Result<()> {
        let installed_dir = self.session_dir.join("installed");
        let sanitized = sanitize_service_name_for_path(service_name)?;
        let marker_file = installed_dir.join(sanitized);

        // Directly try to remove - avoid TOCTOU race
        match fs::remove_file(&marker_file) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Config(format!(
                "Failed to remove install marker: {}",
                e
            ))),
        }
    }

    /// Check if parent shell process is still alive
    pub fn is_shell_alive(&self) -> bool {
        if let Some(pid) = self.metadata.shell_pid {
            Self::process_exists(pid)
        } else {
            // No shell PID tracked, assume alive
            true
        }
    }

    // Private helper methods

    fn load_metadata(session_dir: &Path) -> Result<SessionMetadata> {
        let path = session_dir.join("metadata.json");
        let contents = fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("Failed to read metadata: {}", e)))?;
        serde_json::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse metadata: {}", e)))
    }

    fn save_metadata(session_dir: &Path, metadata: &SessionMetadata) -> Result<()> {
        let path = session_dir.join("metadata.json");
        let contents = serde_json::to_string_pretty(metadata)
            .map_err(|e| Error::Config(format!("Failed to serialize metadata: {}", e)))?;

        // Use atomic write-then-rename to prevent corruption
        Self::atomic_write(&path, &contents)
    }

    fn load_ports(session_dir: &Path) -> Result<HashMap<String, u16>> {
        let path = session_dir.join("ports.json");
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let contents = fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("Failed to read ports: {}", e)))?;
        serde_json::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse ports: {}", e)))
    }

    fn save_ports(session_dir: &Path, ports: &HashMap<String, u16>) -> Result<()> {
        let path = session_dir.join("ports.json");
        let contents = serde_json::to_string_pretty(ports)
            .map_err(|e| Error::Config(format!("Failed to serialize ports: {}", e)))?;

        // Use atomic write-then-rename to prevent corruption
        Self::atomic_write(&path, &contents)
    }

    /// Atomic file write using write-then-rename pattern
    /// This prevents file corruption if the process crashes during write
    fn atomic_write(path: &Path, contents: &str) -> Result<()> {
        use std::io::Write;

        // Write to temp file first
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| Error::Config(format!("Failed to create temp file: {}", e)))?;

        file.write_all(contents.as_bytes())
            .map_err(|e| Error::Config(format!("Failed to write temp file: {}", e)))?;

        // Ensure data is written to disk before rename
        file.sync_all()
            .map_err(|e| Error::Config(format!("Failed to sync temp file: {}", e)))?;

        // Close file explicitly before rename
        drop(file);

        // Atomic rename (on Unix systems)
        fs::rename(&temp_path, path)
            .map_err(|e| Error::Config(format!("Failed to rename temp file: {}", e)))?;

        Ok(())
    }

    fn get_parent_shell_pid() -> Option<u32> {
        // Try to get parent process ID
        #[cfg(unix)]
        {
            use std::os::unix::process::parent_id;
            Some(parent_id())
        }

        #[cfg(not(unix))]
        {
            None
        }
    }

    fn process_exists(pid: u32) -> bool {
        #[cfg(unix)]
        {
            use nix::sys::signal::kill;

            // Validate PID for read-only check (rejects 0 and >i32::MAX)
            let Some(nix_pid) = validate_pid_for_check(pid) else {
                return false;
            };

            // Send signal 0 to check if process exists (doesn't actually signal)
            // Using safe nix wrapper instead of raw libc call
            match kill(nix_pid, None) {
                Ok(_) => true,
                Err(nix::errno::Errno::ESRCH) => false, // No such process
                Err(nix::errno::Errno::EPERM) => true,  // Permission denied means it exists
                Err(_) => false,                        // Other errors treat as non-existent
            }
        }

        #[cfg(not(unix))]
        {
            // On Windows, assume process exists (we can't easily check)
            true
        }
    }
}

/// Sanitize service name for safe filesystem usage (module-level helper)
///
/// Prevents path traversal attacks by:
/// - Rejecting empty names
/// - Rejecting names with path separators (/ or \)
/// - Rejecting names starting with dots (., ..)
/// - Replacing remaining special characters with underscores
///
/// Returns sanitized name or an error if the name is invalid
fn sanitize_service_name_for_path(service_name: &str) -> Result<String> {
    if service_name.is_empty() {
        return Err(Error::Config("Service name cannot be empty".to_string()));
    }

    // Reject path separators entirely - no traversal allowed
    if service_name.contains('/') || service_name.contains('\\') {
        return Err(Error::Config(format!(
            "Service name '{}' contains path separators",
            service_name
        )));
    }

    // Reject leading dots to prevent .. attacks and hidden files
    if service_name.starts_with('.') {
        return Err(Error::Config(format!(
            "Service name '{}' cannot start with a dot",
            service_name
        )));
    }

    // Replace other potentially problematic characters with underscores
    let sanitized: String = service_name
        .chars()
        .map(|c| match c {
            // Allow alphanumeric, dash, underscore
            c if c.is_alphanumeric() || c == '-' || c == '_' => c,
            // Replace everything else with underscore
            _ => '_',
        })
        .collect();

    Ok(sanitized)
}

/// Get the global installed directory (for non-session mode)
fn global_installed_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory".to_string()))?;
    Ok(home.join(".fed").join("installed"))
}

/// Check if a service has been installed (global, non-session mode)
pub fn is_installed_global(service_name: &str) -> Result<bool> {
    let installed_dir = global_installed_dir()?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);
    Ok(marker_file.exists())
}

/// Mark a service as installed (global, non-session mode)
pub fn mark_installed_global(service_name: &str) -> Result<()> {
    let installed_dir = global_installed_dir()?;
    fs::create_dir_all(&installed_dir)
        .map_err(|e| Error::Config(format!("Failed to create installed directory: {}", e)))?;

    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();

    fs::write(&marker_file, timestamp.to_string())
        .map_err(|e| Error::Config(format!("Failed to create install marker: {}", e)))?;

    Ok(())
}

/// Clear install state for a service (global, non-session mode)
pub fn clear_installed_global(service_name: &str) -> Result<()> {
    let installed_dir = global_installed_dir()?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);

    // Directly try to remove - avoid TOCTOU race
    match fs::remove_file(&marker_file) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Config(format!(
            "Failed to remove install marker: {}",
            e
        ))),
    }
}

/// A context wrapper that abstracts away the session vs. global install state logic.
///
/// This struct encapsulates the common pattern of checking if we're in session mode
/// and dispatching to either session-scoped or global install tracking methods.
///
/// # Example
///
/// ```no_run
/// use service_federation::session::SessionContext;
///
/// # fn example() -> Result<(), service_federation::Error> {
/// let ctx = SessionContext::current()?;
/// ctx.mark_installed("my-service")?;
///
/// if ctx.is_installed("my-service")? {
///     println!("Service is installed");
/// }
/// # Ok(())
/// # }
/// ```
pub struct SessionContext {
    session: Option<Session>,
}

impl SessionContext {
    /// Get the current session context.
    ///
    /// Returns a context that either wraps an active session (if `FED_SESSION` env var
    /// is set or `.fed/session` file exists) or falls back to global mode.
    pub fn current() -> Result<Self> {
        let session = Session::current()?;
        Ok(Self { session })
    }

    /// Check if a service has been installed.
    ///
    /// Uses session-scoped tracking if in a session, otherwise uses global tracking.
    pub fn is_installed(&self, service_name: &str) -> Result<bool> {
        if let Some(ref session) = self.session {
            Ok(session.is_installed(service_name))
        } else {
            is_installed_global(service_name)
        }
    }

    /// Mark a service as installed.
    ///
    /// Uses session-scoped tracking if in a session, otherwise uses global tracking.
    pub fn mark_installed(&self, service_name: &str) -> Result<()> {
        if let Some(ref session) = self.session {
            session.mark_installed(service_name)
        } else {
            mark_installed_global(service_name)
        }
    }

    /// Clear install state for a service.
    ///
    /// Uses session-scoped tracking if in a session, otherwise uses global tracking.
    pub fn clear_installed(&self, service_name: &str) -> Result<()> {
        if let Some(ref session) = self.session {
            session.clear_installed(service_name)
        } else {
            clear_installed_global(service_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_id_valid() {
        assert!(Session::validate_id("sess-abc123").is_ok());
        assert!(Session::validate_id("my-session").is_ok());
        assert!(Session::validate_id("test_session").is_ok());
        assert!(Session::validate_id("s").is_ok());
    }

    #[test]
    fn test_validate_id_invalid() {
        assert!(Session::validate_id("").is_err());
        assert!(Session::validate_id("session with spaces").is_err());
        assert!(Session::validate_id("session/slash").is_err());
        assert!(Session::validate_id("session@special").is_err());

        let long_id = "a".repeat(65);
        assert!(Session::validate_id(&long_id).is_err());
    }

    #[test]
    fn test_generate_id() {
        let id1 = Session::generate_id();
        let id2 = Session::generate_id();

        // Should be different
        assert_ne!(id1, id2);

        // Should be valid
        assert!(Session::validate_id(&id1).is_ok());
        assert!(Session::validate_id(&id2).is_ok());

        // Should have correct prefix
        assert!(id1.starts_with("sess-"));
        assert!(id2.starts_with("sess-"));
    }

    #[test]
    fn test_session_context_global_mode() {
        // When no session exists, SessionContext should use global mode
        let ctx = SessionContext::current().expect("Failed to create context");

        // Test install tracking in global mode
        let service_name = "test-service-global";

        // Clear any existing state
        let _ = ctx.clear_installed(service_name);

        // Initially not installed
        assert!(!ctx.is_installed(service_name).expect("is_installed failed"));

        // Mark as installed
        ctx.mark_installed(service_name)
            .expect("mark_installed failed");

        // Now should be installed
        assert!(ctx.is_installed(service_name).expect("is_installed failed"));

        // Clear installation
        ctx.clear_installed(service_name)
            .expect("clear_installed failed");

        // Should not be installed anymore
        assert!(!ctx.is_installed(service_name).expect("is_installed failed"));
    }
}
