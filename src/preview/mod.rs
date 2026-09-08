//! Preview projections (ticket #39): a buffer rendered *as some tool sees
//! it* — the reader (markdown, phase 1), the compiler (Rust, #93). Each
//! renderer is a pure function of the buffer (plus, for Rust, what
//! rust-analyzer knows) producing styled lines and a LineMap back to
//! source lines; the editor caches the result per buffer and re-derives it
//! lazily at render, the syntax-cache pattern.

pub mod markdown;
pub mod rust;

use ratatui::style::Style;

/// One styled fragment of a rendered line.
pub type Frag = (String, Style);

pub struct PreviewDoc {
    /// Rendered lines as styled fragments.
    pub lines: Vec<Vec<Frag>>,
    /// Rendered line → source line (block start).
    pub map: Vec<usize>,
    version: u64,
    width: usize,
    /// Whatever else the projection depended on — the Rust view's hint and
    /// diagnostic stamps; zero for renderers that need only the text.
    stamp: u64,
}

impl PreviewDoc {
    pub(crate) fn new(
        lines: Vec<Vec<Frag>>,
        map: Vec<usize>,
        version: u64,
        width: usize,
        stamp: u64,
    ) -> Self {
        Self {
            lines,
            map,
            version,
            width,
            stamp,
        }
    }

    /// First rendered line at or after the given source line.
    pub fn view_line_for_source(&self, source_line: usize) -> usize {
        match self.map.binary_search(&source_line) {
            Ok(mut i) => {
                while i > 0 && self.map[i - 1] == source_line {
                    i -= 1;
                }
                i
            }
            Err(i) => i.min(self.map.len().saturating_sub(1)),
        }
    }

    pub fn source_line_for_view(&self, view_line: usize) -> usize {
        self.map
            .get(view_line.min(self.map.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    }

    pub fn is_fresh(&self, version: u64, width: usize, stamp: u64) -> bool {
        self.version == version && self.width == width && self.stamp == stamp
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The rendered text of one line, fragments joined (tests, probes).
    pub fn plain(&self, view_line: usize) -> String {
        self.lines
            .get(view_line)
            .map(|f| f.iter().map(|(t, _)| t.as_str()).collect())
            .unwrap_or_default()
    }
}
