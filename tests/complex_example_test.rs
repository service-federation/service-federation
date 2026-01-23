use service_federation::Parser;

#[test]
fn test_parse_complex_dependencies() {
    let parser = Parser::new();
    let config = parser
        .load_config("examples/complex-dependencies.yaml")
        .expect("Failed to load complex-dependencies example");

    // Validate the config
    config.validate().expect("Config validation failed");

    // Check services
    assert!(config.services.contains_key("main-app"));
    assert!(config.services.contains_key("middleware"));
    assert!(config.services.contains_key("auth-external"));
    assert!(config.services.contains_key("analytics"));
    assert!(config.services.contains_key("billing"));

    // Check external dependencies
    assert!(config.dependencies.contains_key("middleware-service"));
    assert!(config.dependencies.contains_key("auth-service"));

    // Check that middleware is an external service
    let middleware = &config.services["middleware"];
    assert_eq!(
        middleware.dependency.as_ref().unwrap(),
        "middleware-service"
    );

    // Check entrypoint
    assert_eq!(config.entrypoint, Some("main-app".to_string()));

    // Check that main-app has HTTP health check
    let main_app = &config.services["main-app"];
    assert!(main_app.healthcheck.is_some());
}

#[test]
fn test_external_service_params() {
    let parser = Parser::new();
    let config = parser
        .load_config("examples/complex-dependencies.yaml")
        .expect("Failed to load");

    let middleware = &config.services["middleware"];

    // Check that parameters are set
    assert!(middleware.parameters.contains_key("PORT"));
    assert!(middleware.parameters.contains_key("AUTH_URL"));
}
