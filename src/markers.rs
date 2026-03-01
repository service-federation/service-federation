use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Sanitize service name for safe filesystem usage.
///
/// Prevents path traversal attacks by:
/// - Rejecting empty names
/// - Rejecting names with path separators (/ or \)
/// - Rejecting names starting with dots (., ..)
/// - Replacing remaining special characters with underscores
fn sanitize_service_name_for_path(service_name: &str) -> Result<String> {
    if service_name.is_empty() {
        return Err(Error::Config("Service name cannot be empty".to_string()));
    }

    if service_name.contains('/') || service_name.contains('\\') {
        return Err(Error::Config(format!(
            "Service name '{}' contains path separators",
            service_name
        )));
    }

    if service_name.starts_with('.') {
        return Err(Error::Config(format!(
            "Service name '{}' cannot start with a dot",
            service_name
        )));
    }

    let sanitized: String = service_name
        .chars()
        .map(|c| match c {
            c if c.is_alphanumeric() || c == '-' || c == '_' => c,
            _ => '_',
        })
        .collect();

    Ok(sanitized)
}

/// Get the global installed directory, scoped by work_dir hash.
fn global_installed_dir(work_dir: &Path) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory".to_string()))?;
    let hash = crate::service::hash_work_dir(work_dir);
    Ok(home.join(".fed").join("installed").join(hash))
}

/// Check if a service has been installed (global, scoped by work_dir).
pub fn is_installed_global(service_name: &str, work_dir: &Path) -> Result<bool> {
    let installed_dir = global_installed_dir(work_dir)?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);
    Ok(marker_file.exists())
}

/// Mark a service as installed (global, scoped by work_dir).
pub fn mark_installed_global(service_name: &str, work_dir: &Path) -> Result<()> {
    let installed_dir = global_installed_dir(work_dir)?;
    fs::create_dir_all(&installed_dir)
        .map_err(|e| Error::Filesystem(format!("Failed to create installed directory: {}", e)))?;

    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();

    fs::write(&marker_file, timestamp.to_string())
        .map_err(|e| Error::Filesystem(format!("Failed to create install marker: {}", e)))?;

    Ok(())
}

/// Clear install state for a service (global, scoped by work_dir).
pub fn clear_installed_global(service_name: &str, work_dir: &Path) -> Result<()> {
    let installed_dir = global_installed_dir(work_dir)?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = installed_dir.join(sanitized);

    match fs::remove_file(&marker_file) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Filesystem(format!(
            "Failed to remove install marker: {}",
            e
        ))),
    }
}

/// Get the global migrated directory, scoped by work_dir hash.
fn global_migrated_dir(work_dir: &Path) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory".to_string()))?;
    let hash = crate::service::hash_work_dir(work_dir);
    Ok(home.join(".fed").join("migrated").join(hash))
}

/// Check if a service has been migrated (global, scoped by work_dir).
pub fn is_migrated_global(service_name: &str, work_dir: &Path) -> Result<bool> {
    let migrated_dir = global_migrated_dir(work_dir)?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = migrated_dir.join(sanitized);
    Ok(marker_file.exists())
}

/// Mark a service as migrated (global, scoped by work_dir).
pub fn mark_migrated_global(service_name: &str, work_dir: &Path) -> Result<()> {
    let migrated_dir = global_migrated_dir(work_dir)?;
    fs::create_dir_all(&migrated_dir)
        .map_err(|e| Error::Filesystem(format!("Failed to create migrated directory: {}", e)))?;

    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = migrated_dir.join(sanitized);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();

    fs::write(&marker_file, timestamp.to_string())
        .map_err(|e| Error::Filesystem(format!("Failed to create migrate marker: {}", e)))?;

    Ok(())
}

