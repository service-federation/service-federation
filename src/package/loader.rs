use crate::config::{Config, Parser};
use crate::error::{Error, Result};
use std::path::Path;

/// Package loader for loading package configurations
pub struct PackageLoader {
    parser: Parser,
}

impl PackageLoader {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    /// Load package configuration from path
    pub fn load(&self, path: &Path) -> Result<Config> {
        let config_path = path.join("service-federation.yaml");

        if !config_path.exists() {
            let alt_path = path.join("service-federation.yml");
            if !alt_path.exists() {
                return Err(Error::Package(format!(
                    "Package at {} does not contain service-federation.yaml or service-federation.yml",
                    path.display()
                )));
            }
            return self.parser.load_config(&alt_path);
        }

        self.parser.load_config(&config_path)
    }

    /// Load package with validation
    pub fn load_and_validate(&self, path: &Path) -> Result<Config> {
        let config = self.load(path)?;
        config.validate()?;
        Ok(config)
    }
}

impl Default for PackageLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_package_success() {
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        // Create a valid package config
        let config_content = r#"
services:
  test-service:
    process: "echo hello"
"#;
        fs::write(package_path.join("service-federation.yaml"), config_content).unwrap();

        let loader = PackageLoader::new();
        let result = loader.load(package_path);

        assert!(result.is_ok());
        let config = result.unwrap();
        assert!(config.services.contains_key("test-service"));
    }

    #[test]
    fn test_load_package_yml_extension() {
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        // Create a valid package config with .yml extension
        let config_content = r#"
services:
  test-service:
    process: "echo hello"
"#;
        fs::write(package_path.join("service-federation.yml"), config_content).unwrap();

        let loader = PackageLoader::new();
        let result = loader.load(package_path);

        assert!(result.is_ok());
    }

    #[test]
    fn test_load_package_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        let loader = PackageLoader::new();
        let result = loader.load(package_path);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Package(_)));
    }
}
