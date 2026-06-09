//! Build-event renderers shared by the `hm` CLI and the cloud plugin.
//!
//! This crate owns the output layer: the [`OutputRenderer`] trait, the
//! [`OutputMode`] selection enum, and the concrete renderers
//! ([`HumanRenderer`], [`ProgressRenderer`], [`JsonRenderer`]). All of
//! them consume [`hm_plugin_protocol::BuildEvent`]s; nothing here depends
//! on `hm` internals (no `RunContext`, no Docker types).

use std::fmt;
use std::io::IsTerminal;

use hm_plugin_protocol::BuildEvent;

/// Whether ANSI color should be used: honors an explicit no-color flag,
/// the `NO_COLOR` env convention, and whether stderr is a TTY.
///
/// Single source of truth for the color rule, shared by the `hm` host
/// context and the cloud plugin's render preferences.
#[must_use]
pub fn color_enabled(no_color_flag: bool) -> bool {
    !no_color_flag && std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

/// Whether stderr is an interactive terminal (drives the progress view).
#[must_use]
pub fn stderr_interactive() -> bool {
    std::io::stderr().is_terminal()
}

/// Whether stdout is NOT a TTY (i.e. piped) — used to force the streaming log view.
#[must_use]
pub fn stdout_piped() -> bool {
    !std::io::stdout().is_terminal()
}

pub mod human;
pub mod json;
pub mod progress;
pub mod spinner;

pub use human::HumanRenderer;
pub use json::JsonRenderer;
pub use progress::ProgressRenderer;

/// Synchronous observer of [`BuildEvent`]s.
///
/// Implementations format events for human consumption (progress bars,
/// coloured log lines) or machine consumption (JSON-lines).
pub trait OutputRenderer: Send + fmt::Debug {
    /// Called once per event in emission order.
    fn on_event(&mut self, event: &BuildEvent);
}

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

/// Build the renderer for a run.
///
/// `format` is the `--format` value (`"human"` or `"json"`); `color`
/// controls ANSI output; `logs` forces the streaming [`HumanRenderer`]
/// over the [`ProgressRenderer`] view (set by `--logs` or a CI
/// environment). Mirrors the prior inline selection in
/// `hm`'s `commands/run/local.rs`.
///
/// # Errors
///
/// Returns an error when `format` is neither `"human"` nor `"json"`.
pub fn renderer_for(
    format: &str,
    color: bool,
    logs: bool,
) -> anyhow::Result<Box<dyn OutputRenderer>> {
    match format {
        "json" => Ok(Box::new(JsonRenderer::new(std::io::stdout()))),
        "human" if logs => Ok(Box::new(HumanRenderer::new(std::io::stderr(), color))),
        "human" => Ok(Box::new(ProgressRenderer::new(std::io::stderr(), color))),
        other => anyhow::bail!("unknown --format '{other}'\n  available: human, json"),
    }
}

/// Drive a renderer from an mpsc stream of events.
///
/// Consumes events until the channel closes or a `BuildEnd` is seen.
/// Mirrors the local broadcast `output_subscriber` loop, but for a
/// single-consumer channel (used by the cloud path).
pub async fn drive(
    mut renderer: Box<dyn OutputRenderer>,
    mut rx: tokio::sync::mpsc::Receiver<BuildEvent>,
) {
    while let Some(ev) = rx.recv().await {
        let end = ev.is_build_end();
        renderer.on_event(&ev);
        if end {
            break;
        }
    }
}

/// Drive a renderer from a [`Stream`] of events until it ends or a
/// `BuildEnd` is seen.
///
/// The `hm-exec` backend handle yields events as a
/// `BoxStream<'static, BuildEvent>`; this function is the counterpart to
/// [`drive`] for that case.
///
/// [`Stream`]: futures::stream::Stream
pub async fn drive_stream(
    mut renderer: Box<dyn OutputRenderer>,
    mut events: futures::stream::BoxStream<'static, BuildEvent>,
) {
    use futures::StreamExt as _;
    while let Some(ev) = events.next().await {
        let end = ev.is_build_end();
        renderer.on_event(&ev);
        if end {
            break;
        }
    }
}
