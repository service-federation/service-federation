//! Parameter configuration types.
//!
//! This module contains the [`Parameter`] struct for configuring
//! variables with environment-specific defaults and type constraints.

use serde::{Deserialize, Serialize};

/// Parameter/variable configuration.
///
/// Parameters can have:
/// - A default value
/// - Environment-specific values (development, staging, production)
/// - Type constraints (e.g., "port" for automatic port allocation)
/// - Either constraints for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_yaml::Value>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub either: Vec<String>,

    // Environment-specific values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub development: Option<serde_yaml::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub develop: Option<serde_yaml::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub staging: Option<serde_yaml::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub production: Option<serde_yaml::Value>,

    #[serde(skip)]
    pub value: Option<String>,
}

impl Parameter {
    /// Check if this parameter is a port type (for automatic allocation).
    pub fn is_port_type(&self) -> bool {
        self.param_type.as_deref() == Some("port")
    }

    /// Get the value for a specific environment.
    /// Priority: environment-specific value > default value > None
    pub fn get_value_for_environment(
        &self,
        env: &super::Environment,
    ) -> Option<&serde_yaml::Value> {
        use super::Environment;

        match env {
            Environment::Development => {
                // Try "development" first, then "develop", then fall back to default
                self.development
                    .as_ref()
                    .or(self.develop.as_ref())
                    .or(self.default.as_ref())
            }
            Environment::Staging => {
                // Try "staging", then fall back to default
                self.staging.as_ref().or(self.default.as_ref())
            }
            Environment::Production => {
                // Try "production", then fall back to default
                self.production.as_ref().or(self.default.as_ref())
            }
        }
    }
}
