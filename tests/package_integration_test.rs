use service_federation::{PackageResolver, Parser};
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_local_package_resolution() {
    // Create a temporary directory structure
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a package directory with a service-federation.yaml
    let package_dir = root.join("database-package");
    fs::create_dir(&package_dir).unwrap();
    fs::write(
        package_dir.join("service-federation.yaml"),
        r#"
services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_USER: admin
      POSTGRES_PASSWORD: secret
    ports:
      - "5432:5432"
"#,
    )
    .unwrap();

    // Create a main config that uses the package
    let main_config_path = root.join("service-federation.yaml");
    fs::write(
        &main_config_path,
        r#"
packages:
  - source: "./database-package"
    as: "db-pkg"

services:
  database:
    extends: "db-pkg.postgres"
    environment:
      POSTGRES_DB: "myapp"
"#,
    )
    .unwrap();

    // Parse the main config
    let parser = Parser::new();
    let config = parser.load_config(&main_config_path).unwrap();

    // Verify packages field is parsed
    assert_eq!(config.packages.len(), 1);
    assert_eq!(config.packages[0].source, "./database-package");
    assert_eq!(config.packages[0].r#as, "db-pkg");

    // Verify service has extends field
    assert_eq!(config.services.len(), 1);
    let database_service = config.services.get("database").unwrap();
    assert_eq!(database_service.extends.as_deref(), Some("db-pkg.postgres"));
    assert_eq!(
        database_service.environment.get("POSTGRES_DB").unwrap(),
        "myapp"
    );

    // Resolve packages
    let mut resolver = PackageResolver::new(root).unwrap();
    let packages = resolver.resolve_all(&config.packages).await.unwrap();

    // Verify package was resolved
    assert_eq!(packages.len(), 1);
    assert!(packages.contains_key("db-pkg"));

    let db_package = packages.get("db-pkg").unwrap();
    assert_eq!(db_package.alias, "db-pkg");
    assert!(db_package.config.services.contains_key("postgres"));

    let postgres_service = db_package.config.services.get("postgres").unwrap();
    assert_eq!(postgres_service.image.as_deref(), Some("postgres:15"));
    assert_eq!(
        postgres_service.environment.get("POSTGRES_USER").unwrap(),
        "admin"
    );
}

#[tokio::test]
async fn test_multiple_packages() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create first package
    let db_package_dir = root.join("db-package");
    fs::create_dir(&db_package_dir).unwrap();
    fs::write(
        db_package_dir.join("service-federation.yaml"),
        r#"
services:
  postgres:
    image: postgres:15
"#,
    )
    .unwrap();

    // Create second package
    let cache_package_dir = root.join("cache-package");
    fs::create_dir(&cache_package_dir).unwrap();
    fs::write(
        cache_package_dir.join("service-federation.yaml"),
        r#"
services:
  redis:
    image: redis:7
"#,
    )
    .unwrap();

    // Create main config using both packages
    let main_config_path = root.join("service-federation.yaml");
    fs::write(
        &main_config_path,
        r#"
packages:
  - source: "./db-package"
    as: "db"
  - source: "./cache-package"
    as: "cache"

services:
  database:
    extends: "db.postgres"

  redis:
    extends: "cache.redis"
"#,
    )
    .unwrap();

    // Parse config
    let parser = Parser::new();
    let config = parser.load_config(&main_config_path).unwrap();

    // Resolve packages
    let mut resolver = PackageResolver::new(root).unwrap();
    let packages = resolver.resolve_all(&config.packages).await.unwrap();

    // Verify both packages were resolved
    assert_eq!(packages.len(), 2);
    assert!(packages.contains_key("db"));
    assert!(packages.contains_key("cache"));
}

#[tokio::test]
async fn test_config_without_packages_still_works() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a config without packages field (backward compatibility)
    let config_path = root.join("service-federation.yaml");
    fs::write(
        &config_path,
        r#"
services:
  web:
    process: "npm start"
    cwd: "."
"#,
    )
    .unwrap();

    // Parse config
    let parser = Parser::new();
    let config = parser.load_config(&config_path).unwrap();

    // Verify it works without packages
    assert_eq!(config.packages.len(), 0);
    assert_eq!(config.services.len(), 1);
    assert!(config.services.contains_key("web"));
}
