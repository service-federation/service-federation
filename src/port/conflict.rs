use std::process::Command;

#[derive(Debug, Clone)]
pub struct PortConflict {
    pub port: u16,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
}

impl PortConflict {
    /// Check if a port is in use and return conflict info
    pub fn check(port: u16) -> Option<Self> {
        // Check if port is actually in use by trying to bind to both addresses.
        // On macOS, binding to 127.0.0.1 can succeed even when 0.0.0.0 is in use,
        // so we need to check both.
        let localhost_available = std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        let any_available = std::net::TcpListener::bind(("0.0.0.0", port)).is_ok();

        if localhost_available && any_available {
            return None; // Port is truly available
        }

        // Port is in use - try to find ALL processes using it
        let processes = Self::find_processes_on_port(port);

        Some(PortConflict { port, processes })
    }

    /// Check if a port is available (can bind to it)
    pub fn is_port_available(port: u16) -> bool {
        std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
            && std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
    }

    /// Find ALL processes using a port (cross-platform)
    fn find_processes_on_port(port: u16) -> Vec<ProcessInfo> {
        #[cfg(target_os = "macos")]
        {
            Self::find_processes_macos(port)
        }

        #[cfg(target_os = "linux")]
        {
            Self::find_processes_linux(port)
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Vec::new()
        }
    }

