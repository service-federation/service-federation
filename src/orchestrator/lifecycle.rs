//! Service lifecycle commands for install and clean operations.
//!
//! This module contains the `ServiceLifecycleCommands` struct which encapsulates
//! all install and clean logic that was previously part of the main Orchestrator.
//! Extracting these operations improves separation of concerns and makes the
//! orchestrator core more focused on service coordination.

use crate::config::Config;
use crate::error::{Error, Result};
use std::path::Path;
use std::process::Stdio;

/// Commands for managing service lifecycle operations (install/clean).
///
/// This struct provides high-level operations for installing service dependencies
/// and cleaning up service artifacts. It abstracts away the details of running
/// shell commands and managing Docker volumes.
///
/// # Example
///
/// ```ignore
/// use service_federation::{Config, orchestrator::ServiceLifecycleCommands};
/// use std::path::Path;
///
/// # async fn example() -> Result<(), service_federation::Error> {
/// let config = Config::default();
/// let work_dir = Path::new(".");
/// let lifecycle = ServiceLifecycleCommands::new(&config, work_dir);
///
/// // Run install for a service
/// lifecycle.run_install("my-service").await?;
///
/// // Clean up service artifacts
/// lifecycle.run_clean("my-service").await?;
/// # Ok(())
/// # }
/// ```
pub struct ServiceLifecycleCommands<'a> {
    config: &'a Config,
    work_dir: &'a Path,
}

impl<'a> ServiceLifecycleCommands<'a> {
    /// Create a new lifecycle commands handler.
    ///
    /// # Arguments
    ///
    /// * `config` - Reference to the service configuration
    /// * `work_dir` - Working directory for resolving relative paths
    pub fn new(config: &'a Config, work_dir: &'a Path) -> Self {
        Self { config, work_dir }
    }

    /// Force run install command for a service (clears install state first).
    ///
    /// This method will:
    /// 1. Clear any existing install state
    /// 2. Run the install command unconditionally
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service to install
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Service not found in configuration
    /// - Install command fails
    /// - Session context is unavailable
    pub async fn run_install(&self, service_name: &str) -> Result<()> {
        // Clear install state first
        let ctx = crate::session::SessionContext::current()?;
        ctx.clear_installed(service_name)?;

        // Run install
        self.run_install_if_needed(service_name).await
    }

