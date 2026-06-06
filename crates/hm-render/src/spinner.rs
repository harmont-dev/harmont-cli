use indicatif::{ProgressBar, ProgressStyle};

/// A simple indeterminate spinner for long-running operations.
#[derive(Debug)]
pub struct Spinner {
    bar: ProgressBar,
}

impl Spinner {
    /// Create and start a spinner with the given message.
    ///
    /// # Panics
    ///
    /// Panics if the embedded progress-bar template fails to compile.
    /// The template is a string literal in this function and is exercised
    /// by every spinner construction; a panic here is a compile-time-fixable
    /// mistake, not a runtime data dependency.
    #[must_use]
    pub fn new(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();
        #[expect(
            clippy::expect_used,
            reason = "static string literal template; a parse failure is a coding mistake caught at startup"
        )]
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .expect("static spinner template is well-formed")
                .tick_strings(&[
                    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}",
                    "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}", "\u{2800}",
                ]),
        );
        bar.set_message(message.to_string());
        bar.enable_steady_tick(std::time::Duration::from_millis(80));
        Self { bar }
    }

    /// Finish the spinner with a success message.
    pub fn finish_with_message(&self, message: &str) {
        self.bar.finish_with_message(message.to_string());
    }

    /// Finish and clear the spinner.
    pub fn finish_and_clear(&self) {
        self.bar.finish_and_clear();
    }
}
