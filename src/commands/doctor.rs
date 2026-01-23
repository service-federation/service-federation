pub async fn run_doctor() -> anyhow::Result<()> {
    println!("Checking system requirements...\n");

    let mut all_ok = true;

    // Check Docker
    print!("Docker: ");
    match tokio::process::Command::new("docker")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("{}", version);
        }
        _ => {
            println!("Not found");
            all_ok = false;
        }
    }

    // Check docker-compose
    print!("docker-compose: ");
    match tokio::process::Command::new("docker-compose")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("{}", version);
        }
        _ => {
            // Try "docker compose" (newer syntax)
            match tokio::process::Command::new("docker")
                .args(["compose", "version"])
                .output()
                .await
            {
                Ok(output) if output.status.success() => {
                    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    println!("{} (via docker compose)", version);
                }
                _ => {
                    println!("Not found (optional)");
                }
            }
        }
    }

    // Check Gradle
    print!("Gradle: ");
    match tokio::process::Command::new("gradle")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().find(|l| l.contains("Gradle")) {
                println!("{}", line.trim());
            } else {
                println!("Installed");
            }
        }
        _ => {
            println!("Not found (optional, needed for gradle services)");
        }
    }

    // Check Java
    print!("Java: ");
    match tokio::process::Command::new("java")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                println!("{}", line.trim());
            } else {
                println!("Installed");
            }
        }
        _ => {
            println!("Not found (optional, needed for Gradle/Java services)");
        }
    }

    // Check Git
    print!("Git: ");
    match tokio::process::Command::new("git")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("{}", version);
        }
        _ => {
            println!("Not found (optional)");
        }
    }

    println!();
    if all_ok {
        println!("All required dependencies are installed");
    } else {
        println!("Some required dependencies are missing");
        eprintln!("\nInstallation guides:");
        eprintln!("  Docker: https://docs.docker.com/get-docker/");
    }

    Ok(())
}
