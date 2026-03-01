use fed::Parser;

#[test]
fn test_parse_simple_example() {
    let parser = Parser::new();
    let config = parser
        .load_config("examples/simple.yaml")
        .expect("Failed to load simple example");

    // Validate the config
    config.validate().expect("Config validation failed");

    // Check services
    assert_eq!(config.services.len(), 3);
    assert!(config.services.contains_key("backend"));
    assert!(config.services.contains_key("database"));
    assert!(config.services.contains_key("frontend"));

    // Check entrypoint
    assert_eq!(config.entrypoint, Some("frontend".to_string()));

    // Check parameters
    assert!(config.parameters.contains_key("BACKEND_PORT"));
    assert!(config.parameters.contains_key("DB_NAME"));

    // Check dependencies
    assert_eq!(config.services["frontend"].depends_on.len(), 1);
    assert_eq!(config.services["backend"].depends_on.len(), 1);
}

#[test]
fn test_parameter_types() {
    let parser = Parser::new();
    let config = parser
        .load_config("examples/simple.yaml")
        .expect("Failed to load");

    let port_param = &config.parameters["BACKEND_PORT"];
    assert!(port_param.is_port_type());

    let db_param = &config.parameters["DB_NAME"];
    assert!(!db_param.is_port_type());
    assert!(db_param.default.is_some());
}
