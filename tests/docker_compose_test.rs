use service_federation::{Orchestrator, Parser};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::sleep;

// Global mutex to serialize compose tests
// Required because FED_SESSION is a process-wide environment variable
// and tests running in parallel would interfere with each other
static COMPOSE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Helper to set up test session and return cleanup guard
/// This ensures each test gets an isolated compose project
/// Also acquires the global test lock to prevent parallel test interference
fn setup_test_session(session_id: &str) -> impl Drop {
    // Acquire lock to serialize compose tests (FED_SESSION is process-wide)
    let lock = COMPOSE_TEST_LOCK.lock().unwrap();
    std::env::set_var("FED_SESSION", session_id);

    // Return a guard that cleans up on drop
    struct SessionGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
    impl Drop for SessionGuard {
        fn drop(&mut self) {
            std::env::remove_var("FED_SESSION");
            // MutexGuard (self.0) will be dropped here, releasing the lock
        }
    }
    SessionGuard(lock)
}

/// Helper to clean up compose projects after tests
async fn cleanup_compose_project(session_id: &str) {
    let project_name = format!("fed-{}", session_id);

    // Try both docker compose v2 and docker-compose v1
    // Add timeout and force stop
    let _ = tokio::process::Command::new("docker")
        .args([
            "compose",
            "-p",
            &project_name,
            "down",
            "-v",
            "--remove-orphans",
            "-t",
            "1",
        ])
        .output()
        .await;

    let _ = tokio::process::Command::new("docker-compose")
        .args([
            "-p",
            &project_name,
            "down",
            "-v",
            "--remove-orphans",
            "-t",
            "1",
        ])
        .output()
        .await;

    // Give docker time to cleanup
    sleep(Duration::from_millis(500)).await;
}

