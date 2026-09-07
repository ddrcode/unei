use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::{OPTIONS, gutter_width, palette};
use crate::core::text::{cell_at_col, line_content, line_graphemes, text_lines, wrap_rows};
use crate::editor::{Editor, Mode};

/// Everything a window needs to draw itself, resolved from either the live
/// editor fields (focused) or a parked `WinState`.
struct WinView<'a> {
    rope: &'a ropey::Rope,
    buf_id: crate::editor::BufId,
    name: String,
    modified: bool,
    read_only: bool,
    /// The file changed on disk while this buffer holds edits (#80).
    disk_conflict: bool,
    cursor_line: usize,
    cursor_col: usize,
    top_line: usize,
    focused: bool,
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

    // refresh syntax caches for every visible buffer before drawing
    let visible_bufs: Vec<crate::editor::BufId> = rects
        .iter()
        .map(|(id, _)| {
            if *id == focused_id {
                ed.current_buffer_id()
            } else {
                ed.parked_window(*id)
                    .map(|s| s.buf_id)
                    .unwrap_or_else(|| ed.current_buffer_id())
            }
        })
        .collect();
    for b in visible_bufs {
        ed.ensure_syntax(b);
    }
    ed.search_ensure_current();
    ed.sync_preview_follow();

    for (id, rect) in &rects {
        if rect.height < 2 || rect.width < 3 {
            continue;
        }
        if ed.is_preview_window(*id) {
            draw_preview_window(f, ed, *id, *rect, *id == focused_id);
            continue;
        }
        let view = if *id == focused_id {
            WinView {
                rope: &ed.buffer.rope,
                buf_id: ed.current_buffer_id(),
                name: buffer_display_name(ed.buffer.path.as_deref()),
                modified: ed.buffer.is_modified(),
                read_only: ed.buffer.read_only,
                disk_conflict: ed.buffer.disk_conflict,
                cursor_line: ed.cursor.line,
                cursor_col: ed.cursor.col,
                top_line: ed.top_line,
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
                buf_id: state.buf_id,
                name: buffer_display_name(buffer.path.as_deref()),
                modified: buffer.is_modified(),
                read_only: buffer.read_only,
                disk_conflict: buffer.disk_conflict,
                cursor_line: state.cursor.line.min(last),
                cursor_col: state.cursor.col,
                top_line: state.top_line.min(last),
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

    if let Some(lines) = &ed.info_float {
        draw_info_float(f, lines, windows_area);
        return;
    }
    if let Some(menu) = &ed.actions_menu {
        draw_actions_menu(f, menu, windows_area);
        return;
    }
    if ed.file_picker.is_some() {
        draw_file_picker(f, ed, windows_area);
        return; // the picker positions its own query cursor
    }
    if ed.buffer_list.is_some() {
        draw_buffer_list(f, ed, windows_area);
        return; // the overlay owns the focus; no text cursor
    }

    match ed.mode {
        Mode::Command => {
            let x = (1 + ed.cmdline.width() as u16).min(area.width - 1);
            f.set_cursor_position((x, area.height - 1));
        }
        _ if ed.focused_is_preview() => {
            if let Some((_, rect)) = rects.iter().find(|(id, _)| *id == focused_id) {
                let (line, top) = ed.preview_nav_state(focused_id);
                let y =
                    rect.y + (line.saturating_sub(top) as u16).min(rect.height.saturating_sub(2));
                f.set_cursor_position((rect.x, y));
            }
        }
        _ => {
            if let Some((_, rect)) = rects.iter().find(|(id, _)| *id == focused_id) {
                let lines_total = text_lines(&ed.buffer.rope);
                // the cursor's display row and cell within it, soft wrap
                // included (#9)
                let (row, xcell) = ed.cursor_display_pos();
                let x = rect.x + gutter_width(lines_total) + xcell as u16;
                let y = rect.y + row as u16;
                f.set_cursor_position((
                    x.min(rect.x + rect.width - 1),
                    y.min(rect.y + rect.height.saturating_sub(2)),
                ));
                if let Some(menu) = &ed.completion {
                    // anchor the popup under the first char of the prefix
                    let back = ed.cursor.col.saturating_sub(menu.anchor_col) as u16;
                    draw_completion_menu(f, menu, x.saturating_sub(back), y, *rect);
                }
            }
        }
    }
}

/// The completion popup (#22): a compact, borderless list anchored at the
/// cursor, below it or above when the window bottom is near.
fn draw_completion_menu(
    f: &mut Frame,
    menu: &crate::editor::analyzer::CompletionMenu,
    anchor_x: u16,
    cursor_y: u16,
    win: Rect,
) {
    use ratatui::widgets::Clear;
    let rows: Vec<(&str, Option<&str>)> = menu.rows().collect();
    if rows.is_empty() {
        return;
    }
    let h = (rows.len().min(10) as u16).max(1);
    let top = menu.selected.saturating_sub(h as usize - 1);
    let label_w = rows.iter().map(|(l, _)| l.width()).max().unwrap_or(8);
    let detail_w = rows
        .iter()
        .filter_map(|(_, d)| d.map(str::width))
        .max()
        .unwrap_or(0);
    let w = ((label_w + detail_w + 3) as u16).clamp(14, win.width.max(14));
    let below = cursor_y + 1;
    let y = if below + h <= win.y + win.height {
        below
    } else {
        cursor_y.saturating_sub(h)
    };
    let x = anchor_x.min((win.x + win.width).saturating_sub(w));
    let rect = Rect::new(x, y, w, h);

    let base = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let lines: Vec<Line> = rows[top..top + h as usize]
        .iter()
        .enumerate()
        .map(|(i, (label, detail))| {
            let selected = top + i == menu.selected;
            let bg = if selected {
                palette::MODE_NORMAL
            } else {
                palette::FLOAT_BG
            };
            let fg = if selected {
                palette::MODE_LABEL_FG
            } else {
                palette::FG
            };
            let mut label_style = Style::default().bg(bg).fg(fg);
            if selected {
                label_style = label_style.add_modifier(Modifier::BOLD);
            }
            let right = detail.map(|d| format!("{d} ")).unwrap_or_default();
            let left = format!(" {label}");
            let pad = (w as usize).saturating_sub(left.width() + right.width());
            let dim = if selected { fg } else { palette::COMMENT };
            Line::from(vec![
                Span::styled(left, label_style),
                Span::styled(" ".repeat(pad), Style::default().bg(bg)),
                Span::styled(right, Style::default().bg(bg).fg(dim)),
            ])
        })
        .collect();
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).style(base), rect);
}

