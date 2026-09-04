use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::{OPTIONS, palette};
use crate::core::text::{cell_at_col, line_content, line_graphemes, text_lines};
use crate::editor::{Editor, Mode};

pub fn render(f: &mut Frame, ed: &mut Editor) {
    let area = f.area();
    if area.height < 3 || area.width < 5 {
        return;
    }
    let text_h = area.height - 2;
    let lines_total = text_lines(&ed.buffer.rope);
    let gutter_w: u16 = if OPTIONS.number {
        (lines_total.to_string().len().max(3) + 1) as u16
    } else {
        0
    };
    let text_w = area.width.saturating_sub(gutter_w);
    ed.set_view(text_w as usize, text_h as usize);

    draw_text(
        f,
        ed,
        Rect::new(0, 0, area.width, text_h),
        gutter_w,
        lines_total,
    );
    draw_statusline(f, ed, Rect::new(0, text_h, area.width, 1), lines_total);
    draw_message_line(f, ed, Rect::new(0, text_h + 1, area.width, 1));

    match ed.mode {
        Mode::Command => {
            let x = (1 + ed.cmdline.width() as u16).min(area.width - 1);
            f.set_cursor_position((x, area.height - 1));
        }
        _ => {
            let grs = line_graphemes(
                &line_content(&ed.buffer.rope, ed.cursor.line),
                OPTIONS.tabstop,
            );
            let cell = cell_at_col(&grs, ed.cursor.col);
            let x = gutter_w + cell.saturating_sub(ed.left_cell) as u16;
            let y = (ed.cursor.line - ed.top_line) as u16;
            f.set_cursor_position((x.min(area.width - 1), y.min(text_h - 1)));
        }
    }
}

fn draw_text(f: &mut Frame, ed: &Editor, area: Rect, gutter_w: u16, lines_total: usize) {
    let base = Style::default().bg(palette::BG).fg(palette::FG);
    let mut rows: Vec<Line> = Vec::with_capacity(area.height as usize);

    for row in 0..area.height {
        let line_idx = ed.top_line + row as usize;
        if line_idx >= lines_total {
            // nvim look: blank past end of buffer, no vim-style tildes
            rows.push(Line::default());
            continue;
        }
        let is_cursor_line = line_idx == ed.cursor.line;
        let line_bg = if is_cursor_line && OPTIONS.cursorline {
            palette::CURSORLINE_BG
        } else {
            palette::BG
        };
        let mut spans: Vec<Span> = Vec::new();
        if gutter_w > 0 {
            let num = format!("{:>width$} ", line_idx + 1, width = gutter_w as usize - 1);
            let num_fg = if is_cursor_line {
                palette::GUTTER_CURRENT_FG
            } else {
                palette::GUTTER_FG
            };
            spans.push(Span::styled(
                num,
                Style::default().bg(palette::GUTTER_BG).fg(num_fg),
            ));
        }
        let visible = visible_slice(
            &line_content(&ed.buffer.rope, line_idx),
            ed.left_cell,
            area.width.saturating_sub(gutter_w) as usize,
        );
        spans.push(Span::styled(
            visible,
            Style::default().bg(line_bg).fg(palette::FG),
        ));
        rows.push(Line::from(spans).style(Style::default().bg(line_bg)));
    }

    f.render_widget(Paragraph::new(rows).style(base), area);
}

/// Cuts the horizontally-scrolled window out of a line, expanding tabs.
fn visible_slice(content: &str, left: usize, width: usize) -> String {
    let mut out = String::new();
    let mut cell = 0usize;
    for g in content.graphemes(true) {
        let w = if g == "\t" {
            OPTIONS.tabstop - (cell % OPTIONS.tabstop)
        } else {
            g.width().max(1)
        };
        let start = cell;
        cell += w;
        if cell <= left {
            continue;
        }
        if start >= left + width {
            break;
        }
        if g == "\t" {
            let vis_from = start.max(left);
            let vis_to = cell.min(left + width);
            out.push_str(&" ".repeat(vis_to - vis_from));
        } else if start < left || cell > left + width {
            // partially visible wide grapheme
            let vis_from = start.max(left);
            let vis_to = cell.min(left + width);
            out.push_str(&" ".repeat(vis_to.saturating_sub(vis_from)));
        } else {
            out.push_str(g);
        }
    }
    out
}

fn draw_statusline(f: &mut Frame, ed: &Editor, area: Rect, lines_total: usize) {
    let (label, color) = match ed.mode {
        Mode::Normal if ed.pending.is_idle() => (" NORMAL ", palette::MODE_NORMAL),
        Mode::Normal => (" NORMAL ", palette::MODE_PENDING),
        Mode::Insert => (" INSERT ", palette::MODE_INSERT),
        Mode::Command => (" COMMAND ", palette::MODE_COMMAND),
    };
    let name = ed
        .buffer
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string());
    let modified = if ed.buffer.is_modified() { " [+]" } else { "" };
    let left = format!(" {name}{modified}");
    let pct = (ed.cursor.line + 1) * 100 / lines_total.max(1);
    let right = format!(" {}:{}  {pct}% ", ed.cursor.line + 1, ed.cursor.col + 1);

    let used = label.width() + left.width() + right.width();
    let pad = (area.width as usize).saturating_sub(used);
    let line = Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .bg(color)
                .fg(palette::MODE_LABEL_FG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{left}{}", " ".repeat(pad)),
            Style::default()
                .bg(palette::STATUSLINE_BG)
                .fg(palette::STATUSLINE_FG),
        ),
        Span::styled(
            right,
            Style::default().bg(palette::STATUSLINE_BG).fg(palette::FG),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_message_line(f: &mut Frame, ed: &Editor, area: Rect) {
    let bg = Style::default().bg(palette::BG);
    let line = if ed.mode == Mode::Command {
        Line::styled(format!(":{}", ed.cmdline), bg.fg(palette::FG))
    } else if let Some(msg) = &ed.message {
        let style = if msg.error {
            bg.fg(palette::ERROR_FG).add_modifier(Modifier::BOLD)
        } else {
            bg.fg(palette::FG)
        };
        Line::styled(msg.text.clone(), style)
    } else {
        let pending = ed.pending_display();
        let pad = (area.width as usize).saturating_sub(pending.width() + 1);
        Line::from(vec![
            Span::styled(" ".repeat(pad), bg),
            Span::styled(pending, bg.fg(palette::COMMENT)),
        ])
    };
    f.render_widget(Paragraph::new(line).style(bg), area);
}
