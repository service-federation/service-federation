# Service Federation (fed)

**Local development orchestration with session-based port stability.**

`fed` runs Docker containers, native processes, and Gradle tasks together with dependency resolution and port allocation. Sessions remember your ports across restarts.

## Quick Start

### Install

```bash
cargo install service-federation
```

### Create `service-federation.yaml`

```yaml
parameters:
  API_PORT:
    type: port
    default: 8080
  DB_PORT:
    type: port
    default: 5432

services:
  database:
    image: postgres:15
    ports: ["{{DB_PORT}}:5432"]
    environment:
      POSTGRES_PASSWORD: password
    healthcheck:
      command: pg_isready -U postgres

  backend:
    process: npm start
    cwd: ./backend
    depends_on: [database]
    environment:
      PORT: '{{API_PORT}}'
      DATABASE_URL: 'postgres://localhost:{{DB_PORT}}/db'
    healthcheck:
      httpGet: 'http://localhost:{{API_PORT}}/health'

  frontend:
    process: npm run dev
    cwd: ./frontend
    depends_on: [backend]
    environment:
      BACKEND_URL: 'http://localhost:{{API_PORT}}'

entrypoint: frontend
```

### Run

```bash
fed start            # Start all services (follows dependencies)
fed ports randomize  # Allocate fresh random ports
fed ports reset      # Clear allocations, use defaults
fed status           # Check status
fed logs backend     # View logs
fed stop             # Stop all
```

## Configuration

### Services

Services are the core of your config. Each service specifies one of four types:

**Process** — run any command:
```yaml
services:
  api:
    process: npm start
    cwd: ./api
```

**Docker Container** — run a Docker image:
```yaml
services:
  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]
```

**Docker Compose Service** — reuse existing docker-compose.yml:
```yaml
services:
  postgres:
    composeFile: ./docker-compose.yml
    composeService: postgres
```

**Gradle Task** — tasks sharing a `cwd` are batched into one gradle command:
```yaml
services:
  auth-service:
    gradleTask: ':auth:bootRun'
    cwd: ./backend
    environment:
      SERVER_PORT: '{{AUTH_PORT}}'

  user-service:
    gradleTask: ':user:bootRun'
    cwd: ./backend  # Same dir → runs as: gradle :auth:bootRun :user:bootRun
    environment:
      SERVER_PORT: '{{USER_PORT}}'
```

### Parameters & Templating

Parameters define values that can be referenced throughout your config with `{{PARAM}}`:

```yaml
parameters:
  API_PORT:
    type: port
    default: 8080  # Tries 8080, falls back to random available

  DB_PORT:
    type: port     # Always random available port

  API_KEY:
    default: "dev-key"  # String parameter
```

Use in services:
```yaml
services:
  api:
    environment:
      PORT: '{{API_PORT}}'
      DATABASE_URL: 'postgres://localhost:{{DB_PORT}}/db'
      API_KEY: '{{API_KEY}}'
```

### Dependencies & Health Checks

Services declare dependencies with `depends_on`. A service waits for its dependencies to become healthy before starting:

```yaml
services:
  database:
    image: postgres:15
    healthcheck:
      command: pg_isready -U postgres  # Runs INSIDE the container
      timeout: 10s

  api:
    process: npm start
    depends_on: [database]
    healthcheck:
      httpGet: 'http://localhost:{{API_PORT}}/health'
      timeout: 5s  # Optional, default 30s

  frontend:
    depends_on: [api]
```

**Health check types:**
- `httpGet` — HTTP request from the host. Use for web services.
- `command` — shell command. For **Docker services**, runs inside the container via `docker exec`. For **process services**, runs on the host.

### Scripts

Scripts are automation commands that can depend on services or other scripts:

```yaml
scripts:
  db:migrate:
    depends_on: [database]
    script: npx prisma db push

  test:integration:
    depends_on: [db:migrate, api]
    environment:
      DATABASE_URL: postgres://localhost/test
    script: npm run test:e2e -- "$@"  # "$@" receives arguments passed after --
```

**Run scripts:**
```bash
fed run db:migrate                    # Run a script
fed db:migrate                        # Shorthand (if no command collision)
fed test:integration -- -t "auth"     # Pass arguments after --
```

Services started as script dependencies continue running after the script completes. Use `fed stop` to stop them.

### Environment Files

`.env` files set parameter values (not service environment directly):

```yaml
parameters:
  API_KEY:
    default: ""
  DATABASE_URL:
    default: "postgres://localhost:5432/dev"

env_file:
  - .env         # API_KEY=secret123
  - .env.local   # Later files override earlier
```

All variables in `.env` files **must be declared** as parameters — undeclared variables cause an error.

**Priority (highest to lowest):**
1. Explicit parameter `value:` field
2. Values from `env_file`
3. Parameter `default:` value

See [`examples/env-file/`](./examples/env-file) for a complete example.