/// Hover / annotated-line float: centered, content-sized, scrollless MVP.
fn draw_info_float(f: &mut Frame, lines: &[String], area: Rect) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear};
    let float = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let max_w = (area.width.saturating_sub(8)).clamp(20, 90) as usize;
    let max_h = (area.height.saturating_sub(4)).clamp(4, 18) as usize;
    let content: Vec<&str> = lines.iter().map(String::as_str).take(max_h).collect();
    let w = content
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(10)
        .clamp(10, max_w) as u16
        + 2;
    let h = content.len() as u16 + 2;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 4;
    let rect = Rect::new(x, y, w, h.min(area.height));
    let rows: Vec<Line> = content
        .iter()
        .map(|l| {
            let mut t: String = l.chars().take(max_w).collect();
            let pad = (w as usize).saturating_sub(2 + t.width());
            t.push_str(&" ".repeat(pad));
            Line::styled(t, float)
        })
        .collect();
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(rows).style(float).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT)),
        ),
        rect,
    );
}

/// Code-actions menu: pick with i/k, Enter applies.
fn draw_actions_menu(f: &mut Frame, menu: &crate::editor::analyzer::ActionsMenu, area: Rect) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear};
    let float = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let w = menu
        .actions
        .iter()
        .map(|a| a.title.width())
        .max()
        .unwrap_or(10)
        .clamp(16, (area.width as usize).saturating_sub(6)) as u16
        + 4;
    let h = (menu.actions.len() as u16 + 2).min(area.height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 3;
    let rect = Rect::new(x, y, w, h);
    let rows: Vec<Line> = menu
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == menu.selected {
                Style::default()
                    .bg(palette::MODE_NORMAL)
                    .fg(palette::MODE_LABEL_FG)
                    .add_modifier(Modifier::BOLD)
            } else {
                float
            };
            let mut t = format!(" {} ", a.title);
            let pad = (w as usize).saturating_sub(2 + t.width());
            t.push_str(&" ".repeat(pad));
            Line::styled(t, style)
        })
        .collect();
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(rows).style(float).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT))
                .title(" code actions ")
                .title_style(Style::default().bg(palette::FLOAT_BG).fg(palette::FG)),
        ),
        rect,
    );
}

