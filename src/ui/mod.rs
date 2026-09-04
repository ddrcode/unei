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

/// Everything a window needs to draw itself, resolved from either the live
/// editor fields (focused) or a parked `WinState`.
struct WinView<'a> {
    rope: &'a ropey::Rope,
    name: String,
    modified: bool,
    cursor_line: usize,
    cursor_col: usize,
    top_line: usize,
    left_cell: usize,
    focused: bool,
}

fn gutter_width(lines_total: usize) -> u16 {
    if OPTIONS.number {
        (lines_total.to_string().len().max(3) + 1) as u16
    } else {
        0
    }
}

pub fn render(f: &mut Frame, ed: &mut Editor) {
    let area = f.area();
    if area.height < 3 || area.width < 5 {
        return;
    }
    let windows_area = Rect::new(0, 0, area.width, area.height - 1);
    ed.set_window_area(windows_area);

    // focused window scrolls against its own text dimensions before drawing
    let rects = ed.window_rects();
    let focused_id = ed.focused_window_id();
    if let Some((_, rect)) = rects.iter().find(|(id, _)| *id == focused_id) {
        let lines_total = text_lines(&ed.buffer.rope);
        let text_w = rect.width.saturating_sub(gutter_width(lines_total));
        ed.set_view(text_w as usize, rect.height.saturating_sub(1) as usize);
    }

    for (id, rect) in &rects {
        if rect.height < 2 || rect.width < 3 {
            continue;
        }
        let view = if *id == focused_id {
            WinView {
                rope: &ed.buffer.rope,
                name: buffer_display_name(ed.buffer.path.as_deref()),
                modified: ed.buffer.is_modified(),
                cursor_line: ed.cursor.line,
                cursor_col: ed.cursor.col,
                top_line: ed.top_line,
                left_cell: ed.left_cell,
                focused: true,
            }
        } else {
            let Some(state) = ed.parked_window(*id) else {
                continue;
            };
            let buffer = ed.buffer_ref(state.buf_id);
            let last = text_lines(&buffer.rope) - 1;
            WinView {
                rope: &buffer.rope,
                name: buffer_display_name(buffer.path.as_deref()),
                modified: buffer.is_modified(),
                cursor_line: state.cursor.line.min(last),
                cursor_col: state.cursor.col,
                top_line: state.top_line.min(last),
                left_cell: state.left_cell,
                focused: false,
            }
        };
        draw_window(f, ed, &view, *rect);
    }

    // separator columns between vertical splits
    let sep_style = Style::default().bg(palette::BG).fg(palette::COMMENT);
    for sep in ed.window_separators() {
        let col: Vec<Line> = (0..sep.height)
            .map(|_| Line::styled("│", sep_style))
            .collect();
        f.render_widget(Paragraph::new(col), sep);
    }

    draw_message_line(f, ed, Rect::new(0, area.height - 1, area.width, 1));

    if ed.buffer_list.is_some() {
        draw_buffer_list(f, ed, windows_area);
        return; // the overlay owns the focus; no text cursor
    }

    match ed.mode {
        Mode::Command => {
            let x = (1 + ed.cmdline.width() as u16).min(area.width - 1);
            f.set_cursor_position((x, area.height - 1));
        }
        _ => {
            if let Some((_, rect)) = rects.iter().find(|(id, _)| *id == focused_id) {
                let lines_total = text_lines(&ed.buffer.rope);
                let grs = line_graphemes(
                    &line_content(&ed.buffer.rope, ed.cursor.line),
                    OPTIONS.tabstop,
                );
                let cell = cell_at_col(&grs, ed.cursor.col);
                let x =
                    rect.x + gutter_width(lines_total) + cell.saturating_sub(ed.left_cell) as u16;
                let y = rect.y + (ed.cursor.line - ed.top_line) as u16;
                f.set_cursor_position((
                    x.min(rect.x + rect.width - 1),
                    y.min(rect.y + rect.height.saturating_sub(2)),
                ));
            }
        }
    }
}

fn buffer_display_name(path: Option<&std::path::Path>) -> String {
    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string())
}

/// One window: its text area plus its own statusline row at the bottom.
fn draw_window(f: &mut Frame, ed: &Editor, view: &WinView, rect: Rect) {
    let text_rect = Rect::new(rect.x, rect.y, rect.width, rect.height - 1);
    let status_rect = Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1);
    draw_text(f, view, text_rect);
    draw_statusline(f, ed, view, status_rect);
}

