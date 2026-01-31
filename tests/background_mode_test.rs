use std::fs;
use std::process::Command;
use std::time::Duration;

fn create_test_config(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test-config.yaml");
    fs::write(&config_path, content).expect("Failed to write test config");
    (temp_dir, config_path)
}

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

fn fed_start(config: &std::path::Path, workdir: &str, service: &str) -> std::process::Output {
    Command::new(fed_binary())
        .args([
            "-c",
            config.to_str().unwrap(),
            "-w",
            workdir,
            "start",
            service,
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed start")
}

fn fed_start_all(config: &std::path::Path, workdir: &str) -> std::process::Output {
    Command::new(fed_binary())
        .args(["-c", config.to_str().unwrap(), "-w", workdir, "start"])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed start")
}

fn fed_stop(config: &std::path::Path, workdir: &str) {
    Command::new(fed_binary())
        .args(["-c", config.to_str().unwrap(), "-w", workdir, "stop"])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .ok();
}

fn fed_status(config: &std::path::Path, workdir: &str) -> String {
    let output = Command::new(fed_binary())
        .args(["-c", config.to_str().unwrap(), "-w", workdir, "status"])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed status");
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn fed_logs(config: &std::path::Path, workdir: &str, service: &str) -> String {
    let output = Command::new(fed_binary())
        .args([
            "-c",
            config.to_str().unwrap(),
            "-w",
            workdir,
            "logs",
            service,
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed logs");
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn assert_start_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "fed start failed (exit {}):\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

const LONG_RUNNING_CONFIG: &str = r#"
services:
  long-running:
    process: |
      echo "Process started"
      sleep 300
"#;

#[test]
fn test_start_and_status() {
    let (temp_dir, config_path) = create_test_config(LONG_RUNNING_CONFIG);
    let workdir = temp_dir.path().to_str().unwrap();

    let output = fed_start(&config_path, workdir, "long-running");
    assert_start_success(&output);

    std::thread::sleep(Duration::from_secs(1));

    let status = fed_status(&config_path, workdir);
    assert!(
        status.contains("long-running"),
        "Status should list the service. Got:\n{}",
        status
    );

    fed_stop(&config_path, workdir);
}

#[test]
fn test_start_and_stop() {
    let (temp_dir, config_path) = create_test_config(LONG_RUNNING_CONFIG);
    let workdir = temp_dir.path().to_str().unwrap();

    let output = fed_start(&config_path, workdir, "long-running");
    assert_start_success(&output);

    std::thread::sleep(Duration::from_secs(1));

    let stop_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "stop",
            "long-running",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed stop");

    assert!(
        stop_output.status.success(),
        "fed stop failed (exit {}):\nstderr: {}",
        stop_output.status,
        String::from_utf8_lossy(&stop_output.stderr),
    );
}

#[test]
fn test_restart() {
    let (temp_dir, config_path) = create_test_config(LONG_RUNNING_CONFIG);
    let workdir = temp_dir.path().to_str().unwrap();

    let output = fed_start(&config_path, workdir, "long-running");
    assert_start_success(&output);

    std::thread::sleep(Duration::from_secs(1));

    let restart_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "restart",
            "long-running",
        ])
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed restart");

    assert!(
        restart_output.status.success(),
        "fed restart failed (exit {}):\nstderr: {}",
        restart_output.status,
        String::from_utf8_lossy(&restart_output.stderr),
    );

    std::thread::sleep(Duration::from_secs(1));

    let status = fed_status(&config_path, workdir);
    assert!(
        status.contains("long-running"),
        "Service should still be listed after restart. Got:\n{}",
        status
    );

    fed_stop(&config_path, workdir);
}

#[test]
fn test_multiple_services() {
    let config = r#"
services:
  svc-a:
    process: |
      echo "Service A"
      sleep 300

  svc-b:
    process: |
      echo "Service B"
      sleep 300
"#;

    let (temp_dir, config_path) = create_test_config(config);
    let workdir = temp_dir.path().to_str().unwrap();

    let output = fed_start_all(&config_path, workdir);
    assert_start_success(&output);

    std::thread::sleep(Duration::from_secs(1));

    let status = fed_status(&config_path, workdir);
    assert!(
        status.contains("svc-a"),
        "Status should list svc-a. Got:\n{}",
        status
    );
    assert!(
        status.contains("svc-b"),
        "Status should list svc-b. Got:\n{}",
        status
    );

    fed_stop(&config_path, workdir);
}

#[test]
fn test_logs_persist_after_exit() {
    let config = r#"
services:
  logger:
    process: |
      echo "log-line-one"
      echo "log-line-two"
      sleep 5
"#;

    let (temp_dir, config_path) = create_test_config(config);
    let workdir = temp_dir.path().to_str().unwrap();

    let output = fed_start(&config_path, workdir, "logger");
    assert_start_success(&output);

    // Wait for the process to produce output and exit
    std::thread::sleep(Duration::from_secs(7));

    let logs = fed_logs(&config_path, workdir, "logger");
    assert!(
        logs.contains("log-line-one"),
        "Logs should contain output from the process. Got:\n{}",
        logs
    );

    fed_stop(&config_path, workdir);
}
