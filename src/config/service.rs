//! Service configuration types.
//!
//! This module contains the [`Service`] struct and related types for
//! configuring services in the federation config.

use super::{CircuitBreakerConfig, DependsOn, HealthCheck, ResourceLimits, RestartPolicy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Service configuration for a single service in the federation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Service {
    // Package extension
    /// Reference to a package service to extend from (format: "package-alias.service-name")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,

    // Process-based service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<String>,

    /// Command to clean up service artifacts (e.g., "rm -rf node_modules")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clean: Option<String>,

    /// Command to build the service (e.g., "npm run build", "cargo build --release")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,

    // Docker-based service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,

    // External dependency service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, String>,

    // Gradle task service
    #[serde(rename = "gradleTask", skip_serializing_if = "Option::is_none")]
    pub gradle_task: Option<String>,

    // Docker Compose service
    #[serde(rename = "composeFile", skip_serializing_if = "Option::is_none")]
    pub compose_file: Option<String>,

    #[serde(rename = "composeService", skip_serializing_if = "Option::is_none")]
    pub compose_service: Option<String>,

    // Common fields
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthcheck: Option<HealthCheck>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<DependsOn>,

    // Restart policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartPolicy>,

    // Service exposure for external consumption
    #[serde(default, skip_serializing_if = "is_false")]
    pub expose: bool,

    // Service profiles for conditional startup
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,

    // Service tags for flexible grouping (e.g., "backend", "critical", "async")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    // Watch paths for auto-restart on file changes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<String>,

    // Resource limits for controlling service resource usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceLimits>,

    /// Grace period for graceful shutdown before SIGKILL (default: 10s)
    /// Format: duration string like "10s", "30s", "1m", "500ms"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grace_period: Option<String>,

    /// Circuit breaker configuration for crash loop detection.
    ///
    /// When enabled, the circuit breaker monitors restart frequency and
    /// temporarily disables restarts if a service is crash looping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circuit_breaker: Option<CircuitBreakerConfig>,

    /// Template string printed after all services start (supports `{{PARAM}}` syntax).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_message: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// Types of services supported by the federation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Process,
    Docker,
    External,
    GradleTask,
    DockerCompose,
    Undefined,
}

impl Service {
    /// Determine the type of service based on configured fields.
    pub fn service_type(&self) -> ServiceType {
        if self.compose_file.is_some() && self.compose_service.is_some() {
            ServiceType::DockerCompose
        } else if self.process.is_some() {
            ServiceType::Process
        } else if self.image.is_some() {
            ServiceType::Docker
        } else if self.dependency.is_some() && self.service.is_some() {
            ServiceType::External
        } else if self.gradle_task.is_some() {
            ServiceType::GradleTask
        } else {
            ServiceType::Undefined
        }
    }

    /// Get grace period duration, defaulting to 10 seconds.
    ///
    /// Parses duration strings like "10s", "30s", "1m", "500ms".
    /// Returns 10 seconds if the format is invalid or not specified.
    pub fn get_grace_period(&self) -> std::time::Duration {
        self.grace_period
            .as_ref()
            .and_then(|s| parse_duration_string(s))
            .unwrap_or(std::time::Duration::from_secs(10))
    }
}

/// Parse duration string like "10s", "30s", "1m", "500ms"
fn parse_duration_string(s: &str) -> Option<std::time::Duration> {
    use std::time::Duration;

    let s = s.trim();
    if s.ends_with("ms") {
        s.trim_end_matches("ms")
            .parse::<u64>()
            .ok()
            .map(Duration::from_millis)
    } else if s.ends_with('s') {
        s.trim_end_matches('s')
            .parse::<u64>()
            .ok()
            .map(Duration::from_secs)
    } else if s.ends_with('m') {
        s.trim_end_matches('m')
            .parse::<u64>()
            .ok()
            .map(|m| Duration::from_secs(m * 60))
    } else {
        // Default to seconds if no suffix
        s.parse::<u64>().ok().map(Duration::from_secs)
    }
}