/// Clear migrate state for a service (global, scoped by work_dir).
pub fn clear_migrated_global(service_name: &str, work_dir: &Path) -> Result<()> {
    let migrated_dir = global_migrated_dir(work_dir)?;
    let sanitized = sanitize_service_name_for_path(service_name)?;
    let marker_file = migrated_dir.join(sanitized);

    match fs::remove_file(&marker_file) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Filesystem(format!(
            "Failed to remove migrate marker: {}",
            e
        ))),
    }
}

/// Clear all install markers for a work_dir (removes the entire directory).
pub fn clear_all_installed_global(work_dir: &Path) -> Result<()> {
    let installed_dir = global_installed_dir(work_dir)?;
    match fs::remove_dir_all(&installed_dir) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Filesystem(format!(
            "Failed to remove install markers: {}",
            e
        ))),
    }
}

/// Clear all migrate markers for a work_dir (removes the entire directory).
pub fn clear_all_migrated_global(work_dir: &Path) -> Result<()> {
    let migrated_dir = global_migrated_dir(work_dir)?;
    match fs::remove_dir_all(&migrated_dir) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Filesystem(format!(
            "Failed to remove migrate markers: {}",
            e
        ))),
    }
}

/// Lifecycle markers for install/migrate state tracking.
///
/// Always uses global (work_dir-scoped) marker files.
pub struct LifecycleMarkers {
    work_dir: PathBuf,
}

impl LifecycleMarkers {
    /// Create a new lifecycle markers context for the given work directory.
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    /// Check if a service has been installed.
    pub fn is_installed(&self, service_name: &str) -> Result<bool> {
        is_installed_global(service_name, &self.work_dir)
    }

    /// Mark a service as installed.
    pub fn mark_installed(&self, service_name: &str) -> Result<()> {
        mark_installed_global(service_name, &self.work_dir)
    }

    /// Clear install state for a service.
    pub fn clear_installed(&self, service_name: &str) -> Result<()> {
        clear_installed_global(service_name, &self.work_dir)
    }

    /// Check if a service has been migrated.
    pub fn is_migrated(&self, service_name: &str) -> Result<bool> {
        is_migrated_global(service_name, &self.work_dir)
    }

    /// Mark a service as migrated.
    pub fn mark_migrated(&self, service_name: &str) -> Result<()> {
        mark_migrated_global(service_name, &self.work_dir)
    }

    /// Clear migrate state for a service.
    pub fn clear_migrated(&self, service_name: &str) -> Result<()> {
        clear_migrated_global(service_name, &self.work_dir)
    }

    /// Clear all install markers for this work directory.
    pub fn clear_all_installed(&self) -> Result<()> {
        clear_all_installed_global(&self.work_dir)
    }

    /// Clear all migrate markers for this work directory.
    pub fn clear_all_migrated(&self) -> Result<()> {
        clear_all_migrated_global(&self.work_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_markers_global_mode() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = LifecycleMarkers::new(temp_dir.path().to_path_buf());

        let service_name = "test-service-global";

        let _ = ctx.clear_installed(service_name);

        assert!(!ctx.is_installed(service_name).expect("is_installed failed"));

        ctx.mark_installed(service_name)
            .expect("mark_installed failed");

        assert!(ctx.is_installed(service_name).expect("is_installed failed"));

        ctx.clear_installed(service_name)
            .expect("clear_installed failed");

        assert!(!ctx.is_installed(service_name).expect("is_installed failed"));
    }

    #[test]
    fn test_global_install_markers_isolated_by_work_dir() {
        let dir_a = tempfile::tempdir().expect("Failed to create temp dir A");
        let dir_b = tempfile::tempdir().expect("Failed to create temp dir B");

        let ctx_a = LifecycleMarkers::new(dir_a.path().to_path_buf());
        let ctx_b = LifecycleMarkers::new(dir_b.path().to_path_buf());

        let service_name = "test-isolated-service";

        let _ = ctx_a.clear_installed(service_name);
        let _ = ctx_b.clear_installed(service_name);

        ctx_a
            .mark_installed(service_name)
            .expect("mark_installed failed");

        assert!(ctx_a
            .is_installed(service_name)
            .expect("is_installed failed"));
        assert!(!ctx_b
            .is_installed(service_name)
            .expect("is_installed failed"));

        let _ = ctx_a.clear_installed(service_name);
    }

    #[test]
    fn test_lifecycle_markers_global_migrate_tracking() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = LifecycleMarkers::new(temp_dir.path().to_path_buf());

        let service_name = "test-service-migrate";

        let _ = ctx.clear_migrated(service_name);

        assert!(!ctx.is_migrated(service_name).expect("is_migrated failed"));

        ctx.mark_migrated(service_name)
            .expect("mark_migrated failed");

        assert!(ctx.is_migrated(service_name).expect("is_migrated failed"));

        ctx.clear_migrated(service_name)
            .expect("clear_migrated failed");

        assert!(!ctx.is_migrated(service_name).expect("is_migrated failed"));
    }

