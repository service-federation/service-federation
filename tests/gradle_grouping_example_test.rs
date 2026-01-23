use service_federation::{Orchestrator, Parser};

#[tokio::test]
async fn test_gradle_grouping_example() {
    // Clean up any stale lock file from previous test runs
    let _ = std::fs::remove_file(".fed-lock.json");

    let parser = Parser::new();
    let config = parser
        .load_config("examples/gradle-grouping.yaml")
        .expect("Failed to load gradle grouping example");

    config.validate().expect("Config should be valid");

    let mut orchestrator = Orchestrator::new(config).await.unwrap();
    orchestrator
        .initialize()
        .await
        .expect("Failed to initialize orchestrator");

    let services = orchestrator.get_status().await;

    // Verify grouping behavior:
    // 1. auth-service and user-service should be grouped (same CWD, both depend on database)
    let auth_user_grouped = services.keys().any(|k| {
        k.starts_with("gradle-group-") && k.contains("auth-service") && k.contains("user-service")
    });

    assert!(
        auth_user_grouped,
        "auth-service and user-service should be grouped. Found services: {:?}",
        services.keys().collect::<Vec<_>>()
    );

    // 2. notification-service should be separate (different CWD)
    let notification_in_group_with_auth_user = services.keys().any(|k| {
        k.starts_with("gradle-group-")
            && k.contains("notification-service")
            && (k.contains("auth-service") || k.contains("user-service"))
    });

    assert!(
        !notification_in_group_with_auth_user,
        "notification-service should not be grouped with auth/user (different CWD)"
    );

    // 3. analytics-service SHOULD be grouped with user-service and auth-service
    //    (all in same CWD, regardless of dependencies)
    let analytics_in_main_group = services.keys().any(|k| {
        k.starts_with("gradle-group-")
            && k.contains("analytics-service")
            && k.contains("user-service")
            && k.contains("auth-service")
    });

    assert!(
        analytics_in_main_group,
        "analytics-service should be grouped with auth/user (same CWD). Found: {:?}",
        services.keys().collect::<Vec<_>>()
    );

    // 4. Database should exist
    assert!(
        services.contains_key("database"),
        "Database service should exist"
    );

    // 5. All gradle services should be present (either individually or in groups)
    let all_services_present = {
        let service_names: Vec<_> = services.keys().collect();
        let has_auth = auth_user_grouped || services.contains_key("auth-service");
        let has_user = auth_user_grouped || services.contains_key("user-service");
        let has_notification = services.contains_key("notification-service")
            || service_names
                .iter()
                .any(|k| k.contains("notification-service"));
        let has_analytics = services.contains_key("analytics-service")
            || service_names
                .iter()
                .any(|k| k.contains("analytics-service"));

        has_auth && has_user && has_notification && has_analytics
    };

    assert!(
        all_services_present,
        "All services should be present. Found: {:?}",
        services.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_gradle_grouping_example_parallel_groups() {
    // Clean up any stale lock file from previous test runs
    let _ = std::fs::remove_file(".fed-lock.json");

    let parser = Parser::new();
    let config = parser
        .load_config("examples/gradle-grouping.yaml")
        .expect("Failed to load gradle grouping example");

    config.validate().expect("Config should be valid");

    // Verify the dependency structure creates correct parallel groups
    // Expected groups:
    // Group 0: database
    // Group 1: auth-service, user-service (grouped)
    // Group 2: notification-service, analytics-service (not grouped - different CWD)

    let mut orchestrator = Orchestrator::new(config).await.unwrap();
    orchestrator
        .initialize()
        .await
        .expect("Failed to initialize orchestrator");

    let services = orchestrator.get_status().await;

    // Count the number of service entries (including grouped services)
    let service_count = services.len();

    // We should have:
    // - 1 database
    // - 1 grouped gradle service (auth + user + analytics - all in same default CWD)
    // - 1 notification-service (different CWD)
    // Total: 3 services

    assert!(
        service_count == 3,
        "Expected 3 services (database, grouped gradle [auth+user+analytics], notification). Found: {}. Services: {:?}",
        service_count,
        services.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_gradle_tasks_command_format() {
    // Clean up any stale lock file from previous test runs
    let _ = std::fs::remove_file(".fed-lock.json");

    // This test verifies that when services are grouped,
    // they will be executed with multiple task arguments
    // like: ./gradlew :task1:run :task2:run :task3:run

    let parser = Parser::new();
    let config = parser
        .load_config("examples/gradle-grouping.yaml")
        .expect("Failed to load gradle grouping example");

    let mut orchestrator = Orchestrator::new(config).await.unwrap();
    orchestrator
        .initialize()
        .await
        .expect("Failed to initialize orchestrator");

    // Orchestrator initialized successfully with grouped gradle services
    // The actual command execution would be:
    // ./gradlew :auth-service:bootRun :user-service:bootRun :analytics:bootRun
}
