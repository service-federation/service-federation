# Service Federation (fed)

Orchestrate your local dev stack from one config file. Docker containers and native processes with dependency-aware startup, healthchecks, and directory-scoped isolation.

## Quick Start

```bash
cargo install service-federation
```

Create `service-federation.yaml`:

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

entrypoint: backend
```

```bash
fed start        # Start services (waits for healthchecks, backgrounds)
fed status       # What's running
fed logs backend # View logs
fed stop         # Stop all
```

That's the whole workflow. `git clone`, add a config, `fed start`. New team members don't need a wiki page.

## Isolation

All state is scoped by working directory — port allocations, Docker container names, Docker volumes, the SQLite state file. Two directories are two independent stacks.

This means git worktrees give you full isolation for free:

```bash
# Main worktree — default ports
~/project $ fed start

# Second worktree — randomized ports, separate volumes and containers
~/project-review $ fed start --randomize
```

Both stacks run simultaneously on the same machine. `--randomize` allocates fresh random ports so nothing collides. Ports persist across restarts — `fed stop && fed start` reuses the same allocations until you `fed ports reset`.

`--randomize` allocates fresh ports on every invocation. For stable randomized ports across restarts, use `fed ports randomize` once, then `fed start` without the flag.

### Workspace management

`fed ws` (alias for `fed workspace`) automates git worktree creation and switching:

```bash
fed ws setup                # One-time: install shell integration
fed ws new -b my-feature    # Create branch + worktree, cd into it
fed ws list                 # Show all worktrees with service status
fed ws cd main              # Switch to another worktree
fed ws rm my-feature        # Stop services and remove worktree
fed ws prune                # Clean up worktrees for deleted branches
```

Worktrees are created as siblings to the repo in a `<repo>-worktrees/` directory. Shell integration enables auto-cd — without it, `fed ws new` and `fed ws cd` print the path instead.

To find your allocated ports:

```bash
fed ports list       # Show allocations
fed ports list --json
```

Or add `startup_message` to your services (see [Startup Messages](#startup-messages)).

### Isolated scripts

For integration tests that need a throwaway stack without interfering with your dev services:

```yaml
scripts:
  test:integration:
    isolated: true     # Fresh ports, scoped volumes, separate containers
    depends_on: [database, api]
    script: npm run test:e2e
```

```bash
fed start                # Dev stack stays running
fed test:integration     # Tests get their own stack, cleaned up after
```

### Cursor and parallel agents

Cursor's parallel agents create git worktrees under the hood — each agent works in its own directory. If your project has a `service-federation.yaml`, each agent can spin up the full stack independently:

```bash
fed install && fed start --randomize
```

No plugin or integration needed. Directory-scoped isolation means it works the same whether Cursor created the worktree or you did.

## Configuration

### Services

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

Also supports **Docker Compose services** (reuse an existing `docker-compose.yml`) and **Gradle tasks** (tasks sharing a `cwd` are batched into one gradle command):

```yaml
services:
  postgres:
    composeFile: ./docker-compose.yml
    composeService: postgres

  auth-service:
    gradleTask: ':auth:bootRun'
    cwd: ./backend
```

### Parameters

Values referenced throughout your config with `{{PARAM}}`:

```yaml
parameters:
  API_PORT:
    type: port
    default: 8080  # Tries 8080, falls back to random if occupied

  DB_PORT:
    type: port     # No default — always random available port

  API_KEY:
    default: "dev-key"  # String parameter
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
```

Health check types:
- `httpGet` — HTTP request from the host.
- `command` — For Docker services, runs inside the container via `docker exec`. For process services, runs on the host.

### Scripts

Commands that can depend on services or other scripts:

```yaml
scripts:
  db:migrate:
    depends_on: [database]
    script: npx prisma db push

  test:integration:
    depends_on: [db:migrate, api]
    script: npm run test:e2e -- "$@"  # "$@" passes arguments from CLI
```

```bash
fed run db:migrate                    # Run a script
fed db:migrate                        # Shorthand (if no command collision)
fed test:integration -- -t "auth"     # Pass arguments after --
```

Services started as script dependencies keep running after the script finishes. Use `fed stop` to stop them.

### Environment Files

`.env` files set parameter values (not service environment directly):

```yaml
parameters:
  API_KEY:
    default: ""

env_file:
  - .env         # API_KEY=secret123
  - .env.local   # Later files override earlier
```

All `.env` variables must be declared as parameters. Priority: explicit `value` field > `env_file` > `default`.

See [`examples/env-file/`](./examples/env-file).

### Startup Messages

Show where to access services after startup:

```yaml
services:
  api:
    startup_message: "API docs: http://localhost:{{API_PORT}}/docs"
  frontend:
    startup_message: "App: http://localhost:{{NEXT_PORT}}"
