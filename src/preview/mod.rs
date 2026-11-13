//! Preview projections (ticket #39): a buffer rendered *as some tool sees
//! it* — the reader (markdown, phase 1), the compiler (Rust, #93), the
//! spreadsheet (CSV, phase 3). Each renderer is a pure function of the
//! buffer (plus, for Rust, what rust-analyzer knows) producing styled lines
//! and a LineMap back to source lines; the editor caches the result per
//! buffer and re-derives it lazily at render, the syntax-cache pattern.

pub mod csv;
pub mod markdown;
pub mod rust;

use std::path::Path;

use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::config::{languages, palette};

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
    /// diagnostic stamps, the table's first visible column; zero for
    /// renderers that need only the text.
    stamp: u64,
    /// Rows drawn pinned at the top while the document is scrolled — a
    /// table's header and its rule; zero for everything else.
    pub frozen: usize,
    /// For a table: (first visible column, column count) — what `j`/`l`
    /// scroll in the preview.
    pub columns: Option<(usize, usize)>,
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
            frozen: 0,
            columns: None,
        }
    }

    /// Marks the document a table: `frozen` header rows, columns scrolled
    /// to `first` of `count`.
    pub(crate) fn with_table(mut self, frozen: usize, first: usize, count: usize) -> Self {
        self.frozen = frozen;
        self.columns = Some((first, count));
        self
    }

    /// `top` adjusted so the reading line is not hidden under the pinned
    /// rows: a scrolled table draws its header over the first `frozen`
    /// screen rows, covering the document rows that would be there.
    pub fn top_showing(&self, top: usize, line: usize) -> usize {
        if top > 0 && line < top + self.frozen {
            line.saturating_sub(self.frozen)
        } else {
            top
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

/// Which projection a buffer has, by its path: the grammar registry names
/// markdown and Rust; CSV has no grammar (nothing to colour in source), so
/// its extension decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    Markdown,
    Rust,
    Csv { tab: bool },
}

pub fn projection_for(path: Option<&Path>) -> Option<Projection> {
    match languages::detect(path) {
        Some("markdown") => Some(Projection::Markdown),
        Some("rust") => Some(Projection::Rust),
        _ => {
            let ext = path?.extension()?.to_str()?.to_ascii_lowercase();
            match ext.as_str() {
                "csv" => Some(Projection::Csv { tab: false }),
                "tsv" => Some(Projection::Csv { tab: true }),
                _ => None,
            }
        }
    }
}

/// Clips styled fragments to a display width, ending with a dim ellipsis.
pub(super) fn clip_frags(frags: Vec<Frag>, max: usize) -> Vec<Frag> {
    let total: usize = frags.iter().map(|(t, _)| t.width()).sum();
    if total <= max {
        return frags;
    }
    let dim = Style::default().fg(palette::COMMENT);
    let mut out: Vec<Frag> = Vec::new();
    let mut used = 0usize;
    for (text, style) in frags {
        for g in text.chars() {
            let w = unicode_width::UnicodeWidthChar::width(g).unwrap_or(0);
            if used + w > max.saturating_sub(1) {
                out.push(("…".to_string(), dim));
                return out;
            }
            match out.last_mut() {
                Some((prev, s)) if *s == style => prev.push(g),
                _ => out.push((g.to_string(), style)),
            }
            used += w;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn projection_by_grammar_then_by_extension() {
        let p = |s: &str| projection_for(Some(&PathBuf::from(s)));
        assert_eq!(p("README.md"), Some(Projection::Markdown));
        assert_eq!(p("src/main.rs"), Some(Projection::Rust));
        assert_eq!(p("pcb/bom.csv"), Some(Projection::Csv { tab: false }));
        assert_eq!(p("parts.TSV"), Some(Projection::Csv { tab: true }));
        assert_eq!(p("notes.txt"), None);
        assert_eq!(projection_for(None), None);
    }

    #[test]
    fn top_showing_keeps_the_reading_line_out_from_under_pinned_rows() {
        let doc =
            PreviewDoc::new(vec![Vec::new(); 10], (0..10).collect(), 1, 80, 0).with_table(2, 0, 3);
        assert_eq!(doc.top_showing(0, 1), 0, "unscrolled: nothing is pinned");
        assert_eq!(
            doc.top_showing(4, 7),
            4,
            "reading line visible below the header"
        );
        assert_eq!(
            doc.top_showing(4, 5),
            3,
            "row 5 would sit under the pinned rule"
        );
        assert_eq!(doc.top_showing(4, 1), 0, "reading the header itself");
        let plain = PreviewDoc::new(vec![Vec::new(); 10], (0..10).collect(), 1, 80, 0);
        assert_eq!(plain.top_showing(4, 4), 4, "no pinned rows, no adjustment");
    }
}
