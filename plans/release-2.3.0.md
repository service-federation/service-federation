# Release 2.3.0 — CI Fix & Release Plan

## Context

CI is red on `main` (3 failing jobs since Feb 6). The `feb-2026` branch inherits those
failures and adds a few new ones from recent work. All 5 issues are low complexity —
mechanical fixes, no architectural changes. Tests pass on both macOS and Ubuntu.

Stargazers include t3dotgg (Theo Browne). Let's not ship with a red badge.

## Phases

### Phase 1: Format (SF-00134)

Run `cargo fmt --all`. Commit.

No manual work — this is pure `rustfmt` application. The drift accumulated across
multiple commits that skipped formatting.

### Phase 2: Clippy (SF-00135 + SF-00137)

Fix 6 clippy errors that fail under `-D warnings`:

| File | Lint | Fix |
|------|------|-----|
| `src/orchestrator/orphans.rs:198` | `unnecessary_map_or` | `pgid == Some(nix_pid)` |
| `src/service/docker.rs:886` | `unnecessary_cast` | Remove `as u32` |
| `src/tui/app.rs:437,541,894` | `clone_on_copy` | Remove `.clone()` |
| `src/config/types.rs:134` | `field_reassign_with_default` | Use struct init with field |
| `src/commands/start.rs:14` | `too_many_arguments` | Introduce `StartOptions` struct |

The `StartOptions` struct bundles the 4 "option" params (`watch`, `replace`, `dry_run`,
`config_path`) so `run_start` drops from 8 args to 5. Only `main.rs` calls it, so the
blast radius is two files.

Commit after all clippy fixes. Verify with `cargo clippy --all-targets -- -D warnings`.

### Phase 3: Rustdoc (SF-00136)

Fix 4 documentation errors:

| File | Issue | Fix |
|------|-------|-----|
| `src/orchestrator/factory.rs:360` | Broken links `restore_process_pid`, `restore_gradle_pid` | Qualify or remove links (functions were renamed in SF-00123) |
| `src/service/docker.rs:165` | `[::]` parsed as doc link | Escape with backslashes: `\[::\]` |
| Private item link | Resolves only with `--document-private-items` | Qualify with full path or remove link |

Commit. Verify with `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`.

### Phase 4: Version bump (SF-00138)

- `Cargo.toml`: `version = "2.3.0"`
- Commit.

### Phase 5: Final verification

Run the full CI suite locally:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items
cargo test
```

All four must pass. Then merge `feb-2026` → `main` and push.

## Execution

All issues are Low complexity — implement directly, no subagents needed for individual
fixes. Each phase is a single commit.
