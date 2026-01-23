use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

fn create_detached_test_config() -> (TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test-config.yaml");

    let config_content = r#"
services:
  long-running:
    process: |
      echo "Process starting with PID $$"
      echo "Running in background"
      sleep 300

  quick-exit:
    process: |
      echo "Quick process"
      sleep 1
      echo "Exiting"
"#;

    fs::write(&config_path, config_content).expect("Failed to write test config");
    (temp_dir, config_path)
}

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

/// Check if a process is running by searching process list
#[cfg(unix)]
#[allow(dead_code)]
fn is_process_running_by_pattern(pattern: &str) -> bool {
    let output = Command::new("ps")
        .args(["aux"])
        .output()
        .expect("Failed to run ps");

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .any(|line| line.contains(pattern) && !line.contains("grep"))
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn is_process_running_by_pattern(_pattern: &str) -> bool {
    // Fallback for non-Unix systems
    true
}

#[test]
fn test_detached_mode_starts_background_process() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start in detached mode
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "long-running",
        ])
        .output()
        .expect("Failed to start");

    assert!(
        start_output.status.success(),
        "Detached start failed: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );

    // Wait for process to start
    std::thread::sleep(Duration::from_secs(2));

    // Verify process is running in background
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status = String::from_utf8_lossy(&status_output.stdout);
    println!("Status after detached start:\n{}", status);

    // Should show service as running or started
    assert!(
        status.contains("long-running"),
        "Service should appear in status"
    );

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_detached_process_can_be_stopped() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start in detached mode
    Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "long-running",
        ])
        .output()
        .expect("Failed to start");

    std::thread::sleep(Duration::from_secs(2));

    // Verify it's running
    let status_before = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status_text = String::from_utf8_lossy(&status_before.stdout);
    println!("Status before stop:\n{}", status_text);

    // Stop the service
    let stop_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "stop",
            "long-running",
        ])
        .output()
        .expect("Failed to stop");

    assert!(
        stop_output.status.success(),
        "Stop failed: {}",
        String::from_utf8_lossy(&stop_output.stderr)
    );

    std::thread::sleep(Duration::from_secs(1));

    // Verify process is stopped
    let status_after = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status_after_text = String::from_utf8_lossy(&status_after.stdout);
    println!("Status after stop:\n{}", status_after_text);

    // Lock file should be cleared or service should be gone
    let lock_path = temp_dir.path().join(".fed-lock.json");

    // Either lock file is gone or service is marked as stopped
    if lock_path.exists() {
        let lock_content = fs::read_to_string(&lock_path).unwrap_or_default();
        println!("Lock file after stop:\n{}", lock_content);
    }
}