/// A window projecting its buffer as a rendered preview (#39).
fn draw_preview_window(
    f: &mut Frame,
    ed: &mut Editor,
    id: crate::editor::windows::WinId,
    rect: Rect,
    focused: bool,
) {
    let text_rect = Rect::new(rect.x, rect.y, rect.width, rect.height - 1);
    let status_rect = Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1);
    let width = rect.width.saturating_sub(2) as usize;
    let (cursor_line, top) = ed.preview_nav_state(id);
    let buf = if focused {
        ed.current_buffer_id()
    } else {
        ed.parked_window(id)
            .map(|s| s.buf_id)
            .unwrap_or_else(|| ed.current_buffer_id())
    };
    let name = buffer_display_name(ed.buffer_ref(buf).path.as_deref());
    let doc = ed.preview_doc_for(buf, width);

    let base = Style::default().bg(palette::BG).fg(palette::FG);
    let mut rows: Vec<Line> = Vec::with_capacity(text_rect.height as usize);
    for row in 0..text_rect.height as usize {
        let idx = top + row;
        let Some(frags) = doc.lines.get(idx) else {
            rows.push(Line::default());
            continue;
        };
        // the reading line stays washed when unfocused too: a following
        // preview marks where the source cursor is
        let reading = idx == cursor_line;
        let line_bg = if reading {
            palette::PREVIEW_READING_BG
        } else {
            palette::BG
        };
        // the margin bar keeps the reading line findable even over lines
        // whose fragments carry their own background (code panels)
        let margin = if reading {
            Span::styled("▎ ", Style::default().fg(palette::PURPLE).bg(line_bg))
        } else {
            Span::styled("  ", Style::default().bg(line_bg))
        };
        let mut spans: Vec<Span> = vec![margin];
        let mut used = 2usize;
        for (text, style) in frags {
            // fragments with their own background (code chips) keep it;
            // transparent ones take the line wash
            let style = if style.bg.is_some() {
                *style
            } else {
                style.bg(line_bg)
            };
            used += text.width();
            spans.push(Span::styled(text.clone(), style));
        }
        if used < rect.width as usize {
            spans.push(Span::styled(
                " ".repeat(rect.width as usize - used),
                Style::default().bg(line_bg),
            ));
        }
        rows.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(rows).style(base), text_rect);

    // statusline: PREVIEW badge when focused, [P] tag otherwise
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0usize;
    if focused {
        let label = " PREVIEW ";
        spans.push(Span::styled(
            label,
            Style::default()
                .bg(palette::PURPLE)
                .fg(palette::MODE_LABEL_FG)
                .add_modifier(Modifier::BOLD),
        ));
        used += label.width();
    }
    let left = format!(" {name} [P]");
    let pct = (cursor_line + 1) * 100 / doc.line_count().max(1);
    let right = if focused {
        format!(" {}/{}  {pct}% ", cursor_line + 1, doc.line_count())
    } else {
        String::new()
    };
    let avail = (rect.width as usize).saturating_sub(used + right.width());
    let mut left = left;
    if left.width() > avail {
        left = left.chars().take(avail).collect();
    }
    let pad = avail.saturating_sub(left.width());
    spans.push(Span::styled(
        format!("{left}{}", " ".repeat(pad)),
        Style::default().bg(palette::STATUSLINE_BG).fg(if focused {
            palette::STATUSLINE_FG
        } else {
            palette::GUTTER_FG
        }),
    ));
    if !right.is_empty() {
        spans.push(Span::styled(
            right,
            Style::default().bg(palette::STATUSLINE_BG).fg(palette::FG),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), status_rect);
}

fn buffer_display_name(path: Option<&std::path::Path>) -> String {
    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string())
}

/// One window: its text area plus its own statusline row at the bottom.
fn draw_window(f: &mut Frame, ed: &Editor, view: &WinView, rect: Rect) {
    let text_rect = Rect::new(rect.x, rect.y, rect.width, rect.height - 1);
    let status_rect = Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1);
    draw_text(f, ed, view, text_rect);
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

