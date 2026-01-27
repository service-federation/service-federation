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
    default: 8080  # Prefers 8080, falls back if unavailable

services:
  database:
    image: postgres:15
    ports: ["5432:5432"]
    environment:
      POSTGRES_PASSWORD: password

  backend:
    process: npm start
    cwd: ./backend
    depends_on: [database]
    environment:
      PORT: '{{API_PORT}}'
      DATABASE_URL: 'postgres://localhost:5432/db'
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
# Start all services (follows dependencies)
fed start

# Check status
fed status

# View logs
fed logs backend --tail 50

# Stop all
fed stop
```

## Sessions

Sessions remember port allocations across restarts.

```bash
# Start a session (creates .fed/session file)
fed session start --id my-project

# Start postgres
fed start postgres
# → postgres gets port 5432

# Stop postgres
fed stop postgres

# Start postgres again
fed start postgres
# → postgres gets 5432 again

# Clean up when done
fed session end
```

Sessions are auto-detected from `.fed/session` in your project directory.

## Service Types

### Process
Run any command:
```yaml
services:
  api:
    process: npm start
    cwd: ./api
```

### Docker Container
```yaml
services:
  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]
```

### Docker Compose Service
Reuse existing docker-compose.yml:
```yaml
services:
  postgres:
    composeFile: ./docker-compose.yml
    composeService: postgres
```

### Gradle Task
Groups tasks by directory:
```yaml
# Same cwd → batched into one gradle command
services:
  auth-service:
    gradleTask: ':auth:bootRun'
    cwd: ./backend
    environment:
      SERVER_PORT: '{{AUTH_PORT}}'

  user-service:
    gradleTask: ':user:bootRun'
    cwd: ./backend  # Same working dir → runs with auth-service
    environment:
      SERVER_PORT: '{{USER_PORT}}'

  payment-service:
    gradleTask: ':payment:bootRun'
    cwd: ./backend  # Same working dir → all three run as: gradle :auth:bootRun :user:bootRun :payment:bootRun
    environment:
      SERVER_PORT: '{{PAYMENT_PORT}}'
```

Gradle initializes once instead of per-task.

## Key Features

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
    extends: java-service  # Inherits all template fields
    ports: ["8080:8080"]
    environment:
      PORT: '8080'  # Add or override specific values

  user-service:
    extends: java-service  # Reuse same template
    ports: ["8081:8081"]
    environment:
      PORT: '8081'
```

See [`examples/templates-example.yaml`](./examples/templates-example.yaml) for a complete example.

### Lifecycle Hooks (Install, Build, Clean)

Manage service setup, build, and cleanup with lifecycle hooks:

```yaml
services:
  backend:
    process: npm start
    cwd: ./backend
    install: npm ci                   # Run before first start
    build: npm run build              # Run with `fed build`
    clean: rm -rf node_modules dist   # Run with `fed clean`
    environment:
      PORT: '{{API_PORT}}'

  database:
    image: postgres:15
    volumes:
      - "fed-db-data:/var/lib/postgresql/data"  # Named volume
```

**Install hooks:**
- Run before the service's first start (tracked to avoid re-running)
- Run manually with `fed install` (forces re-run)
- Cleared by `fed clean` (install will run again on next start)

**Build hooks:**
- Run with `fed build` (always runs, not tracked)
- Useful for compiling code, bundling assets, generating artifacts
- Runs in service's `cwd` with its `environment` variables

**Clean hooks:**
- Run custom cleanup commands (remove build artifacts, caches, etc.)
- Removes Docker volumes with `fed-` prefix
- Bind mounts (e.g., `./data:/data`) are NOT removed (only named volumes)
- Only `fed-*` prefixed Docker volumes are removed to prevent accidental data loss

### Automatic Port Allocation

```yaml
parameters:
  API_PORT:
    type: port
    default: 8080  # Tries 8080, falls back to random available

  DB_PORT:
    type: port  # Always random available port
```

Use in services:
```yaml
services:
  api:
    environment:
      PORT: '{{API_PORT}}'
      DATABASE_URL: 'postgres://localhost:{{DB_PORT}}/db'
```

### Startup Messages

Show where to access services after startup using template-resolved messages:

```yaml
services:
  api:
    process: npm start
    startup_message: "API docs: http://localhost:{{API_PORT}}/docs"
  next:
    process: npm run dev
    startup_message: "Application running on http://localhost:{{NEXT_PORT}}"
```

