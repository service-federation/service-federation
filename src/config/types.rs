//! Core configuration types.
//!
//! This module contains the root [`Config`] struct and related types
//! for the service federation configuration file.

// Allow deprecated PackageAuth - we define it here for backward compatibility
#![allow(deprecated)]

use super::{Dependency, Metadata, Parameter, Script, Service};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration structure for service-federation.yaml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Legacy field for backward compatibility
    #[serde(default)]
    pub parameters: HashMap<String, Parameter>,

    /// Preferred field for variables with environment overrides
    #[serde(default)]
    pub variables: HashMap<String, Parameter>,

    #[serde(default)]
    pub services: HashMap<String, Service>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub templates: HashMap<String, Service>,

    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entrypoints: Vec<String>,

    #[serde(default)]
    pub scripts: HashMap<String, Script>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageReference>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Environment files for setting parameter values.
    /// Variables in these files MUST be declared as parameters in this config.
    /// This provides a way to override parameter defaults without modifying
    /// the config file (e.g., for local secrets or environment-specific values).
    ///
    /// Paths are relative to the config file directory.
    ///
    /// NOTE: Paths do NOT support template substitution (no `{{VAR}}`) to avoid
    /// circular dependencies. Use parameter environment fields (development/staging/production)
    /// for environment-specific defaults instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_file: Vec<String>,
}

impl Config {
    /// Get effective parameters/variables.
    /// Priority: variables (if present) > parameters (backward compatibility)
    pub fn get_effective_parameters(&self) -> &HashMap<String, Parameter> {
        if !self.variables.is_empty() {
            &self.variables
        } else {
            &self.parameters
        }
    }

    /// Get mutable effective parameters/variables.
    pub fn get_effective_parameters_mut(&mut self) -> &mut HashMap<String, Parameter> {
        if !self.variables.is_empty() {
            &mut self.variables
        } else {
            &mut self.parameters
        }
    }

    /// Expand service names and tag references to a list of services.
    ///
    /// Supports both direct service names and tag references:
    /// - `"api"` -> `["api"]`
    /// - `"@backend"` -> all services with "backend" tag
    /// - `["api", "@async"]` -> `["api"]` + all services with "async" tag
    ///
    /// Returns a deduplicated list of service names.
    pub fn expand_service_selection(&self, selection: &[String]) -> Vec<String> {
        let mut result = std::collections::HashSet::new();

        for item in selection {
            if let Some(tag) = item.strip_prefix('@') {
                // Tag reference - find all services with this tag
                for (name, service) in &self.services {
                    if service.tags.contains(&tag.to_string()) {
                        result.insert(name.clone());
                    }
                }
            } else {
                // Direct service name
                result.insert(item.clone());
            }
        }

        let mut services: Vec<String> = result.into_iter().collect();
        services.sort();
        services
    }

    /// Get all services that have a specific tag.
    pub fn services_with_tag(&self, tag: &str) -> Vec<String> {
        let mut services: Vec<String> = self
            .services
            .iter()
            .filter(|(_, service)| service.tags.contains(&tag.to_string()))
            .map(|(name, _)| name.clone())
            .collect();
        services.sort();
        services
    }
}

/// Package reference for importing external packages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageReference {
    /// Package source (local path, github, registry, git+ssh)
    pub source: String,

    /// Alias for this package (used in extends)
    #[serde(rename = "as")]
    pub r#as: String,

    /// Optional authentication (deprecated - configure git credentials using system tools instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[allow(deprecated)]
    pub auth: Option<PackageAuth>,
}

/// Authentication configuration for packages.
///
/// **DEPRECATED**: Package auth configuration is deprecated and will be removed in a future version.
/// Configure git credentials using standard git mechanisms instead:
/// - For SSH: Use ssh-agent to manage your SSH keys
/// - For HTTPS: Use git credential helpers or environment variables (GH_TOKEN, GITHUB_TOKEN)
///
/// This type is kept for backward compatibility but auth credentials are no longer used.
#[deprecated(
    since = "0.1.0",
    note = "Configure git credentials using ssh-agent or git credential helpers instead"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PackageAuth {
    Token {
        token: String,
    },
    SshKey {
        key_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
    },
    Basic {
        username: String,
        password: String,
    },
}
