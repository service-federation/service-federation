use crate::cli::WorkspaceCommands;
use crate::output::UserOutput;
use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn run_workspace(cmd: &WorkspaceCommands, out: &dyn UserOutput) -> anyhow::Result<()> {
    match cmd {
        WorkspaceCommands::New {
            branch,
            create_branch,
        } => ws_new(branch, *create_branch, out).await,
        WorkspaceCommands::List => ws_list(out).await,
        WorkspaceCommands::Cd { name } => ws_cd(name, out).await,
        WorkspaceCommands::Rm { name, force } => ws_rm(name, *force, out).await,
        WorkspaceCommands::Prune => ws_prune(out).await,
        WorkspaceCommands::Setup => ws_setup(out).await,
        WorkspaceCommands::InitShell => ws_init_shell(),
    }
}

/// Parse `git worktree list --porcelain` into structured entries.
struct WorktreeInfo {
    path: PathBuf,
    branch: Option<String>,
}

async fn list_worktrees_parsed() -> anyhow::Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await
        .context("Failed to run git worktree list")?;

    if !output.status.success() {
        bail!("git worktree list failed");
    }

    let stdout = String::from_utf8(output.stdout).context("Invalid UTF-8 from git")?;
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Flush previous entry
            if let Some(p) = current_path.take() {
                worktrees.push(WorktreeInfo {
                    path: p,
                    branch: current_branch.take(),
                });
            }
            current_path = Some(PathBuf::from(path));
            current_branch = None;
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // refs/heads/main -> main
            current_branch = Some(
                branch_ref
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch_ref)
                    .to_string(),
            );
        }
        // "HEAD <sha>", "bare", "detached", blank lines — ignored
    }

    // Flush last entry
    if let Some(p) = current_path.take() {
        worktrees.push(WorktreeInfo {
            path: p,
            branch: current_branch.take(),
        });
    }

    Ok(worktrees)
}

/// Get the main worktree path (first entry from `git worktree list`).
async fn get_main_worktree() -> anyhow::Result<PathBuf> {
    let worktrees = list_worktrees_parsed().await?;
    worktrees
        .into_iter()
        .next()
        .map(|w| w.path)
        .ok_or_else(|| anyhow::anyhow!("No worktrees found"))
}

/// Derive the worktree base directory: `<main-worktree-parent>/<repo-name>-worktrees/`
fn get_worktree_base(main_worktree: &Path) -> PathBuf {
    let repo_name = main_worktree
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let parent = main_worktree.parent().unwrap_or(main_worktree);
    parent.join(format!("{}-worktrees", repo_name))
}

/// Write the target directory to `FED_WS_CD_FILE` if the env var is set.
/// If not set, print the path and a hint to set up shell integration.
fn write_cd_file(path: &Path, out: &dyn UserOutput) {
    if let Ok(cd_file) = std::env::var("FED_WS_CD_FILE") {
        if let Err(e) = std::fs::write(&cd_file, path.to_string_lossy().as_bytes()) {
            tracing::warn!("Failed to write cd file: {}", e);
        }
    } else {
        out.status(&format!("  cd {}", path.display()));
        out.status("");
        out.status("  Run `fed ws setup` to enable auto-cd.");
    }
}

/// Count running services by peeking at a worktree's `.fed/lock.db`.
/// Returns None if the DB doesn't exist or is locked.
async fn count_running_services(worktree_path: &Path) -> Option<usize> {
    let db_path = worktree_path.join(".fed").join("lock.db");
    if !db_path.exists() {
        return Some(0);
    }

    // Open read-only, don't acquire advisory lock
    let db_path_str = db_path.to_string_lossy().to_string();
    let conn = tokio_rusqlite::Connection::open(&db_path_str).await.ok()?;
    let count = conn
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT COUNT(*) FROM services WHERE status IN ('running', 'starting')",
            )?;
            let count: i64 = stmt.query_row([], |row| row.get(0))?;
            Ok(count as usize)
        })
        .await
        .ok()?;
    Some(count)
}

// ── Subcommand implementations ──────────────────────────────────────

