# Command Reference

## Global Flags

| Flag | Description |
|------|-------------|
| `--verbose` / `-v` | Debug output |
| `--version` | Print version |
| `--offline` | Skip git operations (package fetches) |

Run `fed --help` or `fed <command> --help` for full details.

## Starting & Stopping

### `fed start`

Start all services. Waits for health checks, then backgrounds.

| Flag | Description |
|------|-------------|
| `--randomize` | Allocate fresh random ports before starting |
| `--replace` | Kill processes occupying required ports, then start |
| `--dry-run` | Preview what would start without starting |
| `-w` / `--watch` | Watch mode — foreground, auto-restart on file changes |
| `-p` / `--profile <name>` | Include services with this profile (repeatable) |

### `fed stop`

Stop all running services.

### `fed restart`

Restart all services.

## Observability

### `fed status`

Show service status.

| Flag | Description |
|------|-------------|
| `--json` | Machine-readable output |

### `fed logs <service>`

View logs for a service.

| Flag | Description |
|------|-------------|
| `--follow` / `-f` | Stream logs |
| `--tail <n>` | Show last n lines |

## Ports

### `fed ports list`

Show current port allocations.

| Flag | Description |
|------|-------------|
| `--json` | Machine-readable output |

### `fed ports randomize`

Randomize all port allocations. Persists across restarts.

### `fed ports reset`

Clear all port allocations.

## Scripts

### `fed run <script>`

Run a named script. Services in `depends_on` are started first.

```bash
fed run db:migrate
fed db:migrate                    # Shorthand (if no command collision)
fed test:integration -- -t "auth" # Pass arguments after --
```

## Lifecycle

### `fed install`

Run `install` hooks for all services.

### `fed build`

Run `build` hooks for all services (shell and Docker).

| Flag | Description |
|------|-------------|
| `--tag <tag>` | Custom tag for Docker images |
| `--build-arg <KEY=VALUE>` | Extra build arguments |

### `fed clean`

Run `clean` hooks and remove Docker volumes with `fed-` prefix.

## Docker

### `fed docker build`

Build Docker images only (skip shell build hooks).

| Flag | Description |
|------|-------------|
| `--tag <tag>` | Custom tag |
| `--json` | Machine-readable output |

### `fed docker push`

Push built images to registry.

| Flag | Description |
|------|-------------|
| `--tag <tag>` | Push specific tag |

## Packages

### `fed package refresh`

Re-fetch all packages from remote sources.

## Sessions

### `fed session start`

Start a named session.

| Flag | Description |
|------|-------------|
| `--id <name>` | Session identifier |

### `fed session end`

End the current session.

### `fed session cleanup`

Clean up stuck sessions.

## Workspaces (beta)

### `fed ws setup`

Install shell integration for auto-cd.

### `fed ws new`

Create a new worktree with a branch.

| Flag | Description |
|------|-------------|
| `-b <branch>` | Branch name |

### `fed ws list`

Show all worktrees with service status.

### `fed ws cd <name>`

Switch to another worktree.

### `fed ws rm <name>`

Stop services and remove a worktree.

### `fed ws prune`

Clean up worktrees for deleted branches.

## Utilities

### `fed doctor`

Check system requirements (Docker, Gradle, etc.).

### `fed init`

Create a starter `service-federation.yaml`.
