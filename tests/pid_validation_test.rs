//! Comprehensive tests for PID validation across the codebase
//!
//! These tests verify that PIDs are properly validated before storage and use,
//! preventing potential signal misdirection bugs when PIDs exceed i32::MAX.

use service_federation::config::ServiceType;
use service_federation::service::Status;
use service_federation::state::{ServiceState, StateTracker};
use tempfile::TempDir;

/// Helper to create a temp directory for tests
fn create_test_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

// =============================================================================
// PID Validation in StateTracker
// =============================================================================

#[tokio::test]
async fn test_update_service_pid_valid() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Valid PID should succeed
    let result = tracker.update_service_pid("test-service", 12345).await;
    assert!(result.is_ok(), "Valid PID should be accepted");

    let service = tracker.get_service("test-service").await.unwrap();
    assert_eq!(service.pid, Some(12345));
}

#[tokio::test]
async fn test_update_service_pid_max_valid() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Maximum valid PID (i32::MAX) should succeed
    let max_valid_pid = i32::MAX as u32;
    let result = tracker
        .update_service_pid("test-service", max_valid_pid)
        .await;
    assert!(result.is_ok(), "i32::MAX should be a valid PID");

    let service = tracker.get_service("test-service").await.unwrap();
    assert_eq!(service.pid, Some(max_valid_pid));
}

#[tokio::test]
async fn test_update_service_pid_overflow_rejected() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // PID > i32::MAX should be rejected
    let overflow_pid = (i32::MAX as u32) + 1;
    let result = tracker
        .update_service_pid("test-service", overflow_pid)
        .await;
    assert!(result.is_err(), "PID > i32::MAX should be rejected");

    // Error message should mention the overflow
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("i32::MAX") || err_str.contains("exceed"),
        "Error should mention i32::MAX: {}",
        err_str
    );
}

#[tokio::test]
async fn test_update_service_pid_u32_max_rejected() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // u32::MAX should definitely be rejected
    let result = tracker.update_service_pid("test-service", u32::MAX).await;
    assert!(result.is_err(), "u32::MAX should be rejected");
}

#[tokio::test]
async fn test_update_service_pid_zero_rejected() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // PID 0 should be rejected (invalid/kernel PID)
    let result = tracker.update_service_pid("test-service", 0).await;
    assert!(result.is_err(), "PID 0 should be rejected");
}

#[tokio::test]
async fn test_update_service_pid_one_accepted() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // PID 1 (init) is technically valid for storage, even if we won't signal it
    // The check for init happens at signal time, not storage time
    let result = tracker.update_service_pid("test-service", 1).await;
    assert!(result.is_ok(), "PID 1 should be accepted for storage");
}

#[tokio::test]
async fn test_update_service_pid_boundary_values() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    // Test various boundary PIDs
    let test_cases = vec![
        (1u32, true, "PID 1"),
        (100, true, "Low PID"),
        (32768, true, "Default max PID on many systems"),
        (65535, true, "Max on some systems"),
        (4194304, true, "Linux max when increased"),
        (i32::MAX as u32 - 1, true, "Just below i32::MAX"),
        (i32::MAX as u32, true, "Exactly i32::MAX"),
        (i32::MAX as u32 + 1, false, "Just above i32::MAX"),
        (u32::MAX - 1, false, "Just below u32::MAX"),
        (u32::MAX, false, "u32::MAX"),
    ];

    for (pid, should_succeed, desc) in test_cases {
        let service_name = format!("service-{}", pid);
        let service_state = ServiceState::new(
            service_name.clone(),
            ServiceType::Process,
            "default".to_string(),
        );
        tracker.register_service(service_state).await.unwrap();

        let result = tracker.update_service_pid(&service_name, pid).await;
        assert_eq!(
            result.is_ok(),
            should_succeed,
            "{} (PID {}): expected {}, got {}",
            desc,
            pid,
            if should_succeed { "success" } else { "failure" },
            if result.is_ok() { "success" } else { "failure" }
        );
    }
}

#[tokio::test]
async fn test_pid_validation_preserves_existing_pid_on_failure() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "test-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Set a valid PID first
    tracker
        .update_service_pid("test-service", 12345)
        .await
        .expect("Valid PID should work");

    // Try to set an invalid PID
    let result = tracker.update_service_pid("test-service", u32::MAX).await;
    assert!(result.is_err(), "Invalid PID should be rejected");

    // Original PID should be preserved
    let service = tracker.get_service("test-service").await.unwrap();
    assert_eq!(
        service.pid,
        Some(12345),
        "Original PID should be preserved after failed update"
    );
}

// =============================================================================
// PID Validation in State Deserialization
// =============================================================================