```

```
╭──────────────────────────────────────────────────╮
│ API docs: http://localhost:8081/docs             │
├──────────────────────────────────────────────────┤
│ App: http://localhost:3000                       │
╰──────────────────────────────────────────────────╯
```

Entrypoint services sort last. A warning is emitted if an entrypoint has no `startup_message` — particularly useful when ports are randomized.

### Templates

Reusable base configurations:

```yaml
templates:
  java-service:
    image: openjdk:17-slim
    environment:
      JAVA_OPTS: '-Xmx512m'
    healthcheck:
      httpGet: 'http://localhost:{{PORT}}/actuator/health'

services:
  auth-service:
    extends: java-service
    ports: ["8080:8080"]
```

See [`examples/templates-example.yaml`](./examples/templates-example.yaml).

### Profiles

Conditionally include services:

```yaml
services:
  api:
    process: npm start       # No profiles = always included

  worker:
    profiles: [worker]       # Only with -p worker

  debug-tools:
    profiles: [debug]        # Only with -p debug
```

```bash
fed start                    # Starts profileless services only
fed start -p worker          # Starts api + worker
fed start -p worker -p debug # Starts api + worker + debug-tools
```

### Packages

Import service configurations across projects:

```yaml
packages:
  - source: "github:org/repo@v1.0"
    as: "infra"

services:
  database:
    extends: "infra.postgres"
    environment:
      POSTGRES_DB: "myapp"
```

Packages are cached locally. `fed package refresh` to re-fetch, `fed --offline start` to skip git.

See [`examples/service-merging/`](./examples/service-merging).

### Lifecycle Hooks

```yaml
services:
  backend:
    process: npm start
    cwd: ./backend
    install: npm ci                   # Runs before first start, re-run with `fed install`
    build: npm run build              # Runs with `fed build`
    clean: rm -rf node_modules dist   # Runs with `fed clean`
```

`fed clean` also removes Docker volumes with `fed-` prefix.

### Docker Image Builds

```yaml
services:
  web:
    cwd: ./apps/web
    build:
      image: my-app
      # dockerfile: Dockerfile  (default)
      # args:                    (optional build arguments)
      #   NODE_ENV: production
```

`fed build` builds all services with a `build` field. `fed docker build` builds only Docker images. For Docker builds, images are tagged with the git short hash by default.

```bash
fed build                          # Build all (shell + Docker)
fed build --tag v1.0.0             # Custom tag
fed build --build-arg KEY=VALUE    # Extra build args
fed docker build                   # Build Docker images only
fed docker build --tag v1.0.0      # Custom tag
fed docker build --json            # Machine-readable output
fed docker push                    # Push images to registry
fed docker push --tag v1.0.0       # Push specific tag
```

### Sessions

For cases where you want explicit named isolation rather than relying on directory scoping:

```bash
fed session start --id my-project
fed start                            # Ports remembered under this session
fed session end
```

Cross-directory session: `export FED_SESSION=my-project`. Add `.fed/` to `.gitignore`.

## Commands

```bash
fed start                    # Start all services (background)
fed start --randomize        # Randomize ports, then start
fed start --replace          # Kill processes on required ports, then start
fed start --dry-run          # Preview without starting
fed start -w                 # Watch mode (foreground, auto-restart on changes)
fed stop                     # Stop all
fed restart                  # Restart all
fed status [--json]          # Service status
fed logs <svc> [--follow]    # View / stream logs
fed ports list [--json]      # Port allocations
fed ports randomize          # Randomize ports (standalone)
fed ports reset              # Clear allocations
fed run <script> [-- args]   # Run a script
fed install / build / clean  # Lifecycle hooks
fed docker build [--json]    # Build Docker images
fed docker push              # Push images to registry
fed doctor                   # Check system requirements
fed init                     # Create starter config
```

Full reference: `fed --help`, `fed <command> --help`.

## Examples

See [`examples/`](./examples):

- [`simple.yaml`](./examples/simple.yaml) — Basic multi-service setup
- [`scripts-example.yaml`](./examples/scripts-example.yaml) — Scripts with dependencies
- [`env-file/`](./examples/env-file) — Environment files
- [`templates-example.yaml`](./examples/templates-example.yaml) — Service templates
- [`docker-compose-example/`](./examples/docker-compose-example) — Docker Compose integration
- [`gradle-grouping.yaml`](./examples/gradle-grouping.yaml) — Gradle task batching
- [`profiles-example.yaml`](./examples/profiles-example.yaml) — Profiles
- [`service-merging/`](./examples/service-merging) — Package imports

## Troubleshooting

**Services not starting?**
```bash
fed logs <service> --tail 100
```

**Port conflicts?**
```bash
fed start --randomize   # Sidestep conflicts
fed start --replace     # Kill conflicting processes
```

**Stuck sessions?**
```bash
fed session cleanup
```

## Contributing

Issues and PRs welcome.

## License

MIT
