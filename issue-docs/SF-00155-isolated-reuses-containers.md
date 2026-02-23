# SF-00155: `isolated: true` reuses running containers

## Problem

When a script with `isolated: true` depends on a Docker service already running
(e.g. started by `fed start`), the child orchestrator's container gets the **same
name** as the parent's because both read the global `Session::current()` to
compute container names via `docker_container_name()`.

This causes `DockerService::start()` to `rm_force` the parent's container, start
a new one, then remove it on cleanup — leaving the parent without its container.

## Root Cause

`create_docker_service()` in `factory.rs` and the healthcheck runner in
`health.rs` both call `Session::current()` (global state) to get the session ID
for container naming. The ephemeral child orchestrator had no way to override
this, so it produced identical container names.

## Fix

Added `isolation_id: Option<String>` to `Orchestrator`. When set:

1. **`factory.rs`**: `create_docker_service()` uses it as the session ID instead
   of `Session::current()`, producing names like `fed-iso-a1b2c3d4-postgres`.
2. **`health.rs`**: Healthcheck container name resolution uses it similarly.
3. **`scripts.rs`**: `run_script_isolated()` generates a random isolation ID
   (`iso-{:08x}`) and sets it on the child orchestrator before `initialize()`.

Volume scoping works automatically since `DockerService` already uses the
session ID passed through the factory for `scope_volume_with_session()`.

## Files Changed

- `src/orchestrator/core.rs` — New `isolation_id` field + setter
- `src/orchestrator/factory.rs` — Prefer `isolation_id` over `Session::current()`
- `src/orchestrator/health.rs` — Same preference for healthcheck naming
- `src/orchestrator/scripts.rs` — Generate and set isolation ID on child