## Features

### Sessions

Sessions remember port allocations across restarts:

```bash
fed session start --id my-project   # Creates .fed/session file
fed start postgres                  # postgres gets port 5432
fed stop postgres
fed start postgres                  # postgres gets 5432 again (remembered)
fed session end                     # Clean up
```

Sessions are auto-detected from `.fed/session` in your project directory. Add `.fed/` to `.gitignore`.

To use a session across multiple directories, export the env var:
```bash
export FED_SESSION=my-project
```

### Startup Messages

Show where to access services after startup:

```yaml
services:
  api:
    process: npm start
    startup_message: "API docs: http://localhost:{{API_PORT}}/docs"
  frontend:
    process: npm run dev
    startup_message: "App running on http://localhost:{{NEXT_PORT}}"
```

After all services start, messages are printed in a box. Entrypoint services sort last so the primary URL is the visual punchline:

```
╭──────────────────────────────────────────────────╮
│ API docs: http://localhost:8081/docs             │
├──────────────────────────────────────────────────┤
│ App running on http://localhost:3000             │
╰──────────────────────────────────────────────────╯
```

Especially useful with `fed ports randomize` where ports are randomized. A warning is emitted if an entrypoint service has no `startup_message`.

### Isolated Test Execution

Run tests with fresh ports and volumes while your dev stack keeps running:

```yaml
scripts:
  test:integration:
    isolated: true     # Fresh ports, scoped volumes, isolated services
    depends_on: [database, api]
    script: npm run test:e2e
```

```bash
# Terminal 1: Dev stack on default ports
fed start

# Terminal 2: Tests on isolated random ports
fed test:integration
```

Creates a child orchestrator with fresh port allocations, scopes Docker volumes by session, runs the script, then cleans up everything.

### Lifecycle Hooks

Manage setup, build, and cleanup per service:

```yaml
services:
  backend:
    process: npm start
    cwd: ./backend
    install: npm ci                   # Run before first start (tracked)
    build: npm run build              # Run with `fed build`
    clean: rm -rf node_modules dist   # Run with `fed clean`
```

- **Install** — runs before the service's first start. Re-run with `fed install`. Cleared by `fed clean`.
- **Build** — runs with `fed build`. Always runs (not tracked).
- **Clean** — runs custom cleanup. Also removes Docker volumes with `fed-` prefix. Bind mounts are not removed.

### Service Templates

Define base configurations to extend:

```yaml
templates:
  java-service:
    image: openjdk:17-slim
    environment:
      JAVA_OPTS: '-Xmx512m'
    healthcheck:
      httpGet: 'http://localhost:{{PORT}}/actuator/health'
    restart: always

services:
  auth-service:
    extends: java-service
    ports: ["8080:8080"]
    environment:
      PORT: '8080'

  user-service:
    extends: java-service
    ports: ["8081:8081"]
    environment:
      PORT: '8081'
```

See [`examples/templates-example.yaml`](./examples/templates-example.yaml).

### Profiles

Conditionally include services based on active profiles:

```yaml
services:
  api:
    process: npm start
    # No profiles = always included when no profiles are active

  worker:
    profiles: [worker]  # Only started when 'worker' profile is active

  debug-tools:
    image: debug:latest
    profiles: [debug, development]  # Started when either profile is active
```

```bash
fed start                        # Starts only profileless services (api)
fed start -p worker              # Starts api + worker
fed start -p worker -p debug     # Starts api + worker + debug-tools
```

### Packages

Import and reuse service configurations across projects:

```yaml
packages:
  - source: "./packages/database"    # Local path
    as: "db-pkg"
  - source: "github:org/repo@v1.0"   # GitHub (tag, branch, or commit)
    as: "infra"
  - source: "git+ssh://host/path"    # Git SSH
    as: "shared"

services:
  database:
    extends: "db-pkg.postgres"       # Inherit from package service
    environment:
      POSTGRES_DB: "myapp"           # Override specific fields
    ports:
      - "5433:5432"
```

Packages are cached locally. Use `fed package refresh` to re-fetch and `fed --offline start` to skip all git operations.

See [`examples/service-merging/`](./examples/service-merging) for a complete example.

### Resource Limits

Limit memory, CPU, and processes:

```yaml
services:
  database:
    image: postgres:15
    resources:
      memory: 512m
      cpus: "2.0"
      pids: 100
      nofile: 1024

  backend:
    process: npm start
    resources:
      memory: 1g              # Supports: b, k/kb, m/mb, g/gb, t/tb
      cpus: "1.0"
      pids: 50
```

**Docker services** support additional options: `memory_reservation`, `memory_swap`, `cpu_shares`.

**Process services** (Unix/Linux) use: `RLIMIT_AS` (memory), `RLIMIT_NPROC` (pids, Linux), `RLIMIT_NOFILE` (nofile).

### Watch Mode

