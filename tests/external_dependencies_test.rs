use fed::config::{BranchStrategy, Config, DependsOn};
use fed::dependency::ExternalDependencyResolver;
use std::collections::HashMap;

#[test]
fn test_parse_external_dependencies() {
    let yaml = r#"
metadata:
  team: "e-commerce"
  external_dependencies:
    - repo: "https://github.com/company/profile-service"
      description: "User profile management"
      branch_strategy:
        develop: dev
        staging: staging
        production: production
        default: main
      services:
        - name: profile_service
          description: "Main profile API"
        - name: profile_dashboard
          description: "Profile dashboard UI"

services:
  my_service:
    process: "echo hello"
    depends_on:
      - postgres
      - external: profile_service
        alias: customer_api

  postgres:
    image: "postgres:15"
"#;

    let config: Config = serde_yaml::from_str(yaml).unwrap();

    // Verify metadata
    assert!(config.metadata.is_some());
    let metadata = config.metadata.unwrap();
    assert_eq!(metadata.team.unwrap(), "e-commerce");
    assert_eq!(metadata.external_dependencies.len(), 1);

    // Verify external dependency
    let ext_dep = &metadata.external_dependencies[0];
    assert_eq!(ext_dep.repo, "https://github.com/company/profile-service");
    assert_eq!(
        ext_dep.description.as_ref().unwrap(),
        "User profile management"
    );
    assert_eq!(ext_dep.services.len(), 2);

    // Verify branch strategy
    let strategy = ext_dep.branch_strategy.as_ref().unwrap();
    assert_eq!(strategy.resolve_branch("develop"), "dev");
    assert_eq!(strategy.resolve_branch("staging"), "staging");
    assert_eq!(strategy.resolve_branch("production"), "production");
    assert_eq!(strategy.resolve_branch("unknown"), "main");

    // Verify services
    assert_eq!(ext_dep.services[0].name, "profile_service");
    assert_eq!(ext_dep.services[1].name, "profile_dashboard");

    // Verify depends_on parsing
    let my_service = config.services.get("my_service").unwrap();
    assert_eq!(my_service.depends_on.len(), 2);

    // First dependency should be simple
    assert!(my_service.depends_on[0].is_simple());
    assert_eq!(my_service.depends_on[0].service_name(), "postgres");

    // Second dependency should be external
    assert!(my_service.depends_on[1].is_external());
    assert_eq!(my_service.depends_on[1].service_name(), "profile_service");
    assert_eq!(my_service.depends_on[1].alias().unwrap(), "customer_api");
}

#[test]
fn test_branch_strategy_resolution() {
    let mut strategy = BranchStrategy {
        mappings: HashMap::new(),
        default: Some("main".to_string()),
    };

    strategy
        .mappings
        .insert("develop".to_string(), "dev".to_string());
    strategy
        .mappings
        .insert("staging".to_string(), "staging".to_string());
    strategy
        .mappings
        .insert("production".to_string(), "production".to_string());

    assert_eq!(strategy.resolve_branch("develop"), "dev");
    assert_eq!(strategy.resolve_branch("staging"), "staging");
    assert_eq!(strategy.resolve_branch("production"), "production");
    assert_eq!(strategy.resolve_branch("main"), "main");
    assert_eq!(strategy.resolve_branch("feature/test"), "main");
}

#[test]
fn test_branch_strategy_no_default() {
    let mut strategy = BranchStrategy {
        mappings: HashMap::new(),
        default: None,
    };

    strategy
        .mappings
        .insert("develop".to_string(), "dev".to_string());

    // Should use matching branch
    assert_eq!(strategy.resolve_branch("develop"), "dev");
    // Should fall back to current branch if no default
    assert_eq!(strategy.resolve_branch("feature/test"), "feature/test");
}

#[test]
fn test_collect_required_external_services() {
    let yaml = r#"
metadata:
  external_dependencies:
    - repo: "file://./profile-service"
      services:
        - name: profile_service
        - name: profile_dashboard

services:
  my_service:
    process: "echo hello"
    depends_on:
      - postgres
      - external: profile_service
        alias: customer_api
      - external: profile_dashboard

  postgres:
    image: "postgres:15"
"#;

    let config: Config = serde_yaml::from_str(yaml).unwrap();
    let resolver = ExternalDependencyResolver::new(".").unwrap();

    let required = resolver.collect_required_external_services(&config);

    assert_eq!(required.len(), 2);
    assert!(required.contains("profile_service"));
    assert!(required.contains("profile_dashboard"));
    assert!(!required.contains("postgres"));
}

#[test]
fn test_depends_on_simple() {
    let dep = DependsOn::Simple("postgres".to_string());

    assert!(dep.is_simple());
    assert!(!dep.is_external());
    assert_eq!(dep.service_name(), "postgres");
    assert!(dep.alias().is_none());
}

#[test]
fn test_depends_on_external_with_alias() {
    let dep = DependsOn::External {
        external: "profile_service".to_string(),
        alias: Some("customer_api".to_string()),
    };

    assert!(!dep.is_simple());
    assert!(dep.is_external());
    assert_eq!(dep.service_name(), "profile_service");
    assert_eq!(dep.alias().unwrap(), "customer_api");
}

#[test]
fn test_depends_on_external_without_alias() {
    let dep = DependsOn::External {
        external: "profile_service".to_string(),
        alias: None,
    };

    assert!(dep.is_external());
    assert_eq!(dep.service_name(), "profile_service");
    assert!(dep.alias().is_none());
}

#[test]
fn test_service_expose_flag() {
    let yaml = r#"
services:
  api:
    process: "npm start"
    expose: true

  database:
    image: "postgres:15"
    # expose defaults to false
"#;

    let config: Config = serde_yaml::from_str(yaml).unwrap();

    let api = config.services.get("api").unwrap();
    assert!(api.expose);

    let database = config.services.get("database").unwrap();
    assert!(!database.expose);
}

#[test]
fn test_external_service_info() {
    let yaml = r#"
metadata:
  external_dependencies:
    - repo: "file://./service"
      services:
        - name: api
          description: "Main API"
        - name: dashboard
          description: "Dashboard UI"
          alias: ui
"#;

    let config: Config = serde_yaml::from_str(yaml).unwrap();
    let metadata = config.metadata.unwrap();
    let ext_dep = &metadata.external_dependencies[0];

    assert_eq!(ext_dep.services.len(), 2);
    assert_eq!(ext_dep.services[0].name, "api");
    assert_eq!(
        ext_dep.services[0].description.as_ref().unwrap(),
        "Main API"
    );

    assert_eq!(ext_dep.services[1].name, "dashboard");
    assert_eq!(ext_dep.services[1].alias.as_ref().unwrap(), "ui");
}
