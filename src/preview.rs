//! Markdown preview (ticket #39, phase 1): a buffer projected into a
//! rendered document. Pure function of (buffer, width), cached by version —
//! the syntax-cache pattern. Every rendered line carries its source line
//! (the LineMap), which powers follow-scrolling and jump-to-source.

use ratatui::style::{Color, Modifier, Style};
use ropey::Rope;
use tree_sitter::{Node, Parser};
use unicode_width::UnicodeWidthStr;

use crate::config::palette;

/// One styled fragment of a rendered line.
pub type Frag = (String, Style);

pub struct PreviewDoc {
    /// Rendered lines as styled fragments.
    pub lines: Vec<Vec<Frag>>,
    /// Rendered line → source line (block start).
    pub map: Vec<usize>,
    version: u64,
    width: usize,
}

impl PreviewDoc {
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

    pub fn is_fresh(&self, version: u64, width: usize) -> bool {
        self.version == version && self.width == width
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

// ----------------------------------------------------------------------
// styles ("make it pretty")
// ----------------------------------------------------------------------

fn body() -> Style {
    Style::default().fg(palette::FG)
}
fn heading(level: usize) -> Style {
    let color = match level {
        1 => palette::BLUE,
        2 => palette::CYAN,
        3 => palette::YELLOW,
        _ => palette::PALE,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
fn heading_bar(level: usize) -> Style {
    heading(level)
}
fn dim() -> Style {
    Style::default().fg(palette::COMMENT)
}
fn code_chip() -> Style {
    Style::default().fg(palette::GREEN).bg(palette::FLOAT_BG)
}
fn code_block_bg() -> Color {
    palette::FLOAT_BG
}
fn quote_text() -> Style {
    Style::default()
        .fg(palette::PALE)
        .add_modifier(Modifier::ITALIC)
}
fn link_style() -> Style {
    Style::default()
        .fg(palette::CYAN)
        .add_modifier(Modifier::UNDERLINED)
}

// ----------------------------------------------------------------------
// inline rendering: styled fragments with markdown markers stripped
// ----------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Ink {
    bold: bool,
    italic: bool,
    strike: bool,
}

fn inline_frags(text: &str, base: Style) -> Vec<Frag> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .is_err()
    {
        return vec![(text.to_string(), base)];
    }
    let Some(tree) = parser.parse(text, None) else {
        return vec![(text.to_string(), base)];
    };
    let mut out = Vec::new();
    walk_inline(
        tree.root_node(),
        text,
        base,
        Ink {
            bold: false,
            italic: false,
            strike: false,
        },
        &mut out,
    );
    if out.is_empty() {
        out.push((text.to_string(), base));
    }
    out
}

fn apply_ink(mut style: Style, ink: Ink) -> Style {
    if ink.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if ink.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if ink.strike {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

fn push_text(out: &mut Vec<Frag>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    // collapse hard newlines inside inline content; wrapping re-flows later
    let clean = text.replace('\n', " ");
    match out.last_mut() {
        Some((prev, s)) if *s == style => prev.push_str(&clean),
        _ => out.push((clean, style)),
    }
}

fn walk_inline(node: Node, src: &str, base: Style, ink: Ink, out: &mut Vec<Frag>) {
    let mut cursor = node.walk();
    let mut pos = node.start_byte();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if children.is_empty() {
        push_text(
            out,
            &src[node.start_byte()..node.end_byte()],
            apply_ink(base, ink),
        );
        return;
    }
    for child in children {
        if child.start_byte() > pos {
            push_text(out, &src[pos..child.start_byte()], apply_ink(base, ink));
        }
        match child.kind() {
            "emphasis" => walk_inline(
                child,
                src,
                base,
                Ink {
                    italic: true,
                    ..ink
                },
                out,
            ),
            "strong_emphasis" => walk_inline(child, src, base, Ink { bold: true, ..ink }, out),
            "strikethrough" => walk_inline(
                child,
                src,
                base,
                Ink {
                    strike: true,
                    ..ink
                },
                out,
            ),
            "code_span" => {
                let inner = &src[child.start_byte()..child.end_byte()];
                let inner = inner.trim_matches('`');
                push_text(out, &format!(" {inner} "), code_chip());
            }
            "emphasis_delimiter" | "code_span_delimiter" => {}
            "inline_link" | "image" => {
                let mut lc = child.walk();
                let mut label = String::new();
                let mut dest = String::new();
                for part in child.children(&mut lc) {
                    match part.kind() {
                        "link_text" | "image_description" => {
                            label = src[part.start_byte()..part.end_byte()].to_string();
                        }
                        "link_destination" => {
                            dest = src[part.start_byte()..part.end_byte()].to_string();
                        }
                        _ => {}
                    }
                }
                if label.is_empty() {
                    label = dest.clone();
                }
                let prefix = if child.kind() == "image" { "🖼 " } else { "" };
                push_text(
                    out,
                    &format!("{prefix}{label}"),
                    apply_ink(link_style(), ink),
                );
                if !dest.is_empty() && dest != label {
                    push_text(out, &format!(" ({dest})"), dim());
                }
            }
            "uri_autolink" => {
                let inner = &src[child.start_byte()..child.end_byte()];
                push_text(
                    out,
                    inner.trim_matches(['<', '>']),
                    apply_ink(link_style(), ink),
                );
            }
            "backslash_escape" => {
                let t = &src[child.start_byte()..child.end_byte()];
                push_text(out, t.trim_start_matches('\\'), apply_ink(base, ink));
            }
            _ => walk_inline(child, src, base, ink, out),
        }
        pos = child.end_byte();
    }
    if node.end_byte() > pos {
        push_text(out, &src[pos..node.end_byte()], apply_ink(base, ink));
    }
}

// ----------------------------------------------------------------------
// block rendering
// ----------------------------------------------------------------------

struct Renderer {
    lines: Vec<Vec<Frag>>,
    map: Vec<usize>,
    width: usize,
}

impl Renderer {
    fn emit(&mut self, source_line: usize, frags: Vec<Frag>) {
        // the map must stay monotonic for binary search: separator lines
        // emitted after a container inherit the running position
        let line = self.map.last().map_or(source_line, |l| source_line.max(*l));
        self.lines.push(frags);
        self.map.push(line);
    }

    fn blank(&mut self, source_line: usize) {
        if !self.lines.is_empty() && !self.lines.last().is_some_and(Vec::is_empty) {
            self.emit(source_line, Vec::new());
        }
    }

    /// Word-wraps styled fragments to the content width, with a prefix on
    /// the first line and a hang indent on continuations.
    fn emit_wrapped(
        &mut self,
        source_line: usize,
        prefix: Vec<Frag>,
        hang: &str,
        frags: Vec<Frag>,
    ) {
        let prefix_w: usize = prefix.iter().map(|(t, _)| t.width()).sum();
        let avail = self.width.saturating_sub(prefix_w).max(10);
        let mut line: Vec<Frag> = prefix.clone();
        let mut col = 0usize;
        let mut first = true;
        for (text, style) in frags {
            for word in split_words(&text) {
                let w = word.width();
                if col > 0 && col + w > avail {
                    self.emit(source_line, std::mem::take(&mut line));
                    line.push((hang.to_string(), body()));
                    col = 0;
                    first = false;
                }
                if col == 0 && word.trim().is_empty() {
                    continue; // no leading spaces after a wrap
                }
                match line.last_mut() {
                    Some((prev, s)) if *s == style => prev.push_str(&word),
                    _ => line.push((word.to_string(), style)),
                }
                col += w;
            }
        }
        let _ = first;
        if !line.is_empty() {
            self.emit(source_line, line);
        }
    }
}

/// Splits text into words and single spaces (keeping both).
fn split_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch == ' ' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(" ".to_string());
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

fn render_block(node: Node, src: &str, r: &mut Renderer, depth: usize) {
    let line = node.start_position().row;
    match node.kind() {
        "atx_heading" | "setext_heading" => {
            let level = heading_level(node, src);
            let mut content = Vec::new();
            let mut c = node.walk();
            for child in node.children(&mut c) {
                if child.kind() == "inline" || child.kind() == "paragraph" {
                    content = inline_frags(node_text(child, src).trim(), heading(level));
                }
            }
            r.blank(line);
            r.emit_wrapped(
                line,
                vec![("▌ ".to_string(), heading_bar(level))],
                "  ",
                content,
            );
            if level == 1 {
                let text_w: usize = r
                    .lines
                    .last()
                    .map(|l| l.iter().map(|(t, _)| t.width()).sum())
                    .unwrap_or(0);
                r.emit(line, vec![("─".repeat(text_w.clamp(8, r.width)), dim())]);
            }
            r.blank(line);
        }
        "paragraph" => {
            let frags = inline_frags(node_text(node, src).trim_end(), body());
            r.emit_wrapped(line, Vec::new(), "", frags);
            r.blank(line);
        }
        "fenced_code_block" | "indented_code_block" => {
            let mut lang = String::new();
            let mut content = "";
            let mut content_line = line;
            let mut c = node.walk();
            for child in node.children(&mut c) {
                match child.kind() {
                    "info_string" => lang = node_text(child, src).trim().to_string(),
                    "code_fence_content" => {
                        content = node_text(child, src);
                        content_line = child.start_position().row;
                    }
                    _ => {}
                }
            }
            if node.kind() == "indented_code_block" {
                content = node_text(node, src);
            }
            let content = content.trim_end_matches('\n');
            let spans = crate::syntax::highlight_text(content, &lang);
            let bg = code_block_bg();
            for (i, code_line) in content.split('\n').enumerate() {
                let mut frags: Vec<Frag> = vec![("  ".into(), Style::default().bg(bg))];
                let mut last = 0usize;
                let chars: Vec<char> = code_line.chars().collect();
                for (s, e, cap) in spans.get(i).map(Vec::as_slice).unwrap_or(&[]) {
                    let (s, e) = (*s as usize, (*e as usize).min(chars.len()));
                    if s > last {
                        let t: String = chars[last..s].iter().collect();
                        frags.push((t, Style::default().fg(palette::FG).bg(bg)));
                    }
                    let t: String = chars[s.min(chars.len())..e].iter().collect();
                    let style = crate::config::theme::capture_style(*cap as usize).bg(bg);
                    frags.push((t, style));
                    last = e;
                }
                if last < chars.len() {
                    let t: String = chars[last..].iter().collect();
                    frags.push((t, Style::default().fg(palette::FG).bg(bg)));
                }
                // pad the chip to full width
                let used: usize = frags.iter().map(|(t, _)| t.width()).sum();
                if used < r.width {
                    frags.push((" ".repeat(r.width - used), Style::default().bg(bg)));
                }
                r.emit(content_line + i, frags);
            }
            r.blank(line);
        }
        "block_quote" => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                if child.kind() == "paragraph" {
                    let raw = node_text(child, src);
                    let clean: String = raw
                        .lines()
                        .map(|l| l.trim_start_matches(['>', ' ']))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let frags = inline_frags(clean.trim(), quote_text());
                    r.emit_wrapped(
                        child.start_position().row,
                        vec![("▎ ".to_string(), dim())],
                        "▎ ",
                        frags,
                    );
                } else if child.kind() == "block_quote" {
                    render_block(child, src, r, depth + 1);
                }
            }
            r.blank(line);
        }
        "list" => {
            render_list(node, src, r, depth);
            if depth == 0 {
                r.blank(line);
            }
        }
        "thematic_break" => {
            r.blank(line);
            r.emit(line, vec![("─".repeat(r.width.min(40)), dim())]);
            r.blank(line);
        }
        "section" | "document" => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                render_block(child, src, r, depth);
            }
        }
        "pipe_table" => {
            for (i, row) in node_text(node, src).trim_end().split('\n').enumerate() {
                let style = if i == 1 { dim() } else { body() };
                r.emit(line + i, vec![(row.to_string(), style)]);
            }
            r.blank(line);
        }
        "html_block" | "minus_metadata" | "plus_metadata" => {
            for (i, raw) in node_text(node, src).trim_end().split('\n').enumerate() {
                r.emit(line + i, vec![(raw.to_string(), dim())]);
            }
            r.blank(line);
        }
        _ => {
            let mut c = node.walk();
            let kids: Vec<Node> = node.children(&mut c).collect();
            if kids.is_empty() {
                let t = node_text(node, src).trim_end();
                if !t.is_empty() {
                    r.emit(line, vec![(t.to_string(), body())]);
                }
            } else {
                for child in kids {
                    render_block(child, src, r, depth);
                }
            }
        }
    }
}