    #[cfg(target_os = "macos")]
    fn find_processes_macos(port: u16) -> Vec<ProcessInfo> {
        // Use lsof to find ALL processes on macOS
        let output = match Command::new("lsof")
            .args(["-i", &format!(":{}", port), "-P", "-n", "-F", "pcn"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        if !output.status.success() {
            return Vec::new();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut processes = Vec::new();
        let mut seen_pids = std::collections::HashSet::new();
        let mut current_pid: Option<u32> = None;
        let mut current_command: Option<String> = None;

        // Parse lsof output (field format: pPID, cCOMMAND, nNAME)
        // Each process block starts with 'p' line
        for line in stdout.lines() {
            if let Some(stripped) = line.strip_prefix('p') {
                // New process - save the previous one if valid
                if let Some(pid) = current_pid {
                    if !seen_pids.contains(&pid) {
                        seen_pids.insert(pid);
                        processes.push(ProcessInfo {
                            pid,
                            name: current_command
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string()),
                            command: current_command.clone(),
                        });
                    }
                }
                current_pid = stripped.parse::<u32>().ok();
                current_command = None;
            } else if let Some(stripped) = line.strip_prefix('c') {
                current_command = Some(stripped.to_string());
            }
        }

        // Don't forget the last process
        if let Some(pid) = current_pid {
            if !seen_pids.contains(&pid) {
                processes.push(ProcessInfo {
                    pid,
                    name: current_command
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    command: current_command,
                });
            }
        }

        processes
    }

    #[cfg(target_os = "linux")]
    fn find_processes_linux(port: u16) -> Vec<ProcessInfo> {
        // Combine results from ss and lsof for completeness
        let mut processes = Self::find_processes_linux_ss(port);
        let lsof_processes = Self::find_processes_linux_lsof(port);

        // Add lsof results that aren't already in the list
        let seen_pids: std::collections::HashSet<u32> = processes.iter().map(|p| p.pid).collect();
        for p in lsof_processes {
            if !seen_pids.contains(&p.pid) {
                processes.push(p);
            }
        }

        processes
    }

    #[cfg(target_os = "linux")]
    fn find_processes_linux_ss(port: u16) -> Vec<ProcessInfo> {
        let output = match Command::new("ss")
            .args(["-tlnp", &format!("sport = :{}", port)])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        if !output.status.success() {
            return Vec::new();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut processes = Vec::new();
        let mut seen_pids = std::collections::HashSet::new();

        // Parse ss output: look for pid=PID,fd=...
        for line in stdout.lines().skip(1) {
            // Skip header
            if let Some(users_part) = line.split_whitespace().last() {
                // ss can report multiple pids per line
                for part in users_part.split(',') {
                    if let Some(pid_str) = part.strip_prefix("pid=") {
                        if let Ok(pid) = pid_str.parse::<u32>() {
                            if !seen_pids.contains(&pid) {
                                seen_pids.insert(pid);
                                // Get process name from /proc
                                let name = std::fs::read_to_string(format!("/proc/{}/comm", pid))
                                    .ok()
                                    .map(|s| s.trim().to_string())
                                    .unwrap_or_else(|| "unknown".to_string());

                                let command =
                                    std::fs::read_to_string(format!("/proc/{}/cmdline", pid))
                                        .ok()
                                        .map(|s| s.replace('\0', " ").trim().to_string());

                                processes.push(ProcessInfo { pid, name, command });
                            }
                        }
                    }
                }
            }
        }

        processes
    }

    #[cfg(target_os = "linux")]
    fn find_processes_linux_lsof(port: u16) -> Vec<ProcessInfo> {
        let output = match Command::new("lsof")
            .args(["-i", &format!(":{}", port), "-P", "-n", "-F", "pcn"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        if !output.status.success() {
            return Vec::new();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut processes = Vec::new();
        let mut seen_pids = std::collections::HashSet::new();
        let mut current_pid: Option<u32> = None;
        let mut current_command: Option<String> = None;

        for line in stdout.lines() {
            if let Some(stripped) = line.strip_prefix('p') {
                if let Some(pid) = current_pid {
                    if !seen_pids.contains(&pid) {
                        seen_pids.insert(pid);
                        processes.push(ProcessInfo {
                            pid,
                            name: current_command
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string()),
                            command: current_command.clone(),
                        });
                    }
                }
                current_pid = stripped.parse::<u32>().ok();
                current_command = None;
            } else if let Some(stripped) = line.strip_prefix('c') {
                current_command = Some(stripped.to_string());
            }
        }

        if let Some(pid) = current_pid {
            if !seen_pids.contains(&pid) {
                processes.push(ProcessInfo {
                    pid,
                    name: current_command
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    command: current_command,
                });
            }
        }

        processes
    }

    /// Kill ALL processes using the port (except the current process)
    pub fn kill_all_blocking_processes(&self) -> Vec<(u32, std::result::Result<(), String>)> {
        let current_pid = std::process::id();
        let mut results = Vec::new();

        #[cfg(unix)]
        {
            for process in &self.processes {
                // Never kill ourselves - we may be holding the port to reserve it
                if process.pid == current_pid {
                    tracing::debug!(
                        "Skipping self (PID {}) when killing port conflicts on port {}",
                        process.pid,
                        self.port
                    );
                    continue;
                }

                let result = Command::new("kill")
                    .arg(process.pid.to_string())
                    .status()
                    .map_err(|e| format!("Failed to kill process {}: {}", process.pid, e))
                    .and_then(|status| {
                        if status.success() {
                            Ok(())
                        } else {
                            Err(format!("kill exited with status: {}", status))
                        }
                    });
                results.push((process.pid, result));
            }
        }

        #[cfg(not(unix))]
        {
            for process in &self.processes {
                // Never kill ourselves
                if process.pid == current_pid {
                    tracing::debug!(
                        "Skipping self (PID {}) when killing port conflicts on port {}",
                        process.pid,
                        self.port
                    );
                    continue;
                }

                results.push((
                    process.pid,
                    Err("Killing processes is only supported on Unix".to_string()),
                ));
            }
        }

        results
    }

    /// Kill all processes and verify port becomes available, with retries
    pub fn kill_and_verify(&self, max_attempts: u32) -> std::result::Result<(), String> {
        for attempt in 1..=max_attempts {
            // Kill all processes we know about
            let _results = self.kill_all_blocking_processes();

            // Short wait for processes to die
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Check if port is now available
            if Self::is_port_available(self.port) {
                return Ok(());
            }

            // Port still in use - find any new processes that appeared
            if attempt < max_attempts {
                if let Some(new_conflict) = Self::check(self.port) {
                    if !new_conflict.processes.is_empty() {
                        // Kill these too
                        let _ = new_conflict.kill_all_blocking_processes();
                        std::thread::sleep(std::time::Duration::from_millis(100));

                        if Self::is_port_available(self.port) {
                            return Ok(());
                        }
                    }
                }

                // Exponential backoff: 100ms, 200ms, 400ms...
                std::thread::sleep(std::time::Duration::from_millis(100 * (1 << attempt)));
            }
        }

        Err(format!(
            "Port {} still in use after {} attempts to kill blocking processes",
            self.port, max_attempts
        ))
    }
}