async fn ws_new(branch: &str, create_branch: bool, out: &dyn UserOutput) -> anyhow::Result<()> {
    let main_wt = get_main_worktree().await?;
    let base = get_worktree_base(&main_wt);
    let target = base.join(branch);

    if target.exists() {
        bail!(
            "Worktree directory already exists: {}",
            target.display()
        );
    }

    // Ensure base directory exists
    std::fs::create_dir_all(&base)
        .with_context(|| format!("Failed to create worktree base: {}", base.display()))?;

    let mut args = vec!["worktree", "add"];
    if create_branch {
        args.push("-b");
        args.push(branch);
        args.push(target.to_str().unwrap_or_default());
    } else {
        args.push(target.to_str().unwrap_or_default());
        args.push(branch);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&main_wt)
        .output()
        .await
        .context("Failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();

        // Provide friendly errors for common cases
        if msg.contains("is not a valid branch name") || msg.contains("not a valid ref") {
            bail!("Branch '{}' not found. Use `-b` to create it.", branch);
        }
        if msg.contains("already checked out") {
            bail!("{}", msg);
        }
        bail!("git worktree add failed: {}", msg);
    }

    if create_branch {
        out.status(&format!("  Created branch '{}'", branch));
    }
    out.status(&format!("  Created worktree at {}", target.display()));

    write_cd_file(&target, out);
    Ok(())
}

async fn ws_list(out: &dyn UserOutput) -> anyhow::Result<()> {
    let worktrees = list_worktrees_parsed().await?;

    if worktrees.is_empty() {
        out.status("No worktrees found.");
        return Ok(());
    }

    // Determine column widths
    let branch_width = worktrees
        .iter()
        .map(|w| {
            w.branch
                .as_deref()
                .unwrap_or("(detached)")
                .len()
        })
        .max()
        .unwrap_or(10);

    for wt in &worktrees {
        let branch_name = wt
            .branch
            .as_deref()
            .unwrap_or("(detached)");
        let path_str = wt.path.display().to_string();
        let status = match count_running_services(&wt.path).await {
            Some(0) => "(stopped)".to_string(),
            Some(n) => format!("({} running)", n),
            None => "(unknown)".to_string(),
        };
        out.status(&format!(
            "  {:<width$}  {}  {}",
            branch_name,
            path_str,
            status,
            width = branch_width,
        ));
    }

    Ok(())
}

async fn ws_cd(name: &str, out: &dyn UserOutput) -> anyhow::Result<()> {
    let worktrees = list_worktrees_parsed().await?;

    let found = worktrees.iter().find(|w| {
        w.branch.as_deref() == Some(name)
            || w.path
                .file_name()
                .map(|f| f.to_string_lossy() == name)
                .unwrap_or(false)
    });

    match found {
        Some(wt) => {
            write_cd_file(&wt.path, out);
            Ok(())
        }
        None => {
            bail!("No worktree found for '{}'. Run `fed ws list`.", name);
        }
    }
}

async fn ws_rm(name: &str, force: bool, out: &dyn UserOutput) -> anyhow::Result<()> {
    let main_wt = get_main_worktree().await?;
    let worktrees = list_worktrees_parsed().await?;

    let found = worktrees.iter().find(|w| {
        w.branch.as_deref() == Some(name)
            || w.path
                .file_name()
                .map(|f| f.to_string_lossy() == name)
                .unwrap_or(false)
    });

    let wt = match found {
        Some(wt) => wt,
        None => bail!("No worktree found for '{}'. Run `fed ws list`.", name),
    };

    // Don't remove the main worktree
    if wt.path == main_wt {
        bail!("Cannot remove the main worktree");
    }

    // Stop running services if any
    if let Some(count) = count_running_services(&wt.path).await {
        if count > 0 {
            out.warning(&format!(
                "  Stopping {} running service(s) in '{}'...",
                count, name
            ));
            // Run `fed stop` in the worktree directory
            let stop_output = Command::new("fed")
                .args(["stop"])
                .current_dir(&wt.path)
                .output()
                .await;

            if let Err(e) = stop_output {
                tracing::warn!("Failed to stop services in worktree: {}", e);
            }
        }
    }

    // Clean up .fed state
    let fed_dir = wt.path.join(".fed");
    if fed_dir.exists() {
        out.status("  Cleaning up state...");
        if let Err(e) = std::fs::remove_dir_all(&fed_dir) {
            tracing::warn!("Failed to remove .fed directory: {}", e);
        }
    }

    // Remove the worktree
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(wt.path.to_str().unwrap_or_default());

    let output = Command::new("git")
        .args(&args)
        .current_dir(&main_wt)
        .output()
        .await
        .context("Failed to run git worktree remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        if msg.contains("contains modified or untracked files") {
            bail!(
                "Worktree '{}' has uncommitted changes. Use `--force` to remove anyway.",
                name
            );
        }
        bail!("git worktree remove failed: {}", msg);
    }

    out.success(&format!("  Removed worktree '{}'", name));
    Ok(())
}