    /// Run install command for a service if needed.
    ///
    /// This checks if the service has already been installed in the current session
    /// or globally (depending on mode). If already installed, it skips the install step.
    ///
    /// The install command runs in the service's configured working directory with
    /// its environment variables. Special handling is included for npm's ENOTEMPTY
    /// error which can occur when node_modules already exists.
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service to install
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Service not found in configuration
    /// - Install command execution fails
    /// - Install command returns non-zero exit code
    pub async fn run_install_if_needed(&self, service_name: &str) -> Result<()> {
        // Get service config
        let service_config = self
            .config
            .services
            .get(service_name)
            .ok_or_else(|| Error::ServiceNotFound(service_name.to_string()))?;

        // Check if service has install command
        let install_cmd = match &service_config.install {
            Some(cmd) => cmd,
            None => return Ok(()), // No install command, skip
        };

        // Check if already installed
        let ctx = crate::session::SessionContext::current()?;
        let is_installed = ctx.is_installed(service_name)?;

        if is_installed {
            tracing::debug!(
                "Service '{}' already installed, skipping install step",
                service_name
            );
            return Ok(());
        }

        // Run install command
        tracing::info!(
            "Running install command for service '{}': {}",
            service_name,
            install_cmd
        );

        // Get working directory from service config, relative to orchestrator work_dir
        let cwd = if let Some(ref service_cwd) = service_config.cwd {
            let cwd_path = std::path::Path::new(service_cwd);
            if cwd_path.is_absolute() {
                cwd_path.to_path_buf()
            } else {
                self.work_dir.join(service_cwd)
            }
        } else {
            self.work_dir.to_path_buf()
        };

        // Get environment variables (need to resolve parameters first)
        let mut env_vars = std::collections::HashMap::new();
        for (key, value) in &service_config.environment {
            env_vars.insert(key.clone(), value.clone());
        }

        // Run the install command with streaming output
        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(install_cmd)
            .current_dir(cwd)
            .envs(&env_vars)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to execute install command for '{}': {}",
                    service_name, e
                ))
            })?;

        if !status.success() {
            return Err(Error::Config(format!(
                "Install command failed for '{}'",
                service_name
            )));
        }

        // Mark as installed
        let ctx = crate::session::SessionContext::current()?;
        ctx.mark_installed(service_name)?;

        tracing::info!(
            "Successfully completed install for service '{}'",
            service_name
        );
        Ok(())
    }

    /// Run build command for a service.
    ///
    /// This runs the user-defined build command for the service. Build commands
    /// are typically used for compiling code, bundling assets, or other build steps.
    ///
    /// Unlike install, build does not track whether the service has been built before,
    /// so it will always run the build command.
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service to build
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Service not found in configuration
    /// - Service has no build command defined
    /// - Build command execution fails
    /// - Build command returns non-zero exit code
    pub async fn run_build(&self, service_name: &str) -> Result<()> {
        // Get service config
        let service_config = self
            .config
            .services
            .get(service_name)
            .ok_or_else(|| Error::ServiceNotFound(service_name.to_string()))?;

        // Check if service has build command
        let build_cmd = match &service_config.build {
            Some(cmd) => cmd,
            None => {
                return Err(Error::Config(format!(
                    "Service '{}' has no build command defined",
                    service_name
                )));
            }
        };

        // Run build command
        tracing::info!(
            "Running build command for service '{}': {}",
            service_name,
            build_cmd
        );

        // Get working directory from service config, relative to orchestrator work_dir
        let cwd = if let Some(ref service_cwd) = service_config.cwd {
            let cwd_path = std::path::Path::new(service_cwd);
            if cwd_path.is_absolute() {
                cwd_path.to_path_buf()
            } else {
                self.work_dir.join(service_cwd)
            }
        } else {
            self.work_dir.to_path_buf()
        };

        // Get environment variables
        let mut env_vars = std::collections::HashMap::new();
        for (key, value) in &service_config.environment {
            env_vars.insert(key.clone(), value.clone());
        }

        // Run the build command with streaming output
        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(build_cmd)
            .current_dir(cwd)
            .envs(&env_vars)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to execute build command for '{}': {}",
                    service_name, e
                ))
            })?;

        if !status.success() {
            return Err(Error::Config(format!(
                "Build command failed for '{}'",
                service_name
            )));
        }

        tracing::info!(
            "Successfully completed build for service '{}'",
            service_name
        );
        Ok(())
    }

    /// Run clean command for a service.
    ///
    /// This will:
    /// 1. Run the user-defined clean command (if present)
    /// 2. Remove any Docker volumes associated with the service
    /// 3. Clear the install state
    ///
    /// Only Docker volumes with the `fed-` prefix are automatically removed for safety.
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service to clean
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Service not found in configuration
    /// - Clean command execution fails
    /// - Clean command returns non-zero exit code
    pub async fn run_clean(&self, service_name: &str) -> Result<()> {
        // Get service config
        let service_config = self
            .config
            .services
            .get(service_name)
            .ok_or_else(|| Error::ServiceNotFound(service_name.to_string()))?;

        let has_clean_cmd = service_config.clean.is_some();
        let has_volumes = !service_config.volumes.is_empty();

        // If no clean command and no volumes, nothing to do
        if !has_clean_cmd && !has_volumes {
            return Ok(());
        }

        // Run user-defined clean command if present
        if let Some(ref clean_cmd) = service_config.clean {
            tracing::info!(
                "Running clean command for service '{}': {}",
                service_name,
                clean_cmd
            );

            // Get working directory from service config, relative to orchestrator work_dir
            let cwd = if let Some(ref service_cwd) = service_config.cwd {
                let cwd_path = std::path::Path::new(service_cwd);
                if cwd_path.is_absolute() {
                    cwd_path.to_path_buf()
                } else {
                    self.work_dir.join(service_cwd)
                }
            } else {
                self.work_dir.to_path_buf()
            };

            // Get environment variables
            let mut env_vars = std::collections::HashMap::new();
            for (key, value) in &service_config.environment {
                env_vars.insert(key.clone(), value.clone());
            }

            // Run the clean command with streaming output
            let status = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(clean_cmd)
                .current_dir(cwd)
                .envs(&env_vars)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .await
                .map_err(|e| {
                    Error::Config(format!(
                        "Failed to execute clean command for '{}': {}",
                        service_name, e
                    ))
                })?;

            if !status.success() {
                return Err(Error::Config(format!(
                    "Clean command failed for '{}'",
                    service_name
                )));
            }
        }

        // Clean up Docker volumes if present
        if has_volumes {
            self.clean_docker_volumes(service_name, &service_config.volumes)
                .await?;
        }

        // Clear install state since we cleaned up
        let ctx = crate::session::SessionContext::current()?;
        ctx.clear_installed(service_name)?;

        tracing::info!(
            "Successfully completed clean for service '{}'",
            service_name
        );
        Ok(())
    }

    /// Clean up Docker volumes for a service.
    ///
    /// Only removes named volumes (not bind mounts) that follow the fed naming convention.
    /// Named volumes are identified by not starting with `.` or `/`.
    /// For safety, only volumes prefixed with `fed-` are removed to prevent accidental
    /// deletion of user-managed volumes.
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service
    /// * `volumes` - List of volume specifications from service config
    ///
    /// # Errors
    ///
    /// This method logs warnings for failed volume removals but does not return an error.
    /// It's designed to be resilient - if some volumes can't be removed, it continues
    /// attempting to remove the rest.
    async fn clean_docker_volumes(&self, service_name: &str, volumes: &[String]) -> Result<()> {
        // Extract all named volumes first
        let all_named_volumes: Vec<String> = volumes
            .iter()
            .filter_map(|v| Self::extract_named_volume(v))
            .collect();

        // Filter to only fed- prefixed volumes for safety
        let fed_volumes: Vec<String> = all_named_volumes
            .iter()
            .filter(|v| v.starts_with("fed-"))
            .cloned()
            .collect();

        // Log skipped volumes for transparency
        let skipped_volumes: Vec<&String> = all_named_volumes
            .iter()
            .filter(|v| !v.starts_with("fed-"))
            .collect();

        if !skipped_volumes.is_empty() {
            tracing::info!(
                "Skipping non-fed volumes for service '{}': {:?} (only fed-* volumes are auto-cleaned)",
                service_name,
                skipped_volumes
            );
        }

        if fed_volumes.is_empty() {
            tracing::debug!(
                "No fed-managed volumes found for service '{}' to clean",
                service_name
            );
            return Ok(());
        }

        tracing::info!(
            "Removing Docker volumes for service '{}': {:?}",
            service_name,
            fed_volumes
        );

        let mut removed: Vec<String> = Vec::new();
        let mut failed: Vec<(String, String)> = Vec::new();

        for volume in &fed_volumes {
            let output = tokio::process::Command::new("docker")
                .args(["volume", "rm", "-f", volume])
                .output()
                .await;

            match output {
                Ok(result) if result.status.success() => {
                    tracing::info!("Removed Docker volume: {}", volume);
                    removed.push(volume.clone());
                }
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    // Volume might not exist, which is fine
                    if stderr.contains("No such volume") {
                        tracing::debug!("Volume '{}' does not exist, skipping", volume);
                    } else {
                        tracing::warn!(
                            "Failed to remove Docker volume '{}': {}",
                            volume,
                            stderr.trim()
                        );
                        failed.push((volume.clone(), stderr.trim().to_string()));
                    }
                }
                Err(e) => {
                    let err_msg = format!("Failed to run docker volume rm: {}", e);
                    tracing::warn!("Failed to remove volume '{}': {}", volume, err_msg);
                    failed.push((volume.clone(), err_msg));
                }
            }
        }

        // Report summary
        if !removed.is_empty() {
            tracing::info!(
                "Successfully removed {} volume(s) for service '{}'",
                removed.len(),
                service_name
            );
        }

        if !failed.is_empty() {
            tracing::warn!(
                "Failed to remove {} volume(s) for service '{}': {:?}",
                failed.len(),
                service_name,
                failed
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
            );
        }

        Ok(())
    }

    /// Extract named volume from a volume spec.
    ///
    /// Returns Some(volume_name) for named volumes, None for bind mounts.
    /// Named volumes don't start with `.`, `/`, or `~`, don't contain path separators,
    /// and don't look like Windows drive letters.
    ///
    /// # Examples
    ///
    /// ```
    /// use service_federation::orchestrator::ServiceLifecycleCommands;
    ///
    /// // Named volumes
    /// assert_eq!(
    ///     ServiceLifecycleCommands::extract_named_volume("myvolume:/data"),
    ///     Some("myvolume".to_string())
    /// );
    ///
    /// // Bind mounts
    /// assert_eq!(ServiceLifecycleCommands::extract_named_volume("./data:/data"), None);
    /// assert_eq!(ServiceLifecycleCommands::extract_named_volume("/host/path:/data"), None);
    ///
    /// // With options
    /// assert_eq!(
    ///     ServiceLifecycleCommands::extract_named_volume("myvolume:/data:ro"),
    ///     Some("myvolume".to_string())
    /// );
    ///
    /// // Windows paths
    /// assert_eq!(ServiceLifecycleCommands::extract_named_volume("C:/data:/app"), None);
    /// assert_eq!(ServiceLifecycleCommands::extract_named_volume("C:\\data:/app"), None);
    /// ```
    pub fn extract_named_volume(spec: &str) -> Option<String> {
        let parts: Vec<&str> = spec.split(':').collect();
        if parts.is_empty() {
            return None;
        }

        let source = parts[0];

        // Empty source is not a valid volume
        if source.is_empty() {
            return None;
        }

        // Windows drive letter detection: single letter that looks like a drive
        // This catches cases where the spec is "C:/path:/container" which splits to ["C", "/path", "/container"]
        // The source would be just "C" - a single ASCII letter
        if source.len() == 1 {
            let first_char = source.chars().next().unwrap();
            if first_char.is_ascii_alphabetic() {
                // Single letter source with more parts suggests Windows drive path
                // e.g., "C:/foo:/bar" splits to ["C", "/foo", "/bar"]
                if parts.len() >= 2 && (parts[1].starts_with('/') || parts[1].starts_with('\\')) {
                    return None;
                }
            }
        }

        // Bind mounts start with `.`, `/`, or `~`
        if source.starts_with('.')
            || source.starts_with('/')
            || source.starts_with('~')
            || source.contains('/')
            || source.contains('\\')
        {
            return None;
        }

        // It's a named volume
        Some(source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_named_volume_basic() {
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("myvolume:/data"),
            Some("myvolume".to_string())
        );
    }

    #[test]
    fn test_extract_named_volume_with_options() {
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("myvolume:/data:ro"),
            Some("myvolume".to_string())
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("myvolume:/data:ro,cached"),
            Some("myvolume".to_string())
        );
    }

    #[test]
    fn test_extract_named_volume_bind_mount_relative() {
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("./data:/data"),
            None
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("../data:/data"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_bind_mount_absolute() {
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("/host/path:/data"),
            None
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("/var/lib/data:/data"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_tilde_path() {
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("~/data:/data"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_empty() {
        assert_eq!(ServiceLifecycleCommands::extract_named_volume(""), None);
    }

    #[test]
    fn test_extract_named_volume_container_only() {
        // Container-only paths (no source) should return None
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("/data"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_with_slashes_in_name() {
        // Volume names with slashes are bind mounts
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("path/to/data:/data"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_windows_drive_forward_slash() {
        // Windows paths with forward slashes: "C:/data:/container" splits to ["C", "/data", "/container"]
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("C:/data:/data"),
            None
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("D:/Users/me:/home"),
            None
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("E:/Projects:/work"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_windows_drive_backslash() {
        // Windows paths with backslashes contain \ so are already filtered
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("C:\\data:/data"),
            None
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("D:\\Users\\me:/home"),
            None
        );
    }

    #[test]
    fn test_extract_named_volume_single_letter_not_windows() {
        // Single letter volume names without Windows-style paths should work
        // "v:/data" where the second part doesn't start with / or \ is a valid named volume
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("v:data"),
            Some("v".to_string())
        );
    }

    #[test]
    fn test_extract_named_volume_fed_prefix() {
        // Fed-prefixed volumes should be extracted normally
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("fed-myapp-data:/data"),
            Some("fed-myapp-data".to_string())
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("fed-cache:/cache"),
            Some("fed-cache".to_string())
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume(
                "fed-session-abc123-postgres:/var/lib/postgresql"
            ),
            Some("fed-session-abc123-postgres".to_string())
        );
    }

    #[test]
    fn test_extract_named_volume_not_fed_prefix() {
        // Non-fed volumes are still extracted (filtering happens in clean_docker_volumes)
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("postgres-data:/data"),
            Some("postgres-data".to_string())
        );
        assert_eq!(
            ServiceLifecycleCommands::extract_named_volume("myapp-cache:/cache"),
            Some("myapp-cache".to_string())
        );
    }
}