/// The fuzzy file picker: query line on top, ranked matches below, matched
/// characters accented, snacks-style.
fn draw_file_picker(f: &mut Frame, ed: &Editor, area: Rect) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear};

    let Some(p) = &ed.file_picker else { return };

    // widen into a two-pane float (list left, preview right) when there's
    // room and a preview to show; otherwise the list-only float (#26). Sizes
    // scale with the terminal — a big monitor gets a big preview.
    let show_preview = area.width >= 96 && p.preview.is_some();
    let (w, preview_w) = if show_preview {
        // the float spans ~80% of the width; the list takes a quarter (grep
        // rows are longer — path:line:text — so it gets a wider slice), the
        // preview the rest, so a wide screen pours the extra into the preview
        let total = (area.width * 4 / 5).min(area.width.saturating_sub(4));
        let list = if p.kind == crate::editor::file_picker::PickerKind::Grep {
            (total * 2 / 5).clamp(48, 90)
        } else {
            (total / 4).clamp(34, 60)
        };
        (list, total.saturating_sub(list))
    } else {
        ((area.width * 3 / 5).clamp(30, 100), 0)
    };
    let total_w = w + preview_w;
    // ~80% of the height, floored for small terminals, never past the edges
    let h = (area.height * 4 / 5).clamp(8, area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(total_w)) / 2;
    let y = area.height.saturating_sub(h) / 3;
    let rect = Rect::new(x, y, w, h);
    let inner_w = w.saturating_sub(2) as usize;
    let list_rows = h.saturating_sub(3) as usize; // border + query line

    let float = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let dim = Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT);
    let accent = Style::default()
        .bg(palette::FLOAT_BG)
        .fg(palette::MODE_NORMAL)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::with_capacity(list_rows + 1);
    lines.push(Line::from(vec![
        Span::styled(" > ", accent),
        Span::styled(p.query.clone(), float),
    ]));

    let offset = p
        .selected
        .saturating_sub(list_rows.saturating_sub(1))
        .min(p.matches.len().saturating_sub(list_rows));
    if p.matches.is_empty() {
        lines.push(Line::styled("   ── no matches ──", dim));
    }
    for (row, m) in p.matches.iter().enumerate().skip(offset).take(list_rows) {
        let item = p.item(m);
        let selected = row == p.selected;
        // a nerd-font file-type icon leads each row in file mode (#73)
        let icon = (p.kind == crate::editor::file_picker::PickerKind::Files)
            .then(|| crate::config::icons::icon_for(item));
        let icon_w = if icon.is_some() { 2 } else { 0 };
        // truncate long rows: files/symbols keep the tail (the filename),
        // grep keeps the head (path:line) — shift match indices with the cut
        let grep = p.kind == crate::editor::file_picker::PickerKind::Grep;
        let (shown, cut) = {
            let max = inner_w.saturating_sub(3 + icon_w);
            let n = item.chars().count();
            if n <= max {
                (item.to_string(), 0)
            } else if grep {
                (
                    format!("{}…", item.chars().take(max - 1).collect::<String>()),
                    0,
                )
            } else {
                let cut = n - (max - 1);
                (
                    format!("…{}", item.chars().skip(cut).collect::<String>()),
                    cut as i64 - 1, // the ellipsis occupies one slot
                )
            }
        };
        let (row_style, hit_style) = if selected {
            let sel = Style::default()
                .bg(palette::MODE_NORMAL)
                .fg(palette::MODE_LABEL_FG);
            (sel, sel.add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
        } else {
            (float, accent)
        };
        let mut spans = vec![Span::styled(
            if selected { " ▸ " } else { "   " },
            row_style,
        )];
        if let Some((glyph, color)) = icon {
            let istyle = if selected {
                row_style
            } else {
                Style::default().bg(palette::FLOAT_BG).fg(color)
            };
            spans.push(Span::styled(format!("{glyph} "), istyle));
        }
        for (ci, ch) in shown.chars().enumerate() {
            let orig = ci as i64 + cut;
            let hit = orig >= 0 && m.indices.contains(&(orig as u32));
            spans.push(Span::styled(
                ch.to_string(),
                if hit { hit_style } else { row_style },
            ));
        }
        let used: usize = 3 + icon_w + shown.width();
        spans.push(Span::styled(
            " ".repeat(inner_w.saturating_sub(used)),
            row_style,
        ));
        lines.push(Line::from(spans));
    }

    let count = format!(
        " {}/{}{} ",
        p.matches.len(),
        p.total(),
        if p.truncated { "+" } else { "" }
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).style(float).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT))
                .title({
                    use crate::editor::file_picker::PickerKind;
                    match p.kind {
                        PickerKind::Symbols => " symbols ",
                        PickerKind::Grep => " grep ",
                        PickerKind::Files => " files ",
                    }
                })
                .title_style(Style::default().bg(palette::FLOAT_BG).fg(palette::FG))
                .title_bottom(Line::styled(count, dim).right_aligned()),
        ),
        rect,
    );
    if show_preview {
        let name = p.preview_title();
        draw_picker_preview(
            f,
            p.preview.as_ref().unwrap(),
            &name,
            Rect::new(x + w, y, preview_w, h),
        );
    }

    // terminal cursor sits in the query line
    let qx = rect.x + 4 + p.query.width() as u16;
    f.set_cursor_position((qx.min(rect.x + w - 2), rect.y + 1));
}

