use crate::output::UserOutput;
use std::path::Path;

const TEMPLATE: &str = r#"# Service Federation Configuration
# Documentation: https://github.com/service-federation/fed

parameters:
  # Port parameters with preferred defaults
  API_PORT:
    type: port
    default: 8080  # Prefers 8080, falls back if unavailable

  DB_PORT:
    type: port
    default: 5432

  # String parameters
  DB_NAME:
    default: myapp_dev

  DB_USER:
    default: postgres

services:
  # Core services (always run)
  database:
    image: postgres:15-alpine
    ports: ["{{DB_PORT}}:5432"]
    environment:
      POSTGRES_DB: '{{DB_NAME}}'
      POSTGRES_USER: '{{DB_USER}}'
      POSTGRES_PASSWORD: 'password'
    healthcheck:
      command: 'pg_isready -U {{DB_USER}}'

  backend:
    process: npm start
    cwd: ./backend
    depends_on: [database]
    environment:
      PORT: '{{API_PORT}}'
      DATABASE_URL: 'postgres://{{DB_USER}}:password@localhost:{{DB_PORT}}/{{DB_NAME}}'
    healthcheck:
      httpGet: 'http://localhost:{{API_PORT}}/health'

  frontend:
    process: npm run dev
    cwd: ./frontend
    depends_on: [backend]
    environment:
      BACKEND_URL: 'http://localhost:{{API_PORT}}'

  # Development-only services
  # Start with: fed start --profile development
  dev-tools:
    profiles: [development]
    process: npm run dev-server
    cwd: ./dev-tools
    depends_on: [backend]

  # Debugging services
  # Start with: fed start --profile debug
  debugger:
    profiles: [debug, development]
    process: npm run debug-proxy
    cwd: ./tools
    depends_on: [backend]

  # Production-like services
  # Start with: fed start --profile production
  monitoring:
    profiles: [production]
    image: prom/prometheus:latest
    ports: ["9090:9090"]
    depends_on: [backend]

  # Analytics (runs in production OR staging)
  # Start with: fed start --profile production OR fed start --profile staging
  analytics:
    profiles: [production, staging]
    image: grafana/grafana:latest
    ports: ["3000:3000"]
    depends_on: [backend]

# Entrypoint - which service to start by default
entrypoint: frontend

# Profile usage examples:
# fed start                           # Minimal: core services only (no profiles)
# fed start --profile development     # Full dev: core + dev-tools + debugger
# fed start --profile debug           # Debug: core + debugger
# fed start --profile production      # Production-like: core + monitoring + analytics
# fed start --profile staging         # Staging: core + analytics
# fed start -p development -p debug   # Multiple profiles: core + dev-tools + debugger

# Optional: Scripts for common tasks
scripts:
  test:
    depends_on: [backend]
    environment:
      API_URL: 'http://localhost:{{API_PORT}}'
    script: |
      npm test

  seed:
    depends_on: [database]
    script: |
      psql postgres://{{DB_USER}}:password@localhost:{{DB_PORT}}/{{DB_NAME}} -f seed.sql
"#;

pub fn run_init(output: &Path, force: bool, out: &dyn UserOutput) -> anyhow::Result<()> {
    // Check if file exists and force flag not set
    if output.exists() && !force {
        out.error(&format!("Error: {} already exists", output.display()));
        out.error("Use --force to overwrite");
        return Err(anyhow::anyhow!("File already exists"));
    }

    std::fs::write(output, TEMPLATE)?;
    out.success(&format!("Created {}", output.display()));
    out.status("\nNext steps:");
    out.status(&format!(
        "  1. Edit {} to match your services",
        output.display()
    ));
    out.status("  2. Add .fed/ to .gitignore");
    out.status("  3. Run: fed start");

    Ok(())
}