Auto-restart services on file changes:

```bash
fed start -w    # Start with watch mode (foreground)
```

### Circuit Breaker

Prevent crash loops for services with `restart: always`:

```yaml
services:
  flaky-service:
    process: ./might-crash.sh
    restart: always
    circuit_breaker:
      restart_threshold: 5  # Max restarts within window before tripping (default: 5)
      window_secs: 60       # Rolling window in seconds (default: 60)
      cooldown_secs: 300    # Seconds before allowing retry (default: 300)
```

### Graceful Shutdown

Control how long to wait before force-killing:

```yaml
services:
  api:
    process: npm start
    grace_period: 30s  # Default: 10s
```

### Service Tags

Tag services for grouped commands:

```yaml
services:
  api:
    tags: [backend, critical]
  worker:
    tags: [backend, async]
  frontend:
    tags: [frontend]
```

```bash
fed start @backend    # Start all services tagged 'backend'
fed stop @critical    # Stop all tagged 'critical'
```

## Commands

```bash
# Service management
fed start                    # Start all services in background
fed start -w                 # Start with watch mode (foreground, auto-restart on changes)
fed start postgres redis     # Start specific services
fed start --replace          # Kill processes occupying required ports, then start
fed start --dry-run          # Preview what would happen without starting
fed start --output file      # Output mode: file (background), captured (memory), passthrough (stdio)
fed stop                     # Stop all services
fed stop postgres            # Stop specific service
fed restart backend          # Restart specific service
fed status                   # Show service status
fed status --json            # JSON output for scripting
fed logs backend --tail 50   # View service logs
fed logs backend --follow    # Stream logs (Ctrl+C to stop)
fed top                      # Show resource usage (CPU, memory, PID)

# Port management
fed ports list               # Show current port allocations
fed ports list --json        # JSON output for scripting
fed ports randomize          # Allocate fresh random ports for all port parameters
fed ports randomize -f       # Skip confirmation, auto-stop running services
fed ports reset              # Clear allocations (next start uses defaults)
fed ports reset -f           # Skip confirmation, auto-stop running services

# Build lifecycle
fed install                  # Run install commands for all services
fed install backend          # Run install for specific service
fed build                    # Run build commands for all services
fed build backend            # Build specific service
fed clean                    # Run clean commands and remove Docker volumes
fed clean backend            # Clean specific service

# Configuration
fed init                     # Create starter service-federation.yaml
fed validate                 # Validate config without starting
fed doctor                   # Check system requirements (Docker, Gradle, Java, etc.)

# Sessions
fed session start --id dev   # Start named session
fed session list             # List all sessions
fed session end              # End current session
fed session cleanup          # Clean up orphaned sessions

# Packages
fed package list             # List cached packages
fed package list --json      # JSON output
fed package refresh          # Re-fetch all packages used in current config
fed package refresh <source> # Re-fetch a specific package
fed package clear            # Remove entire package cache
fed package clear -f         # Skip confirmation

# Scripts & shell
fed run test                 # Run script from config
fed test                     # Shorthand (if no command collision)
fed test -- -t "specific"    # Pass arguments to script
fed completions bash         # Generate shell completions (bash/zsh/fish)
fed tui                      # Interactive TUI (beta)

# Global flags
fed --config dev.yaml start  # Use specific config file
fed --offline start          # Skip git operations, use cached packages only
fed --workdir ./project start # Set working directory
fed --env staging start      # Set environment (default: development)
fed --profile worker start   # Activate a profile (repeatable)
```

## Examples

See the [`examples/`](./examples) directory:

- [`simple.yaml`](./examples/simple.yaml) — Basic multi-service setup
- [`scripts-example.yaml`](./examples/scripts-example.yaml) — Scripts with dependencies and argument passthrough
- [`env-file/`](./examples/env-file) — Environment file (.env) support
- [`templates-example.yaml`](./examples/templates-example.yaml) — Reusable service templates
- [`resource-limits-example.yaml`](./examples/resource-limits-example.yaml) — Resource limits
- [`docker-compose-example/`](./examples/docker-compose-example) — Integrating with existing docker-compose
- [`gradle-grouping.yaml`](./examples/gradle-grouping.yaml) — Gradle task batching
- [`complex-dependencies/`](./examples/complex-dependencies) — Multi-level dependency graph
- [`profiles-example.yaml`](./examples/profiles-example.yaml) — Environment-specific configs
- [`service-merging/`](./examples/service-merging) — Importing and extending package services

## Troubleshooting

**Services not starting?**
```bash
fed logs <service-name> --tail 100
```

**Port conflicts?**
```yaml
parameters:
  API_PORT:
    type: port
    default: 8080  # Auto-falls back if in use
```

**Stuck sessions?**
```bash
fed session cleanup
```

## Contributing

Issues and PRs welcome! This is an early-stage project.

## License

MIT
