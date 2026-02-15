# SF-00145 + SF-00147: DockerClient extraction & structured Docker errors

## Scope

24 Docker CLI call sites across 9 files. Currently every site manually constructs
`tokio::process::Command::new("docker")`, wraps timeouts ad-hoc, and maps errors
to `Error::Docker(String)`.

## Phase 1: Define `DockerError` and update `Error` variant

**File: `src/docker/error.rs` (new)**

```rust
use std::time::Duration;

#[derive(Debug)]
pub enum DockerError {
    /// Docker command timed out
    Timeout { command: String, timeout: Duration },
    /// Docker command ran but returned non-zero exit
    CommandFailed { command: String, stderr: String, exit_code: Option<i32> },
    /// Docker binary couldn't be executed (not in PATH, permission denied)
    ExecFailed { command: String, source: std::io::Error },
    /// Container doesn't exist
    ContainerNotFound { container: String },
    /// Docker daemon not responding
    DaemonUnavailable,
}
```

- `impl Display for DockerError` produces messages equivalent to current strings
- Update `Error::Docker(String)` → `Error::Docker(DockerError)` in `src/error.rs`
- Update `suggestion()` to give variant-specific hints:
  - `Timeout` → "Docker command timed out. Check Docker daemon health: docker info"
  - `CommandFailed` → (show stderr excerpt) "Check docker logs for details"
  - `ExecFailed` → "Docker not found. Install Docker: https://docs.docker.com/get-docker/"
  - `ContainerNotFound` → "Container may have been removed. Run: fed status"
  - `DaemonUnavailable` → "Start Docker Desktop or: sudo systemctl start docker"
- Update all 74 `Error::Docker(format!(...))` sites to construct the appropriate variant
- Convenience constructors: `DockerError::timeout(cmd, dur)`, `DockerError::failed(cmd, output)`, etc.

**Commit**: "Replace Docker(String) with Docker(DockerError) structured error enum (SF-00147)"

## Phase 2: Create `DockerClient` with core container operations

**File: `src/docker/client.rs` (new)**

`DockerClient` is a lightweight struct (no state beyond optional config) that
encapsulates all Docker CLI interactions with consistent timeout handling and
structured error returns.

```rust
pub struct DockerClient {
    // Future: docker_path, default_timeouts, etc.
}
```

Core methods (all async, returning `Result<T, DockerError>`):

Container lifecycle:
- `rm_force(container: &str, timeout: Duration) -> Result<(), DockerError>`
- `stop(container: &str, timeout: Duration) -> Result<(), DockerError>`
- `kill(container: &str, timeout: Duration) -> Result<(), DockerError>`
- `run(args: &[String], timeout: Duration) -> Result<String, DockerError>` (returns container ID)
- `pull(image: &str, timeout: Duration) -> Result<(), DockerError>`

Inspection:
- `is_running(container: &str, timeout: Duration) -> Result<bool, DockerError>`
- `inspect_ports(container: &str) -> Result<HashMap<String, String>, DockerError>`
- `ps_filtered(filter: &str, format: &str, timeout: Duration) -> Result<Vec<String>, DockerError>`
- `image_exists(image: &str) -> Result<bool, DockerError>`

Exec/logs:
- `exec(container: &str, cmd: &[&str], timeout: Duration) -> Result<Output, DockerError>`
- `logs(container: &str, tail: usize, timeout: Duration) -> Result<Output, DockerError>`

Build/push:
- `build(args: &DockerBuildArgs) -> Result<(), DockerError>` (inherits stdio)
- `push(image: &str) -> Result<(), DockerError>` (inherits stdio)

Daemon:
- `daemon_healthy(timeout: Duration) -> Result<bool, DockerError>`
- `info() -> Result<(), DockerError>`
- `version() -> Result<String, DockerError>`

Compose detection:
- `compose_version() -> Result<ComposeVariant, DockerError>`

Sync variants (for `port/conflict.rs` and `docker/mod.rs`):
- `rm_force_sync`, `ps_filtered_sync`, `is_running_sync`, `daemon_healthy_sync`

Internal helper:
- `run_with_timeout(args, timeout) -> Result<Output, DockerError>` — the single
  point where `tokio::process::Command::new("docker")` is constructed. Handles
  timeout wrapping, stderr capture, and `DockerError` construction.
- `run_sync(args) -> Result<Output, DockerError>` — sync equivalent

All methods that currently silently ignore "No such container" errors encode that
logic once (e.g., `rm_force` returns `Ok(())` for "No such container").

**Commit**: "Add DockerClient struct with core container operations (SF-00145)"

## Phase 3: Migrate call sites to DockerClient

Migrate file by file, largest consumer first:

1. **`service/docker.rs`** (15 sites) — Replace all `tokio::process::Command::new("docker")`
   with `DockerClient` methods. Move timeout constants to `DockerClient` as defaults.
   `DockerService` gets a `client: DockerClient` field.

2. **`orchestrator/orphans.rs`** (2 sites) — `OrphanCleaner` gets `client: &DockerClient`.
   Replace `ps` + `rm -f` with `client.ps_filtered()` + `client.rm_force()`.

3. **`commands/lifecycle.rs`** (5 sites) — `graceful_docker_stop` and
   `remove_orphan_containers_for_workdir` take `&DockerClient` param.

4. **`orchestrator/health.rs`** (1 site) — `docker ps -q` check becomes
   `client.ps_filtered()`.

5. **`healthcheck/command.rs`** (1 site) — `DockerCommandChecker` gets a
   `client: DockerClient` field. `docker exec` becomes `client.exec()`.

6. **`port/conflict.rs`** (2 sites, sync) — Use sync variants.

7. **`commands/doctor.rs`** (3 sites) — Use `client.version()`, `client.info()`,
   `client.compose_version()`.

8. **`commands/docker.rs`** (2 sites) — Use `client.image_exists()` and `client.push()`.

9. **`orchestrator/lifecycle.rs`** (2 sites) — Use `client.build()` and `client.rm_force()`.

10. **`docker/mod.rs`** — Daemon health cache moves into `DockerClient` (or stays
    as module-level statics if simpler). The public API (`is_daemon_healthy()` etc.)
    delegates to `DockerClient`.

11. **`service/compose.rs`** (2 sites) — `ComposeCommand::detect()` uses
    `client.compose_version()`.

**Commit per file or logical group** to keep diffs reviewable.

## Phase 4: Tests and cleanup

- Unit tests for `DockerError` display and conversion
- Unit tests for `DockerClient` argument construction (mock the Command, verify args)
- Remove orphaned timeout constants from migrated files
- Remove `docker/mod.rs` functions that are now `DockerClient` methods
- Verify all existing tests pass
- `cargo fmt && cargo clippy -D warnings && cargo doc -D warnings`

**Commit**: "Add DockerClient tests and clean up migrated code"

## Non-goals

- Docker client library dependency (bollard, etc.) — too heavy, shell-out is fine
- Async-only — sync contexts exist and need sync methods
- Mocking infrastructure — no need, integration tests cover Docker interaction
