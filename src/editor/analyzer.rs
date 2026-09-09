//! Editor-side rust-analyzer glue (ticket #6): lifecycle, document sync,
//! request commands, and event handling. LSP positions are utf-8 BYTE
//! columns (negotiated); editor columns are chars — conversion happens here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::core::text::{line_content, text_lines};
use crate::editor::windows::SplitDir;
use crate::lsp::{self, Event, Severity};

use super::{BufId, Cursor, Editor, file_picker};

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

/// The active completion popup (ticket #22). Holds the full candidate list
/// from the server and a filtered `shown` view (indices into `all`) that
/// narrows as the user keeps typing, without a new server round-trip.
pub struct CompletionMenu {
    all: Vec<lsp::CompletionItem>,
    shown: Vec<usize>,
    pub selected: usize,
    anchor_line: usize,
    /// Char column where the completed identifier begins (popup anchor and
    /// the left edge of the text that accept replaces).
    pub anchor_col: usize,
}

impl CompletionMenu {
    pub fn len(&self) -> usize {
        self.shown.len()
    }
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }
    fn item(&self, i: usize) -> Option<&lsp::CompletionItem> {
        self.all.get(*self.shown.get(i)?)
    }
    pub fn selected_item(&self) -> Option<&lsp::CompletionItem> {
        self.item(self.selected)
    }
    /// Visible (label, detail) pairs, for rendering.
    pub fn rows(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.shown.iter().filter_map(move |&i| {
            let it = self.all.get(i)?;
            Some((it.label.as_str(), it.detail.as_deref()))
        })
    }
}

/// Candidates whose filter text matches `prefix`, ranked: prefix-hits
/// before substring-hits, then by the server's sortText. Indices into `all`.
fn rank_completions(all: &[lsp::CompletionItem], prefix: &str) -> Vec<usize> {
    let p = prefix.to_ascii_lowercase();
    let mut scored: Vec<(u8, &str, usize)> = all
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let key = it.filter_text.to_ascii_lowercase();
            let score = if p.is_empty() {
                1
            } else if key.starts_with(&p) {
                0
            } else if key.contains(&p) {
                1
            } else {
                return None;
            };
            Some((score, it.sort_text.as_str(), i))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(200).map(|(_, _, i)| i).collect()
}

/// The canonical `.rs` path of a buffer, if it is one — the document identity
/// the server knows it by.
fn rust_file_of(buffer: &crate::core::buffer::Buffer) -> Option<PathBuf> {
    let path = buffer.path.as_ref()?;
    if path.extension().and_then(|e| e.to_str()) == Some("rs") {
        Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
    } else {
        None
    }
}

fn byte_to_char_col(line: &str, byte_col: usize) -> usize {
    let b = byte_col.min(line.len());
    line.get(..b)
        .map(|s| s.chars().count())
        .unwrap_or_else(|| line.chars().count())
}

/// An LSP (line, utf-8 byte column) as a char index for applying an edit.
/// Unlike a cursor, an edit may address the *end of the document* — the
/// empty line after the terminating newline — so that line maps to
/// `len_chars` instead of being clamped onto the last visible line, which
/// turned "append at EOF" into "insert at the start of the last line"
/// (#82 §8). Anything beyond is clamped to the end.
fn lsp_pos_to_char(rope: &ropey::Rope, line: usize, byte_col: usize) -> usize {
    if line >= text_lines(rope) {
        return rope.len_chars();
    }
    let content = line_content(rope, line);
    (rope.line_to_char(line) + byte_to_char_col(&content, byte_col)).min(rope.len_chars())
}

/// The identifier under (or just before) a char column: char range
/// `start..end`, or None on blank or punctuation.
fn identifier_at(line: &str, col: usize) -> Option<(usize, usize)> {
    use crate::core::text::{CharClass, char_class};
    let chars: Vec<char> = line.chars().collect();
    let is_word = |i: usize| {
        chars
            .get(i)
            .is_some_and(|&c| char_class(c, false) == CharClass::Word)
    };
    let mut at = col.min(chars.len().saturating_sub(1));
    if !is_word(at) {
        if at > 0 && is_word(at - 1) {
            at -= 1; // cursor just past the name (append position)
        } else {
            return None;
        }
    }
    let mut start = at;
    while start > 0 && is_word(start - 1) {
        start -= 1;
    }
    let mut end = at + 1;
    while is_word(end) {
        end += 1;
    }
    Some((start, end))
}

