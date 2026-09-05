//! Editor-side rust-analyzer glue (ticket #6): lifecycle, document sync,
//! request commands, and event handling. LSP positions are utf-8 BYTE
//! columns (negotiated); editor columns are chars — conversion happens here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::core::text::{line_content, text_lines};
use crate::editor::windows::SplitDir;
use crate::lsp::{self, Event, Severity};

use super::{BufId, Cursor, Editor};

/// didChange debounce: sent once the buffer has been quiet this long.
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(200);

/// Diagnostics of one buffer, converted to char columns for rendering.
#[derive(Default)]
pub struct DiagView {
    version: u64,
    /// line → styled underline spans (start_char, end_char, severity).
    pub spans: HashMap<usize, Vec<(u32, u32, Severity)>>,
    /// line → the ghost text shown at end of line (severity, message).
    pub ghost: HashMap<usize, (Severity, String)>,
    pub counts: (usize, usize), // (errors, warnings)
}

/// A menu of code actions awaiting a choice.
pub struct ActionsMenu {
    pub actions: Vec<lsp::Action>,
    pub selected: usize,
}

fn byte_to_char_col(line: &str, byte_col: usize) -> usize {
    let b = byte_col.min(line.len());
    line.get(..b)
        .map(|s| s.chars().count())
        .unwrap_or_else(|| line.chars().count())
}

fn char_to_byte_col(line: &str, char_col: usize) -> usize {
    line.char_indices()
        .nth(char_col)
        .map(|(b, _)| b)
        .unwrap_or(line.len())
}

