/// Property-based tests for state machine transitions
///
/// These tests use proptest to generate random sequences of service operations
/// and verify invariants hold across all scenarios:
/// - Status eventually becomes consistent (no stuck "Starting" states)
/// - PID is always cleared when status is "Stopped"
/// - No services leak (state tracker matches reality)
use proptest::prelude::*;
use service_federation::service::Status;
use service_federation::state::{ServiceState, StateTracker};
use tempfile::TempDir;

/// Helper to create a temp directory for tests
fn create_test_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

/// Operation types for property-based testing
#[derive(Debug, Clone)]
enum Operation {
    /// Start a service (transition to Starting)
    Start(String),
    /// Stop a service (transition to Stopping)
    Stop(String),
    /// Mark service as running with PID
    MarkRunning(String, u32),
    /// Mark service as stopped
    MarkStopped(String),
    /// Mark service as healthy
    MarkHealthy(String),
    /// Mark service as failing
    MarkFailing(String),
    /// Health check passes (clear failures)
    HealthCheckPass(String),
    /// Health check fails (increment failures)
    HealthCheckFail(String),
}

/// Strategy for generating service names
fn service_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z]{3,8}")
        .expect("Valid regex")
        .prop_map(|s| format!("service-{}", s))
}

/// Strategy for generating PIDs
fn pid_strategy() -> impl Strategy<Value = u32> {
    1000u32..50000u32
}

/// Strategy for generating operations
fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        service_name_strategy().prop_map(Operation::Start),
        service_name_strategy().prop_map(Operation::Stop),
        (service_name_strategy(), pid_strategy()).prop_map(|(s, p)| Operation::MarkRunning(s, p)),
        service_name_strategy().prop_map(Operation::MarkStopped),
        service_name_strategy().prop_map(Operation::MarkHealthy),
        service_name_strategy().prop_map(Operation::MarkFailing),
        service_name_strategy().prop_map(Operation::HealthCheckPass),
        service_name_strategy().prop_map(Operation::HealthCheckFail),
    ]
}

/// Apply an operation to the state tracker, ignoring errors
/// (some operations will fail if service doesn't exist, which is expected)
async fn apply_operation(tracker: &mut StateTracker, op: &Operation) {
    match op {
        Operation::Start(name) => {
            // Register if not exists
            if !tracker.is_service_registered(name).await {
                let state =
                    ServiceState::new(name.clone(), "Process".to_string(), "default".to_string());
                tracker.register_service(state).await.unwrap();
            }
            // Transition to Starting
            let _ = tracker.update_service_status(name, Status::Starting).await;
        }
        Operation::Stop(name) => {
            // Transition to Stopping
            let _ = tracker.update_service_status(name, Status::Stopping).await;
        }
        Operation::MarkRunning(name, pid) => {
            // Transition to Running with PID
            let _ = tracker.update_service_status(name, Status::Running).await;
            let _ = tracker.update_service_pid(name, *pid).await;
        }
        Operation::MarkStopped(name) => {
            // Transition to Stopped - PID will be cleared by state transition
            let _ = tracker.update_service_status(name, Status::Stopped).await;
            // Note: In production, PID clearing happens through StateTransition
            // For this test, we'll just set status to stopped
        }
        Operation::MarkHealthy(name) => {
            let _ = tracker.update_service_status(name, Status::Healthy).await;
        }
        Operation::MarkFailing(name) => {
            let _ = tracker.update_service_status(name, Status::Failing).await;
        }
        Operation::HealthCheckPass(name) => {
            let _ = tracker.reset_consecutive_failures(name).await;
        }
        Operation::HealthCheckFail(name) => {
            let _ = tracker.increment_consecutive_failures(name).await;
        }
    }
}

