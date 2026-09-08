//! The compiler's-eye view (#93): a Rust buffer with everything
//! rust-analyzer knows written into the text — every inferred type, every
//! elided lifetime, inferred generic arguments, parameter names, the
//! reborrows and binding modes the language lets you leave out. Rust before
//! elision. Diagnostics follow their line in full, where the editing view
//! can only afford a truncated ghost at the end of it.
//!
//! The projected text is highlighted *as Rust*, so annotations take exactly
//! the colours they would have had if written by hand. A pure function of
//! (buffer, hints, diagnostics, width); every rendered row maps back to its
//! source line, so follow and jump-to-source work unchanged.

use ratatui::style::{Modifier, Style};
use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Frag, PreviewDoc};
use crate::config::{OPTIONS, palette, theme};
use crate::core::text::{Gr, line_content, line_graphemes, text_lines, wrap_rows};
use crate::lsp::{Diagnostic, InlayHint, Severity};
use crate::syntax::{LineSpan, highlight_text};

/// Renders the projection at `width` cells. `hints` must belong to this
/// text (the editor only passes hints for the buffer's current revision);
/// `stamp` records which hints and diagnostics went in, for freshness.
pub fn render(
    rope: &Rope,
    version: u64,
    width: usize,
    hints: &[InlayHint],
    diags: &[Diagnostic],
    stamp: u64,
) -> PreviewDoc {
    let n = text_lines(rope);
    let gutter_w = n.to_string().len() + 1; // digits plus a space
    let avail = width.saturating_sub(gutter_w).max(10);
    let gutter = Style::default().fg(palette::GUTTER_FG);

    let mut by_line: Vec<Vec<&InlayHint>> = vec![Vec::new(); n];
    for h in hints {
        if let Some(slot) = by_line.get_mut(h.line) {
            slot.push(h);
        }
    }
    let annotated: Vec<String> = (0..n)
        .map(|i| annotate(&line_content(rope, i), &by_line[i]))
        .collect();
    let spans = highlight_text(&annotated.join("\n"), "rust");

    let mut lines: Vec<Vec<Frag>> = Vec::with_capacity(n);
    let mut map: Vec<usize> = Vec::with_capacity(n);
    for (i, text) in annotated.iter().enumerate() {
        let grs = line_graphemes(text, OPTIONS.tabstop);
        let line_spans = spans.get(i).map(Vec::as_slice).unwrap_or(&[]);
        for (ri, row) in wrap_rows(&grs, avail).iter().enumerate() {
            let number = if ri == 0 {
                format!("{:>w$} ", i + 1, w = gutter_w - 1)
            } else {
                " ".repeat(gutter_w)
            };
            let mut frags = vec![(number, gutter)];
            frags.extend(styled_cells(
                text,
                &grs,
                row.start_col,
                row.end_col,
                line_spans,
            ));
            lines.push(frags);
            map.push(i);
        }
        for d in diags.iter().filter(|d| d.line == i) {
            let color = match d.severity {
                Severity::Error => palette::RED,
                Severity::Warning => palette::YELLOW,
                Severity::Info => continue,
            };
            let style = Style::default().fg(color).add_modifier(Modifier::ITALIC);
            let message = d.message.lines().filter(|l| !l.trim().is_empty());
            for (k, msg_line) in message.enumerate() {
                let pieces = wrap_words(msg_line.trim(), avail.saturating_sub(4).max(8));
                for (j, piece) in pieces.into_iter().enumerate() {
                    let lead = if k == 0 && j == 0 { "  ↳ " } else { "    " };
                    lines.push(vec![
                        (" ".repeat(gutter_w), gutter),
                        (lead.to_string(), style),
                        (piece, style),
                    ]);
                    map.push(i);
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(vec![("(empty file)".into(), gutter)]);
        map.push(0);
    }
    PreviewDoc::new(lines, map, version, width, stamp)
}

/// Writes a line's hints into its text. Hints inside the line go in at
/// their byte column (labels arrive padded as the server asked); hints at
/// the end of the line — chaining types, closing-brace labels — become one
/// trailing comment. A column that is not a char boundary drops that hint,
/// never the line.
fn annotate(text: &str, hints: &[&InlayHint]) -> String {
    let content_end = text.trim_end().len();
    let mut inline: Vec<(usize, &str)> = Vec::new();
    let mut trailing: Vec<&str> = Vec::new();
    for h in hints {
        let col = h.col.min(text.len());
        if col >= content_end {
            let label = h.label.trim().trim_start_matches("//").trim_start();
            if !label.is_empty() {
                trailing.push(label);
            }
        } else if text.is_char_boundary(col) {
            inline.push((col, &h.label));
        }
    }
    inline.sort_by_key(|(c, _)| *c);
    let mut out = String::with_capacity(text.len() + 32);
    let mut last = 0usize;
    for (col, label) in inline {
        out.push_str(&text[last..col]);
        out.push_str(label);
        last = col;
    }
    out.push_str(&text[last..]);
    if !trailing.is_empty() {
        out.truncate(out.trim_end().len());
        out.push_str("  // ");
        out.push_str(&trailing.join(" · "));
    }
    out
}

/// The graphemes of `text` in char columns `from..to` as styled fragments:
/// tabs expanded to their cells, theme colours from the highlight spans.
fn styled_cells(text: &str, grs: &[Gr], from: usize, to: usize, spans: &[LineSpan]) -> Vec<Frag> {
    let base = Style::default().fg(palette::FG);
    let mut out: Vec<Frag> = Vec::new();
    let mut si = 0usize;
    for (g, s) in grs.iter().zip(text.graphemes(true)) {
        if g.char_off < from {
            continue;
        }
        if g.char_off >= to {
            break;
        }
        while si < spans.len() && spans[si].1 as usize <= g.char_off {
            si += 1;
        }
        let style = match spans.get(si) {
            Some((start, _, cap)) if *start as usize <= g.char_off => {
                theme::capture_style(*cap as usize)
            }
            _ => base,
        };
        let piece: std::borrow::Cow<str> = if s == "\t" {
            " ".repeat(g.width).into()
        } else {
            s.into()
        };
        match out.last_mut() {
            Some((t, st)) if *st == style => t.push_str(&piece),
            _ => out.push((piece.into_owned(), style)),
        }
    }
    out
}

/// Greedy word wrap for diagnostic text; a word longer than the width is
/// split by chars rather than left to overflow.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let mut word = word.to_string();
        while word.width() > width {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            out.push(head);
        }
        let need = if line.is_empty() {
            word.width()
        } else {
            line.width() + 1 + word.width()
        };
        if need > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(line: usize, col: usize, label: &str) -> InlayHint {
        InlayHint {
            line,
            col,
            label: label.to_string(),
            kind: None,
        }
    }

    #[test]
    fn annotate_splices_inline_and_folds_end_of_line_hints_into_a_comment() {
        let h1 = hint(0, 5, ": u32");
        let h2 = hint(0, 10, "n: ");
        let h3 = hint(0, 16, "Iter<Item>"); // at the end → comment
        let h4 = hint(0, 16, " fn total"); // closing-brace style, padded
        let text = "let x = f(1) + y";
        assert_eq!(
            annotate(text, &[&h3, &h2, &h1, &h4]),
            "let x: u32 = f(n: 1) + y  // Iter<Item> · fn total",
            "sorted by column; trailing hints joined as one comment"
        );
    }

    #[test]
    fn annotate_drops_a_hint_on_a_non_boundary_column_not_the_line() {
        let inside = hint(0, 1, ": X"); // byte 1 of "é" (2 bytes)
        let ok = hint(0, 2, ": Y");
        assert_eq!(annotate("é=1", &[&inside, &ok]), "é: Y=1");
    }

    #[test]
    fn wrap_words_breaks_at_spaces_and_splits_long_words() {
        assert_eq!(
            wrap_words("the quick brown fox", 9),
            vec!["the quick", "brown fox"]
        );
        assert_eq!(wrap_words("abcdefghij k", 4), vec!["abcd", "efgh", "ij k"]);
        assert!(wrap_words("", 10).is_empty());
    }

    #[test]
    fn render_numbers_lines_wraps_rows_and_lists_diagnostics_in_full() {
        let rope =
            Rope::from_str("fn main() {\n    let sum = items.iter().map(|i| i.price).sum();\n}\n");
        let hints = vec![
            hint(1, 11, ": u32"),
            hint(1, 33, ": &Item"),
            hint(1, 47, "::<u32>"),
        ];
        let diags = vec![Diagnostic {
            line: 1,
            col: 4,
            end_line: 1,
            end_col: 7,
            severity: Severity::Error,
            message: "cannot find value `items` in this scope\nnot found in this scope".into(),
        }];
        // wide: every annotation lands in the text, rows map 1:1 plus the
        // diagnostic rows under their line
        let doc = render(&rope, 7, 120, &hints, &diags, 1);
        assert_eq!(doc.plain(0), "1 fn main() {");
        assert_eq!(
            doc.plain(1),
            "2     let sum: u32 = items.iter().map(|i: &Item| i.price).sum::<u32>();"
        );
        assert_eq!(
            doc.plain(2),
            "    ↳ cannot find value `items` in this scope"
        );
        assert_eq!(
            doc.plain(3),
            "      not found in this scope",
            "every message line, not just the headline"
        );
        assert_eq!(doc.plain(4), "3 }");
        assert_eq!(doc.map, vec![0, 1, 1, 1, 2]);
        assert!(doc.is_fresh(7, 120, 1));
        assert!(!doc.is_fresh(7, 120, 2), "hints or diagnostics changed");
        // narrow: 44 cells minus the gutter leaves 42, so the annotated line
        // wraps at a word boundary onto a blank-gutter continuation row
        let doc = render(&rope, 7, 44, &hints, &diags, 1);
        assert_eq!(doc.plain(1), "2     let sum: u32 = items.iter().map(|i: ");
        assert_eq!(doc.plain(2), "  &Item| i.price).sum::<u32>();");
        assert_eq!(doc.map[2], 1, "continuation row maps to the same line");
    }

    #[test]
    fn render_without_hints_is_the_numbered_source() {
        let rope = Rope::from_str("let a = 1;\n\tlet b = a;\n");
        let doc = render(&rope, 1, 80, &[], &[], 0);
        assert_eq!(doc.plain(0), "1 let a = 1;");
        assert_eq!(
            doc.plain(1),
            "2     let b = a;",
            "tab expanded to its cells"
        );
        assert_eq!(doc.map, vec![0, 1]);
    }
}