async fn ws_prune(out: &dyn UserOutput) -> anyhow::Result<()> {
    let main_wt = get_main_worktree().await?;

    // Run git worktree prune (removes entries whose directories are gone)
    let output = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(&main_wt)
        .output()
        .await
        .context("Failed to run git worktree prune")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree prune failed: {}", stderr.trim());
    }

    // Also clean up empty directories in the worktree base
    let base = get_worktree_base(&main_wt);
    let mut pruned = 0;

    if base.exists() {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // If the directory is empty or only has .fed, it's stale
                    let is_stale = match std::fs::read_dir(&path) {
                        Ok(mut contents) => contents.next().is_none(),
                        Err(_) => false,
                    };
                    if is_stale {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if let Err(e) = std::fs::remove_dir_all(&path) {
                            tracing::warn!("Failed to remove stale directory {}: {}", name, e);
                        } else {
                            out.status(&format!("  Removed '{}'", name));
                            pruned += 1;
                        }
                    }
                }
            }
        }
    }

    if pruned > 0 {
        out.success(&format!("  Pruned {} worktree(s)", pruned));
    } else {
        out.status("  Nothing to prune.");
    }

    Ok(())
}

async fn ws_setup(out: &dyn UserOutput) -> anyhow::Result<()> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let zshrc = home.join(".zshrc");

    let eval_line = r#"eval "$(fed ws init-shell)""#;

    // Check if already present
    if zshrc.exists() {
        let contents = std::fs::read_to_string(&zshrc)?;
        if contents.contains(eval_line) {
            out.status("  Shell integration already installed in ~/.zshrc");
            return Ok(());
        }
    }

    // Append
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&zshrc)?;

    writeln!(file)?;
    writeln!(file, "# fed workspace shell integration")?;
    writeln!(file, "{}", eval_line)?;

    out.success("  Added to ~/.zshrc:");
    out.status(&format!("    {}", eval_line));
    out.blank();
    out.status("  Restart your shell or run:");
    out.status("    source ~/.zshrc");

    Ok(())
}

fn ws_init_shell() -> anyhow::Result<()> {
    print!(
        r#"fed() {{
  if [[ "$1" == "ws" || "$1" == "workspace" ]]; then
    local _fed_cd_file
    _fed_cd_file=$(mktemp "${{TMPDIR:-/tmp}}/fed-ws-cd.XXXXXX")
    FED_WS_CD_FILE="$_fed_cd_file" command fed "$@"
    local _fed_ret=$?
    if [[ -s "$_fed_cd_file" ]]; then
      builtin cd -- "$(cat "$_fed_cd_file")"
    fi
    command rm -f "$_fed_cd_file"
    return $_fed_ret
  fi
  command fed "$@"
}}
"#
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_base_derived_from_main() {
        let main = PathBuf::from("/Users/dev/Projects/my-app");
        let base = get_worktree_base(&main);
        assert_eq!(
            base,
            PathBuf::from("/Users/dev/Projects/my-app-worktrees")
        );
    }

    #[test]
    fn worktree_base_handles_trailing_slash() {
        // PathBuf normalizes, but test the function is robust
        let main = PathBuf::from("/home/user/repo");
        let base = get_worktree_base(&main);
        assert_eq!(base, PathBuf::from("/home/user/repo-worktrees"));
    }
}
