# Configuration Reference

Service Federation is configured through a `service-federation.yaml` file in your project root. Run `fed init` to generate a starter config.

## Services

Every service has one type-defining field — `process`, `image`, `composeFile`+`composeService`, or `gradleTask`.

### Process

Run any command on the host:

```yaml
services:
  api:
    process: npm start
    cwd: ./api
```

### Docker Container

Run a Docker image:

```yaml
services:
  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]
```

### Docker Compose Service

Reuse services from an existing `docker-compose.yml`:

```yaml
services:
  postgres:
    composeFile: ./docker-compose.yml
    composeService: postgres
```

### Gradle Task

Tasks sharing a `cwd` are batched into a single Gradle command:

```yaml
services:
  auth-service:
    gradleTask: ':auth:bootRun'
    cwd: ./backend
```

## Parameters

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

Parameters can also be set via environment variables or `.env` files. Priority: explicit `value` field > `env_file` > `default`.

## Dependencies & Health Checks

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

### Health check types

- `httpGet` — HTTP request from the host.
- `command` — For Docker services, runs inside the container via `docker exec`. For process services, runs on the host.

## Environment Files

`.env` files set parameter values (not service environment directly):

```yaml
parameters:
  API_KEY:
    default: ""

env_file:
  - .env         # API_KEY=secret123
  - .env.local   # Later files override earlier
```

All `.env` variables must be declared as parameters.

See [`examples/env-file/`](../examples/env-file).

## Startup Messages

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

## Templates

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

See [`examples/templates-example.yaml`](../examples/templates-example.yaml).

## Profiles

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

## Packages

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

See [`examples/service-merging/`](../examples/service-merging).

## Lifecycle Hooks

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

## Docker Image Builds

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

`fed build` builds all services with a `build` field. `fed docker build` builds only Docker images. Images are tagged with the git short hash by default.

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

## Entrypoint

Declare the main service — affects startup order display and `startup_message` sorting:

```yaml
entrypoint: backend
```

## Sessions

For cases where you want explicit named isolation rather than relying on directory scoping:

```bash
fed session start --id my-project
fed start                            # Ports remembered under this session
fed session end
```

Cross-directory session: `export FED_SESSION=my-project`. Add `.fed/` to `.gitignore`.

See also [Isolation](./isolation.md) for how directory scoping works.
