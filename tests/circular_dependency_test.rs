/// Tests for circular dependency detection
///
/// When a service's process command invokes `fed` (directly or indirectly),
/// it creates an infinite loop. We detect this at startup by checking for
/// the FED_SPAWNED_BY_SERVICE environment variable that we set when spawning
/// service processes.
use std::process::Command;

/// Test that `fed` detects when it's invoked from within a service process
/// and exits with an appropriate error message.
#[test]
fn test_circular_dependency_detection() {
    // Build the binary first
    let build = Command::new("cargo")
        .args(["build", "--bin", "fed"])
        .output()
        .expect("Failed to build fed binary");
    assert!(
        build.status.success(),
        "Failed to build fed: {:?}",
        String::from_utf8_lossy(&build.stderr)
    );

    // Run fed with FED_SPAWNED_BY_SERVICE set (simulating being called from a service)
    let output = Command::new("cargo")
        .args(["run", "--bin", "fed", "--", "start"])
        .env("FED_SPAWNED_BY_SERVICE", "test-service")
        .output()
        .expect("Failed to run fed");

    // Should exit with non-zero status
    assert!(
        !output.status.success(),
        "Expected fed to fail when FED_SPAWNED_BY_SERVICE is set, but it succeeded"
    );

    // Check error message contains helpful information
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Circular dependency detected"),
        "Expected 'Circular dependency detected' in error message, got: {}",
        stderr
    );
    assert!(
        stderr.contains("test-service"),
        "Expected service name 'test-service' in error message, got: {}",
        stderr
    );
    assert!(
        stderr.contains("infinite loop"),
        "Expected 'infinite loop' warning in error message, got: {}",
        stderr
    );
}

/// Test that `fed` runs normally when FED_SPAWNED_BY_SERVICE is not set
#[test]
fn test_normal_startup_without_circular_dependency_marker() {
    // Build the binary first
    let build = Command::new("cargo")
        .args(["build", "--bin", "fed"])
        .output()
        .expect("Failed to build fed binary");
    assert!(
        build.status.success(),
        "Failed to build fed: {:?}",
        String::from_utf8_lossy(&build.stderr)
    );

    // Run fed --help without the marker (should work normally)
    let output = Command::new("cargo")
        .args(["run", "--bin", "fed", "--", "--help"])
        .env_remove("FED_SPAWNED_BY_SERVICE") // Explicitly remove in case it's set
        .output()
        .expect("Failed to run fed");

    // Should succeed
    assert!(
        output.status.success(),
        "Expected fed --help to succeed, but got error: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should show help output
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Service Federation"),
        "Expected help output, got: {}",
        stdout
    );
}
