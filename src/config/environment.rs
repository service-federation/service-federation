use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Environment type for variable resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Environment {
    #[default]
    Development,
    Staging,
    Production,
}

impl Environment {
    /// Get all possible environment names (including aliases)
    pub fn all_names() -> Vec<&'static str> {
        vec!["development", "develop", "staging", "production"]
    }

    /// Check if a string is a valid environment name
    pub fn is_valid_name(s: &str) -> bool {
        matches!(
            s.to_lowercase().as_str(),
            "development" | "develop" | "staging" | "production"
        )
    }
}

impl FromStr for Environment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "development" | "develop" => Ok(Environment::Development),
            "staging" => Ok(Environment::Staging),
            "production" => Ok(Environment::Production),
            _ => Err(format!(
                "Invalid environment '{}'. Valid values: development, develop, staging, production",
                s
            )),
        }
    }
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Environment::Development => write!(f, "development"),
            Environment::Staging => write!(f, "staging"),
            Environment::Production => write!(f, "production"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_from_str() {
        assert_eq!(
            Environment::from_str("development").unwrap(),
            Environment::Development
        );
        assert_eq!(
            Environment::from_str("develop").unwrap(),
            Environment::Development
        );
        assert_eq!(
            Environment::from_str("staging").unwrap(),
            Environment::Staging
        );
        assert_eq!(
            Environment::from_str("production").unwrap(),
            Environment::Production
        );
    }

    #[test]
    fn test_environment_from_str_case_insensitive() {
        assert_eq!(
            Environment::from_str("DEVELOPMENT").unwrap(),
            Environment::Development
        );
        assert_eq!(
            Environment::from_str("Staging").unwrap(),
            Environment::Staging
        );
    }

    #[test]
    fn test_environment_from_str_invalid() {
        assert!(Environment::from_str("invalid").is_err());
        assert!(Environment::from_str("test").is_err());
    }

    #[test]
    fn test_environment_display() {
        assert_eq!(Environment::Development.to_string(), "development");
        assert_eq!(Environment::Staging.to_string(), "staging");
        assert_eq!(Environment::Production.to_string(), "production");
    }

    #[test]
    fn test_environment_default() {
        assert_eq!(Environment::default(), Environment::Development);
    }
}
