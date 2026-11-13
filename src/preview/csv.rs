//! The spreadsheet's view (#39 phase 3): a CSV or TSV buffer as the grid a
//! spreadsheet would show — one row per record, columns at their natural
//! width, a rule under the bold header, numbers right-aligned, the source
//! line number of every record in a dim gutter. A pure function of (buffer,
//! width, first visible column); every row maps to the source line its
//! record starts on, so follow and jump-to-source work unchanged.
//!
//! Wide tables do not shrink to fit — a 17-column bill of materials squeezed
//! into 100 cells reads as nothing — they scroll by column (`j`/`l` in the
//! preview), the gutter staying put. A column is capped at half the pane so
//! one URL column cannot push the rest off screen.

use ratatui::style::{Modifier, Style};
use ropey::Rope;
use unicode_width::UnicodeWidthStr;

use super::{Frag, PreviewDoc, clip_frags};
use crate::config::palette;
use crate::core::text::text_lines;

/// One parsed record: its cells and the source line it starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub line: usize,
    pub cells: Vec<String>,
}

const CANDIDATES: [char; 4] = [',', ';', '\t', '|'];

/// The separator the file uses: tab for a `.tsv`, otherwise the busiest of
/// `,` `;` tab `|` on the header line, counted outside quotes; comma when
/// none appears (a one-column file).
pub fn sniff_delimiter(text: &str, tab: bool) -> char {
    if tab {
        return '\t';
    }
    let header = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut counts = [0usize; CANDIDATES.len()];
    let mut quoted = false;
    for c in header.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && let Some(i) = CANDIDATES.iter().position(|&d| d == c) {
            counts[i] += 1;
        }
    }
    let (best, n) = counts
        .iter()
        .enumerate()
        .fold((0, 0), |acc, (i, &n)| if n > acc.1 { (i, n) } else { acc });
    if n == 0 { ',' } else { CANDIDATES[best] }
}

/// RFC 4180 records: quoted fields, `""` escapes, delimiters and newlines
/// inside quotes. A CR before LF belongs to the line end, a blank line is no
/// record, and the trailing newline makes no empty record.
pub fn parse(text: &str, delim: char) -> Vec<Record> {
    let mut records = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut line = 0usize; // source line of the char being read
    let mut start = 0usize; // source line the current record began on
    let mut quoted = false; // inside a quoted field
    let mut fresh = true; // at the start of a field — a quote here opens one
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cell.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                '\n' => {
                    cell.push('\n');
                    line += 1;
                }
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if fresh => {
                quoted = true;
                fresh = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                line += 1;
                if !cells.is_empty() || !cell.trim().is_empty() {
                    cells.push(std::mem::take(&mut cell));
                    records.push(Record {
                        line: start,
                        cells: std::mem::take(&mut cells),
                    });
                }
                cell.clear();
                fresh = true;
                start = line;
            }
            c if c == delim => {
                cells.push(std::mem::take(&mut cell));
                fresh = true;
            }
            c => {
                cell.push(c);
                fresh = false;
            }
        }
    }
    if !cells.is_empty() || !cell.trim().is_empty() {
        cells.push(cell);
        records.push(Record { line: start, cells });
    }
    records
}