After all services start, messages are printed in a box. Entrypoint services sort last so the primary URL is always the visual punchline:

```
╭──────────────────────────────────────────────────╮
│ API docs: http://localhost:8081/docs              │
├──────────────────────────────────────────────────┤
│ Application running on http://localhost:3000      │
╰──────────────────────────────────────────────────╯
```

With `--isolated` mode (randomized ports), this is the easiest way to find the assigned URLs. A warning is emitted if an entrypoint service has no `startup_message`.

### Dependency Management

```yaml
services:
  database:
    process: postgres -D data

  backend:
    depends_on: [database]  # Waits for database to be healthy

  frontend:
    depends_on: [backend]  # Waits for backend
```

### Scripts

Scripts are automation commands that can depend on services or other scripts:

```yaml
scripts:
  db:migrate:
    depends_on: [database]
    script: npx prisma db push

  test:integration:
    depends_on: [db:migrate, api]  # Can depend on scripts AND services
    environment:
      DATABASE_URL: postgres://localhost/test
    script: npm run test:e2e
```

**Run scripts:**
```bash
fed run db:migrate           # Run a script
fed db:migrate               # Shorthand (if no command collision)
fed test:integration -- -t "auth"  # Pass arguments after --
```

**Receiving arguments in scripts:** Use `"$@"` to receive arguments passed after `--`:

```yaml
scripts:
  test:integration:
    script: npm run test:integration -- "$@"  # Passes args to npm
```

Then `fed test:integration -- --run file.ts` becomes `npm run test:integration -- --run file.ts`.

**Important**: Services started as script dependencies continue running after the script completes. To stop them, use `fed stop` manually.

### Isolated Test Execution

Run tests with fresh ports and volumes while your dev stack is running:

```yaml
scripts:
  test:integration:
    isolated: true     # Fresh ports, scoped volumes, isolated services
    depends_on: [database, api]
    script: npm run test:e2e
```

**How it works:**
1. Creates a child orchestrator with fresh port allocations
2. Scopes Docker volumes by session (`myvolume` → `fed-{session}-myvolume`)
3. Starts dependencies on the new isolated ports
4. Resolves environment variables with the new ports
5. Runs the script
6. Cleans up all services and scoped volumes after completion

**Use case:**
```bash
# Terminal 1: Dev stack on default ports
fed start

# Terminal 2: Tests on isolated random ports (doesn't affect dev stack)
fed test:integration
```

Run tests without stopping your dev stack.

### Environment Files (.env)

Use `.env` files to provide parameter values without modifying your config file:

```yaml
parameters:
  API_KEY:
    default: ""  # Declare the parameter
  DATABASE_URL:
    default: "postgres://localhost:5432/dev"

# .env files set parameter values (variables MUST be declared above)
env_file:
  - .env           # API_KEY=secret123
  - .env.local     # Later files override earlier

services:
  api:
    environment:
      # Reference parameters (values come from .env via parameters)
      API_KEY: '{{API_KEY}}'
      DATABASE_URL: '{{DATABASE_URL}}'
      PORT: '{{API_PORT}}'
```

**How it works:**
- `.env` files set **parameter values**, not service environment directly
- All variables in `.env` files **must be declared** as parameters in your config
- Undeclared variables cause an error (prevents typos and hidden configuration)
- All configuration lives in `service-federation.yaml`

**Priority (highest to lowest):**
1. Explicit parameter `value:` field
2. Values from `env_file`
3. Parameter `default:` value

