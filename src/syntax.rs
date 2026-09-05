//! Per-buffer syntax highlighting cache.
//!
//! Tree-sitter is the single highlighting mechanism (docs/decisions.md).
//! The engine drives tree-sitter core directly with deterministic layering
//! (the ready-made highlighter mangles event order across injection layers):
//! captures paint a per-byte canvas parents-before-children, injected
//! languages (markdown fences, inline markdown, …) are parsed with included
//! ranges and painted on top, innermost last. The canvas is folded into
//! per-line char spans for the renderer.
//!
//! A buffer re-highlights whenever its version changes (checked lazily at
//! render); unregistered languages render in the default color.

use std::collections::HashMap;

use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};

use crate::config::languages::{self, LangConfig};
use crate::core::buffer::Buffer;
use crate::core::text::text_lines;
use crate::editor::BufId;

/// Buffers beyond this size render plain (keeps huge files responsive).
const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;
/// Injection nesting cap (markdown → rust → doc-comment markdown → …).
const MAX_DEPTH: usize = 4;
const UNSTYLED: u16 = u16::MAX;

/// One styled run within a line: char columns `start..end`, theme capture.
pub type LineSpan = (u32, u32, u16);

struct Cached {
    version: u64,
    lines: Vec<Vec<LineSpan>>,
}

#[derive(Default)]
pub struct Syntax {
    cache: HashMap<BufId, Cached>,
}

impl Syntax {
    /// Refreshes the cache for a buffer if its content changed. Returns
    /// whether highlights exist for it.
    pub fn ensure(&mut self, id: BufId, buffer: &Buffer) -> bool {
        let Some(lang) = languages::detect(buffer.path.as_deref()) else {
            self.cache.remove(&id);
            return false;
        };
        if let Some(cached) = self.cache.get(&id)
            && cached.version == buffer.version()
        {
            return true;
        }
        let lines = highlight(&buffer.rope, lang);
        self.cache.insert(
            id,
            Cached {
                version: buffer.version(),
                lines,
            },
        );
        true
    }

