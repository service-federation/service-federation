use super::{ServiceManager, Status};
use crate::config::{Dependency, Service as ServiceConfig};
use crate::dependency::GitOperations;
use crate::error::{Error, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;

/// External service from git dependency
pub struct ExternalService {
    name: String,
    config: ServiceConfig,
    dependencies: HashMap<String, Dependency>,
    dependency_path: Option<PathBuf>,
    status: Status,
}

impl ExternalService {
    pub fn new(
        name: String,
        config: ServiceConfig,
        _environment: HashMap<String, String>,
        _work_dir: String,
    ) -> Self {
        Self {
            name,
            config,
            dependencies: HashMap::new(),
            dependency_path: None,
            status: Status::Stopped,
        }
    }

    pub fn set_dependencies(&mut self, dependencies: HashMap<String, Dependency>) {
        self.dependencies = dependencies;
    }

    async fn resolve_dependency(&mut self) -> Result<PathBuf> {
        let dep_name = self
            .config
            .dependency
            .as_ref()
            .ok_or_else(|| Error::Config("No dependency specified".to_string()))?;

        let dependency = self
            .dependencies
            .get(dep_name)
            .ok_or_else(|| Error::Config(format!("Dependency '{}' not found", dep_name)))?;

        // Parse repo URL
        let target_path = if dependency.repo.starts_with("file://") {
            PathBuf::from(dependency.repo.trim_start_matches("file://"))
        } else {
            // For remote repos, clone to .service-federation directory
            let cache_dir = PathBuf::from(".service-federation/dependencies");
            std::fs::create_dir_all(&cache_dir)?;

            let repo_name = dependency
                .repo
                .split('/')
                .next_back()
                .unwrap_or(dep_name)
                .trim_end_matches(".git");

            let target = cache_dir.join(repo_name);

            GitOperations::clone_or_update(
                &dependency.repo,
                dependency.branch.as_deref(),
                &target,
            )?;

            target
        };

        Ok(target_path)
    }
}

#[async_trait]
impl ServiceManager for ExternalService {
    async fn start(&mut self) -> Result<()> {
        self.status = Status::Starting;

        // Resolve the external dependency
        let path = self.resolve_dependency().await?;
        self.dependency_path = Some(path);

        // External services are managed externally
        // This is a placeholder for orchestrating the external service
        self.status = Status::Running;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.status = Status::Stopping;
        // External services are stopped by the orchestrator
        self.status = Status::Stopped;
        Ok(())
    }

    async fn kill(&mut self) -> Result<()> {
        self.stop().await
    }

    async fn health(&self) -> Result<bool> {
        Ok(self.status == Status::Running || self.status == Status::Healthy)
    }

    fn status(&self) -> Status {
        self.status
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
