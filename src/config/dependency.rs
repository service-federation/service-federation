//! Dependency configuration types.
//!
//! This module contains types for configuring service dependencies,
//! including external dependencies and metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// External git repository dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Git repository URL
    pub repo: String,

    /// Optional branch to checkout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Policy for handling dependency failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyFailurePolicy {
    /// Stop the dependent service when dependency fails (default)
    #[default]
    Stop,
    /// Restart the dependent service when dependency fails
    Restart,
    /// Ignore dependency failures and keep the service running
    Ignore,
}

/// Dependency reference - supports both simple strings and structured external refs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependsOn {
    /// Simple service name dependency
    Simple(String),
    /// External service dependency with alias
    External {
        external: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alias: Option<String>,
    },
    /// Structured dependency with health propagation policy
    Structured {
        service: String,
        #[serde(default, skip_serializing_if = "is_default_policy")]
        on_failure: DependencyFailurePolicy,
    },
}

fn is_default_policy(policy: &DependencyFailurePolicy) -> bool {
    *policy == DependencyFailurePolicy::default()
}

impl DependsOn {
    /// Check if this is a simple dependency.
    pub fn is_simple(&self) -> bool {
        matches!(self, DependsOn::Simple(_))
    }

    /// Check if this is an external dependency.
    pub fn is_external(&self) -> bool {
        matches!(self, DependsOn::External { .. })
    }

    /// Get the service name (either simple name or external name).
    pub fn service_name(&self) -> &str {
        match self {
            DependsOn::Simple(name) => name,
            DependsOn::External { external, .. } => external,
            DependsOn::Structured { service, .. } => service,
        }
    }

    /// Get the alias for external dependencies.
    pub fn alias(&self) -> Option<&str> {
        match self {
            DependsOn::External { alias, .. } => alias.as_deref(),
            _ => None,
        }
    }

    /// Get the failure policy for this dependency.
    pub fn failure_policy(&self) -> DependencyFailurePolicy {
        match self {
            DependsOn::Structured { on_failure, .. } => *on_failure,
            _ => DependencyFailurePolicy::default(),
        }
    }
}

/// Metadata section for service configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_dependencies: Vec<ExternalDependency>,
}

/// External dependency definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub repo: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_strategy: Option<BranchStrategy>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<ExternalServiceInfo>,
}

/// Branch strategy for external dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchStrategy {
    /// Branch mappings: local_branch -> remote_branch
    #[serde(flatten)]
    pub mappings: HashMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl BranchStrategy {
    /// Resolve the target branch based on current branch and strategy.
    pub fn resolve_branch(&self, current_branch: &str) -> String {
        self.mappings
            .get(current_branch)
            .cloned()
            .or_else(|| self.default.clone())
            .unwrap_or_else(|| current_branch.to_string())
    }
}

/// Information about a service exposed by an external dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalServiceInfo {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}
