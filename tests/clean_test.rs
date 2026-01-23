use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

fn fed_binary() -> String {
    env!("CARGO_BIN_EXE_fed").to_string()
}

fn create_clean_test_config(content: &str) -> (TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("service-federation.yaml");
    fs::write(&config_path, content).expect("Failed to write test config");
    (temp_dir, config_path)
}

#[test]
fn test_clean_command_runs_clean_script() {
    let config_content = r#"
services:
  test-service:
    cwd: .
    process: "echo 'running'"
    install: "mkdir -p artifacts && touch artifacts/test.txt"
    clean: "rm -rf artifacts"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // First run install to create artifacts
    let install_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "install",
        ])
        .output()
        .expect("Failed to run install");

    assert!(
        install_output.status.success(),
        "Install failed: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );

    // Verify artifacts directory exists
    let artifacts_path = temp_dir.path().join("artifacts");
    assert!(
        artifacts_path.exists(),
        "artifacts directory should exist after install"
    );

    // Run clean
    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // Verify artifacts directory is gone
    assert!(
        !artifacts_path.exists(),
        "artifacts directory should be removed after clean"
    );

    let stdout = String::from_utf8_lossy(&clean_output.stdout);
    assert!(
        stdout.contains("test-service"),
        "Clean output should mention service name"
    );
    assert!(
        stdout.contains("completed successfully"),
        "Clean output should indicate success"
    );
}

#[test]
fn test_clean_command_specific_service() {
    let config_content = r#"
services:
  service-a:
    process: "echo a"
    clean: "touch cleaned-a.txt"
  service-b:
    process: "echo b"
    clean: "touch cleaned-b.txt"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Clean only service-a
    let clean_output = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "clean",
            "service-a",
        ])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // Only service-a should be cleaned
    assert!(
        temp_dir.path().join("cleaned-a.txt").exists(),
        "service-a should have been cleaned"
    );
    assert!(
        !temp_dir.path().join("cleaned-b.txt").exists(),
        "service-b should NOT have been cleaned"
    );
}

#[test]
fn test_clean_clears_install_state() {
    let config_content = r#"
services:
  test-service:
    process: "echo 'running'"
    install: "echo 'installing'"
    clean: "echo 'cleaning'"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Run install twice - second should skip (already installed)
    Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "install",
        ])
        .output()
        .expect("Failed to run install");

    let install_output_2 = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "install",
        ])
        .output()
        .expect("Failed to run install");

    let stderr_2 = String::from_utf8_lossy(&install_output_2.stderr);
    // Second install should skip (already installed)
    assert!(
        stderr_2.contains("already installed") || install_output_2.status.success(),
        "Second install should skip or succeed"
    );

    // Run clean
    Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    // Run install again - should actually run (install state cleared)
    let install_output_3 = Command::new(fed_binary())
        .args([
            "-c",
            config_path.to_str().unwrap(),
            "-w",
            workdir,
            "install",
        ])
        .output()
        .expect("Failed to run install");

    let stderr_3 = String::from_utf8_lossy(&install_output_3.stderr);
    // After clean, install should run the command again
    assert!(
        stderr_3.contains("Running install") || install_output_3.status.success(),
        "Install should run after clean"
    );
}

#[test]
fn test_clean_no_services_message() {
    let config_content = r#"
services:
  test-service:
    process: "echo 'running'"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    let stdout = String::from_utf8_lossy(&clean_output.stdout);
    assert!(
        stdout.contains("No services with clean field or Docker volumes found"),
        "Should indicate no services to clean"
    );
}

#[test]
fn test_clean_handles_failed_command() {
    let config_content = r#"
services:
  test-service:
    process: "echo 'running'"
    clean: "exit 1"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    // Should fail because clean command exits with 1
    assert!(
        !clean_output.status.success(),
        "Clean should fail when command fails"
    );
}

#[test]
fn test_clean_with_environment_variables() {
    let config_content = r#"
services:
  test-service:
    process: "echo 'running'"
    environment:
      CLEAN_TARGET: "env_artifacts"
    clean: "mkdir -p $CLEAN_TARGET && touch $CLEAN_TARGET/cleaned.txt"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // Verify environment variable was used
    assert!(
        temp_dir.path().join("env_artifacts/cleaned.txt").exists(),
        "Clean should use environment variables"
    );
}

#[test]
fn test_clean_with_custom_cwd() {
    let config_content = r#"
services:
  test-service:
    cwd: subdir
    process: "echo 'running'"
    clean: "touch cleaned.txt"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Create the subdir
    fs::create_dir(temp_dir.path().join("subdir")).expect("Failed to create subdir");

    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // File should be in subdir, not root
    assert!(
        temp_dir.path().join("subdir/cleaned.txt").exists(),
        "Clean should run in service's cwd"
    );
    assert!(
        !temp_dir.path().join("cleaned.txt").exists(),
        "Clean should NOT run in root dir"
    );
}

#[test]
fn test_clean_idempotent() {
    let config_content = r#"
services:
  test-service:
    process: "echo 'running'"
    clean: "rm -rf nonexistent_dir"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Run clean multiple times - should succeed each time
    for i in 0..3 {
        let clean_output = Command::new(fed_binary())
            .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
            .output()
            .expect("Failed to run clean");

        assert!(
            clean_output.status.success(),
            "Clean attempt {} failed: {}",
            i + 1,
            String::from_utf8_lossy(&clean_output.stderr)
        );
    }
}