/// The picker's scroll-free preview pane: the file's head, syntax-highlighted
/// through the same tree-sitter path (#26), or a note for binary/empty files.
fn draw_picker_preview(
    f: &mut Frame,
    preview: &crate::editor::file_picker::Preview,
    name: &str,
    rect: Rect,
) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear};
    let float = Style::default().bg(palette::FLOAT_BG).fg(palette::FG);
    let dim = Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT);
    let inner_h = rect.height.saturating_sub(2) as usize;
    let inner_w = rect.width.saturating_sub(2) as usize;

    // an optional note (binary size, empty, …) heads the pane; content lines
    // (highlighted text, or a plain hex dump) fill the rest
    let mut rows: Vec<Line> = Vec::new();
    if let Some(note) = &preview.note {
        rows.push(Line::styled(format!(" {note}"), dim));
    }
    let body_h = inner_h.saturating_sub(rows.len());
    let text = preview.lines.join("\n");
    let spans = preview
        .lang
        .as_deref()
        .map(|lang| crate::syntax::highlight_text(&text, lang));
    for (i, raw) in preview.lines.iter().take(body_h).enumerate() {
        // the grep match line gets a reading-line wash across the whole row
        let focused = Some(i) == preview.focus;
        let bg = if focused {
            palette::PREVIEW_READING_BG
        } else {
            palette::FLOAT_BG
        };
        let base = Style::default().bg(bg).fg(palette::FG);
        let chars: Vec<char> = raw.chars().take(inner_w).collect();
        let mut out: Vec<Span> = Vec::new();
        let mut last = 0usize;
        for (s, e, cap) in spans
            .as_ref()
            .and_then(|s| s.get(i))
            .map_or(&[][..], Vec::as_slice)
        {
            let (s, e) = (*s as usize, (*e as usize).min(chars.len()));
            if s >= chars.len() {
                break;
            }
            if s > last {
                out.push(Span::styled(
                    chars[last..s].iter().collect::<String>(),
                    base,
                ));
            }
            let style = crate::config::theme::capture_style(*cap as usize).bg(bg);
            out.push(Span::styled(chars[s..e].iter().collect::<String>(), style));
            last = e;
        }
        if last < chars.len() {
            out.push(Span::styled(chars[last..].iter().collect::<String>(), base));
        }
        if focused {
            out.push(Span::styled(
                " ".repeat(inner_w.saturating_sub(chars.len())),
                base,
            ));
        }
        rows.push(Line::from(out));
    }

    let title = format!(" {name} ");
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(rows).style(float).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().bg(palette::FLOAT_BG).fg(palette::COMMENT))
                .title(Line::styled(title, dim))
                .title_alignment(ratatui::layout::Alignment::Right),
        ),
        rect,
    );
}

