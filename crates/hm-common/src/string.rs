//! String-related utilities and extension traits.
//!
//! [`align`] and [`ellipsis`] are ported from atuin
//! (`crates/atuin-common/src/string/`), MIT-licensed. See
//! <https://github.com/atuinsh/atuin>. Upstream gates these behind a `unicode`
//! Cargo feature; here they are always compiled.

pub mod align;
pub mod ellipsis;
pub mod escape;

pub use align::{AlignExt, Alignment};
pub use ellipsis::{EllipsizeExt, Ellipsized, Indicator, Pos};
pub use escape::EscapeNonPrintablePosixExt;

use unicode_width::UnicodeWidthStr;

/// How much room to truncate or pad into, and the unit it is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// A UTF-8 byte budget.
    Bytes(usize),
    /// A display-column budget via `unicode-width` - a double-width glyph such
    /// as `世` or `🦀` counts as two. Use for presentation.
    Columns(usize),
}

impl Measure {
    /// The numeric limit, in this budget's own unit.
    pub(crate) const fn amount(self) -> usize {
        match self {
            Self::Bytes(n) | Self::Columns(n) => n,
        }
    }

    /// Total cost of `s` in this budget's unit.
    pub(crate) fn cost(self, s: &str) -> usize {
        match self {
            Self::Bytes(_) => s.len(),
            Self::Columns(_) => s.width(),
        }
    }
}
