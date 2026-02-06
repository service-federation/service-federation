use std::io::Write;

/// Abstraction over user-facing output.
///
/// Command modules use this trait instead of `println!`/`eprintln!` so that
/// output can be suppressed in TUI mode (which renders its own UI) or
/// redirected to JSON in a future machine-readable mode.
pub trait UserOutput: Send + Sync {
    /// Informational status message (e.g., "Stopping all services...")
    fn status(&self, message: &str);

    /// Success message (e.g., "All services started successfully!")
    fn success(&self, message: &str);

    /// Warning message (e.g., "Failed to clean orphaned containers")
    fn warning(&self, message: &str);

    /// Error message (e.g., "Error starting dependency 'foo'")
    fn error(&self, message: &str);

    /// Inline progress (no trailing newline). Call `finish_progress` after.
    fn progress(&self, message: &str);

    /// Finish an inline progress line with a result.
    fn finish_progress(&self, result: &str);

    /// A blank line separator.
    fn blank(&self);
}

/// Standard CLI output — writes to stdout/stderr with ANSI colors.
pub struct CliOutput;

impl UserOutput for CliOutput {
    fn status(&self, message: &str) {
        println!("{}", message);
    }

    fn success(&self, message: &str) {
        println!("{}", message);
    }

    fn warning(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn error(&self, message: &str) {
        eprintln!("\x1b[31m{}\x1b[0m", message);
    }

    fn progress(&self, message: &str) {
        print!("{}", message);
        std::io::stdout().flush().ok();
    }

    fn finish_progress(&self, result: &str) {
        println!("{}", result);
    }

    fn blank(&self) {
        println!();
    }
}

/// Suppresses all output. Used in TUI mode where the TUI renders its own UI.
pub struct QuietOutput;

impl UserOutput for QuietOutput {
    fn status(&self, _message: &str) {}
    fn success(&self, _message: &str) {}
    fn warning(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
    fn progress(&self, _message: &str) {}
    fn finish_progress(&self, _result: &str) {}
    fn blank(&self) {}
}