    #[test]
    fn test_global_migrate_markers_isolated_by_work_dir() {
        let dir_a = tempfile::tempdir().expect("Failed to create temp dir A");
        let dir_b = tempfile::tempdir().expect("Failed to create temp dir B");

        let ctx_a = LifecycleMarkers::new(dir_a.path().to_path_buf());
        let ctx_b = LifecycleMarkers::new(dir_b.path().to_path_buf());

        let service_name = "test-isolated-migrate";

        let _ = ctx_a.clear_migrated(service_name);
        let _ = ctx_b.clear_migrated(service_name);

        ctx_a
            .mark_migrated(service_name)
            .expect("mark_migrated failed");

        assert!(ctx_a.is_migrated(service_name).expect("is_migrated failed"));
        assert!(!ctx_b.is_migrated(service_name).expect("is_migrated failed"));

        let _ = ctx_a.clear_migrated(service_name);
    }

    #[test]
    fn test_clear_all_installed_removes_all_markers() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = LifecycleMarkers::new(temp_dir.path().to_path_buf());

        ctx.mark_installed("svc-a").expect("mark_installed failed");
        ctx.mark_installed("svc-b").expect("mark_installed failed");
        ctx.mark_installed("svc-c").expect("mark_installed failed");

        assert!(ctx.is_installed("svc-a").unwrap());
        assert!(ctx.is_installed("svc-b").unwrap());
        assert!(ctx.is_installed("svc-c").unwrap());

        ctx.clear_all_installed()
            .expect("clear_all_installed failed");

        assert!(!ctx.is_installed("svc-a").unwrap());
        assert!(!ctx.is_installed("svc-b").unwrap());
        assert!(!ctx.is_installed("svc-c").unwrap());
    }

    #[test]
    fn test_clear_all_migrated_removes_all_markers() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = LifecycleMarkers::new(temp_dir.path().to_path_buf());

        ctx.mark_migrated("svc-a").expect("mark_migrated failed");
        ctx.mark_migrated("svc-b").expect("mark_migrated failed");
        ctx.mark_migrated("svc-c").expect("mark_migrated failed");

        assert!(ctx.is_migrated("svc-a").unwrap());
        assert!(ctx.is_migrated("svc-b").unwrap());
        assert!(ctx.is_migrated("svc-c").unwrap());

        ctx.clear_all_migrated().expect("clear_all_migrated failed");

        assert!(!ctx.is_migrated("svc-a").unwrap());
        assert!(!ctx.is_migrated("svc-b").unwrap());
        assert!(!ctx.is_migrated("svc-c").unwrap());
    }

    #[test]
    fn test_clear_all_is_idempotent_on_empty() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = LifecycleMarkers::new(temp_dir.path().to_path_buf());

        // Should succeed even with no markers
        ctx.clear_all_installed()
            .expect("clear_all_installed on empty failed");
        ctx.clear_all_migrated()
            .expect("clear_all_migrated on empty failed");
    }
}
