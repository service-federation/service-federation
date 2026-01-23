use super::HealthChecker;
use crate::error::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::sync::OnceLock;
use std::time::Duration;

/// Global shared HTTP client for health checks.
///
/// Using a shared client prevents file descriptor exhaustion when running
/// many services with HTTP health checks. The client maintains a connection
/// pool that is reused across all health checkers.
///
/// The timeout is set high (30s) since individual requests will use their own
/// timeout via `timeout()` on the request itself.
static SHARED_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

/// Get or create the shared HTTP client.
///
/// This is lazy-initialized on first use. The client is configured with:
/// - 30 second timeout (as fallback, individual requests override this)
/// - Connection pooling enabled (default)
/// - Keep-alive enabled (default)
fn get_shared_client() -> &'static Client {
    SHARED_HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to create shared HTTP client")
    })
}

/// HTTP-based health checker
pub struct HttpChecker {
    url: String,
    client: Client,
    timeout: Duration,
}

impl HttpChecker {
    /// Create a new HTTP health checker with its own client.
    ///
    /// Consider using [`with_shared_client`](Self::with_shared_client) instead
    /// to share a single client across all health checkers, which prevents
    /// file descriptor exhaustion with many services.
    ///
    /// # Errors
    ///
    /// Returns error if URL is malformed or uses unsupported scheme.
    pub fn new(url: String, timeout: Duration) -> Result<Self> {
        // Validate URL format at construction time
        Self::validate_url(&url)?;

        let client = Client::builder().timeout(timeout).build().map_err(|e| {
            crate::error::Error::Config(format!("Failed to create HTTP client: {}", e))
        })?;

        Ok(Self {
            url,
            client,
            timeout,
        })
    }

    /// Create a new HTTP health checker using the global shared client.
    ///
    /// This is the preferred constructor for production use as it:
    /// - Prevents file descriptor exhaustion with many services
    /// - Reuses TCP connections where possible
    /// - Reduces memory overhead
    ///
    /// # Errors
    ///
    /// Returns error if URL is malformed or uses unsupported scheme.
    pub fn with_shared_client(url: String, timeout: Duration) -> Result<Self> {
        // Validate URL format at construction time
        Self::validate_url(&url)?;

        Ok(Self {
            url,
            client: get_shared_client().clone(),
            timeout,
        })
    }

    /// Validate that a URL is well-formed and uses HTTP/HTTPS scheme.
    fn validate_url(url: &str) -> Result<()> {
        match url::Url::parse(url) {
            Ok(parsed) => {
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return Err(crate::error::Error::Config(format!(
                        "Invalid healthcheck URL '{}': scheme must be http or https, got '{}'",
                        url, scheme
                    )));
                }
                Ok(())
            }
            Err(e) => Err(crate::error::Error::Config(format!(
                "Invalid healthcheck URL '{}': {}",
                url, e
            ))),
        }
    }
}

#[async_trait]
impl HealthChecker for HttpChecker {
    async fn check(&self) -> Result<bool> {
        // Apply per-request timeout to override the client's default timeout.
        // This is necessary when using the shared client which has a long timeout.
        match self
            .client
            .get(&self.url)
            .timeout(self.timeout)
            .send()
            .await
        {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_checker_unreachable_port() {
        // Use a valid but unlikely-to-be-used port
        let checker = HttpChecker::new(
            "http://localhost:59999/health".to_string(),
            Duration::from_secs(1),
        )
        .expect("Should create HTTP checker");

        let result = checker.check().await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_http_checker_with_shared_client() {
        // Test that the shared client version works
        let checker = HttpChecker::with_shared_client(
            "http://localhost:59998/health".to_string(),
            Duration::from_secs(1),
        )
        .expect("Should create HTTP checker");

        let result = checker.check().await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_shared_client_reuse() {
        // Create multiple checkers - they should all use the same underlying client
        let checker1 = HttpChecker::with_shared_client(
            "http://localhost:59997/health".to_string(),
            Duration::from_secs(1),
        )
        .expect("Should create checker1");
        let checker2 = HttpChecker::with_shared_client(
            "http://localhost:59996/health".to_string(),
            Duration::from_secs(2),
        )
        .expect("Should create checker2");

        // Both should work (fail to connect, but not panic)
        assert!(!checker1.check().await.unwrap());
        assert!(!checker2.check().await.unwrap());

        // Verify different timeouts are preserved
        assert_eq!(checker1.timeout(), Duration::from_secs(1));
        assert_eq!(checker2.timeout(), Duration::from_secs(2));
    }

    #[test]
    fn test_rejects_invalid_url() {
        // Malformed URL should fail
        let result = HttpChecker::new("not-a-url".to_string(), Duration::from_secs(1));
        assert!(result.is_err());

        // Wrong scheme should fail
        let result = HttpChecker::new("ftp://localhost/health".to_string(), Duration::from_secs(1));
        assert!(result.is_err());

        // Valid HTTP URL should work
        let result = HttpChecker::new(
            "http://localhost/health".to_string(),
            Duration::from_secs(1),
        );
        assert!(result.is_ok());

        // Valid HTTPS URL should work
        let result = HttpChecker::new(
            "https://localhost/health".to_string(),
            Duration::from_secs(1),
        );
        assert!(result.is_ok());
    }
}
