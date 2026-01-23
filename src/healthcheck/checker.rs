use crate::error::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;

/// Health checker trait for services
#[async_trait]
pub trait HealthChecker: Send + Sync {
    /// Check if the service is healthy
    async fn check(&self) -> Result<bool>;

    /// Get the timeout duration
    fn timeout(&self) -> Duration;
}

/// Check health with retry logic using exponential backoff
/// Starts with `interval` and doubles each retry up to a maximum of 30 seconds
pub async fn check_with_retry<C: HealthChecker + ?Sized>(
    checker: &C,
    max_retries: usize,
    interval: Duration,
) -> bool {
    let mut current_delay = interval;
    let max_delay = Duration::from_secs(30);

    for attempt in 0..max_retries {
        match checker.check().await {
            Ok(true) => return true,
            Ok(false) | Err(_) => {
                // Don't sleep after the last attempt
                if attempt < max_retries - 1 {
                    sleep(current_delay).await;

                    // Exponential backoff: double the delay each time, capped at max_delay
                    current_delay = std::cmp::min(current_delay * 2, max_delay);
                }
            }
        }
    }
    false
}
