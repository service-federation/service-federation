//! Tests that script output streams in real-time rather than being buffered.
//!
//! Uses rexpect (PTY-based testing) to verify output appears before script completion.
//! Only runs on Unix systems.

use std::io::Write;
use tempfile::TempDir;

/// Test that script output is streamed immediately, not buffered until completion.
///
/// This test:
/// 1. Creates a script that prints "streamed" then sleeps for 60 seconds
/// 2. Spawns `fed run` via PTY
/// 3. Expects to see "streamed" within 10 seconds
/// 4. If we see it, output is streaming (not buffered until the 60s sleep completes)
#[test]
#[cfg(unix)]
fn test_script_output_streams_immediately() {
    use rexpect::spawn;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");

    let config = r#"
scripts:
  print_and_wait:
    script: |
      echo "streamed"
      sleep 60
"#;

    std::fs::File::create(&config_path)
        .expect("Failed to create config")
        .write_all(config.as_bytes())
        .expect("Failed to write config");

    // Build the command - use -c flag to specify config path
    let fed_binary = env!("CARGO_BIN_EXE_fed");
    let command = format!(
        "{} -c {} run print_and_wait",
        fed_binary,
        config_path.display()
    );

    // Spawn with 10 second timeout - if output is buffered, we'd wait 60+ seconds
    let mut session = spawn(&command, Some(10_000)).expect("Failed to spawn fed");

    // This should succeed quickly if output streams immediately
    // If buffered, it would timeout waiting for the 60s sleep
    session
        .exp_string("streamed")
        .expect("Should see 'streamed' output before script completes - output may be buffered");

    // Process is killed when session drops
}

/// Test that stderr also streams immediately.
#[test]
#[cfg(unix)]
fn test_script_stderr_streams_immediately() {
    use rexpect::spawn;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");

    let config = r#"
scripts:
  print_stderr_and_wait:
    script: |
      echo "stderr_output" >&2
      sleep 60
"#;

    std::fs::File::create(&config_path)
        .expect("Failed to create config")
        .write_all(config.as_bytes())
        .expect("Failed to write config");

    let fed_binary = env!("CARGO_BIN_EXE_fed");
    let command = format!(
        "{} -c {} run print_stderr_and_wait",
        fed_binary,
        config_path.display()
    );

    let mut session = spawn(&command, Some(10_000)).expect("Failed to spawn fed");

    // PTY captures both stdout and stderr
    session
        .exp_string("stderr_output")
        .expect("Should see stderr output before script completes");
}

/// Test that multiple lines stream as they're produced.
#[test]
#[cfg(unix)]
fn test_script_multiple_lines_stream() {
    use rexpect::spawn;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");

    let config = r#"
scripts:
  multi_line:
    script: |
      echo "line1"
      sleep 1
      echo "line2"
      sleep 1
      echo "line3"
      sleep 60
"#;

    std::fs::File::create(&config_path)
        .expect("Failed to create config")
        .write_all(config.as_bytes())
        .expect("Failed to write config");

    let fed_binary = env!("CARGO_BIN_EXE_fed");
    let command = format!("{} -c {} run multi_line", fed_binary, config_path.display());

    // 15 seconds should be enough for line1, line2, line3 (with 1s sleeps between)
    // but not enough if buffered until the 60s sleep
    let mut session = spawn(&command, Some(15_000)).expect("Failed to spawn fed");

    session.exp_string("line1").expect("Should see line1");
    session.exp_string("line2").expect("Should see line2");
    session.exp_string("line3").expect("Should see line3");
}
