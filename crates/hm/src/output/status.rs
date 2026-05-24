use owo_colors::OwoColorize;

/// Print a success message to stdout.
pub fn print_success(msg: &str) {
    let check = format!("{}", "\u{2714}".green().bold());
    tracing::info!(target: "user::stdout", "{check} {msg}");
}

/// Print a warning message to stderr.
pub fn print_warning(msg: &str) {
    let bang = format!("{}", "!".yellow().bold());
    tracing::info!(target: "user::stderr", "{bang} {msg}");
}

/// Print an error message to stderr.
pub fn print_error(msg: &str) {
    let cross = format!("{}", "\u{2718}".red().bold());
    tracing::info!(target: "user::stderr", "{cross} {msg}");
}

/// Print an info message to stdout.
pub fn print_info(msg: &str) {
    let arrow = format!("{}", "\u{25b6}".cyan());
    tracing::info!(target: "user::stdout", "{arrow} {msg}");
}
