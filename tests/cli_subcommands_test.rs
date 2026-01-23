//! Comprehensive CLI subcommand tests
//!
//! This module provides smoke tests for all CLI subcommands to ensure
//! they parse arguments correctly and produce expected output.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

fn create_test_config(temp_dir: &TempDir) -> String {
    let config_path = temp_dir.path().join("service-federation.yaml");
    let config = r#"
variables:
  TEST_PORT:
    default: "19876"

services:
  test-service:
    process: |
      echo "Test service running"
      sleep 1

  docker-service:
    image: alpine:latest
    command: ["echo", "hello"]

scripts:
  test-script:
    script: echo "Script executed"

install:
  - echo "Installing..."

clean:
  - echo "Cleaning..."
"#;
    fs::write(&config_path, config).expect("Failed to write config");
    config_path.to_str().unwrap().to_string()
}

// ============================================================================
// Help and version tests
// ============================================================================

#[test]
fn test_help_flag() {
    let output = Command::new(fed_binary())
        .arg("--help")
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Service Federation"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
}

#[test]
fn test_help_subcommand() {
    let output = Command::new(fed_binary())
        .arg("help")
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Commands:"));
}

// ============================================================================
// start subcommand
// ============================================================================

#[test]
fn test_start_help() {
    let output = Command::new(fed_binary())
        .args(["start", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Start services"));
}

#[test]
fn test_start_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "start",
            "--dry-run",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "start --dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_start_missing_config() {
    let output = Command::new(fed_binary())
        .args(["-c", "/nonexistent/config.yaml", "start"])
        .output()
        .expect("Failed to run fed");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("No such file") || stderr.contains("error"),
        "Expected file not found error, got: {}",
        stderr
    );
}

// ============================================================================
// stop subcommand
// ============================================================================

