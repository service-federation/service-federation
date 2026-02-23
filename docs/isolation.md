# Isolation

Service Federation scopes all state by working directory. This gives you isolation between projects, worktrees, and CI agents without any explicit configuration.

## Directory Scoping

All state is scoped to the directory containing your `service-federation.yaml`:

- Port allocations
- Docker container names
- Docker volumes
- The SQLite state file

Two directories are two independent stacks. There's nothing to configure — it just works.

## Port Management

```bash
fed ports list            # Show current allocations
fed ports list --json     # Machine-readable output
fed ports randomize       # Randomize ports (persisted across restarts)
fed ports reset           # Clear all allocations
```

Ports persist across restarts — `fed stop && fed start` reuses the same allocations until you `fed ports reset`.

### Randomization

There are two ways to randomize ports:

- **`fed start --randomize`** — Allocates fresh random ports on every invocation. Good for one-off isolated runs.
- **`fed ports randomize`** — Randomizes once, then `fed start` (without `--randomize`) reuses those allocations. Good for stable worktree setups.

## Git Worktrees

Git worktrees give you full isolation for free because each worktree is a separate directory:

```bash
# Main worktree — default ports
~/project $ fed start

# Second worktree — randomized ports, separate volumes and containers
~/project-review $ fed start --randomize
```

Both stacks run simultaneously on the same machine.

### Workspace Management (beta)

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

## Cursor and Parallel Agents

Cursor's parallel agents create git worktrees under the hood — each agent works in its own directory. If your project has a `service-federation.yaml`, each agent can spin up the full stack independently:

```bash
fed install && fed start --randomize
```

No plugin or integration needed. Directory-scoped isolation means it works the same whether Cursor created the worktree or you did.

## Sessions

For cases where you want explicit named isolation rather than relying on directory scoping:

```bash
fed session start --id my-project
fed start                            # Ports remembered under this session
fed session end
```

Cross-directory session: `export FED_SESSION=my-project`. Add `.fed/` to `.gitignore`.

Sessions are useful when multiple directories need to share a single set of port allocations, or when you want a named handle for cleanup:

```bash
fed session cleanup    # Clean up stuck sessions
```

## Isolated Scripts

Scripts can opt into full isolation with `isolated: true` — fresh ports, scoped containers and volumes, automatic cleanup. See [Scripts](./scripts.md#isolated-scripts) for details.
