use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn create_test_config(
    temp_dir: &tempfile::TempDir,
    filename: &str,
    content: &str,
) -> std::path::PathBuf {
    let config_path = temp_dir.path().join(filename);
    fs::write(&config_path, content).expect("Failed to write test config");
    config_path
}

fn overwrite_config(config_path: &Path, content: &str) {
    fs::write(config_path, content).expect("Failed to overwrite test config");
}

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

fn run_fed(args: &[&str]) -> std::process::Output {
    Command::new(fed_binary())
        .args(args)
        .env("FED_NON_INTERACTIVE", "1")
        .output()
        .expect("Failed to run fed")
}

fn get_service_pid(workdir: &Path, service: &str) -> Option<u32> {
    let db_path = workdir.join(".fed/lock.db");
    let conn = rusqlite::Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT pid FROM services WHERE id = ?1",
        rusqlite::params![service],
        |row| row.get::<_, Option<u32>>(0),
    )
    .ok()
    .flatten()
}

fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !is_pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !is_pid_alive(pid)
}

const ONE_SERVICE_CONFIG: &str = r#"
services:
  alpha:
    process: |
      echo "alpha started"
      sleep 300
"#;

const DIFFERENT_VALID_CONFIG: &str = r#"
services:
  beta:
    process: echo "beta"
"#;

const TWO_SERVICE_CONFIG: &str = r#"
services:
  alpha:
    process: |
      echo "alpha started"
      sleep 300
  beta:
    process: |
      echo "beta started"
      sleep 300
"#;

const INVALID_CONFIG: &str = r#"
services:
  alpha:
    process: [this is not valid yaml
"#;

#[test]
fn test_stop_stops_services_not_in_current_config() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let workdir = temp_dir.path();
    let config_path = create_test_config(&temp_dir, "config.yaml", ONE_SERVICE_CONFIG);

    let start = run_fed(&[
        "-c",
        config_path.to_str().unwrap(),
        "-w",
        workdir.to_str().unwrap(),
        "start",
        "alpha",
    ]);
    assert!(
        start.status.success(),
        "fed start failed: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr),
    );

    std::thread::sleep(Duration::from_secs(1));
    let pid = get_service_pid(workdir, "alpha").expect("Expected alpha pid in lock db");
    assert!(is_pid_alive(pid), "Expected alpha PID {} alive", pid);

    // Simulate config change: alpha removed/renamed, but still running in state
    overwrite_config(&config_path, DIFFERENT_VALID_CONFIG);

    let stop = run_fed(&[
        "-c",
        config_path.to_str().unwrap(),
        "-w",
        workdir.to_str().unwrap(),
        "stop",
    ]);
    assert!(
        stop.status.success(),
        "fed stop failed: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stop.stdout),
        String::from_utf8_lossy(&stop.stderr),
    );

    assert!(
        wait_for_pid_exit(pid, Duration::from_secs(8)),
        "Expected alpha PID {} to exit after stop",
        pid
    );
}

#[test]
fn test_stop_from_state_does_not_clear_unstopped_services() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let workdir = temp_dir.path();
    let config_path = create_test_config(&temp_dir, "config.yaml", TWO_SERVICE_CONFIG);

    let start = run_fed(&[
        "-c",
        config_path.to_str().unwrap(),
        "-w",
        workdir.to_str().unwrap(),
        "start",
        "alpha",
        "beta",
    ]);
    assert!(
        start.status.success(),
        "fed start failed: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr),
    );

    std::thread::sleep(Duration::from_secs(1));
    let alpha_pid = get_service_pid(workdir, "alpha").expect("Expected alpha pid");
    let beta_pid = get_service_pid(workdir, "beta").expect("Expected beta pid");
    assert!(is_pid_alive(alpha_pid), "Expected alpha PID alive");
    assert!(is_pid_alive(beta_pid), "Expected beta PID alive");

    // Make config invalid so `fed stop` falls back to state-tracker-only stop.
    overwrite_config(&config_path, INVALID_CONFIG);

    let stop_alpha = run_fed(&[
        "-c",
        config_path.to_str().unwrap(),
        "-w",
        workdir.to_str().unwrap(),
        "stop",
        "alpha",
    ]);
    assert!(
        stop_alpha.status.success(),
        "fed stop alpha failed: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stop_alpha.stdout),
        String::from_utf8_lossy(&stop_alpha.stderr),
    );

    assert!(
        wait_for_pid_exit(alpha_pid, Duration::from_secs(8)),
        "Expected alpha PID {} to exit after stop alpha",
        alpha_pid
    );
    assert!(is_pid_alive(beta_pid), "Expected beta to remain running");

    // Stopping all (still with invalid config) should stop beta too.
    let stop_all = run_fed(&[
        "-c",
        config_path.to_str().unwrap(),
        "-w",
        workdir.to_str().unwrap(),
        "stop",
    ]);
    assert!(
        stop_all.status.success(),
        "fed stop all failed: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stop_all.stdout),
        String::from_utf8_lossy(&stop_all.stderr),
    );

    assert!(
        wait_for_pid_exit(beta_pid, Duration::from_secs(8)),
        "Expected beta PID {} to exit after stop all",
        beta_pid
    );
}
