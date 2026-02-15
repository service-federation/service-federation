use super::HealthChecker;
use crate::docker::DockerClient;
use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Command-based health checker
pub struct CommandChecker {
    command: String,
    args: Vec<String>,
    timeout: Duration,
    environment: HashMap<String, String>,
}

impl CommandChecker {
    pub fn new(command: String, args: Vec<String>, timeout: Duration) -> Self {
        Self {
            command,
            args,
            timeout,
            environment: HashMap::new(),
        }
    }

    pub fn with_environment(
        command: String,
        args: Vec<String>,
        timeout: Duration,
        environment: HashMap<String, String>,
    ) -> Self {
        Self {
            command,
            args,
            timeout,
            environment,
        }
    }
}

#[async_trait]
impl HealthChecker for CommandChecker {
    async fn check(&self) -> Result<bool> {
        let result = tokio::time::timeout(
            self.timeout,
            Command::new(&self.command)
                .args(&self.args)
                .envs(&self.environment)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        )
        .await;

        match result {
            Ok(Ok(status)) => Ok(status.success()),
            Ok(Err(_)) | Err(_) => Ok(false),
        }
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// Docker command-based health checker - runs commands inside a Docker container
pub struct DockerCommandChecker {
    container_name: String,
    command: String,
    timeout: Duration,
}

impl DockerCommandChecker {
    pub fn new(container_name: String, command: String, timeout: Duration) -> Self {
        Self {
            container_name,
            command,
            timeout,
        }
    }
}

#[async_trait]
impl HealthChecker for DockerCommandChecker {
    async fn check(&self) -> Result<bool> {
        match DockerClient::new()
            .exec_sh(&self.container_name, &self.command, self.timeout)
            .await
        {
            Ok(output) => Ok(output.status.success()),
            Err(_) => Ok(false),
        }
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }
}