/// Centered floating buffer list, material-style: rounded border, contrast
/// background. `%`/`#` mark current/alternate like vim's :ls, `[+]` modified.
fn draw_buffer_list(f: &mut Frame, ed: &Editor, area: Rect) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear};

    let Some(list) = &ed.buffer_list else { return };
    let entries = ed.buffer_entries();

    let rows: Vec<String> = entries
        .iter()
        .map(|e| {
            let flag = if e.current {
                '%'
            } else if e.alternate {
                '#'
            } else {
                ' '
            };
            let modified = if e.modified { " [+]" } else { "" };
            format!(" {:>2} {} {}{} ", e.id, flag, e.name, modified)
        })
        .collect();

    let content_w = rows.iter().map(|r| r.width()).max().unwrap_or(0).max(16) as u16;
    let w = (content_w + 2).min(area.width.saturating_sub(2));
    let h = (entries.len() as u16 + 2).min(area.height);
    let x = area.width.saturating_sub(w) / 2;
    let y = area.height.saturating_sub(h) / 3;
    let rect = Rect::new(x, y, w, h);

    let float = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let style = if i == list.selected {
                Style::default()
                    .bg(palette::MODE_NORMAL)
                    .fg(palette::MODE_LABEL_FG)
                    .add_modifier(Modifier::BOLD)
            } else {
                float
            };
            Line::styled(
                format!("{r:<width$}", width = w.saturating_sub(2) as usize),
                style,
            )
        })
        .collect();

    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).style(float).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT))
                .title(" buffers ")
                .title_style(Style::default().bg(palette::FLOAT_BG).fg(palette::FG)),
        ),
        rect,
    );
}

fn draw_text(f: &mut Frame, view: &WinView, area: Rect) {
    let base = Style::default().bg(palette::BG).fg(palette::FG);
    let lines_total = text_lines(view.rope);
    let gutter_w = gutter_width(lines_total);
    let mut rows: Vec<Line> = Vec::with_capacity(area.height as usize);

    for row in 0..area.height {
        let line_idx = view.top_line + row as usize;
        if line_idx >= lines_total {
            // nvim look: blank past end of buffer, no vim-style tildes
            rows.push(Line::default());
            continue;
        }
        // cursorline only in the focused window, like vim
        let is_cursor_line = view.focused && line_idx == view.cursor_line;
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
            &line_content(view.rope, line_idx),
            view.left_cell,
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

/// Per-window statusline: the focused window carries the mode badge (and a
/// zoom flag); unfocused windows show a dimmed name only, nvim-style.
fn draw_statusline(f: &mut Frame, ed: &Editor, view: &WinView, area: Rect) {
    let lines_total = text_lines(view.rope);
    let modified = if view.modified { " [+]" } else { "" };
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0usize;

    if view.focused {
        let (label, color) = match ed.mode {
            Mode::Normal if ed.pending.is_idle() => (" NORMAL ", palette::MODE_NORMAL),
            Mode::Normal => (" NORMAL ", palette::MODE_PENDING),
            Mode::Insert => (" INSERT ", palette::MODE_INSERT),
            Mode::Command => (" COMMAND ", palette::MODE_COMMAND),
        };
        spans.push(Span::styled(
            label,
            Style::default()
                .bg(color)
                .fg(palette::MODE_LABEL_FG)
                .add_modifier(Modifier::BOLD),
        ));
        used += label.width();
    }

    let zoom = if view.focused && ed.is_zoomed() {
        " [Z]"
    } else {
        ""
    };
    let left = format!(" {}{modified}{zoom}", view.name);
    let name_fg = if view.focused {
        palette::STATUSLINE_FG
    } else {
        palette::GUTTER_FG
    };
    let pct = (view.cursor_line + 1) * 100 / lines_total.max(1);
    let right = if view.focused {
        format!(" {}:{}  {pct}% ", view.cursor_line + 1, view.cursor_col + 1)
    } else {
        String::new()
    };

    // trim the name from the left when the window is narrow
    let avail = (area.width as usize).saturating_sub(used + right.width());
    let left = if left.width() > avail {
        let tail: String = left
            .chars()
            .rev()
            .scan(0usize, |w, c| {
                *w += c.to_string().width();
                (*w < avail).then_some(c)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    } else {
        left
    };
    let pad = avail.saturating_sub(left.width());
    spans.push(Span::styled(
        format!("{left}{}", " ".repeat(pad)),
        Style::default().bg(palette::STATUSLINE_BG).fg(name_fg),
    ));
    if !right.is_empty() {
        spans.push(Span::styled(
            right,
            Style::default().bg(palette::STATUSLINE_BG).fg(palette::FG),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
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