**Features:**
- Supports standard `.env` format (KEY=VALUE, comments with #, quoting)
- Multiple files loaded in order (later overrides earlier)
- Fail-fast on missing files and undeclared variables
- Useful for secrets that shouldn't be in version control

See [`examples/env-file/`](./examples/env-file) for a complete example.

### Health Checks

```yaml
services:
  api:
    healthcheck:
      httpGet: 'http://localhost:{{API_PORT}}/health'
      timeout: 5s  # Optional, default 30s

  database:
    image: postgres:15
    healthcheck:
      command: pg_isready -U postgres  # Runs INSIDE the container
      timeout: 10s
```

**Health check types:**
- `httpGet`: HTTP request from host to check if service is responding
- `command`: Shell command to verify service health

**Important behavior:**
- For **Docker services**, `command` health checks run **inside the container** via `docker exec`
- For **process services**, `command` health checks run on the host
- `httpGet` always runs from the host

### Resource Limits

Limit memory, CPU, and processes:

```yaml
services:
  database:
    image: postgres:15
    resources:
      memory: 512m              # Max memory usage
      cpus: "2.0"               # Max CPU cores
      pids: 100                 # Max number of processes/threads
      nofile: 1024              # Max open file descriptors

  backend:
    process: npm start
    cwd: ./backend
    resources:
      memory: 1g                # Supports: b, k/kb, m/mb, g/gb, t/tb
      cpus: "1.0"               # Decimal format: "0.5", "2.0", etc.
      pids: 50                  # Prevent fork bombs

  # Docker-specific resource options
  worker:
    image: worker:latest
    resources:
      memory: 256m
      memory_reservation: 128m  # Soft limit (Docker only)
      memory_swap: 512m         # Swap limit (Docker only)
      cpu_shares: 512           # Relative CPU weight (Docker only)
```

**Docker Services:**
- `memory`: Hard memory limit via `--memory`
- `memory_reservation`: Soft limit via `--memory-reservation`
- `memory_swap`: Swap limit via `--memory-swap`
- `cpus`: CPU limit via `--cpus`
- `cpu_shares`: Relative weight via `--cpu-shares`
- `pids`: Process limit via `--pids-limit`
- `nofile`: File descriptor limit via `--ulimit nofile`

**Process Services (Unix/Linux):**
- `memory`: Address space limit via `RLIMIT_AS`
- `pids`: Process limit via `RLIMIT_NPROC` (Linux)
- `nofile`: File descriptor limit via `RLIMIT_NOFILE`

**Memory Format:**
- Bytes: `4096`, `8192b`
- Kilobytes: `512k`, `512kb`
- Megabytes: `256m`, `256mb` (recommended)
- Gigabytes: `2g`, `2gb`
- Terabytes: `1t`, `1tb`
- Decimals: `1.5g` (1.5 gigabytes)

**Without limits:**
- Services can allocate unlimited memory
- Process-based services can fork indefinitely
- Docker containers can consume all machine resources

**With limits:**
- Services fail cleanly when hitting limits
- Other services remain stable
- Laptop won't freeze from runaway processes

### Profiles

Conditionally include services based on active profiles:

```yaml
services:
  api:
    process: npm start
    # No profiles = always included when no profiles are active

  worker:
    process: npm run worker
    profiles: [worker]  # Only started when 'worker' profile is active

  debug-tools:
    image: debug:latest
    profiles: [debug, development]  # Started when 'debug' OR 'development' profile is active
```

**Usage:**
```bash
fed start                        # Starts only services without profiles (api)
fed start -p worker              # Starts api + worker
fed start -p worker -p debug     # Starts api + worker + debug-tools
fed start --profile development  # Starts api + debug-tools
```

**Profile behavior:**
- Services **without** `profiles` are always started when no profiles are active
- Services **with** `profiles` are only started when at least one matching profile is active
- Multiple `-p`/`--profile` flags can be combined

### Circuit Breaker

Limit restarts to prevent crash loops:

```yaml
services:
  flaky-service:
    process: ./might-crash.sh
    restart: always
    circuit_breaker:
      threshold: 5        # Max restarts before tripping
      cooldown: 60s       # Wait time before allowing restarts again
```

When a service restarts more than `threshold` times, the circuit breaker "trips" and prevents further restarts until `cooldown` expires. This prevents infinite crash loops from consuming resources.

### Graceful Shutdown

Control how long to wait before force-killing a service:

```yaml
services:
  api:
    process: npm start
    grace_period: 30s  # Wait 30s for graceful shutdown before SIGKILL (default: 10s)
```

### Service Tags

Tag services for grouping:

```yaml
services:
  api:
    process: npm start
    tags: [backend, critical]

  worker:
    process: npm run worker
    tags: [backend, async]

  frontend:
    process: npm run dev
    tags: [frontend]
```

**Usage:**
```bash
fed start @backend    # Start all services tagged 'backend' (api, worker)
fed stop @critical    # Stop all services tagged 'critical' (api)
```

## Commands

```bash
# Service management
fed start                    # Start all services in background (default)
fed start -w                 # Start with watch mode (foreground, auto-restart on file changes)
fed start postgres redis     # Start specific services
fed start --replace          # Kill processes occupying required ports, then start
fed start --dry-run          # Preview what would happen without starting
fed start --output file      # Output mode: file (background), captured (memory), passthrough (stdio)
fed stop                     # Stop all services
fed stop postgres            # Stop specific service
fed restart backend          # Restart specific service
fed status                   # Show service status
fed status --json            # Show service status as JSON (for scripting)
fed logs backend --tail 50   # View service logs
fed logs backend --follow    # Stream logs (Ctrl+C to stop)
fed top                      # Show resource usage (CPU, memory, PID)

# Build lifecycle
fed install                  # Run install commands for all services
fed install backend          # Run install for specific service
fed build                    # Run build commands for all services
fed build backend            # Build specific service
fed clean                    # Run clean commands and remove Docker volumes
fed clean backend            # Clean specific service

# Configuration management
fed init                     # Create starter service-federation.yaml
fed validate                 # Validate config without starting services
fed doctor                   # Check system requirements (Docker, Gradle, Java, etc.)
fed port backend             # Quickly show port for a service

# Session management
fed session start --id dev   # Start named session
fed session list             # List all sessions
fed session end              # End current session
fed session cleanup          # Clean up orphaned sessions

# Developer experience
fed completions bash         # Generate shell completions (bash/zsh/fish)
fed tui                      # Interactive TUI (beta)
fed run test                 # Run script from config
fed test                     # Shorthand for scripts (if no command collision)
fed test -- -t "specific"   # Pass arguments to script
fed --config dev.yaml start  # Use specific config file
```

## Real-World Examples

See the [`examples/`](./examples) directory for complete configurations:

- [`simple.yaml`](./examples/simple.yaml) - Basic multi-service setup
- [`scripts-example.yaml`](./examples/scripts-example.yaml) - Scripts with dependencies and argument passthrough
- [`env-file/`](./examples/env-file) - Environment file (.env) support
- [`templates-example.yaml`](./examples/templates-example.yaml) - Reusable service templates
- [`resource-limits-example.yaml`](./examples/resource-limits-example.yaml) - Resource limits
- [`docker-compose-example/`](./examples/docker-compose-example) - Integrating with existing docker-compose
- [`gradle-grouping.yaml`](./examples/gradle-grouping.yaml) - Gradle task batching
- [`complex-dependencies/`](./examples/complex-dependencies) - Multi-level dependency graph
- [`profiles-example.yaml`](./examples/profiles-example.yaml) - Environment-specific configs

## Tips & Best Practices

### Use Sessions for Development

```bash
# Start a session in your project directory
cd ~/my-project
fed session start --id my-project

# This creates .fed/session file - now all fed commands
# in this directory will use the session automatically

# Add .fed/ to .gitignore
echo ".fed/" >> .gitignore
```

**Alternative:** If you need to use the session in multiple directories, export the env var:
```bash
export FED_SESSION=my-project
```

### Health Checks

```yaml
services:
  api:
    healthcheck:
      httpGet: 'http://localhost:{{PORT}}/health'

  frontend:
    depends_on: [api]  # Waits for api to be healthy
```

### Mix Docker Compose with Native Processes

```yaml
services:
  # Use existing docker-compose for databases
  postgres:
    composeFile: docker-compose.yml
    composeService: postgres

  # Run your app natively
  backend:
    process: cargo run --bin api
    depends_on: [postgres]
```

## Troubleshooting

**Services not starting?**
```bash
fed logs <service-name> --tail 100
```

**Port conflicts?**
```yaml
# Use port parameters instead of hardcoded ports
parameters:
  API_PORT:
    type: port
    default: 8080  # Auto-falls back if in use
```

**Stuck sessions?**
```bash
fed session cleanup
```

## Status

**Features:**
- Process, Docker, Docker Compose, Gradle services
- Dependency resolution and health checks
- Session-based port allocation with TOCTOU race prevention
- Parameter templating with port fallback
- Service templates for reusable configurations
- Lifecycle hooks (install, build, clean)
- Resource limits (memory, CPU, PIDs, file descriptors)
- Environment file (.env) support with strict variable checking
- Scripts with service/script dependencies and argument passthrough
- Isolated test execution with `isolated: true` for fresh ports and scoped volumes
- TUI (beta)
- Log streaming with `--follow` flag and search/filter
- Watch mode with `fed start -w` (auto-restart on file changes)
- Shell completions (bash/zsh/fish)
- Graceful shutdown with configurable timeouts

**Roadmap:**
- Remote dependencies (GitHub, Git sources)
- TUI detail view enhancements

## Contributing

Issues and PRs welcome! This is an early-stage project.

## License

MIT