#[test]
fn test_stop_help() {
    let output = Command::new(fed_binary())
        .args(["stop", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Stop services"));
}

#[test]
fn test_stop_no_services() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "stop",
        ])
        .output()
        .expect("Failed to run fed");

    // Should succeed even with no running services
    assert!(
        output.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// restart subcommand
// ============================================================================

#[test]
fn test_restart_help() {
    let output = Command::new(fed_binary())
        .args(["restart", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Restart services"));
}

// ============================================================================
// status subcommand
// ============================================================================

#[test]
fn test_status_help() {
    let output = Command::new(fed_binary())
        .args(["status", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("status") || stdout.contains("Show"));
}

#[test]
fn test_status_no_services() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "status",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_status_json_output() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "status",
            "--json",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "status --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be valid JSON (either empty array or object)
    assert!(
        stdout.trim().starts_with('[')
            || stdout.trim().starts_with('{')
            || stdout.trim().is_empty(),
        "Expected JSON output, got: {}",
        stdout
    );
}

// ============================================================================
// logs subcommand
// ============================================================================

#[test]
fn test_logs_help() {
    let output = Command::new(fed_binary())
        .args(["logs", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("logs") || stdout.contains("Show"));
}

// ============================================================================
// run subcommand (scripts)
// ============================================================================

#[test]
fn test_run_help() {
    let output = Command::new(fed_binary())
        .args(["run", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Run") || stdout.contains("script"));
}

#[test]
fn test_run_missing_script() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "nonexistent-script",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("Script") || stderr.contains("error"),
        "Expected script not found error, got: {}",
        stderr
    );
}

#[test]
fn test_run_with_args() {
    let temp_dir = TempDir::new().unwrap();

    // Create a config with a script that echoes its arguments via $@
    let config_content = r#"
scripts:
  echo-args:
    script: 'echo "ARGS:" "$@"'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-args",
            "--",
            "hello",
            "world",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with args should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello") && stdout.contains("world"),
        "Output should contain passed arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_with_args_no_separator() {
    // Test that args work even without -- separator
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-args:
    script: 'echo "ARGS:" "$@"'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-args",
            "arg1",
            "arg2",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with args (no separator) should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("arg1") && stdout.contains("arg2"),
        "Output should contain passed arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_auto_append_args() {
    // Test that args are auto-appended when script doesn't use $@
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-cmd:
    script: 'echo'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-cmd",
            "--",
            "hello",
            "world",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with auto-append should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello") && stdout.contains("world"),
        "Output should contain auto-appended arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_auto_append_with_special_chars() {
    // Test that auto-appended args are properly escaped
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-cmd:
    script: 'echo'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-cmd",
            "--",
            "hello world", // space
            "it's",        // quote
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with special chars should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world") && stdout.contains("it's"),
        "Output should contain properly escaped arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_positional_params_still_work() {
    // Test that $@ still works when explicitly used
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-wrapped:
    script: 'echo "WRAPPED:" "$@"'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-wrapped",
            "--",
            "arg1",
            "arg2",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with $@ should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("WRAPPED:") && stdout.contains("arg1") && stdout.contains("arg2"),
        "Output should contain wrapped arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_dollar_star_detected() {
    // Test that $* is also detected as positional params
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-star:
    script: 'echo "STAR:" $*'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-star",
            "--",
            "a",
            "b",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with $* should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("STAR:") && stdout.contains("a") && stdout.contains("b"),
        "Output should contain star arguments, got: {}",
        stdout
    );
}

#[test]
fn test_run_dollar_one_detected() {
    // Test that $1 is also detected as positional params
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
scripts:
  echo-first:
    script: 'echo "FIRST:" $1'

services:
  dummy:
    process: "true"
"#;
    let config_path = temp_dir.path().join("service-federation.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "run",
            "echo-first",
            "--",
            "first",
            "second",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "fed run with $1 should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FIRST:") && stdout.contains("first"),
        "Output should contain first argument, got: {}",
        stdout
    );
}

// ============================================================================
// install subcommand
// ============================================================================

#[test]
fn test_install_help() {
    let output = Command::new(fed_binary())
        .args(["install", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("install") || stdout.contains("Install"));
}

#[test]
fn test_install_runs() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "install",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Installing") || stdout.contains("install"),
        "Expected install output, got: {}",
        stdout
    );
}

// ============================================================================
// clean subcommand
// ============================================================================

#[test]
fn test_clean_help() {
    let output = Command::new(fed_binary())
        .args(["clean", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("clean") || stdout.contains("Clean"));
}

#[test]
fn test_clean_runs() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "clean",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// session subcommand
// ============================================================================

#[test]
fn test_session_help() {
    let output = Command::new(fed_binary())
        .args(["session", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("session") || stdout.contains("Session"));
}

#[test]
fn test_session_list() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "session",
            "list",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "session list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// init subcommand
// ============================================================================

#[test]
fn test_init_help() {
    let output = Command::new(fed_binary())
        .args(["init", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init") || stdout.contains("Initialize"));
}

#[test]
fn test_init_creates_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("service-federation.yaml");

    // Ensure no config exists
    assert!(!config_path.exists());

    // init uses --output/-o flag, not the global -c flag
    let output = Command::new(fed_binary())
        .args(["init", "-o", config_path.to_str().unwrap()])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Config should now exist
    assert!(
        config_path.exists(),
        "init should create service-federation.yaml"
    );
}

#[test]
fn test_init_does_not_overwrite() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("service-federation.yaml");

    // Create existing config
    fs::write(&config_path, "existing: config").unwrap();

    // init uses --output/-o flag, not the global -w flag
    let output = Command::new(fed_binary())
        .args(["init", "-o", config_path.to_str().unwrap()])
        .output()
        .expect("Failed to run fed");

    // Should fail or warn about existing config
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Either fails or warns
    assert!(
        !output.status.success() || stderr.contains("exists") || stdout.contains("exists"),
        "init should not silently overwrite existing config"
    );

    // Original content should be preserved
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("existing"),
        "init should not overwrite existing config"
    );
}

// ============================================================================
// validate subcommand
// ============================================================================

#[test]
fn test_validate_help() {
    let output = Command::new(fed_binary())
        .args(["validate", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Validate") || stdout.contains("validate"));
}

#[test]
fn test_validate_valid_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "validate",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "validate failed on valid config: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_validate_invalid_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("service-federation.yaml");

    // Write invalid YAML
    fs::write(&config_path, "invalid: yaml: content: [").unwrap();

    let output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            temp_dir.path().to_str().unwrap(),
            "validate",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        !output.status.success(),
        "validate should fail on invalid config"
    );
}

// ============================================================================
// completions subcommand
// ============================================================================

#[test]
fn test_completions_help() {
    let output = Command::new(fed_binary())
        .args(["completions", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("completions") || stdout.contains("shell"));
}

#[test]
fn test_completions_bash() {
    let output = Command::new(fed_binary())
        .args(["completions", "bash"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "completions bash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("complete") || stdout.contains("_fed"),
        "Expected bash completion script"
    );
}

#[test]
fn test_completions_zsh() {
    let output = Command::new(fed_binary())
        .args(["completions", "zsh"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "completions zsh failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("compdef") || stdout.contains("_fed") || stdout.contains("_arguments"),
        "Expected zsh completion script"
    );
}

#[test]
fn test_completions_fish() {
    let output = Command::new(fed_binary())
        .args(["completions", "fish"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "completions fish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("complete") || stdout.contains("fed"),
        "Expected fish completion script"
    );
}

// ============================================================================
// port subcommand
// ============================================================================

#[test]
fn test_port_help() {
    let output = Command::new(fed_binary())
        .args(["port", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("port") || stdout.contains("Port"));
}

// ============================================================================
// doctor subcommand
// ============================================================================

#[test]
fn test_doctor_help() {
    let output = Command::new(fed_binary())
        .args(["doctor", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor") || stdout.contains("Check"));
}

#[test]
fn test_doctor_runs() {
    let output = Command::new(fed_binary())
        .args(["doctor"])
        .output()
        .expect("Failed to run fed");

    // Doctor should always succeed (it's diagnostic)
    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should check for Docker at minimum
    assert!(
        stdout.contains("Docker")
            || stdout.contains("docker")
            || stdout.contains("✓")
            || stdout.contains("✗"),
        "Expected doctor to check system requirements, got: {}",
        stdout
    );
}

// ============================================================================
// top subcommand
// ============================================================================

#[test]
fn test_top_help() {
    let output = Command::new(fed_binary())
        .args(["top", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("top") || stdout.contains("resource") || stdout.contains("usage"));
}

// Note: `top` is interactive and blocks, so we only test help
// The actual `top` functionality is tested in other integration tests

// ============================================================================
// debug subcommand
// ============================================================================

#[test]
fn test_debug_help() {
    let output = Command::new(fed_binary())
        .args(["debug", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("debug") || stdout.contains("Debug"));
}

// ============================================================================
// tui subcommand
// ============================================================================

#[test]
fn test_tui_help() {
    let output = Command::new(fed_binary())
        .args(["tui", "--help"])
        .output()
        .expect("Failed to run fed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tui") || stdout.contains("TUI") || stdout.contains("interactive"));
}

// Note: We can't easily test the interactive TUI without a PTY

// ============================================================================
// Global options tests
// ============================================================================

#[test]
fn test_config_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args(["-c", &config_path, "validate"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "-c flag not working: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_workdir_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    // -w sets working directory for execution, but still needs -c to find config
    let output = Command::new(fed_binary())
        .args([
            "-c",
            &config_path,
            "-w",
            temp_dir.path().to_str().unwrap(),
            "validate",
        ])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "-w flag not working: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_env_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args(["-c", &config_path, "-e", "production", "validate"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "-e flag not working: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_profile_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args(["-c", &config_path, "-p", "dev", "validate"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "-p flag not working: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_multiple_profiles() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = create_test_config(&temp_dir);

    let output = Command::new(fed_binary())
        .args(["-c", &config_path, "-p", "dev", "-p", "debug", "validate"])
        .output()
        .expect("Failed to run fed");

    assert!(
        output.status.success(),
        "multiple -p flags not working: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// Error handling tests
// ============================================================================

#[test]
fn test_unknown_command() {
    let output = Command::new(fed_binary())
        .args(["nonexistent-command-xyz123"])
        .output()
        .expect("Failed to run fed");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("Unknown") || stderr.contains("unrecognized"),
        "Expected error for unknown command, got: {}",
        stderr
    );
}

#[test]
fn test_invalid_flag() {
    let output = Command::new(fed_binary())
        .args(["--invalid-flag-12345"])
        .output()
        .expect("Failed to run fed");

    assert!(!output.status.success());
}
