use chrono::{DateTime, Utc};

/// Gracefully stop a Docker container.
///
/// Tries `docker stop` first (sends SIGTERM, waits), then `docker rm -f`.
pub async fn graceful_docker_stop(container_id: &str) -> bool {
    use std::process::Command;

    // Try graceful stop first (SIGTERM with 10 second timeout)
    let stop_result = Command::new("docker")
        .args(["stop", "-t", "10", container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = stop_result {
        if status.success() {
            // Remove the stopped container
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            return true;
        }
    }

    // Fallback: force remove
    let rm_result = Command::new("docker")
        .args(["rm", "-f", container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    rm_result.map(|s| s.success()).unwrap_or(false)
}

/// Check if a PID belongs to the expected process by comparing start times.
///
/// Returns true if the PID appears valid (start time matches or cannot be determined),
/// false if the PID was clearly reused by a different process.
///
/// This is a defensive check against PID reuse - if a service's PID was recorded
/// but the process died and a new process reused the same PID, we don't want to
/// accidentally kill an unrelated process.
pub fn validate_pid_start_time(pid: u32, expected_start: DateTime<Utc>) -> bool {
    #[cfg(target_os = "linux")]
    {
        // On Linux, read /proc/<pid>/stat to get process start time
        let stat_path = format!("/proc/{}/stat", pid);
        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
            // The stat file format has the process name in parens which can contain spaces,
            // so we find the closing paren and parse from there
            if let Some(close_paren) = stat.rfind(')') {
                let fields: Vec<&str> = stat[close_paren + 2..].split_whitespace().collect();
                // Field 20 (0-indexed after the first two fields) is starttime
                if let Some(&starttime_str) = fields.get(19) {
                    if let Ok(starttime_jiffies) = starttime_str.parse::<u64>() {
                        let now = chrono::Utc::now();
                        let expected_age = now.signed_duration_since(expected_start);

                        // If the expected service is very old (>24h), be lenient
                        if expected_age.num_hours() > 24 {
                            return true;
                        }

                        // Get uptime to calculate approximate process start
                        if let Ok(uptime_str) = std::fs::read_to_string("/proc/uptime") {
                            if let Some(uptime_secs_str) = uptime_str.split_whitespace().next() {
                                if let Ok(uptime_secs) = uptime_secs_str.parse::<f64>() {
                                    // Assume 100 jiffies per second (common default)
                                    let jiffies_per_sec: u64 = 100;
                                    let process_age_secs = uptime_secs
                                        - (starttime_jiffies as f64 / jiffies_per_sec as f64);

                                    // If process started more than 60 seconds before our expected time,
                                    // it's likely a different process that reused the PID
                                    let expected_age_secs = expected_age.num_seconds() as f64;
                                    let time_diff = (process_age_secs - expected_age_secs).abs();

                                    if time_diff > 60.0 {
                                        tracing::warn!(
                                            "PID {} appears to be reused: process age {:.0}s vs expected {:.0}s",
                                            pid,
                                            process_age_secs,
                                            expected_age_secs
                                        );
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use ps to get process start time
        use chrono::TimeZone;
        use std::process::Command;
        if let Ok(output) = Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
        {
            if output.status.success() {
                let lstart = String::from_utf8_lossy(&output.stdout);
                // Parse the lstart format: "Mon Jan  1 12:00:00 2024"
                let lstart = lstart.trim();
                if !lstart.is_empty() {
                    if let Ok(process_start) =
                        chrono::NaiveDateTime::parse_from_str(lstart, "%a %b %e %H:%M:%S %Y")
                    {
                        // ps lstart returns local time, not UTC
                        let process_start_utc = chrono::Local
                            .from_local_datetime(&process_start)
                            .earliest()
                            .map(|dt| dt.with_timezone(&chrono::Utc));
                        let Some(process_start_utc) = process_start_utc else {
                            // Ambiguous/nonexistent time (DST transition) — trust the PID
                            tracing::debug!(
                                "PID {} start time {:?} is ambiguous during DST, trusting PID",
                                pid,
                                process_start
                            );
                            return true;
                        };
                        let time_diff = (process_start_utc - expected_start).num_seconds().abs();

                        // Allow 60 seconds tolerance for timing differences
                        if time_diff > 60 {
                            tracing::warn!(
                                "PID {} appears to be reused: process started at {} vs expected {}",
                                pid,
                                process_start_utc,
                                expected_start
                            );
                            return false;
                        }
                    }
                }
            }
        }
    }

    // Default: trust the PID if we can't determine start time
    true
}

/// Gracefully kill a process.
///
/// Sends SIGTERM first, waits up to 5 seconds, then sends SIGKILL.
pub async fn graceful_process_kill(pid: u32) -> bool {
    use std::process::Command;
    use std::time::Duration;

    // Check if process exists first
    let exists = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !exists {
        // Process already dead
        return true;
    }

    // Send SIGTERM
    let term_result = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if term_result.is_err() {
        return false;
    }

    // Wait for process to exit (up to 5 seconds)
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;

        let still_exists = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !still_exists {
            return true;
        }
    }

    // Process didn't exit, send SIGKILL
    let kill_result = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    // Wait a bit more for SIGKILL to take effect
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if finally dead
    let dead = !Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    kill_result.is_ok() && dead
}
