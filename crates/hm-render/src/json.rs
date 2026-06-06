//! JSON-lines [`OutputRenderer`] — replaces the former
//! `hm-plugin-output-json` WASM plugin. Serialises each
//! [`BuildEvent`] as a single JSON line.

use std::fmt;
use std::io::Write;

use hm_plugin_protocol::BuildEvent;

use crate::OutputRenderer;

/// Renders [`BuildEvent`]s as newline-delimited JSON (one object per
/// line). Suitable for piping into `jq` or other machine consumers.
#[derive(Debug)]
pub struct JsonRenderer<W> {
    out: W,
}

impl<W> JsonRenderer<W> {
    /// Create a new renderer writing to `out`.
    #[must_use]
    pub const fn new(out: W) -> Self {
        Self { out }
    }
}

impl<W> OutputRenderer for JsonRenderer<W>
where
    W: Write + Send + fmt::Debug,
{
    fn on_event(&mut self, event: &BuildEvent) {
        if let Ok(mut bytes) = serde_json::to_vec(event) {
            bytes.push(b'\n');
            let _ = self.out.write_all(&bytes);
        }
    }
}