    pub fn line_spans(&self, id: BufId, line: usize) -> &[LineSpan] {
        self.cache
            .get(&id)
            .and_then(|c| c.lines.get(line))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn has_highlights(&self, id: BufId) -> bool {
        self.cache.contains_key(&id)
    }

    pub fn invalidate(&mut self, id: BufId) {
        self.cache.remove(&id);
    }
}

/// Highlights a standalone snippet (fenced code in previews) into per-line
/// spans, resolving the language through the registry incl. aliases.
pub fn highlight_text(text: &str, lang: &str) -> Vec<Vec<LineSpan>> {
    let line_count = text.split('\n').count();
    let mut lines: Vec<Vec<LineSpan>> = vec![Vec::new(); line_count];
    let Some(config) = languages::config_for(lang) else {
        return lines;
    };
    let mut canvas = vec![UNSTYLED; text.len()];
    paint_layer(text.as_bytes(), config, None, 0, &mut canvas);
    fold_canvas(text, &canvas, &mut lines);
    lines
}

/// Highlights a whole buffer into per-line spans. Any failure (or an
/// oversized buffer) yields no spans — plain rendering, never an error.
fn highlight(rope: &Rope, lang: &str) -> Vec<Vec<LineSpan>> {
    let line_count = text_lines(rope);
    let mut lines: Vec<Vec<LineSpan>> = vec![Vec::new(); line_count];
    if rope.len_bytes() > MAX_HIGHLIGHT_BYTES {
        return lines;
    }
    let Some(config) = languages::config_for(lang) else {
        return lines;
    };
    let source = rope.to_string();
    let mut canvas = vec![UNSTYLED; source.len()];
    paint_layer(source.as_bytes(), config, None, 0, &mut canvas);
    fold_canvas(&source, &canvas, &mut lines);
    lines
}

/// Parses (a region of) the source with one language and paints its
/// captures, then recurses into its injections.
fn paint_layer(
    source: &[u8],
    config: &'static LangConfig,
    ranges: Option<&[tree_sitter::Range]>,
    depth: usize,
    canvas: &mut [u16],
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let mut parser = Parser::new();
    if parser.set_language(&config.language).is_err() {
        return;
    }
    if let Some(ranges) = ranges
        && parser.set_included_ranges(ranges).is_err()
    {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };

    // captures sorted so parents paint before children (start asc, end desc);
    // later query patterns override earlier ones on identical ranges
    // parents paint before children (start asc, end desc). Identical-range
    // ties: bundled patterns resolve FIRST-wins (the official tree-sitter
    // convention their queries assume — e.g. json keys, python function
    // names), while registry `highlights_extra` patterns always override.
    // "Paints later" = "wins", so the rank orders accordingly.
    let mut paints: Vec<(usize, usize, u16, (u8, usize))> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(&config.highlights, tree.root_node(), source);
    while let Some(m) = it.next() {
        let rank = if m.pattern_index >= config.extra_pattern_start {
            (1u8, m.pattern_index)
        } else {
            (0u8, config.extra_pattern_start - m.pattern_index)
        };
        for cap in m.captures() {
            if let Some(theme) = config.theme_map[cap.index as usize] {
                let r = cap.node.byte_range();
                if r.start < r.end {
                    paints.push((r.start, r.end, theme, rank));
                }
            }
        }
    }
    paints.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.3.cmp(&b.3))
    });
    let canvas_len = canvas.len();
    for (start, end, theme, _) in paints {
        canvas[start..end.min(canvas_len)].fill(theme);
    }

    // injections: each match contributes content node(s) + a language name
    let Some(injections) = &config.injections else {
        return;
    };
    let content_idx = injections
        .capture_names()
        .iter()
        .position(|n| *n == "injection.content");
    let language_idx = injections
        .capture_names()
        .iter()
        .position(|n| *n == "injection.language");
    let Some(content_idx) = content_idx else {
        return;
    };

    let mut jobs: Vec<(String, Vec<tree_sitter::Range>)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(injections, tree.root_node(), source);
    while let Some(m) = it.next() {
        // language: #set! injection.language "x", or the captured node's text
        let mut language = injections
            .property_settings(m.pattern_index)
            .iter()
            .find(|p| p.key.as_ref() == "injection.language")
            .and_then(|p| p.value.as_ref())
            .map(|v| v.to_string());
        let mut content: Vec<Node> = Vec::new();
        for cap in m.captures() {
            if Some(cap.index as usize) == language_idx {
                language = cap
                    .node
                    .utf8_text(source)
                    .ok()
                    .map(|s| s.trim().to_string());
            } else if cap.index as usize == content_idx {
                content.push(cap.node);
            }
        }
        if let Some(language) = language
            && !content.is_empty()
        {
            jobs.push((language, content.iter().map(Node::range).collect()));
        }
    }
    for (language, ranges) in jobs {
        if let Some(inner) = languages::config_for(&language) {
            paint_layer(source, inner, Some(&ranges), depth + 1, canvas);
        }
    }
}

/// Converts the per-byte canvas into per-line char spans, merging runs.
fn fold_canvas(source: &str, canvas: &[u16], lines: &mut [Vec<LineSpan>]) {
    let bytes = source.as_bytes();
    let mut byte = 0usize;
    let mut line = 0usize;
    let mut col = 0u32;
    let mut run_start = 0u32;
    let mut run_style = UNSTYLED;

    let flush = |lines: &mut [Vec<LineSpan>], line: usize, start: u32, end: u32, style: u16| {
        if style != UNSTYLED
            && end > start
            && let Some(spans) = lines.get_mut(line)
        {
            spans.push((start, end, style));
        }
    };

    while byte < bytes.len() {
        if bytes[byte] == b'\n' {
            flush(lines, line, run_start, col, run_style);
            line += 1;
            col = 0;
            run_start = 0;
            run_style = UNSTYLED;
            byte += 1;
            continue;
        }
        let style = canvas[byte];
        if style != run_style {
            flush(lines, line, run_start, col, run_style);
            run_start = col;
            run_style = style;
        }
        let len = match bytes[byte] {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        };
        byte += len;
        col += 1;
    }
    flush(lines, line, run_start, col, run_style);
}