fn draw_text(f: &mut Frame, ed: &Editor, view: &WinView, area: Rect) {
    let base = Style::default().bg(palette::BG).fg(palette::FG);
    let lines_total = text_lines(view.rope);
    let gutter_w = gutter_width(lines_total);
    let highlighted = ed.syntax.has_highlights(view.buf_id);
    let height = area.height as usize;
    let text_w = area.width.saturating_sub(gutter_w) as usize;
    let mut rows: Vec<Line> = Vec::with_capacity(height);

    // one buffer line may fill several display rows (soft wrap, #9): walk
    // lines from the top and emit their rows until the window is full
    let mut line_idx = view.top_line;
    while rows.len() < height {
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
        let num_fg = if is_cursor_line {
            palette::GUTTER_CURRENT_FG
        } else if view.focused {
            palette::GUTTER_FG
        } else {
            palette::dimmed(palette::GUTTER_FG)
        };
        let gutter_style = Style::default().bg(palette::GUTTER_BG).fg(num_fg);
        let syntax_spans = if highlighted {
            ed.syntax.line_spans(view.buf_id, line_idx)
        } else {
            &[]
        };
        let (selection, sel_bg) = if view.focused {
            let sel = selection_on_line(ed, line_idx);
            if matches!(sel, SelSpan::None) {
                (flash_on_line(ed, line_idx), palette::YANK_FLASH_BG)
            } else {
                (sel, palette::SELECTION_BG)
            }
        } else {
            (SelSpan::None, palette::SELECTION_BG)
        };
        let diag = ed.diag_view(view.buf_id);
        let diag_spans: &[(u32, u32, crate::lsp::Severity)] = diag
            .and_then(|d| d.spans.get(&line_idx))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let search_spans: Vec<(u32, u32)> =
            if ed.search.highlight && view.buf_id == ed.current_buffer_id() {
                ed.search.matches_on_line(line_idx).collect()
            } else {
                Vec::new()
            };
        let search_current = if view.focused && line_idx == view.cursor_line {
            ed.search.match_at(line_idx, view.cursor_col as u32)
        } else {
            None
        };
        let inks = LineInks {
            syntax: syntax_spans,
            diags: diag_spans,
            selection,
            sel_bg,
            search: &search_spans,
            search_current,
            line_bg,
            focused: view.focused,
        };
        let content = line_content(view.rope, line_idx);
        let wrapped = wrap_rows(&line_graphemes(&content, OPTIONS.tabstop), text_w.max(1));
        let last_row = wrapped.len() - 1;
        for (ri, wr) in wrapped.iter().enumerate() {
            if rows.len() >= height {
                break; // the last line may show only its first rows
            }
            let mut spans: Vec<Span> = Vec::new();
            if gutter_w > 0 {
                // the number sits on the first row; continuation rows leave
                // the gutter blank
                let num = if ri == 0 {
                    format!("{:>width$} ", line_idx + 1, width = gutter_w as usize - 1)
                } else {
                    " ".repeat(gutter_w as usize)
                };
                spans.push(Span::styled(num, gutter_style));
            }
            let used = styled_visible(
                &content,
                wr.start_cell,
                wr.end_cell,
                text_w,
                &inks,
                &mut spans,
            );
            // end-of-line diagnostic ghost text (<leader>dh toggles), after
            // the line's last row
            if ri == last_row
                && ed.ghost_text
                && ed.mode != Mode::Insert
                && let Some((sev, msg)) = diag.and_then(|d| d.ghost.get(&line_idx))
                && used + 4 < text_w
            {
                let room = text_w - used - 3;
                let mut text: String = format!("● {msg}");
                if text.width() > room {
                    text = text
                        .chars()
                        .take(room.saturating_sub(1))
                        .collect::<String>()
                        + "…";
                }
                let color = match sev {
                    crate::lsp::Severity::Error => palette::RED,
                    _ => palette::YELLOW,
                };
                let color = if view.focused {
                    color
                } else {
                    palette::dimmed(color)
                };
                let ghost_w = text.width();
                // replace part of the padding with the ghost
                if let Some(last) = spans.last_mut() {
                    let pad = last.content.len();
                    let keep = 2.min(pad);
                    *last = Span::styled(" ".repeat(keep), Style::default().bg(line_bg));
                    spans.push(Span::styled(
                        text,
                        Style::default()
                            .bg(line_bg)
                            .fg(color)
                            .add_modifier(Modifier::ITALIC),
                    ));
                    let rest = (text_w - used).saturating_sub(keep + ghost_w);
                    spans.push(Span::styled(" ".repeat(rest), Style::default().bg(line_bg)));
                }
            }
            rows.push(Line::from(spans).style(Style::default().bg(line_bg)));
        }
        line_idx += 1;
    }

    f.render_widget(Paragraph::new(rows).style(base), area);
}

/// Renders the horizontally-scrolled window of a line as styled spans:
/// tree-sitter capture colors over the line background, tabs expanded,
/// padded to full width (so the cursorline highlight spans the window).
/// The selection's footprint on one line.
#[derive(Clone, Copy)]
enum SelSpan {
    None,
    /// Char columns start..end (exclusive).
    Chars(u32, u32),
    /// The whole line, padding included.
    Line,
    /// Display cells left..=right (blockwise).
    Cells(u32, u32),
}

struct LineInks<'a> {
    syntax: &'a [crate::syntax::LineSpan],
    diags: &'a [(u32, u32, crate::lsp::Severity)],
    selection: SelSpan,
    sel_bg: ratatui::style::Color,
    /// hlsearch spans on this line, with the current match singled out.
    search: &'a [(u32, u32)],
    search_current: Option<(u32, u32)>,
    line_bg: ratatui::style::Color,
    focused: bool,
}