impl Editor {
    fn current_rust_file(&self) -> Option<PathBuf> {
        let path = self.buffer.path.as_ref()?;
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            Some(abs)
        } else {
            None
        }
    }

    /// Cursor position as (line, utf-8 byte column) for requests.
    fn cursor_lsp_pos(&self) -> (usize, usize) {
        let line = line_content(&self.buffer.rope, self.cursor.line);
        (self.cursor.line, char_to_byte_col(&line, self.cursor.col))
    }

    /// Periodic pump, called from the main loop. Returns true when anything
    /// changed that warrants a redraw.
    pub fn lsp_tick(&mut self) -> bool {
        self.lsp_ensure_started();
        self.lsp_sync_current();

        let Some(client) = &mut self.lsp else {
            return false;
        };
        let events = client.drain();
        if events.is_empty() {
            return false;
        }
        for event in events {
            self.handle_lsp_event(event);
        }
        true
    }

    /// Starts rust-analyzer when a Rust file inside a cargo project shows up.
    fn lsp_ensure_started(&mut self) {
        if self.lsp.is_some() || self.lsp_broken {
            return;
        }
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let Some(root) = lsp::project_root(&file) else {
            return;
        };
        match lsp::Client::start(root) {
            Some(client) => self.lsp = Some(client),
            None => {
                self.lsp_broken = true;
                self.err("rust-analyzer not found on PATH");
            }
        }
    }

    /// Opens the current document on the server and pushes debounced edits.
    fn lsp_sync_current(&mut self) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let Some(client) = &mut self.lsp else { return };
        if !file.starts_with(client.root()) {
            return;
        }
        let version = self.buffer.version() as i64;
        if !client.is_open(&file) {
            client.did_open(&file, &self.buffer.rope.to_string(), version);
            return;
        }
        let quiet = self
            .lsp_dirty_since
            .map(|t| t.elapsed() >= CHANGE_DEBOUNCE)
            .unwrap_or(false);
        if quiet {
            client.did_change(&file, &self.buffer.rope.to_string(), version);
            self.lsp_dirty_since = None;
        }
    }

    pub(crate) fn lsp_note_edit(&mut self) {
        self.lsp_dirty_since = Some(Instant::now());
    }

    pub(crate) fn lsp_did_save(&mut self, path: &Path) {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        // make sure the server sees the latest content before the save event
        if let Some(client) = &mut self.lsp {
            let version = self.buffer.version() as i64;
            if client.is_open(&abs) {
                client.did_change(&abs, &self.buffer.rope.to_string(), version);
            }
            client.did_save(&abs);
        }
    }

    pub(crate) fn lsp_did_close(&mut self, path: &Path) {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(client) = &mut self.lsp {
            client.did_close(&abs);
        }
        self.diag_views.retain(|p, _| *p != abs);
    }

    pub fn lsp_shutdown(&mut self) {
        if let Some(mut client) = self.lsp.take() {
            client.shutdown();
        }
    }

    pub fn lsp_progress(&self) -> Option<&str> {
        self.lsp.as_ref()?.progress.as_deref()
    }

    // ------------------------------------------------------------------
    // commands
    // ------------------------------------------------------------------

    pub(crate) fn analyzer_hover(&mut self) {
        let Some(file) = self.current_rust_file() else {
            self.err("hover: not a rust buffer");
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        if let Some(client) = &mut self.lsp {
            client.hover(&file, line, col);
        }
    }

    pub(crate) fn analyzer_definition(&mut self) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        if let Some(client) = &mut self.lsp {
            client.definition(&file, line, col);
        }
    }

    pub(crate) fn analyzer_code_actions(&mut self) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        let diags: Vec<serde_json::Value> = Vec::new();
        if let Some(client) = &mut self.lsp {
            client.code_actions(&file, line, col, col + 1, diags);
        }
    }

    pub(crate) fn analyzer_expand_macro(&mut self) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        if let Some(client) = &mut self.lsp {
            client.expand_macro(&file, line, col);
        }
    }

    /// The line-scope hover: current line with all inlay hints spliced in.
    pub(crate) fn analyzer_inlay_line(&mut self) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        let text = line_content(&self.buffer.rope, line);
        let len = text.len();
        if let Some(client) = &mut self.lsp {
            client.inlay_line(&file, line, col, text, len);
        }
    }

    pub(crate) fn toggle_ghost_text(&mut self) {
        self.ghost_text = !self.ghost_text;
        self.msg(if self.ghost_text {
            "diagnostic text on"
        } else {
            "diagnostic text off"
        });
    }

    // ------------------------------------------------------------------
    // events
    // ------------------------------------------------------------------

    fn handle_lsp_event(&mut self, event: Event) {
        match event {
            Event::DiagnosticsUpdated(path) => self.rebuild_diag_view(&path),
            Event::Progress(_) => {}
            Event::Hover(Some(text)) => self.show_info_float(text),
            Event::Hover(None) => self.msg("hover: no information"),
            Event::InlayLine(annotated, signature) => {
                let mut lines = Vec::new();
                if let Some(a) = annotated {
                    lines.push(a);
                }
                if let Some(sig) = signature {
                    if !lines.is_empty() {
                        lines.push(String::new());
                    }
                    lines.push(format!("→ {sig}"));
                }
                if lines.is_empty() {
                    self.msg("no type annotations on this line");
                } else {
                    self.info_float = Some(lines);
                }
            }
            Event::Definition(Some(loc)) => self.jump_to(loc),
            Event::Definition(None) => self.msg("definition not found"),
            Event::Actions(actions) => {
                if actions.is_empty() {
                    self.msg("no code actions here");
                } else {
                    self.actions_menu = Some(ActionsMenu {
                        actions,
                        selected: 0,
                    });
                }
            }
            Event::ActionResolved(v) => {
                let edit = v.get("edit").cloned().unwrap_or(serde_json::Value::Null);
                if edit.is_object() {
                    self.apply_workspace_edit(&edit);
                } else {
                    self.msg("action has no edit");
                }
            }
            Event::ApplyEdit(edit) => self.apply_workspace_edit(&edit),
            Event::ExpandedMacro(Some((name, text))) => {
                self.open_expansion(&name, &text);
            }
            Event::ExpandedMacro(None) => self.msg("no macro under cursor"),
            Event::ServerExited(e) => {
                self.lsp = None;
                self.lsp_broken = true;
                self.err(e);
            }
        }
    }

    fn show_info_float(&mut self, text: String) {
        // markdown fences add noise in a plain float; drop the fence lines
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("```"))
            .map(str::to_string)
            .collect();
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            self.msg("hover: no information");
            return;
        }
        self.info_float = Some(lines);
    }

    fn jump_to(&mut self, loc: lsp::Location) {
        let path = loc.path.to_string_lossy().into_owned();
        self.open_path(&path, None);
        let last = text_lines(&self.buffer.rope) - 1;
        let line = loc.line.min(last);
        let content = line_content(&self.buffer.rope, line);
        self.cursor = Cursor::new(line, byte_to_char_col(&content, loc.col));
        self.goal = None;
        self.clamp_cursor();
        self.refresh_focused_view();
    }

    /// Shows a macro expansion in a horizontal split, as a rust scratch
    /// buffer (highlighted, never written unless the user insists).
    fn open_expansion(&mut self, name: &str, text: &str) {
        let title = format!("[expansion:{name}].rs");
        self.split_window(SplitDir::Horizontal, true);
        self.buffer = crate::core::buffer::Buffer::from_text(text);
        self.buffer.path = Some(PathBuf::from(title));
        self.cursor = Cursor::default();
        self.goal = None;
        self.refresh_focused_view();
    }

    // ------------------------------------------------------------------
    // diagnostics views
    // ------------------------------------------------------------------

    /// Buffer id currently showing `path`, if any.
    fn buffer_for_path(&self, path: &Path) -> Option<BufId> {
        self.buffer_entries().into_iter().find_map(|e| {
            let id = e.id;
            let buffer = self.buffer_ref(id);
            let bp = buffer.path.as_ref()?;
            let canon = std::fs::canonicalize(bp).unwrap_or_else(|_| bp.clone());
            (canon == path).then_some(id)
        })
    }

    fn rebuild_diag_view(&mut self, path: &Path) {
        let Some(buf_id) = self.buffer_for_path(path) else {
            self.diag_views.remove(path);
            return;
        };
        let Some(client) = &self.lsp else { return };
        let Some(diags) = client.diagnostics.get(path) else {
            return;
        };
        let rope = &self.buffer_ref(buf_id).rope;
        let last = text_lines(rope) - 1;
        let mut view = DiagView {
            version: self.buffer_ref(buf_id).version(),
            ..Default::default()
        };
        for d in diags {
            if matches!(d.severity, Severity::Info) {
                continue;
            }
            let line = d.line.min(last);
            let content = line_content(rope, line);
            let start = byte_to_char_col(&content, d.col) as u32;
            let end = if d.end_line == d.line {
                byte_to_char_col(&content, d.end_col) as u32
            } else {
                content.chars().count() as u32
            };
            let end = end.max(start + 1);
            view.spans
                .entry(line)
                .or_default()
                .push((start, end, d.severity));
            match d.severity {
                Severity::Error => view.counts.0 += 1,
                Severity::Warning => view.counts.1 += 1,
                Severity::Info => {}
            }
            // first diagnostic on the line wins the ghost slot; errors beat
            // warnings
            let msg = d.message.lines().next().unwrap_or("").to_string();
            match view.ghost.get(&line) {
                Some((Severity::Error, _)) => {}
                Some((Severity::Warning, _)) if d.severity == Severity::Warning => {}
                _ => {
                    view.ghost.insert(line, (d.severity, msg));
                }
            }
        }
        self.diag_views.insert(path.to_path_buf(), view);
    }

    /// Diagnostics for a buffer, for the renderer (None when text drifted —
    /// positions would lie until the server republishes).
    pub fn diag_view(&self, buf_id: BufId) -> Option<&DiagView> {
        let buffer = self.buffer_ref(buf_id);
        let path = buffer.path.as_ref()?;
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let view = self.diag_views.get(&canon)?;
        (view.version == buffer.version()).then_some(view)
    }

    // ------------------------------------------------------------------
    // workspace edits
    // ------------------------------------------------------------------

    pub(crate) fn apply_workspace_edit(&mut self, edit: &serde_json::Value) {
        let mut per_file: Vec<(PathBuf, Vec<serde_json::Value>)> = Vec::new();
        if let Some(changes) = edit.get("changes").and_then(|c| c.as_object()) {
            for (uri, edits) in changes {
                if let (Some(path), Some(list)) = (
                    uri.strip_prefix("file://").map(PathBuf::from),
                    edits.as_array(),
                ) {
                    per_file.push((path, list.clone()));
                }
            }
        }
        if let Some(doc_changes) = edit.get("documentChanges").and_then(|c| c.as_array()) {
            for change in doc_changes {
                let Some(uri) = change["textDocument"]["uri"].as_str() else {
                    continue; // create/rename/delete ops: out of MVP scope
                };
                if let (Some(path), Some(list)) = (
                    uri.strip_prefix("file://").map(PathBuf::from),
                    change["edits"].as_array(),
                ) {
                    per_file.push((path, list.clone()));
                }
            }
        }
        if per_file.is_empty() {
            self.msg("edit: nothing to apply");
            return;
        }
        let mut applied = 0;
        for (path, edits) in per_file {
            if self.apply_edits_to_file(&path, edits) {
                applied += 1;
            }
        }
        self.msg(format!("applied edits to {applied} file(s)"));
        self.lsp_note_edit();
    }

    fn apply_edits_to_file(&mut self, path: &Path, mut edits: Vec<serde_json::Value>) -> bool {
        // the target must be (or become) a buffer; current buffer stays focused
        let buf_id = match self.buffer_for_path(path) {
            Some(id) => id,
            None => {
                let display = path.to_string_lossy().into_owned();
                let before = self.current_buffer_id();
                self.open_path(&display, None);
                let opened = self.current_buffer_id();
                if opened != before {
                    self.switch_to(before);
                }
                match self.buffer_for_path(path) {
                    Some(id) => id,
                    None => return false,
                }
            }
        };
        // bottom-up so earlier edits keep their positions
        edits.sort_by_key(|e| {
            let s = &e["range"]["start"];
            std::cmp::Reverse((
                s["line"].as_u64().unwrap_or(0),
                s["character"].as_u64().unwrap_or(0),
            ))
        });
        let focused = buf_id == self.current_buffer_id();
        let cursor = if focused {
            self.cursor
        } else {
            Cursor::default()
        };
        let buffer = if focused {
            &mut self.buffer
        } else {
            match self.buffer_mut_by_id(buf_id) {
                Some(b) => b,
                None => return false,
            }
        };
        buffer.begin_change(cursor);
        for e in &edits {
            let range = &e["range"];
            let new_text = e["newText"].as_str().unwrap_or("");
            let (Some(sl), Some(sc), Some(el), Some(ec)) = (
                range["start"]["line"].as_u64(),
                range["start"]["character"].as_u64(),
                range["end"]["line"].as_u64(),
                range["end"]["character"].as_u64(),
            ) else {
                continue;
            };
            let last = text_lines(&buffer.rope) - 1;
            let (sl, el) = ((sl as usize).min(last), (el as usize).min(last));
            let sline = line_content(&buffer.rope, sl);
            let eline = line_content(&buffer.rope, el);
            let start = buffer.rope.line_to_char(sl) + byte_to_char_col(&sline, sc as usize);
            let end = buffer.rope.line_to_char(el) + byte_to_char_col(&eline, ec as usize);
            let end = end.min(buffer.rope.len_chars()).max(start);
            buffer.remove(start..end);
            buffer.insert(start, new_text);
        }
        let committed = buffer.end_change();
        if focused {
            self.note_change_committed(committed);
            self.clamp_cursor();
            self.scroll_to_cursor();
        }
        true
    }
}