fn is_plain_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn char_to_byte_col(line: &str, char_col: usize) -> usize {
    line.char_indices()
        .nth(char_col)
        .map(|(b, _)| b)
        .unwrap_or(line.len())
}

impl Editor {
    pub(crate) fn current_rust_file(&self) -> Option<PathBuf> {
        rust_file_of(&self.buffer)
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

    /// Opens the current document on the server, then pushes debounced edits
    /// for *every* buffer that has some — parked ones included, so switching
    /// away mid-debounce can't strand a change (#82 §9).
    fn lsp_sync_current(&mut self) {
        if let Some(file) = self.current_rust_file()
            && let Some(client) = &mut self.lsp
            && file.starts_with(client.root())
            && !client.is_open(&file)
        {
            let version = self.buffer.version() as i64;
            client.did_open(&file, &self.buffer.rope.to_string(), version);
            self.lsp_dirty.remove(&self.current); // didOpen carried it
            if self.projected_buffers().contains(&self.current) {
                self.request_inlay_hints(self.current);
            }
            return;
        }
        let quiet: Vec<BufId> = self
            .lsp_dirty
            .iter()
            .filter(|(_, since)| since.elapsed() >= CHANGE_DEBOUNCE)
            .map(|(id, _)| *id)
            .collect();
        for id in quiet {
            self.lsp_dirty.remove(&id);
            if id != self.current && self.slot_index(id).is_none() {
                continue; // closed meanwhile
            }
            let buffer = self.buffer_ref(id);
            let Some(file) = rust_file_of(buffer) else {
                continue;
            };
            let (content, version) = (buffer.rope.to_string(), buffer.version() as i64);
            if let Some(client) = &mut self.lsp
                && client.is_open(&file)
            {
                client.did_change(&file, &content, version);
                // a projection of this buffer wants the hints for the text
                // the server now holds (#93)
                if self.projected_buffers().contains(&id) {
                    self.request_inlay_hints(id);
                }
            }
        }
    }

    /// Asks for every hint of a projected buffer (#93) — but only once the
    /// server holds its current text: hints computed on an older text would
    /// be filed under a revision they don't describe.
    pub(crate) fn request_inlay_hints(&mut self, id: BufId) {
        if self.lsp_dirty.contains_key(&id) {
            return; // the flush that sends the text asks afterwards
        }
        let buffer = self.buffer_ref(id);
        let Some(file) = rust_file_of(buffer) else {
            return;
        };
        let (lines, version) = (text_lines(&buffer.rope), buffer.version());
        if let Some(client) = &mut self.lsp
            && client.is_open(&file)
        {
            client.inlay_document(&file, lines, version);
        }
    }

    /// The hints that describe a buffer's *current* text, else none —
    /// misplaced annotations are worse than a bare line.
    pub(crate) fn fresh_inlay_hints(&self, id: BufId) -> Vec<lsp::InlayHint> {
        let version = self.buffer_ref(id).version();
        match self.inlay_hints.get(&id) {
            Some((rev, hints)) if *rev == version => hints.clone(),
            _ => Vec::new(),
        }
    }

    /// The server's diagnostics for a buffer, when they still describe its
    /// text (the same rule the underlines follow).
    pub(crate) fn buffer_diagnostics(&self, id: BufId) -> Vec<lsp::Diagnostic> {
        if self.diag_view(id).is_none() {
            return Vec::new();
        }
        let buffer = self.buffer_ref(id);
        let Some(path) = buffer.path.as_ref() else {
            return Vec::new();
        };
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        self.lsp
            .as_ref()
            .and_then(|c| c.diagnostics.get(&canon))
            .cloned()
            .unwrap_or_default()
    }

    /// What, besides text and width, the compiler's-eye view depends on:
    /// whether fresh hints exist and which diagnostics generation is in.
    pub(crate) fn projection_stamp(&self, id: BufId) -> u64 {
        let version = self.buffer_ref(id).version();
        let fresh = self
            .inlay_hints
            .get(&id)
            .is_some_and(|(rev, _)| *rev == version);
        (fresh as u64) | (self.diag_epoch << 1)
    }

    /// Records that the current buffer changed; every mutation path — keys,
    /// paste, reload, an applied LSP edit — must land here.
    pub(crate) fn lsp_note_edit(&mut self) {
        self.lsp_dirty.insert(self.current, Instant::now());
    }

    /// Forgets pending edits for a buffer that is going away.
    pub(crate) fn lsp_forget_buffer(&mut self, id: BufId) {
        self.lsp_dirty.remove(&id);
        self.inlay_hints.remove(&id);
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

    /// `didSave` for a parked buffer: the server sees that buffer's text
    /// first (never the current one's).
    pub(crate) fn lsp_did_save_buffer(&mut self, id: BufId, path: &Path) {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let (content, version) = {
            let b = self.buffer_ref(id);
            (b.rope.to_string(), b.version() as i64)
        };
        self.lsp_dirty.remove(&id);
        if let Some(client) = &mut self.lsp {
            if client.is_open(&abs) {
                client.did_change(&abs, &content, version);
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

    /// `Space c n`: open the rename prompt pre-filled with the identifier
    /// under the cursor — edit the name, don't retype it (#97).
    pub(crate) fn analyzer_rename_prompt(&mut self) {
        if self.current_rust_file().is_none() {
            self.err("rename: not a rust buffer");
            return;
        }
        if self.lsp.is_none() {
            self.err("rename: rust-analyzer is not running");
            return;
        }
        let line = line_content(&self.buffer.rope, self.cursor.line);
        let Some((start, end)) = identifier_at(&line, self.cursor.col) else {
            self.err("rename: no identifier under the cursor");
            return;
        };
        let name: String = line.chars().skip(start).take(end - start).collect();
        let (lsp_line, lsp_col) = (self.cursor.line, char_to_byte_col(&line, start));
        self.open_prompt(
            super::Prompt::Rename {
                line: lsp_line,
                col: lsp_col,
            },
            name,
        );
    }

    /// Enter on the rename prompt: validate and ask the server (#97).
    pub(crate) fn analyzer_rename(&mut self, line: usize, col: usize, new_name: &str) {
        let Some(file) = self.current_rust_file() else {
            return;
        };
        if new_name.is_empty() {
            self.msg("rename cancelled");
            return;
        }
        if !is_plain_identifier(new_name) {
            self.err(format!("rename: `{new_name}` is not an identifier"));
            return;
        }
        let current = line_content(
            &self.buffer.rope,
            line.min(text_lines(&self.buffer.rope) - 1),
        );
        let old = identifier_at(&current, byte_to_char_col(&current, col))
            .map(|(s, e)| current.chars().skip(s).take(e - s).collect::<String>());
        if old.as_deref() == Some(new_name) {
            self.msg("rename: same name");
            return;
        }
        if let Some(client) = &mut self.lsp {
            client.rename(&file, line, col, new_name);
            self.msg(format!("renaming to `{new_name}`…"));
        }
    }

    /// The folder a location list shows paths relative to: the server's
    /// project root (a file argument makes its own folder the working
    /// folder, and references live all over the crate), canonical as the
    /// server spells it. Rows still jump by absolute path.
    fn display_root(&self) -> PathBuf {
        let root = self
            .lsp
            .as_ref()
            .map(|c| c.root().to_path_buf())
            .unwrap_or_else(|| self.root().to_path_buf());
        std::fs::canonicalize(&root).unwrap_or(root)
    }

    /// `Space c r`: every reference to the symbol under the cursor, as a
    /// location list once the server answers (#98).
    pub(crate) fn analyzer_references(&mut self) {
        let Some(file) = self.current_rust_file() else {
            self.err("references: not a rust buffer");
            return;
        };
        let (line, col) = self.cursor_lsp_pos();
        match &mut self.lsp {
            Some(client) => {
                client.references(&file, line, col);
                self.msg("finding references…");
            }
            None => self.err("references: rust-analyzer is not running"),
        }
    }

    /// The server's references as rows: `path:line: text` with the name
    /// accented, in path then line order; the line text comes from the
    /// open buffer when there is one, from disk otherwise.
    fn on_references(&mut self, mut locs: Vec<lsp::Location>) {
        if locs.is_empty() {
            self.msg("no references");
            return;
        }
        locs.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        let root = self.display_root();
        let mut rows = Vec::with_capacity(locs.len());
        let mut cache: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for loc in locs {
            let lines = match cache.iter().position(|(p, _)| *p == loc.path) {
                Some(i) => &cache[i].1,
                None => {
                    let lines = match self.buffer_for_path(&loc.path) {
                        Some(id) => {
                            let rope = &self.buffer_ref(id).rope;
                            (0..text_lines(rope))
                                .map(|l| line_content(rope, l))
                                .collect()
                        }
                        None => std::fs::read_to_string(&loc.path)
                            .map(|s| s.lines().map(str::to_string).collect())
                            .unwrap_or_default(),
                    };
                    cache.push((loc.path.clone(), lines));
                    &cache.last().unwrap().1
                }
            };
            let Some(text) = lines.get(loc.line) else {
                continue;
            };
            let rel = loc
                .path
                .strip_prefix(&root)
                .unwrap_or(&loc.path)
                .to_string_lossy()
                .into_owned();
            let (display, accent) =
                file_picker::location_row(&rel, loc.line, text, loc.col, loc.end_col);
            rows.push(file_picker::LocationRow {
                display,
                accent,
                hit: file_picker::GrepHit {
                    path: loc.path.to_string_lossy().into_owned(),
                    line: loc.line,
                    col: byte_to_char_col(text, loc.col),
                },
            });
        }
        let n = rows.len();
        file_picker::open_locations(self, " references ", rows);
        self.msg(format!("{n} reference(s)"));
    }

    /// `Space d d`: every diagnostic the server has published, all files,
    /// errors first, as a location list (#99).
    pub(crate) fn open_diagnostics_list(&mut self) {
        let Some(client) = &self.lsp else {
            self.err("diagnostics: rust-analyzer is not running");
            return;
        };
        let root = self.display_root();
        let mut all: Vec<(u8, PathBuf, lsp::Diagnostic)> = client
            .diagnostics
            .iter()
            .flat_map(|(path, ds)| {
                ds.iter().map(move |d| {
                    let rank = match d.severity {
                        Severity::Error => 0,
                        Severity::Warning => 1,
                        Severity::Info => 2,
                    };
                    (rank, path.clone(), d.clone())
                })
            })
            .collect();
        if all.is_empty() {
            self.msg("no diagnostics");
            return;
        }
        all.sort_by(|a, b| (a.0, &a.1, a.2.line, a.2.col).cmp(&(b.0, &b.1, b.2.line, b.2.col)));
        let rows = all
            .into_iter()
            .map(|(_, path, d)| {
                let glyph = match d.severity {
                    Severity::Error => 'E',
                    Severity::Warning => 'W',
                    Severity::Info => 'I',
                };
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                let headline = d.message.lines().next().unwrap_or("").trim();
                let col = self
                    .buffer_for_path(&path)
                    .map(|id| {
                        let rope = &self.buffer_ref(id).rope;
                        let line = d.line.min(text_lines(rope) - 1);
                        byte_to_char_col(&line_content(rope, line), d.col)
                    })
                    .unwrap_or(d.col);
                file_picker::LocationRow {
                    display: format!("{glyph} {rel}:{}: {headline}", d.line + 1),
                    accent: vec![0],
                    hit: file_picker::GrepHit {
                        path: path.to_string_lossy().into_owned(),
                        line: d.line,
                        col,
                    },
                }
            })
            .collect::<Vec<_>>();
        let n = rows.len();
        file_picker::open_locations(self, " diagnostics ", rows);
        self.msg(format!("{n} diagnostic(s)"));
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

    // ---- completion (ticket #22, iteration one) ----------------------

    /// Trigger completion at the cursor (Ctrl+N/Ctrl+P in insert mode).
    /// Rust only for now — a request whose response opens the popup a tick
    /// later, through the same `lsp_tick` pump as hover.
    pub(crate) fn request_completion(&mut self) {
        let Some(file) = self.current_rust_file() else {
            self.msg("completion: Rust only for now");
            return;
        };
        let line = line_content(&self.buffer.rope, self.cursor.line);
        let chars: Vec<char> = line.chars().collect();
        let mut start = self.cursor.col.min(chars.len());
        while start > 0
            && crate::core::text::char_class(chars[start - 1], false)
                == crate::core::text::CharClass::Word
        {
            start -= 1;
        }
        self.completion_req = Some((self.cursor.line, start));
        let (l, c) = self.cursor_lsp_pos();
        if let Some(client) = &mut self.lsp {
            client.completion(&file, l, c);
        }
    }

    /// A completion response arrived: open the popup unless the cursor has
    /// since left the line the request was made on.
    fn on_completion(&mut self, items: Vec<lsp::CompletionItem>) {
        let Some((line, anchor_col)) = self.completion_req.take() else {
            return;
        };
        if line != self.cursor.line || self.cursor.col < anchor_col {
            return;
        }
        self.build_completion(items, line, anchor_col);
    }

    /// Rank `items` against the current prefix and open the popup (or report
    /// none). Shared by the live path and the test hook.
    pub(crate) fn build_completion(
        &mut self,
        items: Vec<lsp::CompletionItem>,
        anchor_line: usize,
        anchor_col: usize,
    ) {
        let prefix = self.completion_prefix(anchor_line, anchor_col);
        let shown = rank_completions(&items, &prefix);
        if shown.is_empty() {
            self.completion = None;
            self.msg("no completions");
            return;
        }
        self.completion = Some(CompletionMenu {
            all: items,
            shown,
            selected: 0,
            anchor_line,
            anchor_col,
        });
    }

    fn completion_prefix(&self, line: usize, anchor_col: usize) -> String {
        line_content(&self.buffer.rope, line)
            .chars()
            .skip(anchor_col)
            .take(self.cursor.col.saturating_sub(anchor_col))
            .collect()
    }

    pub(crate) fn completion_next(&mut self) {
        if let Some(m) = &mut self.completion
            && !m.shown.is_empty()
        {
            m.selected = (m.selected + 1) % m.shown.len();
        }
    }

    pub(crate) fn completion_prev(&mut self) {
        if let Some(m) = &mut self.completion
            && !m.shown.is_empty()
        {
            m.selected = (m.selected + m.shown.len() - 1) % m.shown.len();
        }
    }

    /// Re-narrow after the user typed or deleted while the popup is open.
    pub(crate) fn completion_refilter(&mut self) {
        let Some(mut m) = self.completion.take() else {
            return;
        };
        if self.cursor.line != m.anchor_line || self.cursor.col < m.anchor_col {
            return; // moved off the identifier — popup closes
        }
        let prefix = self.completion_prefix(m.anchor_line, m.anchor_col);
        m.shown = rank_completions(&m.all, &prefix);
        if m.shown.is_empty() {
            return; // nothing matches — closes
        }
        m.selected = 0;
        self.completion = Some(m);
    }

    /// Accept the selected candidate: replace the typed prefix with its
    /// insert text (one undo step, folded into the open insert session).
    pub(crate) fn completion_accept(&mut self) {
        let Some(m) = self.completion.take() else {
            return;
        };
        if self.cursor.line != m.anchor_line || self.cursor.col < m.anchor_col {
            return;
        }
        let Some(text) = m.selected_item().map(|it| it.insert_text.clone()) else {
            return;
        };
        let base = self.buffer.rope.line_to_char(m.anchor_line);
        let start = base + m.anchor_col;
        let end = base + self.cursor.col;
        self.buffer.begin_change(self.cursor);
        self.buffer.remove(start..end);
        self.buffer.insert(start, &text);
        self.buffer.end_change();
        self.cursor.col = m.anchor_col + text.chars().count();
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    /// Open a popup from synthetic items without a live server (tests and
    /// programmatic drivers).
    pub fn inject_completion(&mut self, items: Vec<lsp::CompletionItem>, anchor_col: usize) {
        self.build_completion(items, self.cursor.line, anchor_col);
    }

    fn handle_lsp_event(&mut self, event: Event) {
        match event {
            Event::Completion(items) => self.on_completion(items),
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
            Event::InlayHints(path, revision, hints) => {
                if let Some(id) = self.buffer_for_path(&path) {
                    // answers may overtake each other; keep the newest
                    let newer = self
                        .inlay_hints
                        .get(&id)
                        .is_none_or(|(rev, _)| revision >= *rev);
                    if newer {
                        self.inlay_hints.insert(id, (revision, hints));
                    }
                }
            }
            Event::InlayRefresh => {
                for id in self.projected_buffers() {
                    self.request_inlay_hints(id);
                }
            }
            Event::References(locs) => self.on_references(locs),
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
                if !edit.is_object() {
                    self.msg("action has no edit");
                } else if let Some((files, edits)) = self.apply_workspace_edit(&edit) {
                    self.msg(format!("applied {edits} edit(s) in {files} file(s)"));
                } else {
                    self.msg("edit: nothing to apply");
                }
            }
            Event::ApplyEdit(edit) => {
                if let Some((files, edits)) = self.apply_workspace_edit(&edit) {
                    self.msg(format!("applied {edits} edit(s) in {files} file(s)"));
                } else {
                    self.msg("edit: nothing to apply");
                }
            }
            Event::Renamed(name, Err(reason)) => {
                self.err(format!("rename to `{name}`: {reason}"));
            }
            Event::Renamed(name, Ok(edit)) => match self.apply_workspace_edit(&edit) {
                Some((files, edits)) if files > 1 => self.msg(format!(
                    "renamed to `{name}`: {edits} places in {files} files — :wa writes them all"
                )),
                Some((_, edits)) => {
                    self.msg(format!("renamed to `{name}`: {edits} place(s)"));
                }
                None => self.msg(format!("rename to `{name}`: nothing to rename here")),
            },
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
        self.diag_epoch += 1;
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

    /// Applies a WorkspaceEdit (`changes` or `documentChanges`), opening
    /// files into buffers as needed; one undo step per file. Returns
    /// (files, edits) applied, None when there was nothing to apply.
    pub(crate) fn apply_workspace_edit(
        &mut self,
        edit: &serde_json::Value,
    ) -> Option<(usize, usize)> {
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
            return None;
        }
        let (mut files, mut edits) = (0usize, 0usize);
        for (path, list) in per_file {
            let n = list.len();
            if self.apply_edits_to_file(&path, list) {
                files += 1;
                edits += n;
            }
        }
        self.lsp_note_edit();
        (files > 0).then_some((files, edits))
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
            let start = lsp_pos_to_char(&buffer.rope, sl as usize, sc as usize);
            let end = lsp_pos_to_char(&buffer.rope, el as usize, ec as usize).max(start);
            buffer.remove(start..end);
            buffer.insert(start, new_text);
        }
        // the edit may have removed the terminating newline (a whole-document
        // replacement, say); restore the buffer invariant
        let n = buffer.rope.len_chars();
        if n == 0 || buffer.rope.char(n - 1) != '\n' {
            buffer.insert(n, "\n");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_under_or_just_before_the_cursor() {
        let line = "let total_sum = foo(bar);";
        assert_eq!(identifier_at(line, 0), Some((0, 3)), "on `let`");
        assert_eq!(identifier_at(line, 6), Some((4, 13)), "inside total_sum");
        assert_eq!(
            identifier_at(line, 13),
            Some((4, 13)),
            "just past it (append)"
        );
        assert_eq!(identifier_at(line, 14), None, "on `=`… nothing before it");
        assert_eq!(identifier_at(line, 20), Some((20, 23)), "bar");
        assert_eq!(identifier_at(line, 24), None, "on `;` after `)`: nothing");
        assert_eq!(identifier_at("", 0), None);
        assert_eq!(identifier_at("   ", 1), None);
    }

    #[test]
    fn plain_identifiers_only() {
        assert!(is_plain_identifier("foo_bar2"));
        assert!(is_plain_identifier("_x"));
        assert!(is_plain_identifier("größe"));
        assert!(!is_plain_identifier("2fast"));
        assert!(!is_plain_identifier("with space"));
        assert!(!is_plain_identifier("a-b"));
        assert!(!is_plain_identifier(""));
    }
}
