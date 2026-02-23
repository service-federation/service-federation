#[cfg(unix)]
mod tests {
    use fs2::FileExt;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use tempfile::TempDir;

    const WARNING_TEXT: &str =
        "Another fed instance (PID";

    fn fed_binary() -> String {
        env!("CARGO_BIN_EXE_fed").to_string()
    }

    fn create_test_workspace() -> (TempDir, PathBuf) {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = temp_dir.path().join("test-config.yaml");

        let config = r#"
services:
  app:
    process: sleep 60
"#;

        fs::write(&config_path, config).expect("failed to write test config");
        (temp_dir, config_path)
    }

    fn hold_workspace_lock(workdir: &Path, pid_text: &str) -> std::fs::File {
        let fed_dir = workdir.join(".fed");
        fs::create_dir_all(&fed_dir).expect("failed to create .fed dir");
        let lock_path = fed_dir.join(".lock");

        let mut lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .expect("failed to open lock file");

        lock_file
            .try_lock_exclusive()
            .expect("failed to hold exclusive lock in test");
        lock_file.set_len(0).expect("failed to truncate lock file");
        writeln!(lock_file, "{}", pid_text).expect("failed to write pid text");
        lock_file.sync_all().expect("failed to sync lock file");

        lock_file
    }

    fn run_status(config_path: &Path, workdir: &Path) -> Output {
        Command::new(fed_binary())
            .args([
                "-c",
                config_path.to_str().expect("config path is not valid UTF-8"),
                "-w",
                workdir.to_str().expect("workdir path is not valid UTF-8"),
                "status",
            ])
            .output()
            .expect("failed to run fed status")
    }

    fn combined_output(output: &Output) -> String {
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    #[test]
    fn status_warns_when_lock_is_held_and_pid_is_alive() {
        let (temp_dir, config_path) = create_test_workspace();
        // Write "<PID> fed" to simulate a real fed instance holding the lock
        let _lock = hold_workspace_lock(
            temp_dir.path(),
            &format!("{} fed", std::process::id()),
        );

        let output = run_status(&config_path, temp_dir.path());
        assert!(
            output.status.success(),
            "status command failed: {}",
            combined_output(&output)
        );

        let text = combined_output(&output);
        assert!(
            text.contains(WARNING_TEXT),
            "Expected lock warning under contention. Output:\n{}",
            text
        );
    }

    #[test]
    fn status_does_not_warn_when_lock_pid_is_stale() {
        let (temp_dir, config_path) = create_test_workspace();
        let _lock = hold_workspace_lock(temp_dir.path(), "999999");

        let output = run_status(&config_path, temp_dir.path());
        assert!(
            output.status.success(),
            "status command failed: {}",
            combined_output(&output)
        );

        let text = combined_output(&output);
        assert!(
            !text.contains(WARNING_TEXT),
            "Did not expect warning for stale lock PID. Output:\n{}",
            text
        );
    }

    #[test]
    fn status_does_not_warn_for_non_fed_alive_pid_expected_behavior() {
        let (temp_dir, config_path) = create_test_workspace();
        let mut sleeper = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to start helper sleep process");

        let sleeper_pid = sleeper.id().to_string();
        let _lock = hold_workspace_lock(temp_dir.path(), &sleeper_pid);

        let output = run_status(&config_path, temp_dir.path());

        let _ = sleeper.kill();
        let _ = sleeper.wait();

        assert!(
            output.status.success(),
            "status command failed: {}",
            combined_output(&output)
        );

        let text = combined_output(&output);
        assert!(
            !text.contains(WARNING_TEXT),
            "Expected no warning for alive non-fed PID. Output:\n{}",
            text
        );
    }
}