/// The yank flash's footprint on `line` (same shapes as a selection).
fn flash_on_line(ed: &Editor, line: usize) -> SelSpan {
    use crate::editor::FlashRegion;
    let Some((_, region)) = ed.yank_flash else {
        return SelSpan::None;
    };
    match region {
        FlashRegion::Line(l1, l2) => {
            if line >= l1 && line <= l2 {
                SelSpan::Line
            } else {
                SelSpan::None
            }
        }
        FlashRegion::Block(l1, l2, left, right) => {
            if line >= l1 && line <= l2 {
                SelSpan::Cells(left as u32, right as u32)
            } else {
                SelSpan::None
            }
        }
        FlashRegion::Char(start, end) => {
            let rope = &ed.buffer.rope;
            let end = end.min(rope.len_chars());
            let start = start.min(end);
            if end == 0 {
                return SelSpan::None;
            }
            let (sl, el) = (
                rope.char_to_line(start),
                rope.char_to_line(end.saturating_sub(1)),
            );
            if line < sl || line > el {
                return SelSpan::None;
            }
            let base = rope.line_to_char(line);
            let s = start.saturating_sub(base) as u32;
            let e = if line == el {
                (end - base) as u32
            } else {
                u32::MAX
            };
            SelSpan::Chars(s, e)
        }
    }
}

/// Selection footprint of the current visual mode on `line` (focused only).
fn selection_on_line(ed: &Editor, line: usize) -> SelSpan {
    use crate::core::commands::VisualKind;
    let Mode::Visual(kind) = ed.mode else {
        return SelSpan::None;
    };
    let (a, c) = (ed.visual_anchor, ed.cursor);
    let (l1, l2) = (a.line.min(c.line), a.line.max(c.line));
    if line < l1 || line > l2 {
        return SelSpan::None;
    }
    match kind {
        VisualKind::Line => SelSpan::Line,
        VisualKind::Block => {
            let cell_of = |cur: crate::core::buffer::Cursor| {
                let grs = line_graphemes(&line_content(&ed.buffer.rope, cur.line), OPTIONS.tabstop);
                cell_at_col(&grs, cur.col)
            };
            let (ca, cc) = (cell_of(a), cell_of(c));
            SelSpan::Cells(ca.min(cc) as u32, ca.max(cc) as u32)
        }
        VisualKind::Char => {
            let (first, last) = if (a.line, a.col) <= (c.line, c.col) {
                (a, c)
            } else {
                (c, a)
            };
            if l1 == l2 {
                SelSpan::Chars(first.col as u32, last.col as u32 + 1)
            } else if line == first.line {
                SelSpan::Chars(first.col as u32, u32::MAX)
            } else if line == last.line {
                SelSpan::Chars(0, last.col as u32 + 1)
            } else {
                SelSpan::Line
            }
        }
    }
}