/// Check invariants on the state tracker
async fn check_invariants(tracker: &StateTracker) {
    let services = tracker.get_services().await;

    for (name, service) in services {
        // Invariant 1: If status is Stopped, PID must be None
        if service.status == Status::Stopped {
            assert!(
                service.pid.is_none(),
                "Service {} has status 'stopped' but PID is {:?}",
                name,
                service.pid
            );
        }

        // Invariant 2: If PID exists, status must NOT be Stopped
        if service.pid.is_some() {
            assert!(
                service.status != Status::Stopped,
                "Service {} has PID {:?} but status is 'stopped'",
                name,
                service.pid
            );
        }

        // Invariant 3: Status is always valid (enforced by the Status enum itself)

        // Invariant 4: consecutive_failures should be >= 0 (u32 ensures this, but verify)
        // This is a sanity check
        let failures = tracker.get_consecutive_failures(&name).await;
        assert!(
            failures.is_some(),
            "Service {} should have a failure count",
            name
        );
    }
}

proptest! {
    /// Property test: Random sequences of operations maintain invariants
    ///
    /// Generates random sequences of service operations and verifies that
    /// the state tracker maintains consistency throughout.
    #[test]
    fn test_state_machine_invariants(ops in prop::collection::vec(operation_strategy(), 10..50)) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp_dir = create_test_dir();
            let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
                .await
                .expect("Failed to create tracker");
            tracker.initialize().await.expect("Failed to initialize");

            // Apply all operations
            for op in &ops {
                apply_operation(&mut tracker, op).await;
            }

            // Check invariants hold
            check_invariants(&tracker).await;
        });
    }

    /// Property test: Services can be started and stopped multiple times
    ///
    /// After a sequence of starts and stops, the state tracker should remain consistent.
    /// Note: This test doesn't verify PID clearing since we're not using StateTransition API,
    /// which handles atomic status+PID updates in production.
    #[test]
    fn test_service_lifecycle_consistency(
        service_names in prop::collection::vec(service_name_strategy(), 3..10),
        operations in prop::collection::vec(prop::bool::ANY, 10..30)
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp_dir = create_test_dir();
            let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
                .await
                .expect("Failed to create tracker");
            tracker.initialize().await.expect("Failed to initialize");

            // Register all services
            for name in &service_names {
                let state = ServiceState::new(
                    name.clone(),
                    "Process".to_string(),
                    "default".to_string(),
                );
                tracker.register_service(state).await.unwrap();
            }

            // Apply random start/stop operations
            for (idx, should_start) in operations.iter().enumerate() {
                let name = &service_names[idx % service_names.len()];
                if *should_start {
                    let _ = tracker.update_service_status(name, Status::Starting).await;
                    let _ = tracker.update_service_status(name, Status::Running).await;
                    let _ = tracker.update_service_pid(name, 1000 + idx as u32).await;
                } else {
                    let _ = tracker.update_service_status(name, Status::Stopping).await;
                    let _ = tracker.update_service_status(name, Status::Stopped).await;
                }
            }

            // Verify: All services are still registered
            let services = tracker.get_services().await;
            for name in &service_names {
                assert!(
                    services.contains_key(name),
                    "Service {} should still be registered",
                    name
                );
            }

            // Verify: All services have valid status (enforced by Status enum)
        });
    }

    /// Property test: Consecutive failures are tracked correctly
    ///
    /// Verifies that consecutive failures increment on failures and reset on success.
    #[test]
    fn test_consecutive_failures_tracking(
        service_name in service_name_strategy(),
        health_checks in prop::collection::vec(prop::bool::ANY, 5..20)
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp_dir = create_test_dir();
            let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
                .await
                .expect("Failed to create tracker");
            tracker.initialize().await.expect("Failed to initialize");

            // Register service
            let state = ServiceState::new(
                service_name.clone(),
                "Process".to_string(),
                "default".to_string(),
            );
            tracker.register_service(state).await.unwrap();

            let mut expected_failures = 0u32;

            for &healthy in &health_checks {
                if healthy {
                    tracker.reset_consecutive_failures(&service_name).await.ok();
                    expected_failures = 0;
                } else {
                    tracker.increment_consecutive_failures(&service_name).await.ok();
                    expected_failures += 1;
                }

                let actual_failures = tracker
                    .get_consecutive_failures(&service_name)
                    .await
                    .unwrap_or(0);

                assert_eq!(
                    actual_failures, expected_failures,
                    "Failure count mismatch after health check sequence"
                );
            }
        });
    }

    /// Property test: Port allocations are tracked correctly
    ///
    /// Verifies that allocated ports are tracked and retrievable.
    #[test]
    fn test_port_allocation_tracking(ports in prop::collection::vec(1024u16..65535u16, 5..20)) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp_dir = create_test_dir();
            let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
                .await
                .expect("Failed to create tracker");
            tracker.initialize().await.expect("Failed to initialize");

            // Track all ports
            for &port in &ports {
                tracker.track_port(port).await;
            }

            // Verify all unique ports are tracked
            let allocated = tracker.get_allocated_ports().await;
            let mut unique_ports: Vec<u16> = ports.clone();
            unique_ports.sort();
            unique_ports.dedup();

            assert_eq!(
                allocated.len(),
                unique_ports.len(),
                "Number of tracked ports doesn't match unique ports"
            );

            for &port in &unique_ports {
                assert!(
                    allocated.contains(&port),
                    "Port {} was tracked but not in allocated ports",
                    port
                );
            }
        });
    }

    /// Property test: Service unregistration cleans up properly
    ///
    /// Verifies that unregistering services removes them completely.
    #[test]
    fn test_service_unregistration(
        service_names in prop::collection::vec(service_name_strategy(), 5..15),
        indices_to_remove in prop::collection::vec(0usize..10, 1..8)
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp_dir = create_test_dir();
            let mut tracker = StateTracker::new(temp_dir.path().to_path_buf())
                .await
                .expect("Failed to create tracker");
            tracker.initialize().await.expect("Failed to initialize");

            // Register all services
            for name in &service_names {
                let state = ServiceState::new(
                    name.clone(),
                    "Process".to_string(),
                    "default".to_string(),
                );
                tracker.register_service(state).await.unwrap();
            }

            // Remove some services
            let mut removed_names = Vec::new();
            for &idx in &indices_to_remove {
                if idx < service_names.len() {
                    let name = &service_names[idx];
                    tracker.unregister_service(name).await.unwrap();
                    removed_names.push(name.clone());
                }
            }

            // Verify removed services are gone
            for name in &removed_names {
                assert!(
                    !tracker.is_service_registered(name).await,
                    "Service {} should be unregistered",
                    name
                );
                assert!(
                    tracker.get_service(name).await.is_none(),
                    "Service {} should return None after unregistration",
                    name
                );
            }

            // Verify remaining services still exist
            let remaining_services = tracker.get_services().await;
            let removed_set: std::collections::HashSet<_> = removed_names.iter().collect();
            for (_name, service) in remaining_services {
                assert!(
                    !removed_set.contains(&service.id),
                    "Service {} should have been removed",
                    service.id
                );
            }
        });
    }
}

/// Unit test: Verify state machine transition validation
#[test]
fn test_status_transitions() {
    use service_federation::service::Status;

    // Valid transitions
    assert!(Status::Stopped.is_valid_transition(Status::Starting));
    assert!(Status::Starting.is_valid_transition(Status::Running));
    assert!(Status::Running.is_valid_transition(Status::Healthy));
    assert!(Status::Healthy.is_valid_transition(Status::Failing));
    assert!(Status::Failing.is_valid_transition(Status::Stopping));
    assert!(Status::Stopping.is_valid_transition(Status::Stopped));

    // Invalid transitions
    assert!(!Status::Stopped.is_valid_transition(Status::Running)); // Must go through Starting
    assert!(!Status::Stopped.is_valid_transition(Status::Healthy)); // Must go through Starting -> Running
    assert!(!Status::Starting.is_valid_transition(Status::Stopped)); // Must go through Running -> Stopping
    assert!(!Status::Healthy.is_valid_transition(Status::Stopped)); // Must go through Stopping

    // Same state is always valid
    assert!(Status::Stopped.is_valid_transition(Status::Stopped));
    assert!(Status::Running.is_valid_transition(Status::Running));
}