fn heading_level(node: Node, _src: &str) -> usize {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "atx_h1_marker" | "setext_h1_underline" => return 1,
            "atx_h2_marker" | "setext_h2_underline" => return 2,
            "atx_h3_marker" => return 3,
            "atx_h4_marker" => return 4,
            "atx_h5_marker" => return 5,
            "atx_h6_marker" => return 6,
            _ => {}
        }
    }
    1
}

fn render_list(node: Node, src: &str, r: &mut Renderer, depth: usize) {
    let bullets = ["•", "◦", "▪"];
    let indent = "  ".repeat(depth);
    let mut c = node.walk();
    let mut ordinal = 0usize;
    for item in node.children(&mut c) {
        if item.kind() != "list_item" {
            continue;
        }
        ordinal += 1;
        let line = item.start_position().row;
        let mut marker = format!("{indent}{} ", bullets[depth.min(2)]);
        let mut task: Option<bool> = None;
        let mut ic = item.walk();
        for part in item.children(&mut ic) {
            match part.kind() {
                "list_marker_dot" | "list_marker_parenthesis" => {
                    marker = format!("{indent}{ordinal}. ");
                }
                "task_list_marker_checked" => task = Some(true),
                "task_list_marker_unchecked" => task = Some(false),
                _ => {}
            }
        }
        if let Some(done) = task {
            let tick = if done { "✓" } else { "○" };
            marker = format!("{indent}{tick} ");
        }
        let hang = " ".repeat(marker.width());
        let marker_style = if task == Some(true) {
            Style::default().fg(palette::GREEN)
        } else {
            Style::default().fg(palette::ORANGE)
        };
        let mut emitted_para = false;
        let mut ic = item.walk();
        for part in item.children(&mut ic) {
            match part.kind() {
                "paragraph" => {
                    let base = if task == Some(true) {
                        body()
                            .add_modifier(Modifier::CROSSED_OUT)
                            .fg(palette::COMMENT)
                    } else {
                        body()
                    };
                    let frags = inline_frags(node_text(part, src).trim(), base);
                    if emitted_para {
                        r.emit_wrapped(
                            part.start_position().row,
                            vec![(hang.clone(), body())],
                            &hang,
                            frags,
                        );
                    } else {
                        r.emit_wrapped(line, vec![(marker.clone(), marker_style)], &hang, frags);
                        emitted_para = true;
                    }
                }
                "list" => render_list(part, src, r, depth + 1),
                "fenced_code_block" => render_block(part, src, r, depth + 1),
                _ => {}
            }
        }
        if !emitted_para {
            r.emit(line, vec![(marker.clone(), marker_style)]);
        }
    }
}

/// Renders a markdown buffer at the given content width.
pub fn render(rope: &Rope, version: u64, width: usize) -> PreviewDoc {
    let src = rope.to_string();
    let width = width.clamp(20, 100);
    let mut renderer = Renderer {
        lines: Vec::new(),
        map: Vec::new(),
        width,
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .is_ok()
        && let Some(tree) = parser.parse(src.as_bytes(), None)
    {
        render_block(tree.root_node(), &src, &mut renderer, 0);
    }
    while renderer.lines.last().is_some_and(Vec::is_empty) {
        renderer.lines.pop();
        renderer.map.pop();
    }
    if renderer.lines.is_empty() {
        renderer
            .lines
            .push(vec![("(empty document)".into(), dim())]);
        renderer.map.push(0);
    }
    PreviewDoc {
        lines: renderer.lines,
        map: renderer.map,
        version,
        width,
    }
}
