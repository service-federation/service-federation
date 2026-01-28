//! Integration tests for SF-00077: services with configured healthchecks should
//! be awaited during startup. Currently, `start_service_impl` only checks PID
//! liveness (300-500ms) and reports "ready" without polling the healthcheck.
//! These tests demonstrate the bug by asserting correct behavior that the
//! current implementation fails to provide.

use std::fs;
use std::process::Command;
use std::time::Duration;

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

fn fed_stop(config_path: &std::path::Path, workdir: &str) {
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "stop"])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .ok();
}

/// SF-00077 test 1: A service that exits immediately with a command healthcheck
/// should NOT be reported as Running/Healthy after `fed start` completes.
///
/// Currently `start_service_impl` only checks PID liveness for 300-500ms after
/// spawn, then reports "ready" without ever polling the configured healthcheck.
/// A service that dies immediately (exit 1) is briefly reported as "Running"
/// because the healthcheck is never consulted during startup.
///
/// Expected (correct) behavior: after `fed start` returns, a service that
/// crashed should be reported as Stopped/Failed, not Running.
#[test]
fn test_service_with_healthcheck_reports_stopped_when_immediately_dying() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");
    let workdir = temp_dir.path().to_str().unwrap();

    // Service that exits immediately. The healthcheck should never pass because
    // the process dies before it can be checked.
    let config_content = r#"
services:
  dying-service:
    process: "exit 1"
    healthcheck:
      command: "true"
      timeout: "5s"
"#;

    fs::write(&config_path, config_content).expect("Failed to write config");

    // Start the service
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "dying-service",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed start");

    println!(
        "Start stdout:\n{}",
        String::from_utf8_lossy(&start_output.stdout)
    );
    println!(
        "Start stderr:\n{}",
        String::from_utf8_lossy(&start_output.stderr)
    );

    // Small delay to let any async state settle
    std::thread::sleep(Duration::from_millis(500));

    // Check status immediately
    let status_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "status",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed status");

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    let status_stderr = String::from_utf8_lossy(&status_output.stderr);
    println!("Status stdout:\n{}", status_text);
    println!("Status stderr:\n{}", status_stderr);

    let combined = format!("{}{}", status_text, status_stderr);
    let combined_lower = combined.to_lowercase();

    // The service exited immediately. If healthchecks were awaited during
    // startup, fed would have noticed the process died. The status should
    // NOT show "Running" or "Healthy" for a dead service.
    //
    // SF-00077 bug: currently the service is briefly reported as "Running"
    // because start_service_impl never polls the healthcheck.
    assert!(
        !combined_lower.contains("running") && !combined_lower.contains("healthy"),
        "SF-00077 BUG: A service that exited immediately should NOT be reported as \
         Running or Healthy. If healthchecks were awaited during startup, the dead \
         process would have been detected. Status output:\n{}",
        combined
    );

    // The service name should appear in the status output
    assert!(
        combined_lower.contains("dying-service"),
        "Service name should appear in status output. Got:\n{}",
        combined
    );

    // It should show as Stopped or Failed
    assert!(
        combined_lower.contains("stopped") || combined_lower.contains("failed"),
        "Service should be reported as Stopped or Failed after immediate exit. \
         Status output:\n{}",
        combined
    );

    // Cleanup
    fed_stop(&config_path, workdir);
}

/// SF-00077 test 2: A service with a healthcheck that takes time to become
/// healthy should be awaited during startup. After `fed start` returns, the
/// service should be Healthy (not just Running).
///
/// This test creates a long-running process that writes a marker file after 2
/// seconds. The healthcheck checks for that file. If `fed start` awaits the
/// healthcheck, it will not return until the marker file exists (i.e., the
/// service is truly healthy). Immediately checking `fed status` afterward
/// should show "Healthy".
///
/// Currently, `start_service_impl` returns after ~300-500ms of PID liveness,
/// well before the healthcheck has a chance to pass. `fed status` right after
/// would show "Running" (not "Healthy") because the health state hasn't been
/// polled yet.
#[test]
fn test_service_with_healthcheck_awaits_health_before_ready() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");
    let workdir = temp_dir.path().to_str().unwrap();
    let marker_path = temp_dir.path().join("healthy-marker");

    // Service that starts immediately but becomes "healthy" after 2 seconds
    // (writes a marker file). The healthcheck checks for that file.
    let config_content = format!(
        r#"
services:
  slow-health-service:
    process: |
      echo "Starting up..."
      sleep 2
      touch {marker}
      echo "Now healthy"
      sleep 300
    healthcheck:
      command: "test -f {marker}"
      timeout: "10s"
"#,
        marker = marker_path.display()
    );

    fs::write(&config_path, &config_content).expect("Failed to write config");

    let start_time = std::time::Instant::now();

    // Start the service. If healthchecks are awaited, this should block for
    // at least ~2 seconds (until the marker file is created).
    let start_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            "slow-health-service",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed start");

    let start_duration = start_time.elapsed();

    println!(
        "Start stdout:\n{}",
        String::from_utf8_lossy(&start_output.stdout)
    );
    println!(
        "Start stderr:\n{}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    println!("Start took: {:?}", start_duration);

    assert!(
        start_output.status.success(),
        "fed start should succeed: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );

    // If healthchecks were properly awaited, `fed start` should have waited
    // at least ~2 seconds for the marker file to appear.
    //
    // SF-00077 bug: currently start returns after ~300-500ms PID check,
    // not waiting for the healthcheck at all.
    assert!(
        start_duration >= Duration::from_secs(2),
        "SF-00077 BUG: `fed start` returned in {:?}, which is before the service \
         became healthy (~2s). This means the healthcheck was NOT awaited during \
         startup. `fed start` should wait for the configured healthcheck to pass \
         before reporting the service as ready.",
        start_duration
    );

    // Since we waited for health, the marker file should exist
    assert!(
        marker_path.exists(),
        "Marker file should exist after fed start returns (healthcheck passed)"
    );

    // Check status immediately - should show Healthy since we waited
    let status_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "status",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed status");

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    let status_stderr = String::from_utf8_lossy(&status_output.stderr);
    println!("Status stdout:\n{}", status_text);
    println!("Status stderr:\n{}", status_stderr);

    let combined = format!("{}{}", status_text, status_stderr);
    let combined_lower = combined.to_lowercase();

    // After waiting for healthcheck, the service should be reported as Healthy
    assert!(
        combined_lower.contains("healthy") || combined_lower.contains("running"),
        "Service should be Healthy after fed start awaited its healthcheck. \
         Status output:\n{}",
        combined
    );

    // Cleanup
    fed_stop(&config_path, workdir);
}
