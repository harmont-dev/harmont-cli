pub mod format;
pub mod human;
pub mod json;
pub mod spinner;
pub mod status;

/// How to render output. Determined at startup from CLI flags and TTY detection.
#[derive(Debug, Clone)]
pub enum OutputMode {
    Human {
        /// Whether ANSI colors are enabled.
        color: bool,
        /// Whether stdout is an interactive terminal (enables prompts, spinners).
        interactive: bool,
    },
    Json,
}

impl OutputMode {
    /// True when output should be JSON, suitable for scripting.
    #[must_use]
    pub const fn is_json(&self) -> bool {
        matches!(self, Self::Json)
    }

    /// True when output is meant for a human reader (color/spinner-friendly).
    #[must_use]
    pub const fn is_human(&self) -> bool {
        matches!(self, Self::Human { .. })
    }

    /// True when ANSI color codes should be emitted.
    #[must_use]
    pub const fn color_enabled(&self) -> bool {
        matches!(self, Self::Human { color: true, .. })
    }

    /// True when stdout is interactive (allows prompts and spinners).
    #[must_use]
    pub const fn interactive(&self) -> bool {
        matches!(
            self,
            Self::Human {
                interactive: true,
                ..
            }
        )
    }

    /// True when OSC 8 hyperlinks should be emitted (interactive + color).
    #[must_use]
    pub const fn use_hyperlinks(&self) -> bool {
        matches!(
            self,
            Self::Human {
                interactive: true,
                color: true
            }
        )
    }
}