/// Helper to check if docker is available
async fn docker_available() -> bool {
    tokio::process::Command::new("docker")
        .args(["version"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Helper to check if docker compose is available
async fn docker_compose_available() -> bool {
    // Try v2
    let v2 = tokio::process::Command::new("docker")
        .args(["compose", "version"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if v2 {
        return true;
    }

    // Try v1
    tokio::process::Command::new("docker-compose")
        .args(["--version"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_file_not_found() {
    let yaml = r#"
services:
  test:
    composeFile: ./nonexistent.yml
    composeService: nginx
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    let result = orchestrator.initialize().await;

    // Should fail because compose file doesn't exist
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("does not exist") || err_msg.contains("Compose file"));
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_service_not_found() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let yaml = r#"
services:
  test:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: nonexistent_service
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    let result = orchestrator.start("test").await;

    // Should fail because service doesn't exist in compose file
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found") || err_msg.contains("no such service"),
        "Expected 'not found' error, got: {}",
        err_msg
    );
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_basic_lifecycle() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-lifecycle";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    let yaml = r#"
services:
  nginx:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: nginx
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    // Start service
    orchestrator.start("nginx").await.expect("Start failed");

    // Give it time to start
    sleep(Duration::from_secs(2)).await;

    // Check status - should be running
    let status = orchestrator
        .get_service("nginx")
        .await
        .expect("Get status failed");
    assert!(
        status.to_string().contains("running") || status.to_string().contains("healthy"),
        "Service should be running, got: {}",
        status
    );

    // Stop service
    orchestrator.stop("nginx").await.expect("Stop failed");

    // Check status - should be stopped
    let status = orchestrator
        .get_service("nginx")
        .await
        .expect("Get status failed");
    assert_eq!(status.to_string(), "stopped");

    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_environment_override() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-env";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    let yaml = r#"
parameters:
  TEST_VALUE:
    default: overridden_value

services:
  busybox:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: busybox
    environment:
      TEST_VAR: '{{TEST_VALUE}}'
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    orchestrator.start("busybox").await.expect("Start failed");
    sleep(Duration::from_secs(2)).await;

    // Get logs to verify environment override
    let logs = orchestrator
        .get_logs("busybox", Some(50))
        .await
        .expect("Get logs failed");
    let logs_text = logs.join("\n");

    // Should contain overridden value, not default
    assert!(
        logs_text.contains("overridden_value"),
        "Logs should contain overridden value, got: {}",
        logs_text
    );

    orchestrator.stop("busybox").await.expect("Stop failed");
    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_with_process_dependency() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-mixed";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    let yaml = r#"
services:
  redis:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: redis

  app:
    process: |
      echo "Connecting to redis..."
      sleep 2
      echo "App running"
      sleep 30
    depends_on:
      - redis
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    // Start app - should start redis first
    orchestrator.start("app").await.expect("Start failed");
    sleep(Duration::from_secs(3)).await;

    // Both should be running
    let redis_status = orchestrator
        .get_service("redis")
        .await
        .expect("Get redis status failed");
    let app_status = orchestrator
        .get_service("app")
        .await
        .expect("Get app status failed");

    assert!(
        redis_status.to_string().contains("running")
            || redis_status.to_string().contains("healthy"),
        "Redis should be running"
    );
    assert!(
        app_status.to_string().contains("running") || app_status.to_string().contains("healthy"),
        "App should be running"
    );

    orchestrator.stop_all().await.expect("Stop all failed");
    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
#[allow(clippy::await_holding_lock)] // Intentional: serialize compose tests
async fn test_compose_project_isolation() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    // Acquire lock to prevent interference from parallel tests
    let _lock = COMPOSE_TEST_LOCK.lock().unwrap();

    // Create two separate compose files in temp locations
    let temp_dir1 = tempfile::tempdir().unwrap();
    let temp_dir2 = tempfile::tempdir().unwrap();

    let compose1_path = temp_dir1.path().join("docker-compose.yml");
    let compose2_path = temp_dir2.path().join("docker-compose.yml");

    let compose_content = r#"
version: '3.8'
services:
  test:
    image: nginx:alpine
    ports:
      - "0:80"
"#;

    std::fs::write(&compose1_path, compose_content).unwrap();
    std::fs::write(&compose2_path, compose_content).unwrap();

    let yaml1 = format!(
        r#"
services:
  nginx1:
    composeFile: {}
    composeService: test
"#,
        compose1_path.display()
    );

    let yaml2 = format!(
        r#"
services:
  nginx2:
    composeFile: {}
    composeService: test
"#,
        compose2_path.display()
    );

    let parser = Parser::new();

    // Use different sessions for each orchestrator to test isolation
    std::env::set_var("FED_SESSION", "test-isolation-1");
    let config1 = parser.parse_config(&yaml1).expect("Failed to parse 1");
    let orch1_temp = tempfile::tempdir().unwrap();
    let mut orch1 = Orchestrator::new(config1, orch1_temp.path().to_path_buf())
        .await
        .unwrap();
    orch1.set_auto_resolve_conflicts(true);
    orch1.initialize().await.expect("Init 1 failed");
    orch1.start("nginx1").await.expect("Start 1 failed");

    // Switch to second session for second orchestrator
    std::env::set_var("FED_SESSION", "test-isolation-2");
    let config2 = parser.parse_config(&yaml2).expect("Failed to parse 2");
    let orch2_temp = tempfile::tempdir().unwrap();
    let mut orch2 = Orchestrator::new(config2, orch2_temp.path().to_path_buf())
        .await
        .unwrap();
    orch2.set_auto_resolve_conflicts(true);
    orch2.initialize().await.expect("Init 2 failed");
    orch2.start("nginx2").await.expect("Start 2 failed");

    sleep(Duration::from_secs(2)).await;

    // Both should be running independently
    let status1 = orch1
        .get_service("nginx1")
        .await
        .expect("Get status 1 failed");
    let status2 = orch2
        .get_service("nginx2")
        .await
        .expect("Get status 2 failed");

    assert!(status1.to_string().contains("running") || status1.to_string().contains("healthy"));
    assert!(status2.to_string().contains("running") || status2.to_string().contains("healthy"));

    // Cleanup both orchestrators
    orch1.stop_all().await.expect("Stop 1 failed");
    orch2.stop_all().await.expect("Stop 2 failed");

    // Clean up both projects
    cleanup_compose_project("test-isolation-1").await;
    cleanup_compose_project("test-isolation-2").await;

    // Clean up environment
    std::env::remove_var("FED_SESSION");
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_idempotent_start() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-idempotent";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    let yaml = r#"
services:
  nginx:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: nginx
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    // Start once
    orchestrator
        .start("nginx")
        .await
        .expect("First start failed");
    sleep(Duration::from_secs(2)).await;

    // Start again - should be idempotent
    let result = orchestrator.start("nginx").await;
    assert!(result.is_ok(), "Second start should succeed");

    orchestrator.stop("nginx").await.expect("Stop failed");
    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_health_check() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-health";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    let yaml = r#"
services:
  nginx:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: nginx
"#;

    let parser = Parser::new();
    let config = parser.parse_config(yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");

    orchestrator.start("nginx").await.expect("Start failed");

    // Wait for container to be fully up (compose has its own healthcheck)
    sleep(Duration::from_secs(3)).await;

    let status = orchestrator
        .get_service("nginx")
        .await
        .expect("Get status failed");

    // Should be running (compose services use compose's own healthcheck)
    assert!(
        status.to_string().contains("running") || status.to_string().contains("healthy"),
        "Service should be running or healthy, got: {}",
        status
    );

    orchestrator.stop("nginx").await.expect("Stop failed");
    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Run with --ignored flag, requires Docker
async fn test_compose_port_conflict() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let session_id = "test-port";
    let _guard = setup_test_session(session_id);
    cleanup_compose_project(session_id).await;

    // Start a service on port 18080
    let yaml1 = r#"
services:
  nginx1:
    composeFile: tests/fixtures/compose/test-services.yml
    composeService: nginx
"#;

    let parser = Parser::new();
    let config1 = parser.parse_config(yaml1).expect("Failed to parse");

    let orch1_temp = tempfile::tempdir().unwrap();
    let mut orch1 = Orchestrator::new(config1, orch1_temp.path().to_path_buf())
        .await
        .unwrap();
    orch1.set_auto_resolve_conflicts(true);
    orch1.initialize().await.expect("Init failed");
    orch1.start("nginx1").await.expect("Start 1 failed");

    sleep(Duration::from_secs(2)).await;

    // Try to start another service on the same port
    let config2 = parser.parse_config(yaml1).expect("Failed to parse");
    let orch2_temp = tempfile::tempdir().unwrap();
    let mut orch2 = Orchestrator::new(config2, orch2_temp.path().to_path_buf())
        .await
        .unwrap();
    orch2.set_auto_resolve_conflicts(true);
    orch2.initialize().await.expect("Init 2 failed");

    let result = orch2.start("nginx1").await;

    // Should fail due to port conflict (or succeed if idempotent)
    // The behavior depends on docker compose version
    if let Err(e) = result {
        let err_msg = e.to_string();
        assert!(
            err_msg.contains("port") || err_msg.contains("already") || err_msg.contains("conflict"),
            "Expected port conflict error, got: {}",
            err_msg
        );
    }

    orch1.stop_all().await.expect("Stop failed");
    cleanup_compose_project(session_id).await;
}

#[tokio::test]
#[ignore] // Requires Docker, tests cleanup behavior
async fn test_compose_process_cleanup() {
    if !docker_available().await || !docker_compose_available().await {
        eprintln!("Skipping test - Docker or Docker Compose not available");
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let compose_path = temp_dir.path().join("docker-compose.yml");

    // Create a simple compose file with random ports
    let compose_content = r#"
version: '3.8'
services:
  test-nginx:
    image: nginx:alpine
    ports:
      - "0:80"
  test-redis:
    image: redis:7-alpine
    ports:
      - "0:6379"
"#;

    std::fs::write(&compose_path, compose_content).unwrap();

    let yaml = format!(
        r#"
services:
  compose-process:
    cwd: {}
    process: docker-compose up
"#,
        temp_dir.path().display()
    );

    let parser = Parser::new();
    let config = parser.parse_config(&yaml).expect("Failed to parse");

    let orch_temp_dir = tempfile::tempdir().unwrap();
    let mut orchestrator = Orchestrator::new(config, orch_temp_dir.path().to_path_buf())
        .await
        .unwrap();
    orchestrator.set_auto_resolve_conflicts(true);
    orchestrator.initialize().await.expect("Init failed");
    orchestrator
        .start("compose-process")
        .await
        .expect("Start failed");

    // Wait for containers to start
    sleep(Duration::from_secs(5)).await;

    // Verify containers are running
    let ps_output = tokio::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_path.to_str().unwrap(),
            "ps",
            "--format",
            "json",
        ])
        .output()
        .await
        .expect("Failed to run docker compose ps");

    let ps_before = String::from_utf8_lossy(&ps_output.stdout);
    assert!(
        ps_before.contains("running") || ps_before.contains("Up"),
        "Containers should be running"
    );

    // Stop the service
    orchestrator
        .stop("compose-process")
        .await
        .expect("Stop failed");

    // Wait a bit for cleanup
    sleep(Duration::from_secs(3)).await;

    // Verify containers are stopped (should work now with process group handling)
    let ps_output = tokio::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_path.to_str().unwrap(),
            "ps",
            "--format",
            "json",
        ])
        .output()
        .await
        .expect("Failed to run docker compose ps");

    let ps_after = String::from_utf8_lossy(&ps_output.stdout);

    // With the fix, containers should be stopped
    assert!(
        !ps_after.contains("running") && !ps_after.contains("Up"),
        "Containers should be stopped but are still running: {}",
        ps_after
    );

    // Cleanup
    let _ = tokio::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_path.to_str().unwrap(),
            "down",
            "-v",
        ])
        .output()
        .await;
}