/// Renders the cells `[left, end)` of a line as styled spans — one display
/// row of a soft-wrapped line (#9) — padded to `width`.
fn styled_visible(
    content: &str,
    left: usize,
    end: usize,
    width: usize,
    inks: &LineInks,
    out: &mut Vec<Span<'static>>,
) -> usize {
    let LineInks {
        syntax,
        diags,
        selection,
        sel_bg,
        search,
        search_current,
        line_bg,
        focused,
    } = *inks;
    // inactive panels render with all inks blended toward the background
    let ink = |c: ratatui::style::Color| if focused { c } else { palette::dimmed(c) };
    let default_style = Style::default().bg(line_bg).fg(ink(palette::FG));
    let style_at = |char_off: usize| -> Style {
        let off = char_off as u32;
        let mut style = default_style;
        for (start, end, capture) in syntax {
            if *start <= off && off < *end {
                let cap = crate::config::theme::capture_style(*capture as usize);
                style = cap.fg(ink(cap.fg.unwrap_or(palette::FG))).bg(line_bg);
                break;
            }
            if *start > off {
                break;
            }
        }
        // diagnostic underline overlay: smuggled modifier bits the Kitty
        // backend turns into straight-red / curly-yellow underlines
        for (start, end, sev) in diags {
            if *start <= off && off < *end {
                style = match sev {
                    crate::lsp::Severity::Error => style.add_modifier(Modifier::RAPID_BLINK),
                    _ => style.add_modifier(Modifier::SLOW_BLINK),
                };
                break;
            }
        }
        style
    };

    let mut run = String::new();
    let mut run_style = default_style;
    let mut used = 0usize;
    let mut char_off = 0usize;
    let mut cell = 0usize;
    for g in content.graphemes(true) {
        let w = if g == "\t" {
            OPTIONS.tabstop - (cell % OPTIONS.tabstop)
        } else {
            g.width().max(1)
        };
        let start = cell;
        cell += w;
        let chars = g.chars().count();
        let this_off = char_off;
        char_off += chars;
        if cell <= left {
            continue;
        }
        if start >= end {
            break;
        }
        let vis_from = start.max(left);
        let vis_to = cell.min(end);
        let piece: String = if g == "\t" || start < left || cell > end {
            " ".repeat(vis_to.saturating_sub(vis_from))
        } else {
            g.to_string()
        };
        let selected = match selection {
            SelSpan::None => false,
            SelSpan::Line => true,
            SelSpan::Chars(s, e) => (this_off as u32) >= s && (this_off as u32) < e,
            SelSpan::Cells(l, r) => (start as u32) >= l && (start as u32) <= r,
        };
        let off32 = this_off as u32;
        let style = if selected {
            style_at(this_off).bg(sel_bg)
        } else if search_current.is_some_and(|(s, e)| off32 >= s && off32 < e) {
            style_at(this_off).bg(palette::SEARCH_CURRENT_BG)
        } else if search.iter().any(|(s, e)| off32 >= *s && off32 < *e) {
            style_at(this_off).bg(palette::SEARCH_BG)
        } else {
            style_at(this_off)
        };
        if style != run_style && !run.is_empty() {
            out.push(Span::styled(std::mem::take(&mut run), run_style));
        }
        run_style = style;
        used += piece.width();
        run.push_str(&piece);
    }
    if !run.is_empty() {
        out.push(Span::styled(run, run_style));
    }
    if used < width {
        let pad_style = if matches!(selection, SelSpan::Line) {
            default_style.bg(sel_bg)
        } else {
            default_style
        };
        out.push(Span::styled(" ".repeat(width - used), pad_style));
    }
    used
}

/// Per-window statusline: the focused window carries the mode badge (and a
/// zoom flag); unfocused windows show a dimmed name only, nvim-style.
fn draw_statusline(f: &mut Frame, ed: &Editor, view: &WinView, area: Rect) {
    let lines_total = text_lines(view.rope);
    let modified = if view.modified { " [+]" } else { "" };
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0usize;

    if view.focused {
        use crate::core::commands::VisualKind;
        let (label, color) = match ed.mode {
            Mode::Normal if ed.pending.is_idle() => (" NORMAL ", palette::MODE_NORMAL),
            Mode::Normal => (" NORMAL ", palette::MODE_PENDING),
            Mode::Insert => (" INSERT ", palette::MODE_INSERT),
            Mode::Replace => (" REPLACE ", palette::MODE_INSERT),
            Mode::Command => (" COMMAND ", palette::MODE_COMMAND),
            Mode::Visual(VisualKind::Char) => (" VISUAL ", palette::MODE_COMMAND),
            Mode::Visual(VisualKind::Line) => (" V-LINE ", palette::MODE_COMMAND),
            Mode::Visual(VisualKind::Block) => (" V-BLOCK ", palette::MODE_COMMAND),
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
    let ro = if view.read_only { " [RO]" } else { "" };
    let conflict = if view.disk_conflict { " [!]" } else { "" };
    let left = format!(" {}{modified}{ro}{conflict}{zoom}", view.name);
    let name_fg = if view.focused {
        palette::STATUSLINE_FG
    } else {
        palette::GUTTER_FG
    };
    let pct = (view.cursor_line + 1) * 100 / lines_total.max(1);
    let right = if view.focused {
        let mut ra = String::new();
        if let Some(p) = ed.lsp_progress() {
            ra = format!(" ra:{p} ");
        } else if let Some(d) = ed.diag_view(view.buf_id) {
            let (e, w) = d.counts;
            if e + w > 0 {
                ra = format!(" ✘{e} ▲{w} ");
            }
        }
        format!(
            "{ra} {}:{}  {pct}% ",
            view.cursor_line + 1,
            view.cursor_col + 1
        )
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
        let prefix = match ed.prompt {
            crate::editor::Prompt::Command => ':',
            crate::editor::Prompt::Search { forward: true } => '/',
            crate::editor::Prompt::Search { forward: false } => '?',
        };
        Line::styled(format!("{prefix}{}", ed.cmdline), bg.fg(palette::FG))
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
