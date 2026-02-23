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

That's the whole workflow. `git clone`, add a config, `fed start`, the project is running.

## Why fed

- **One config, one command** — Docker containers, native processes, Gradle tasks, and Compose services all live in one `service-federation.yaml`. `fed start` handles dependency ordering and health checks.
- **Directory-scoped isolation** — All state (ports, containers, volumes) is scoped by working directory. Two directories are two independent stacks. Git worktrees give you parallel environments for free.
- **No Docker Compose sprawl** — Port parameters, templating, profiles, and cross-project packages replace the pile of override files and `.env` juggling.

## Isolated Scripts

Run integration tests against a throwaway stack without touching your dev services:

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

`isolated: true` gives the script fresh ports, scoped Docker containers and volumes, and automatic cleanup when it finishes. See [docs/scripts.md](docs/scripts.md) for details.

## Worktree & Cursor Isolation

Git worktrees are first-class. Each worktree gets its own ports, containers, and volumes:

```bash
~/project        $ fed start                # Default ports
~/project-review $ fed start --randomize    # Randomized ports, separate stack
```

Cursor's parallel agents create worktrees under the hood — `fed install && fed start --randomize` just works in each one. No plugin needed.

`fed ws` manages worktrees directly: `fed ws new -b feature`, `fed ws list`, `fed ws cd main`. See [docs/isolation.md](docs/isolation.md).

## Commands

```bash
fed start [--randomize|--replace|--dry-run|-w]  # Start services
fed stop / restart                               # Stop / restart
fed status [--json]                              # Service status
fed logs <svc> [--follow]                        # View logs
fed ports list [--json] / randomize / reset      # Port management
fed run <script> [-- args]                       # Run a script
fed install / build / clean                      # Lifecycle hooks
fed docker build [--json] / push                 # Docker images
fed ws new / list / cd / rm                      # Worktrees (beta)
fed doctor                                       # Check requirements
fed init                                         # Create starter config
```

Global flags: `--verbose` / `-v`, `--version`, `--offline`. Full reference: [docs/commands.md](docs/commands.md).

## Configuration

Services can be processes, Docker images, Compose services, or Gradle tasks. Config supports parameters with port allocation, `.env` files, templates, profiles, cross-project packages, lifecycle hooks, and startup messages.

Full reference: [docs/configuration.md](docs/configuration.md).

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

## Documentation

- [Configuration Reference](docs/configuration.md) — Services, parameters, health checks, templates, profiles, packages
- [Scripts](docs/scripts.md) — Scripts, isolated scripts, argument passing
- [Isolation](docs/isolation.md) — Directory scoping, worktrees, sessions, Cursor agents
- [Command Reference](docs/commands.md) — All commands and flags

## Contributing

Issues and PRs welcome.

## License

MIT