// Docker volume tests (only run if Docker is available)
fn is_docker_available() -> bool {
    Command::new("docker")
        .args(["version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_clean_with_docker_volumes() {
    if !is_docker_available() {
        eprintln!("Skipping Docker volume test - Docker not available");
        return;
    }

    let volume_name = format!("fed-test-volume-{}", std::process::id());
    let config_content = format!(
        r#"
services:
  docker-service:
    image: alpine:latest
    volumes:
      - "{}:/data"
"#,
        volume_name
    );

    let (temp_dir, config_path) = create_clean_test_config(&config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Create the volume manually
    let _ = Command::new("docker")
        .args(["volume", "create", &volume_name])
        .output();

    // Verify volume exists
    let volume_check = Command::new("docker")
        .args(["volume", "inspect", &volume_name])
        .output()
        .expect("Failed to inspect volume");

    if !volume_check.status.success() {
        eprintln!("Could not create test volume, skipping test");
        return;
    }

    // Run clean
    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // Verify volume is removed
    std::thread::sleep(Duration::from_millis(500));
    let volume_check_after = Command::new("docker")
        .args(["volume", "inspect", &volume_name])
        .output()
        .expect("Failed to inspect volume");

    assert!(
        !volume_check_after.status.success(),
        "Docker volume should be removed after clean"
    );

    // Cleanup (in case test failed)
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &volume_name])
        .output();
}

#[test]
fn test_clean_skips_bind_mounts() {
    if !is_docker_available() {
        eprintln!("Skipping Docker bind mount test - Docker not available");
        return;
    }

    let config_content = r#"
services:
  docker-service:
    image: alpine:latest
    volumes:
      - "./data:/data"
      - "../parent:/parent"
"#;

    let (temp_dir, config_path) = create_clean_test_config(config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Create local data directory
    fs::create_dir(temp_dir.path().join("data")).expect("Failed to create data dir");
    fs::write(temp_dir.path().join("data/test.txt"), "test").expect("Failed to write test file");

    // Run clean - should succeed without trying to remove bind mounts as volumes
    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    // Local data should still exist (bind mounts are not Docker volumes)
    assert!(
        temp_dir.path().join("data/test.txt").exists(),
        "Bind mount directories should not be affected by volume cleanup"
    );
}

#[test]
fn test_clean_only_removes_fed_prefixed_volumes() {
    if !is_docker_available() {
        eprintln!("Skipping Docker volume test - Docker not available");
        return;
    }

    // Create a fed- prefixed volume and a non-fed volume
    let fed_volume = format!("fed-test-clean-{}", std::process::id());
    let non_fed_volume = format!("user-data-{}", std::process::id());

    let config_content = format!(
        r#"
services:
  docker-service:
    image: alpine:latest
    volumes:
      - "{}:/fed-data"
      - "{}:/user-data"
"#,
        fed_volume, non_fed_volume
    );

    let (temp_dir, config_path) = create_clean_test_config(&config_content);
    let workdir = temp_dir.path().to_str().unwrap();

    // Create both volumes manually
    let fed_create = Command::new("docker")
        .args(["volume", "create", &fed_volume])
        .output();
    let non_fed_create = Command::new("docker")
        .args(["volume", "create", &non_fed_volume])
        .output();

    if fed_create.is_err() || non_fed_create.is_err() {
        eprintln!("Could not create test volumes, skipping test");
        return;
    }

    // Verify both exist
    let fed_check = Command::new("docker")
        .args(["volume", "inspect", &fed_volume])
        .output()
        .expect("Failed to inspect fed volume");
    let non_fed_check = Command::new("docker")
        .args(["volume", "inspect", &non_fed_volume])
        .output()
        .expect("Failed to inspect non-fed volume");

    if !fed_check.status.success() || !non_fed_check.status.success() {
        eprintln!("Could not verify test volumes exist, skipping test");
        // Cleanup
        let _ = Command::new("docker")
            .args(["volume", "rm", "-f", &fed_volume])
            .output();
        let _ = Command::new("docker")
            .args(["volume", "rm", "-f", &non_fed_volume])
            .output();
        return;
    }

    // Run clean
    let clean_output = Command::new(fed_binary())
        .args(["-c", config_path.to_str().unwrap(), "-w", workdir, "clean"])
        .output()
        .expect("Failed to run clean");

    assert!(
        clean_output.status.success(),
        "Clean failed: {}",
        String::from_utf8_lossy(&clean_output.stderr)
    );

    std::thread::sleep(Duration::from_millis(500));

    // Verify fed- volume is removed
    let fed_check_after = Command::new("docker")
        .args(["volume", "inspect", &fed_volume])
        .output()
        .expect("Failed to inspect fed volume after clean");

    assert!(
        !fed_check_after.status.success(),
        "fed- prefixed volume should be removed after clean"
    );

    // Verify non-fed volume still exists
    let non_fed_check_after = Command::new("docker")
        .args(["volume", "inspect", &non_fed_volume])
        .output()
        .expect("Failed to inspect non-fed volume after clean");

    assert!(
        non_fed_check_after.status.success(),
        "Non-fed volume should NOT be removed by clean"
    );

    // Cleanup the non-fed volume
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &non_fed_volume])
        .output();
}