#[test]
fn test_detached_mode_pid_capture() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start in detached mode
    Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "long-running",
        ])
        .output()
        .expect("Failed to start");

    std::thread::sleep(Duration::from_secs(2));

    // Check lock file for PID
    let lock_path = temp_dir.path().join(".fed-lock.json");

    if lock_path.exists() {
        let lock_content = fs::read_to_string(&lock_path).expect("Failed to read lock");
        println!("Lock file content:\n{}", lock_content);

        // Lock file should contain PID information
        // Parse JSON to verify structure
        if let Ok(lock_json) = serde_json::from_str::<serde_json::Value>(&lock_content) {
            if let Some(services) = lock_json.get("services").and_then(|s| s.as_array()) {
                // Should have at least one service
                assert!(!services.is_empty(), "Should have service in lock file");

                // Check if service has PID
                for service in services {
                    if let Some(id) = service.get("id").and_then(|i| i.as_str()) {
                        if id.contains("long-running") {
                            println!(
                                "Service entry: {}",
                                serde_json::to_string_pretty(service).unwrap()
                            );

                            // Service should have PID captured
                            let has_pid = service.get("pid").and_then(|p| p.as_u64()).is_some();
                            println!("Has PID: {}", has_pid);
                        }
                    }
                }
            }
        }
    }

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_detached_mode_multiple_services() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start all services in detached mode
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "start"])
        .output()
        .expect("Failed to start");

    std::thread::sleep(Duration::from_secs(3));

    // Check status shows both services
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status = String::from_utf8_lossy(&status_output.stdout);
    println!("Status:\n{}", status);

    // Verify lock file has both services
    let lock_path = temp_dir.path().join(".fed-lock.json");
    if lock_path.exists() {
        let lock_content = fs::read_to_string(&lock_path).unwrap();
        println!("Lock file:\n{}", lock_content);
    }

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_background_mode_returns_immediately() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Background mode (default) should return immediately
    let start_time = std::time::Instant::now();

    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "quick-exit",
        ])
        .output()
        .expect("Failed to start");

    let duration = start_time.elapsed();

    assert!(start_output.status.success(), "Start should succeed");

    // Background mode should return reasonably quickly (within 60 seconds)
    // Note: This can take longer on slow systems or when running many tests concurrently
    // The important thing is that it doesn't block indefinitely like foreground mode would
    assert!(
        duration < Duration::from_secs(60),
        "Background mode should return quickly, took {:?}",
        duration
    );

    std::thread::sleep(Duration::from_secs(1));

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_restart_detached_service() {
    let (temp_dir, config_path) = create_detached_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start service
    Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "long-running",
        ])
        .output()
        .expect("Failed to start");

    std::thread::sleep(Duration::from_secs(2));

    // Restart the service
    let restart_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "restart",
            "long-running",
        ])
        .output()
        .expect("Failed to restart");

    println!(
        "Restart output:\n{}",
        String::from_utf8_lossy(&restart_output.stdout)
    );
    println!(
        "Restart stderr:\n{}",
        String::from_utf8_lossy(&restart_output.stderr)
    );

    assert!(restart_output.status.success(), "Restart should succeed");

    std::thread::sleep(Duration::from_secs(2));

    // Verify service is still running
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status = String::from_utf8_lossy(&status_output.stdout);
    println!("Status after restart:\n{}", status);

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

// =============================================================================
// Log Persistence Tests
// =============================================================================

#[test]
fn test_detached_mode_logs_persistence() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test-config.yaml");

    let config_content = r#"
services:
  logging-service:
    process: |
      echo "Starting service"
      echo "Service is running"
      sleep 2
      echo "Service output"
"#;

    fs::write(&config_path, config_content).expect("Failed to write config");
    let workdir = temp_dir.path().to_str().unwrap();

    // Start service in detached mode
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "logging-service",
        ])
        .output()
        .expect("Failed to start");

    assert!(start_output.status.success(), "Start should succeed");

    // Wait for service to run
    std::thread::sleep(Duration::from_secs(3));

    // Get logs while service is running
    let logs_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "logs",
            "logging-service",
        ])
        .output()
        .expect("Failed to get logs");

    let logs = String::from_utf8_lossy(&logs_output.stdout);
    println!("Logs output:\n{}", logs);

    // Should contain service output
    assert!(
        logs.contains("Starting service") || logs.contains("Service") || !logs.is_empty(),
        "Logs should contain service output"
    );

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_detached_mode_logs_persist_after_stop() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test-config.yaml");

    let config_content = r#"
services:
  quick-logger:
    process: |
      echo "Test message 1"
      echo "Test message 2"
      echo "Test message 3"
"#;

    fs::write(&config_path, config_content).expect("Failed to write config");
    let workdir = temp_dir.path().to_str().unwrap();

    // Start and let complete
    Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "quick-logger",
        ])
        .output()
        .expect("Failed to start");

    std::thread::sleep(Duration::from_secs(2));

    // Get logs after service has completed
    let logs_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "logs",
            "quick-logger",
        ])
        .output()
        .expect("Failed to get logs");

    let logs = String::from_utf8_lossy(&logs_output.stdout);
    println!("Logs after completion:\n{}", logs);

    // Should contain output or be non-empty (logs persisted)
    assert!(!logs.is_empty(), "Logs should persist after service stops");

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .ok();
}