/// A cell as one row shows it: a newline inside a quoted field becomes `⏎`,
/// a tab a space, other control characters vanish.
fn display_text(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    for c in cell.chars() {
        match c {
            '\n' => out.push('⏎'),
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Drops the whitespace a row ends with — the padding of an empty last
/// cell, the space after the last separator — so rows end where their text
/// does.
fn trim_row(mut frags: Vec<Frag>) -> Vec<Frag> {
    while let Some((text, _)) = frags.last_mut() {
        let kept = text.trim_end().len();
        text.truncate(kept);
        if text.is_empty() {
            frags.pop(); // an empty cell, or padding that was all spaces
        } else {
            break;
        }
    }
    frags
}

/// Renders the grid at `width` cells, columns from `first_col` on. `tab`
/// says the file is a `.tsv`; anything else has its delimiter sniffed.
/// The document's stamp is the first visible column, so scrolling by
/// column re-renders exactly as resizing does.
pub fn render(rope: &Rope, version: u64, width: usize, first_col: usize, tab: bool) -> PreviewDoc {
    let text = rope.to_string();
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let records = parse(text, sniff_delimiter(text, tab));
    let ncols = records.iter().map(|r| r.cells.len()).max().unwrap_or(0);

    let gutter = Style::default().fg(palette::GUTTER_FG);
    let dim = Style::default().fg(palette::COMMENT);
    let body = Style::default().fg(palette::FG);
    if ncols == 0 {
        let lines = vec![vec![("(empty table)".to_string(), dim)]];
        return PreviewDoc::new(lines, vec![0], version, width, 0);
    }
    let first_col = first_col.min(ncols - 1);
    let gutter_w = text_lines(rope).to_string().len() + 1; // digits plus a space
    let avail = width.saturating_sub(gutter_w + 1).max(10); // minus the lead cell
    let cap = (avail / 2).max(16);

    let shown: Vec<Vec<String>> = records
        .iter()
        .map(|r| r.cells.iter().map(|c| display_text(c)).collect())
        .collect();
    let mut widths = vec![1usize; ncols];
    for row in &shown {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.width().min(cap));
        }
    }
    // a column whose every non-empty body cell is a number reads right-aligned
    let numeric: Vec<bool> = (0..ncols)
        .map(|i| {
            let mut seen = false;
            for row in shown.iter().skip(1) {
                let cell = row.get(i).map(String::as_str).unwrap_or("").trim();
                if cell.is_empty() {
                    continue;
                }
                if cell.parse::<f64>().is_err() {
                    return false;
                }
                seen = true;
            }
            seen
        })
        .collect();

    let mut lines: Vec<Vec<Frag>> = Vec::with_capacity(records.len() + 1);
    let mut map: Vec<usize> = Vec::with_capacity(records.len() + 1);
    for (ri, (rec, row)) in records.iter().zip(&shown).enumerate() {
        let style = if ri == 0 {
            body.add_modifier(Modifier::BOLD)
        } else {
            body
        };
        let lead = if first_col > 0 {
            ("‹".to_string(), dim)
        } else {
            (" ".to_string(), body)
        };
        let mut frags: Vec<Frag> = vec![
            (format!("{:>w$} ", rec.line + 1, w = gutter_w - 1), gutter),
            lead,
        ];
        for (i, &col_w) in widths.iter().enumerate().skip(first_col) {
            if i > first_col {
                frags.push((" │ ".to_string(), dim));
            }
            let text = row.get(i).cloned().unwrap_or_default();
            let cell = clip_frags(vec![(text, style)], col_w);
            let used: usize = cell.iter().map(|(t, _)| t.width()).sum();
            let pad = col_w.saturating_sub(used);
            if numeric[i] {
                if pad > 0 {
                    frags.push((" ".repeat(pad), body));
                }
                frags.extend(cell);
            } else {
                frags.extend(cell);
                if pad > 0 && i + 1 < ncols {
                    frags.push((" ".repeat(pad), body));
                }
            }
        }
        lines.push(trim_row(frags));
        map.push(rec.line);
        if ri == 0 {
            // the rule under the header has no line of its own: it maps to
            // the header's
            let mut rule: Vec<Frag> = vec![(" ".repeat(gutter_w), gutter), ("─".to_string(), dim)];
            for (i, &col_w) in widths.iter().enumerate().skip(first_col) {
                if i > first_col {
                    rule.push(("─┼─".to_string(), dim));
                }
                rule.push(("─".repeat(col_w), dim));
            }
            lines.push(rule);
            map.push(rec.line);
        }
    }
    let frozen = lines.len().min(2);
    PreviewDoc::new(lines, map, version, width, first_col as u64)
        .with_table(frozen, first_col, ncols)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(r: &Record) -> Vec<&str> {
        r.cells.iter().map(String::as_str).collect()
    }

    #[test]
    fn parse_handles_quotes_delimiters_and_embedded_newlines() {
        let recs = parse("a,\"b,c\",\"say \"\"hi\"\"\",\"two\nlines\",e\nx,y\n", ',');
        assert_eq!(recs.len(), 2);
        assert_eq!(
            cells(&recs[0]),
            vec!["a", "b,c", "say \"hi\"", "two\nlines", "e"]
        );
        assert_eq!(recs[0].line, 0);
        assert_eq!(
            recs[1].line, 2,
            "the quoted newline counts as a source line"
        );
        assert_eq!(cells(&recs[1]), vec!["x", "y"]);
    }

    #[test]
    fn parse_tolerates_crlf_blank_lines_and_a_trailing_newline() {
        let recs = parse("x,y\r\n\r\n1,\r\n", ',');
        assert_eq!(recs.len(), 2, "the blank line is no record, nor is the end");
        assert_eq!(cells(&recs[0]), vec!["x", "y"]);
        assert_eq!(recs[1].line, 2);
        assert_eq!(
            cells(&recs[1]),
            vec!["1", ""],
            "a trailing empty field survives"
        );
        let recs = parse("a,b", ',');
        assert_eq!(cells(&recs[0]), vec!["a", "b"], "no final newline at all");
        assert!(parse("\n  \n", ',').is_empty());
    }

    #[test]
    fn sniff_prefers_the_busiest_separator_and_defaults_to_comma() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", false), ';');
        assert_eq!(sniff_delimiter("Ref,Value,Notes (a; b)\n", false), ',');
        assert_eq!(sniff_delimiter("a\tb\tc\n", false), '\t');
        assert_eq!(sniff_delimiter("a|b\n", false), '|');
        assert_eq!(sniff_delimiter("single\n", false), ',');
        assert_eq!(
            sniff_delimiter("\"a,b\";\"c\"\n", false),
            ';',
            "commas inside quotes do not count"
        );
        assert_eq!(
            sniff_delimiter("\n\na;b\n", false),
            ';',
            "first non-empty line"
        );
        assert_eq!(
            sniff_delimiter("a,b\n", true),
            '\t',
            "a .tsv is tab whatever it holds"
        );
    }

    #[test]
    fn render_aligns_numbers_right_and_rules_under_the_header() {
        let rope = Rope::from_str("Ref,Qty,Value\nC1,1,10uF\nR1,12,4k7\n");
        let doc = render(&rope, 1, 60, 0, false);
        assert_eq!(doc.plain(0), "1  Ref │ Qty │ Value");
        assert!(doc.plain(1).contains("─┼─"), "{:?}", doc.plain(1));
        assert_eq!(doc.plain(2), "2  C1  │   1 │ 10uF");
        assert_eq!(doc.plain(3), "3  R1  │  12 │ 4k7");
        assert_eq!(
            doc.map,
            vec![0, 0, 1, 2],
            "the rule maps to the header's line"
        );
        assert_eq!(doc.frozen, 2);
        assert_eq!(doc.columns, Some((0, 3)));
        let bold = doc.lines[0]
            .iter()
            .any(|(t, s)| t == "Ref" && s.add_modifier.contains(Modifier::BOLD));
        assert!(bold, "header cells are bold");
        assert!(doc.is_fresh(1, 60, 0));
        assert!(
            !doc.is_fresh(1, 60, 1),
            "a different first column is a different document"
        );
    }

    #[test]
    fn render_scrolls_by_column_and_marks_the_hidden_ones() {
        let rope = Rope::from_str("Ref,Qty,Value\nC1,1,10uF\n");
        let doc = render(&rope, 1, 60, 1, false);
        assert_eq!(doc.plain(0), "1 ‹Qty │ Value");
        assert_eq!(doc.plain(2), "2 ‹  1 │ 10uF");
        assert_eq!(doc.columns, Some((1, 3)));
        assert!(doc.is_fresh(1, 60, 1));
        let doc = render(&rope, 1, 60, 99, false);
        assert_eq!(doc.columns, Some((2, 3)), "clamped to the last column");
        assert_eq!(doc.plain(0), "1 ‹Value");
    }

    #[test]
    fn render_caps_a_column_at_half_the_pane_and_clips_with_an_ellipsis() {
        let url = "https://item.szlcsc.com/datasheet/CL21B106KOQNNNE/97045.html";
        let rope = Rope::from_str(&format!("Ref,Datasheet,Qty\nC1,{url},1\n"));
        // 40 cells: gutter 2 + lead 1 leave 37, so a column is capped at 18
        let doc = render(&rope, 1, 40, 0, false);
        let row = doc.plain(2);
        assert!(row.contains('…'), "{row}");
        assert!(!row.contains("97045"), "clipped, not wrapped: {row}");
        let cell_w = doc.lines[2]
            .iter()
            .skip_while(|(t, _)| !t.starts_with("https"))
            .take_while(|(t, _)| !t.contains('│'))
            .map(|(t, _)| t.width())
            .sum::<usize>();
        assert_eq!(cell_w, 18, "{row}");
        assert_eq!(doc.plain(0), "1  Ref │ Datasheet          │ Qty");
    }

    #[test]
    fn render_strips_the_byte_order_mark_and_shows_a_multiline_cell_on_one_row() {
        let rope = Rope::from_str("\u{feff}Ref,Note\nC1,\"a\nb\"\nC2,x\n");
        let doc = render(&rope, 1, 60, 0, false);
        assert_eq!(doc.plain(0), "1  Ref │ Note");
        assert_eq!(doc.plain(2), "2  C1  │ a⏎b");
        assert_eq!(doc.plain(3), "4  C2  │ x", "numbered by source line");
        assert_eq!(doc.map, vec![0, 0, 1, 3]);
    }

    #[test]
    fn wide_cells_measure_in_display_cells() {
        let rope = Rope::from_str("Maker,Qty\nSAMSUNG(三星),1\nX,2\n");
        let doc = render(&rope, 1, 60, 0, false);
        assert_eq!(doc.plain(2), "2  SAMSUNG(三星) │   1");
        assert_eq!(
            doc.plain(3),
            "3  X             │   2",
            "padded to the CJK cell's width"
        );
    }

    #[test]
    fn an_empty_buffer_is_an_empty_table() {
        let doc = render(&Rope::from_str(""), 1, 60, 0, false);
        assert_eq!(doc.plain(0), "(empty table)");
        assert_eq!(doc.map, vec![0]);
        assert_eq!(doc.columns, None);
    }

    #[test]
    fn a_ragged_table_pads_short_rows_and_an_empty_column_keeps_its_header_width() {
        let rope = Rope::from_str("A,B,Stock\n1,2\n3,4,\n");
        let doc = render(&rope, 1, 60, 0, false);
        assert_eq!(doc.plain(0), "1  A │ B │ Stock");
        assert_eq!(doc.plain(2), "2  1 │ 2 │");
        assert_eq!(doc.plain(3), "3  3 │ 4 │");
    }
}