// Test that services with invalid PIDs (> i32::MAX or 0) are sanitized when
// state is loaded from SQLite. This prevents potential signal misdirection bugs.
#[tokio::test]
async fn test_deserialization_sanitizes_invalid_pids() {
    let temp_dir = create_test_dir();

    // Phase 1: Create a state with a valid PID and save it
    {
        let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
            .await
            .unwrap();
        tracker.initialize().await.expect("Init failed");

        let service_state = ServiceState::new(
            "test-service".to_string(),
            ServiceType::Process,
            "default".to_string(),
        );
        tracker.register_service(service_state).await.unwrap();
        tracker
            .update_service_pid("test-service", 12345)
            .await
            .expect("Valid PID");
        tracker.force_save().await.expect("Save failed");
    }
    // Tracker is dropped here, closing the database connection

    // Phase 2: Manually corrupt the database to have an invalid PID
    let db_path = temp_dir.path().join(".fed/lock.db");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("Open database");
        conn.execute(
            "UPDATE services SET pid = 2147483650 WHERE id = 'test-service'",
            [],
        )
        .expect("Corrupt PID in database");
    }

    // Phase 3: Load the state - invalid PIDs should be sanitized
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    let result = tracker.initialize().await;

    // Initialization should succeed (sanitization happens during load)
    assert!(result.is_ok(), "Load should succeed with sanitization");

    // The service with invalid PID should be removed during sanitization
    // (since we can't safely signal it)
    let service = tracker.get_service("test-service").await;
    assert!(
        service.is_none(),
        "Service with invalid PID should be removed during load"
    );
}

// =============================================================================
// Integration Tests for PID Flow
// =============================================================================

#[tokio::test]
async fn test_pid_flow_through_state_tracker() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    // Simulate full lifecycle: register -> update PID -> update status -> save
    let service_state = ServiceState::new(
        "lifecycle-service".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Valid PID
    tracker
        .update_service_pid("lifecycle-service", 54321)
        .await
        .expect("Valid PID update");

    tracker
        .update_service_status("lifecycle-service", Status::Running)
        .await
        .expect("Status update");

    tracker.force_save().await.expect("Save");

    // Reload and verify
    let mut tracker2 = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker2.initialize().await.expect("Reload init");

    // Note: Service may be cleaned up if process 54321 isn't running
    // This is expected behavior - we're testing the flow, not persistence
}

#[tokio::test]
async fn test_multiple_services_with_pids() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    // Register multiple services with different PIDs
    let pids = [1000u32, 2000, 3000, 4000, 5000];

    for (i, pid) in pids.iter().enumerate() {
        let name = format!("service-{}", i);
        let service_state =
            ServiceState::new(name.clone(), ServiceType::Process, "default".to_string());
        tracker.register_service(service_state).await.unwrap();
        tracker
            .update_service_pid(&name, *pid)
            .await
            .expect("PID update");
    }

    // Verify all PIDs were stored correctly
    for (i, pid) in pids.iter().enumerate() {
        let name = format!("service-{}", i);
        let service = tracker.get_service(&name).await.expect("Service exists");
        assert_eq!(service.pid, Some(*pid), "PID mismatch for {}", name);
    }
}

#[tokio::test]
async fn test_pid_update_for_nonexistent_service() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    // Try to update PID for a service that doesn't exist
    let result = tracker
        .update_service_pid("nonexistent-service", 12345)
        .await;
    assert!(result.is_err(), "Should fail for nonexistent service");
}

// =============================================================================
// Edge Cases and Stress Tests
// =============================================================================

#[tokio::test]
async fn test_rapid_pid_updates() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "rapid-update".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Rapidly update PID many times
    for pid in 1000..1100u32 {
        tracker
            .update_service_pid("rapid-update", pid)
            .await
            .expect("PID update");
    }

    // Final PID should be 1099
    let service = tracker.get_service("rapid-update").await.unwrap();
    assert_eq!(service.pid, Some(1099));
}

#[tokio::test]
async fn test_pid_validation_error_messages_are_useful() {
    let temp_dir = create_test_dir();
    let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    tracker.initialize().await.expect("Init failed");

    let service_state = ServiceState::new(
        "error-test".to_string(),
        ServiceType::Process,
        "default".to_string(),
    );
    tracker.register_service(service_state).await.unwrap();

    // Test overflow error message
    let overflow_result = tracker.update_service_pid("error-test", u32::MAX).await;
    assert!(overflow_result.is_err());
    let err_msg = overflow_result.unwrap_err().to_string();
    assert!(
        err_msg.contains("error-test") || err_msg.contains("i32::MAX"),
        "Error should be descriptive: {}",
        err_msg
    );

    // Test zero PID error message
    let zero_result = tracker.update_service_pid("error-test", 0).await;
    assert!(zero_result.is_err());
    let err_msg = zero_result.unwrap_err().to_string();
    assert!(
        err_msg.contains("0") || err_msg.contains("zero"),
        "Error should mention zero: {}",
        err_msg
    );
}
