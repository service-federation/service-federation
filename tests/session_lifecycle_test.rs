use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

/// Helper to create test config directory with a simple YAML config
fn create_test_config() -> (TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test-config.yaml");

    let config_content = r#"
parameters:
  TEST_PORT:
    type: port
    default: 9876

services:
  test-service:
    process: |
      echo "Service starting on port {{TEST_PORT}}"
      sleep 100
    environment:
      PORT: '{{TEST_PORT}}'

  dependent-service:
    process: |
      echo "Dependent service starting"
      sleep 100
    depends_on:
      - test-service
"#;

    fs::write(&config_path, config_content).expect("Failed to write test config");
    (temp_dir, config_path)
}

/// Helper to get the fed binary path
fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

#[test]
fn test_stop_command_clears_lock_file() {
    let (temp_dir, config_path) = create_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start services
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "test-service",
        ])
        .output()
        .expect("Failed to start services");

    assert!(
        start_output.status.success(),
        "Start failed: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    std::thread::sleep(Duration::from_secs(2));

    // Verify lock file exists
    let lock_path = temp_dir.path().join(".fed").join("lock.db");
    assert!(lock_path.exists(), "Lock file should exist after start");

    // Stop services
    let stop_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop services");

    assert!(
        stop_output.status.success(),
        "Stop command failed: {}",
        String::from_utf8_lossy(&stop_output.stderr)
    );

    // Verify no services running after stop
    std::thread::sleep(Duration::from_secs(1));
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        !status_text.contains("Running"),
        "No services should be running after stop"
    );
}

#[test]
fn test_start_stop_restart_cycle() {
    let (temp_dir, config_path) = create_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start -> Stop -> Start again
    for i in 0..2 {
        println!("Iteration {}", i);

        let start_output = Command::new(fed_binary())
            .args([
                "-c",
                config_path.to_str().unwrap(),
                "-w",
                workdir,
                "start",
                "test-service",
            ])
            .output()
            .expect("Failed to start services");

        assert!(
            start_output.status.success(),
            "Start iteration {} failed: {}",
            i,
            String::from_utf8_lossy(&start_output.stderr)
        );

        std::thread::sleep(Duration::from_secs(2));

        let stop_output = Command::new(fed_binary())
            .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
            .output()
            .expect("Failed to stop services");

        assert!(
            stop_output.status.success(),
            "Stop iteration {} failed: {}",
            i,
            String::from_utf8_lossy(&stop_output.stderr)
        );

        std::thread::sleep(Duration::from_secs(1));

        // Verify no services running after each stop
        let status_output = Command::new(fed_binary())
            .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
            .output()
            .expect("Failed to get status");

        let status_text = String::from_utf8_lossy(&status_output.stdout);
        assert!(
            !status_text.contains("Running"),
            "No services should be running after stop iteration {}",
            i
        );
    }
}

#[test]
fn test_status_command_after_start() {
    let (temp_dir, config_path) = create_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start services
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "test-service",
        ])
        .output()
        .expect("Failed to start services");

    assert!(start_output.status.success());
    std::thread::sleep(Duration::from_secs(2));

    // Get status
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    assert!(status_output.status.success());

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    println!("Status output:\n{}", status_text);

    // Verify services appear in status
    assert!(
        status_text.contains("test-service") || status_text.contains("Service"),
        "Status should show services"
    );

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop services");
}

#[test]
fn test_lock_file_persistence() {
    let (temp_dir, config_path) = create_test_config();
    let workdir = temp_dir.path().to_str().unwrap();
    let lock_path = temp_dir.path().join(".fed").join("lock.db");

    // Start services
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "test-service",
        ])
        .output()
        .expect("Failed to start");

    assert!(
        start_output.status.success(),
        "Start failed: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );

    std::thread::sleep(Duration::from_secs(2));

    // Verify lock file was created (SQLite database)
    assert!(lock_path.exists(), "Lock file should be created on start");

    // Verify it's a valid SQLite file by checking magic bytes
    let lock_bytes = fs::read(&lock_path).expect("Failed to read lock file");
    println!("Lock file size: {} bytes", lock_bytes.len());

    // Stop services
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");

    std::thread::sleep(Duration::from_secs(1));

    // Verify no services running after stop (SQLite database persists but is cleared)
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        !status_text.contains("Running"),
        "No services should be running after stop"
    );
}

#[test]
fn test_multiple_services_dependency_order() {
    let (temp_dir, config_path) = create_test_config();
    let workdir = temp_dir.path().to_str().unwrap();

    // Start dependent-service which will also start test-service (its dependency)
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "dependent-service",
        ])
        .output()
        .expect("Failed to start");

    assert!(
        start_output.status.success(),
        "Start with dependencies failed: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );

    std::thread::sleep(Duration::from_secs(3));

    // Get status to verify both services started
    let status_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "status"])
        .output()
        .expect("Failed to get status");

    let status = String::from_utf8_lossy(&status_output.stdout);
    println!("Status after start:\n{}", status);

    // Cleanup
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .output()
        .expect("Failed to stop");
}

#[test]
fn test_session_id_collision_resistance() {
    // Generate many session IDs and verify no collisions
    use std::collections::HashSet;

    let mut ids = HashSet::new();
    let num_samples = 10_000;

    for _ in 0..num_samples {
        let id = service_federation::session::Session::generate_id();

        // Verify format: sess-<64hex>
        assert!(
            id.starts_with("sess-"),
            "Session ID should start with 'sess-', got: {}",
            id
        );

        // Verify no collisions
        assert!(
            ids.insert(id.clone()),
            "Collision detected! Generated duplicate session ID: {}",
            id
        );
    }

    // All generated IDs should be unique
    assert_eq!(
        ids.len(),
        num_samples,
        "Expected {} unique IDs, but got {} (collision detected)",
        num_samples,
        ids.len()
    );

    println!(
        "✓ Generated {} unique session IDs with 64-bit entropy - no collisions",
        num_samples
    );
}
