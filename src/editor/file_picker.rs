//! The fuzzy file picker (snacks.nvim-inspired, zero configuration).
//!
//! Listing walks the working folder with ripgrep's `ignore` rules (gitignore
//! respected, hidden files skipped); matching and ranking use nucleo, the
//! fuzzy engine behind Helix. Typed characters edit the query; commands sit
//! on control keys (`config::keymap::picker_token`).

use std::path::{Component, Path, PathBuf};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::config::keymap::{self, Key};
use crate::core::buffer::Cursor;
use crate::core::commands::PickerCmd;
use crate::core::hex::{hex_dump, is_binary};
use crate::core::text::first_non_blank;
use crate::editor::windows::SplitDir;

use unicode_width::UnicodeWidthStr;

use super::Editor;

/// What the picker is picking: files to open (#26), symbols in the current
/// buffer (#62), or lines matching a regex across the project (grep, #72).
/// The list, preview and rendering are shared; only the source differs.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PickerKind {
    Files,
    Symbols,
    Grep,
    /// A static list of places — references, diagnostics (#98, #99) — with
    /// grep's row shape, preview and jump, but fuzzy-filtered like the
    /// other static lists rather than re-searched per keystroke.
    Locations,
    /// The open buffers (#102): `:ls`-style rows, a preview of each
    /// buffer's own text, Enter switches, Ctrl+D closes.
    Buffers,
}

impl PickerKind {
    /// Rows are `path:line: text` and Enter jumps to a position.
    pub fn is_location_list(self) -> bool {
        matches!(self, PickerKind::Grep | PickerKind::Locations)
    }
}

/// One row of a location list: its display text, the char positions to
/// accent when nothing is typed (the referenced name, the severity), and
/// where it jumps.
pub struct LocationRow {
    pub display: String,
    pub accent: Vec<u32>,
    pub hit: GrepHit,
}

/// One grep result: where to jump when it's chosen (path relative to root,
/// 0-based line and char column of the match).
pub struct GrepHit {
    pub path: String,
    pub line: usize,
    pub col: usize,
}

/// Live-grep caps: stop after this many matches, and never read a file bigger
/// than this (keeps each keystroke's synchronous walk bounded).
const MAX_GREP_MATCHES: usize = 500;
const MAX_GREP_FILE_BYTES: u64 = 512 * 1024;

/// Hard cap on the number of files walked (keeps huge trees responsive).
const MAX_FILES: usize = 50_000;
/// Preview reads at most this many bytes / renders at most this many lines —
/// enough to recall what a file is; open it in the editor for more (#26).
const PREVIEW_BYTES: usize = 128 * 1024;
const PREVIEW_LINES: usize = 240;

/// The selected file's head, for the picker's preview pane. Scroll-free by
/// design: the first lines only.
#[derive(Clone)]
pub struct Preview {
    pub lines: Vec<String>,
    /// Registry language for highlighting, if detected (never for a hex dump).
    pub lang: Option<String>,
    /// A one-line header shown above the content: the size for a binary hex
    /// dump, or a standalone status (empty / blank / unreadable) with no lines.
    pub note: Option<String>,
    /// The line within `lines` to highlight (a grep match); none otherwise.
    pub focus: Option<usize>,
}

pub struct Match {
    pub item: usize,
    pub score: u32,
    /// Char positions of the matched query characters, for highlighting.
    pub indices: Vec<u32>,
}

pub struct FilePicker {
    pub query: String,
    items: Vec<String>,
    pub matches: Vec<Match>,
    pub selected: usize,
    pub truncated: bool,
    root: PathBuf,
    pub preview: Option<Preview>,
    /// The path the current preview was built for (skip re-reads on typing).
    preview_for: Option<PathBuf>,
    pub kind: PickerKind,
    /// Symbols mode: the source line for each item, parallel to `items`.
    targets: Vec<usize>,
    /// Grep and locations: where each result jumps to, parallel to `items`.
    grep_hits: Vec<GrepHit>,
    /// Locations: the accent of each row when the query is empty.
    accents: Vec<Vec<u32>>,
    /// Buffers: the buffer behind each row, and its preview (built from
    /// the buffer's text, which may differ from the file on disk).
    buffers: Vec<(super::BufId, Preview)>,
    /// The frame title.
    pub label: &'static str,
    /// Display cells of the widest row, measured when the rows are set —
    /// the list pane is sized to it (#114), and measuring per frame would
    /// cost a walk over every row.
    pub widest: usize,
}

/// Display cells of the widest row.
fn widest(items: &[String]) -> usize {
    items.iter().map(|s| s.width()).max().unwrap_or(0)
}

/// The picker's panes as (x, y, width, height).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerLayout {
    pub list: (u16, u16, u16, u16),
    pub preview: Option<(u16, u16, u16, u16)>,
}

/// Where the picker's panes go (#114), from the terminal size, the widest
/// row (plus what each row wears: marker, icon) and whether there is a
/// preview to show. The list is as wide as its longest row needs, between
/// a floor and *half* the picker — paths read whole wherever they can,
/// and the preview takes what is left. A terminal too narrow for the two
/// side by side stacks the preview under the list rather than dropping it.
pub fn layout(
    area_w: u16,
    area_h: u16,
    widest_row: usize,
    row_decor: usize,
    has_preview: bool,
) -> PickerLayout {
    let h = (area_h * 4 / 5).clamp(8, area_h.saturating_sub(2).max(8));
    let need = (widest_row + row_decor + 2).min(u16::MAX as usize) as u16; // + borders
    if has_preview && area_w >= 96 {
        let total = (area_w * 4 / 5).min(area_w.saturating_sub(4));
        let list = need.clamp(34, (total / 2).max(34));
        let x = area_w.saturating_sub(total) / 2;
        let y = area_h.saturating_sub(h) / 3;
        return PickerLayout {
            list: (x, y, list, h),
            preview: Some((x + list, y, total - list, h)),
        };
    }
    if has_preview && area_h >= 24 {
        // stacked: full-width rows, the preview underneath
        let w = area_w.saturating_sub(4).max(30).min(area_w);
        let list_h = (h * 2 / 5).max(8);
        let x = area_w.saturating_sub(w) / 2;
        let y = area_h.saturating_sub(h) / 3;
        return PickerLayout {
            list: (x, y, w, list_h),
            preview: Some((x, y + list_h, w, h - list_h)),
        };
    }
    let w = (area_w * 3 / 5).clamp(30, 100).min(area_w);
    PickerLayout {
        list: (
            area_w.saturating_sub(w) / 2,
            area_h.saturating_sub(h) / 3,
            w,
            h,
        ),
        preview: None,
    }
}

/// A row cut to `max` cells, with the original char index of every shown
/// char (None for the ellipsis) so fuzzy hits and the dimmed directory can
/// be mapped onto it. Location rows keep their head (`path:line:` is the
/// point); everything else keeps its tail — the filename whole, and the
/// first path segment too when it fits: `grammars/…/tree_sitter/alloc.h`
/// rather than `…02/src/tree_sitter/alloc.h` (#114).
pub fn fit_row(item: &str, max: usize, keep_tail: bool) -> (String, Vec<Option<usize>>) {
    let chars: Vec<char> = item.chars().collect();
    let n = chars.len();
    if n <= max {
        return (item.to_string(), (0..n).map(Some).collect());
    }
    if max < 2 {
        return ("…".to_string(), vec![None]);
    }
    let (a, b) = if !keep_tail {
        (max - 1, n)
    } else {
        let name_start = chars.iter().rposition(|&c| c == '/').map_or(0, |i| i + 1);
        let first_seg = chars.iter().position(|&c| c == '/').map(|i| i + 1);
        let name_len = n - name_start;
        match first_seg {
            // "head/…/tail": the tail starts at a segment boundary and
            // holds at least "/name"
            Some(h) if h < name_start && h + 2 + name_len <= max => {
                let mut b = n - (max - h - 1);
                while chars[b] != '/' {
                    b += 1;
                }
                (h, b)
            }
            _ => (0, n - (max - 1)),
        }
    };
    let mut shown = String::with_capacity(max * 2);
    let mut origin = Vec::with_capacity(max);
    for (i, c) in chars.iter().enumerate().take(a) {
        shown.push(*c);
        origin.push(Some(i));
    }
    shown.push('…');
    origin.push(None);
    for (i, c) in chars.iter().enumerate().skip(b) {
        shown.push(*c);
        origin.push(Some(i));
    }
    (shown, origin)
}

/// The directory part of the path in a row — char range up to and
/// including its last slash — for dimming, so the name stands out (#114).
/// The path is the first blank-delimited token holding a slash, cut at a
/// colon (`path:line:` rows); rows without a slash have none.
pub fn dir_range(item: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = item.chars().collect();
    let slash = chars.iter().position(|&c| c == '/')?;
    let start = chars[..slash]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |i| i + 1);
    let end = chars[slash..]
        .iter()
        .position(|&c| c.is_whitespace() || c == ':')
        .map_or(chars.len(), |i| slash + i);
    let last = chars[start..end].iter().rposition(|&c| c == '/')?;
    Some((start, start + last + 1))
}

impl FilePicker {
    pub fn item(&self, m: &Match) -> &str {
        &self.items[m.item]
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// Appends pasted text to the query (a path, a grep pattern) as one
    /// update — the picker owns input while it's open, paste included (#82
    /// §11). Single-line: newlines become spaces.
    pub(crate) fn push_query(&mut self, text: &str) {
        let flat = text.replace('\n', " ");
        self.query.push_str(flat.trim());
        self.refilter();
    }

    /// The preview pane's title for the current selection: `path:line` for a
    /// grep hit, otherwise the file's name.
    pub fn preview_title(&self) -> String {
        let Some(m) = self.matches.get(self.selected) else {
            return "preview".to_string();
        };
        match self.kind {
            PickerKind::Grep => {
                let h = &self.grep_hits[m.item];
                format!("{}:{}", h.path, h.line + 1)
            }
            PickerKind::Locations => {
                // the row's own `path:line:` head — hits carry absolute paths
                let row = self.item(m);
                let head = row.split(": ").next().unwrap_or(row);
                head.trim_start_matches(|c: char| c.is_ascii_uppercase() || c == ' ')
                    .to_string()
            }
            _ => Path::new(self.item(m))
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.item(m).to_string()),
        }
    }

    /// Absolute path of the currently selected match.
    fn selected_path(&self) -> Option<PathBuf> {
        let m = self.matches.get(self.selected)?;
        Some(self.root.join(self.item(m)))
    }

    /// Rebuilds the preview for the selection. Files show their head; grep
    /// shows the matched file around the hit line; symbols have no preview.
    fn refresh_preview(&mut self) {
        match self.kind {
            PickerKind::Files => {
                let path = self.selected_path();
                if path == self.preview_for {
                    return;
                }
                self.preview = path.as_deref().map(build_preview);
                self.preview_for = path;
            }
            PickerKind::Grep | PickerKind::Locations => {
                // rebuild on every move: the line, not just the file, frames it
                self.preview = self
                    .matches
                    .get(self.selected)
                    .map(|m| &self.grep_hits[m.item])
                    .map(|hit| build_preview_at(&self.root.join(&hit.path), hit.line));
                self.preview_for = None;
            }
            PickerKind::Buffers => {
                self.preview = self
                    .matches
                    .get(self.selected)
                    .and_then(|m| self.buffers.get(m.item))
                    .map(|(_, p)| p.clone());
                self.preview_for = None;
            }
            PickerKind::Symbols => {}
        }
    }

    /// Live grep (#72): the query is a regex (unei's search dialect, smartcase),
    /// run in-process across the project — `ignore` walks the tree, the Rust
    /// `regex` matches each line. Synchronous but bounded: needs two chars,
    /// caps matches and file size, skips anything that isn't UTF-8 text.
    fn grep_search(&mut self) {
        self.items.clear();
        self.grep_hits.clear();
        self.matches.clear();
        self.truncated = false;
        let Some(re) = (self.query.chars().count() >= 2)
            .then(|| crate::search::compile(&self.query))
            .flatten()
        else {
            return; // too short, or an incomplete/invalid regex — no results
        };
        'walk: for entry in walk(&self.root).flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_GREP_FILE_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue; // binary / non-utf8 — skip
            };
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .into_owned();
            for (line_idx, line) in text.lines().enumerate() {
                let Some(mat) = re.find(line) else { continue };
                let col = line[..mat.start()].chars().count();
                let (item, indices) = grep_row(&rel, line_idx, line, mat.start(), mat.end());
                let idx = self.items.len();
                self.items.push(item);
                self.grep_hits.push(GrepHit {
                    path: rel.clone(),
                    line: line_idx,
                    col,
                });
                self.matches.push(Match {
                    item: idx,
                    score: 0,
                    indices,
                });
                if self.items.len() >= MAX_GREP_MATCHES {
                    self.truncated = true;
                    break 'walk;
                }
            }
        }
        self.widest = widest(&self.items);
    }

    fn refilter(&mut self) {
        self.selected = 0;
        if self.kind == PickerKind::Grep {
            self.grep_search();
            self.refresh_preview();
            return;
        }
        if self.query.is_empty() {
            self.matches = (0..self.items.len())
                .map(|item| Match {
                    item,
                    score: 0,
                    indices: self.accents.get(item).cloned().unwrap_or_default(),
                })
                .collect();
            self.refresh_preview();
            return;
        }
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(&self.query, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let mut indices = Vec::new();
            let haystack = Utf32Str::new(item, &mut buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut indices) {
                indices.dedup();
                out.push(Match {
                    item: i,
                    score,
                    indices,
                });
            }
        }
        // best score first; ties by path length then name
        out.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| self.items[a.item].len().cmp(&self.items[b.item].len()))
                .then_with(|| self.items[a.item].cmp(&self.items[b.item]))
        });
        self.matches = out;
        self.refresh_preview();
    }
}

/// The one walk over the working folder, for the listing and for grep:
/// `.gitignore`/`.ignore`/git excludes honoured (in or out of a repo),
/// dotfiles *listed* — `.gitignore` and `.envrc` are files one edits (#110)
/// — and `.git/` itself always skipped, since without a repo check the
/// walker would descend into it.
fn walk(root: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(root)
        .require_git(false)
        .hidden(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
}

/// Walks the working folder and opens the picker.
pub fn open(ed: &mut Editor) {
    let mut items = Vec::new();
    let mut truncated = false;
    for entry in walk(ed.root()).flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if items.len() >= MAX_FILES {
            truncated = true;
            break;
        }
        let rel = entry.path().strip_prefix(ed.root()).unwrap_or(entry.path());
        items.push(rel.to_string_lossy().into_owned());
    }
    items.sort();
    let mut picker = FilePicker {
        query: String::new(),
        items,
        matches: Vec::new(),
        selected: 0,
        truncated,
        root: ed.root().to_path_buf(),
        preview: None,
        preview_for: None,
        kind: PickerKind::Files,
        targets: Vec::new(),
        grep_hits: Vec::new(),
        accents: Vec::new(),
        buffers: Vec::new(),
        label: " files ",
        widest: 0,
    };
    picker.widest = widest(&picker.items);
    picker.refilter();
    ed.file_picker = Some(picker);
}

/// Opens the live-grep picker (#72): empty until you type, then each keystroke
/// runs a regex search across the project in-process.
pub fn open_grep(ed: &mut Editor) {
    let mut picker = FilePicker {
        query: String::new(),
        items: Vec::new(),
        matches: Vec::new(),
        selected: 0,
        truncated: false,
        root: ed.root().to_path_buf(),
        preview: None,
        preview_for: None,
        kind: PickerKind::Grep,
        targets: Vec::new(),
        grep_hits: Vec::new(),
        accents: Vec::new(),
        buffers: Vec::new(),
        label: " grep ",
        widest: 0,
    };
    picker.widest = widest(&picker.items);
    picker.refilter();
    ed.file_picker = Some(picker);
}

/// Opens the symbol picker (#62): the current buffer's definitions, from
/// tree-sitter, to fuzzy-filter and jump to. Rust for now.
pub fn open_symbols(ed: &mut Editor) {
    let Some(path) = ed.buffer.path.as_deref() else {
        ed.err("no symbols: unsaved buffer");
        return;
    };
    let head = ed.buffer.rope.lines().map(|l| l.to_string()).take(5);
    let Some(lang) = crate::syntax::detect_lang_for(path, head) else {
        ed.err("no symbol support for this file type");
        return;
    };
    let symbols = crate::syntax::document_symbols(&ed.buffer.rope, lang);
    if symbols.is_empty() {
        ed.err("no symbols found");
        return;
    }
    let mut items = Vec::with_capacity(symbols.len());
    let mut targets = Vec::with_capacity(symbols.len());
    for s in &symbols {
        items.push(format!("{:<6} {}", s.kind, s.name));
        targets.push(s.line);
    }
    let mut picker = FilePicker {
        query: String::new(),
        items,
        matches: Vec::new(),
        selected: 0,
        truncated: false,
        root: ed.root().to_path_buf(),
        preview: None,
        preview_for: None,
        kind: PickerKind::Symbols,
        targets,
        grep_hits: Vec::new(),
        accents: Vec::new(),
        buffers: Vec::new(),
        label: " symbols ",
        widest: 0,
    };
    picker.widest = widest(&picker.items);
    picker.refilter();
    ed.file_picker = Some(picker);
}

/// Opens a location list (#98, #99): rows already formatted by the source
/// — references, diagnostics — in the order the source chose, to
/// fuzzy-filter, preview on the line, and jump to.
pub fn open_locations(ed: &mut Editor, label: &'static str, rows: Vec<LocationRow>) {
    let mut items = Vec::with_capacity(rows.len());
    let mut accents = Vec::with_capacity(rows.len());
    let mut grep_hits = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.display);
        accents.push(row.accent);
        grep_hits.push(row.hit);
    }
    let mut picker = FilePicker {
        query: String::new(),
        items,
        matches: Vec::new(),
        selected: 0,
        truncated: false,
        root: ed.root().to_path_buf(),
        preview: None,
        preview_for: None,
        kind: PickerKind::Locations,
        targets: Vec::new(),
        grep_hits,
        accents,
        buffers: Vec::new(),
        label,
        widest: 0,
    };
    picker.widest = widest(&picker.items);
    picker.refilter();
    ed.file_picker = Some(picker);
}

/// `Space b` (#102): the open buffers as a picker — `:ls`-style rows
/// (`%` current, `#` alternate, `[+]` modified), a preview of each buffer's
/// own text, fuzzy-filtered; Enter switches, Ctrl+V/Ctrl+X open in a
/// split, Ctrl+D closes the selected buffer. Opens on the current buffer.
pub fn open_buffers(ed: &mut Editor) {
    let entries = ed.buffer_entries();
    let mut items = Vec::with_capacity(entries.len());
    let mut buffers = Vec::with_capacity(entries.len());
    let mut current = 0;
    for (i, e) in entries.iter().enumerate() {
        let flag = if e.current {
            '%'
        } else if e.alternate {
            '#'
        } else {
            ' '
        };
        let modified = if e.modified { " [+]" } else { "" };
        items.push(format!("{:>2} {} {}{}", e.id, flag, e.name, modified));
        buffers.push((e.id, buffer_preview(ed, e.id)));
        if e.current {
            current = i;
        }
    }
    let mut picker = FilePicker {
        query: String::new(),
        items,
        matches: Vec::new(),
        selected: 0,
        truncated: false,
        root: ed.root().to_path_buf(),
        preview: None,
        preview_for: None,
        kind: PickerKind::Buffers,
        targets: Vec::new(),
        grep_hits: Vec::new(),
        accents: Vec::new(),
        buffers,
        label: " buffers ",
        widest: 0,
    };
    picker.widest = widest(&picker.items);
    picker.refilter();
    picker.selected = current;
    picker.refresh_preview();
    ed.file_picker = Some(picker);
}

/// A buffer's head as the picker previews it: its own text (unsaved edits
/// included), highlighted by its path's language.
fn buffer_preview(ed: &Editor, id: super::BufId) -> Preview {
    let buffer = ed.buffer_ref(id);
    let n = crate::core::text::text_lines(&buffer.rope).min(PREVIEW_LINES);
    let lines: Vec<String> = (0..n)
        .map(|l| crate::core::text::line_content(&buffer.rope, l))
        .collect();
    let lang = buffer.path.as_deref().and_then(|p| {
        crate::syntax::detect_lang_for(p, lines.iter().take(5).cloned()).map(str::to_string)
    });
    let blank = lines.iter().all(|l| l.trim().is_empty());
    Preview {
        note: blank.then(|| "empty".to_string()),
        lines: if blank { Vec::new() } else { lines },
        lang,
        focus: None,
    }
}

/// A location's row in grep's shape — `path:line: text` with the range
/// accented — for a source that already has the line's text (#98).
pub fn location_row(
    rel: &str,
    line_idx: usize,
    line: &str,
    mb0: usize,
    mb1: usize,
) -> (String, Vec<u32>) {
    grep_row(rel, line_idx, line, mb0, mb1)
}

pub fn handle_key(ed: &mut Editor, key: Key) {
    match keymap::picker_token(key) {
        Some(cmd) => command(ed, cmd),
        None => {
            if let Key::Char(c) = key
                && let Some(p) = &mut ed.file_picker
            {
                p.query.push(c);
                p.refilter();
            }
        }
    }
}

fn command(ed: &mut Editor, cmd: PickerCmd) {
    let Some(p) = &mut ed.file_picker else { return };
    match cmd {
        PickerCmd::Up => {
            p.selected = p.selected.saturating_sub(1);
            p.refresh_preview();
        }
        PickerCmd::Down => {
            p.selected = (p.selected + 1).min(p.matches.len().saturating_sub(1));
            p.refresh_preview();
        }
        PickerCmd::DeleteChar => {
            p.query.pop();
            p.refilter();
        }
        PickerCmd::ClearQuery => {
            p.query.clear();
            p.refilter();
        }
        PickerCmd::Dismiss => ed.file_picker = None,
        PickerCmd::Open | PickerCmd::OpenVsplit | PickerCmd::OpenHsplit => {
            let Some(m) = p.matches.get(p.selected) else {
                return;
            };
            match p.kind {
                PickerKind::Files => {
                    let rel = p.item(m).to_string();
                    ed.file_picker = None;
                    let split = match cmd {
                        PickerCmd::OpenVsplit => Some(SplitDir::Vertical),
                        PickerCmd::OpenHsplit => Some(SplitDir::Horizontal),
                        _ => None,
                    };
                    ed.open_path(&rel, split);
                }
                PickerKind::Symbols => {
                    let line = p.targets[m.item];
                    ed.file_picker = None;
                    ed.record_jump();
                    ed.cursor = Cursor::new(line, first_non_blank(&ed.buffer.rope, line));
                    ed.goal = None;
                    ed.clamp_cursor();
                    ed.scroll_to_cursor();
                }
                PickerKind::Buffers => {
                    let id = p.buffers[m.item].0;
                    ed.file_picker = None;
                    match cmd {
                        PickerCmd::OpenVsplit => ed.split_window(SplitDir::Vertical, false),
                        PickerCmd::OpenHsplit => ed.split_window(SplitDir::Horizontal, false),
                        _ => {}
                    }
                    ed.switch_to(id);
                }
                PickerKind::Grep | PickerKind::Locations => {
                    let hit = &p.grep_hits[m.item];
                    let (path, line, col) = (hit.path.clone(), hit.line, hit.col);
                    ed.file_picker = None;
                    let split = match cmd {
                        PickerCmd::OpenVsplit => Some(SplitDir::Vertical),
                        PickerCmd::OpenHsplit => Some(SplitDir::Horizontal),
                        _ => None,
                    };
                    ed.open_path(&path, split); // records the jump
                    ed.cursor = Cursor::new(line, col);
                    ed.goal = None;
                    ed.clamp_cursor();
                    ed.scroll_to_cursor();
                }
            }
        }
        PickerCmd::CloseEntry => {
            // Ctrl+D in the buffer picker: close the selected buffer and
            // rebuild the list (a refused close — unsaved changes — leaves
            // the list as it was, the error already on the message line)
            if p.kind != PickerKind::Buffers {
                return;
            }
            let Some(m) = p.matches.get(p.selected) else {
                return;
            };
            let id = p.buffers[m.item].0;
            let (query, selected) = (p.query.clone(), p.selected);
            let before = ed.buffer_count();
            ed.close_buffer(id, false);
            if ed.buffer_count() == before {
                return;
            }
            open_buffers(ed);
            if let Some(p) = &mut ed.file_picker {
                p.query = query;
                p.refilter();
                p.selected = selected.min(p.matches.len().saturating_sub(1));
                p.refresh_preview();
            }
        }
        PickerCmd::CreatePath => {
            if p.kind != PickerKind::Files {
                return; // creating a path is meaningless in the symbol picker
            }
            let rel = p.query.trim().to_string();
            if !valid_new_path(&rel) {
                ed.err(format!("Invalid path: {rel}"));
                return;
            }
            ed.file_picker = None;
            let abs = ed.root().join(&rel);
            if let Some(parent) = abs.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                ed.err(format!("Cannot create {}: {e}", parent.display()));
                return;
            }
            ed.open_path(&rel, None);
        }
    }
}

/// Reads a file's head for the preview: the first lines, with the language
/// detected for highlighting. Binary, empty, or unreadable files return a
/// note instead of content.
fn build_preview(path: &Path) -> Preview {
    let note = |n: String| Preview {
        lines: Vec::new(),
        lang: None,
        note: Some(n),
        focus: None,
    };
    let Ok(data) = std::fs::read(path) else {
        return note("unreadable".into());
    };
    if data.is_empty() {
        return note("empty file".into());
    }
    let head = &data[..data.len().min(PREVIEW_BYTES)];
    if is_binary(head) {
        // not text — a hex dump beats rendering mojibake (#67)
        return Preview {
            lines: hex_dump(head, PREVIEW_LINES),
            lang: None,
            note: Some(format!("binary · {}", human_size(data.len()))),
            focus: None,
        };
    }
    let text = String::from_utf8_lossy(head);
    let lines: Vec<String> = text
        .lines()
        .take(PREVIEW_LINES)
        .map(str::to_string)
        .collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return note("blank".into());
    }
    let lang =
        crate::syntax::detect_lang_for(path, lines.iter().take(5).cloned()).map(str::to_string);
    Preview {
        lines,
        lang,
        note: None,
        focus: None,
    }
}

/// Builds a preview of `path` framed on `line` (0-based) for a grep hit (#72):
/// a window starting a few lines above the match, with that line marked so the
/// renderer can highlight it.
fn build_preview_at(path: &Path, line: usize) -> Preview {
    let note = |n: String| Preview {
        lines: Vec::new(),
        lang: None,
        note: Some(n),
        focus: None,
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return note("unreadable".into());
    };
    let all: Vec<&str> = text.lines().collect();
    if all.is_empty() {
        return note("empty file".into());
    }
    let start = line.saturating_sub(6); // a little context above the hit
    let end = (start + PREVIEW_LINES).min(all.len());
    let lines: Vec<String> = all[start..end].iter().map(|s| s.to_string()).collect();
    let lang =
        crate::syntax::detect_lang_for(path, lines.iter().take(5).cloned()).map(str::to_string);
    Preview {
        focus: Some(line - start),
        lines,
        lang,
        note: None,
    }
}

/// Formats one grep result row — `"path:line: text"` with leading indent
/// trimmed and long lines clipped — and the char columns of the match within
/// it, for accenting. `mb0`/`mb1` are the match's byte range in `line`.
fn grep_row(rel: &str, line_idx: usize, line: &str, mb0: usize, mb1: usize) -> (String, Vec<u32>) {
    const MAX: usize = 200;
    let trimmed = line.trim_start();
    let lead = line.len() - trimmed.len();
    let prefix = format!("{rel}:{}: ", line_idx + 1);
    let pchars = prefix.chars().count();
    let shown: String = trimmed.chars().take(MAX).collect();
    let item = format!("{prefix}{shown}");
    let item_len = item.chars().count();
    // shift the match into the trimmed/clipped text, then into display columns
    let tb0 = mb0.saturating_sub(lead).min(trimmed.len());
    let tb1 = mb1.saturating_sub(lead).min(trimmed.len());
    let c0 = (pchars + trimmed[..tb0].chars().count()).min(item_len);
    let c1 = (pchars + trimmed[..tb1].chars().count()).min(item_len);
    (item, (c0..c1).map(|x| x as u32).collect())
}

/// Whether a file's head is binary rather than text: a NUL byte, bytes that
/// don't decode as UTF-8 (a multibyte char clipped by the sample boundary
/// doesn't count), or valid text littered with control chars. A plain
/// NUL test misses short 6502 images whose sampled head carries no NUL.
fn human_size(bytes: usize) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / (1 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

/// New paths stay inside the working folder: relative, no `..`, non-empty.
fn valid_new_path(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let path = std::path::Path::new(rel);
    !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::{build_preview, grep_row, valid_new_path};

    #[test]
    fn grep_row_formats_and_locates_the_match() {
        // line 10 (0-based 9), "42" at bytes 12..14 of the untrimmed line
        let (item, idx) = grep_row("src/main.rs", 9, "    let x = 42;", 12, 14);
        assert_eq!(item, "src/main.rs:10: let x = 42;");
        // prefix "src/main.rs:10: " is 16 chars, "let x = " is 8 → match at 24..26
        assert_eq!(idx, vec![24, 25]);
    }

    #[test]
    fn grep_row_clips_long_lines_without_panic() {
        let long = format!("{}needle", "x".repeat(400));
        let (item, _) = grep_row("f", 0, &long, 400, 406);
        assert!(item.chars().count() <= "f:1: ".chars().count() + 200);
    }

    #[test]
    fn binary_preview_is_a_hex_dump_not_a_bare_note() {
        let dir = std::env::temp_dir().join(format!("unei-pvhex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rom.prg");
        std::fs::write(&f, [0x01u8, 0x08, 0xA9, 0x02, 0x8D, 0x20, 0xD0]).unwrap();
        let pv = build_preview(&f);
        assert!(pv.note.unwrap().starts_with("binary"));
        assert!(!pv.lines.is_empty(), "binary preview should carry hex rows");
        assert!(
            pv.lines[0].starts_with("00000000  01 08 a9"),
            "{:?}",
            pv.lines
        );
        assert!(pv.lang.is_none()); // hex is plain, never syntax-highlighted
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_reads_text_and_detects_language() {
        let dir = std::env::temp_dir().join(format!("unei-pv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("m.rs");
        std::fs::write(&f, "fn main() {}\n// tail\n").unwrap();
        let pv = build_preview(&f);
        assert!(pv.note.is_none());
        assert_eq!(pv.lines.first().map(String::as_str), Some("fn main() {}"));
        assert_eq!(pv.lang.as_deref(), Some("rust"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_flags_binary_and_empty() {
        let dir = std::env::temp_dir().join(format!("unei-pvb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("b.bin");
        std::fs::write(&bin, [0u8, 1, 2, 3, 0]).unwrap();
        assert!(build_preview(&bin).note.unwrap().starts_with("binary"));
        let empty = dir.join("e.txt");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(build_preview(&empty).note.as_deref(), Some("empty file"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_path_validation() {
        assert!(valid_new_path("notes.md"));
        assert!(valid_new_path("docs/deep/notes.md"));
        assert!(!valid_new_path(""));
        assert!(!valid_new_path("/etc/passwd"));
        assert!(!valid_new_path("../outside.txt"));
        assert!(!valid_new_path("docs/../../outside.txt"));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn the_list_is_as_wide_as_its_longest_row_up_to_half() {
        // 140 cols: the float is 112 wide; a 39-cell path plus marker,
        // icon and borders needs 46 — it gets exactly that, not the floor
        let l = layout(140, 40, 39, 5, true);
        assert_eq!(l.list.2, 46);
        assert_eq!(l.preview.unwrap().2, 112 - 46);
        // a shallow project keeps the floor, and a big preview
        assert_eq!(layout(140, 40, 10, 5, true).list.2, 34);
        // deep paths stop at half the picker
        assert_eq!(layout(140, 40, 120, 5, true).list.2, 56);
        // the panes sit side by side on one row
        let (lx, ly, lw, lh) = l.list;
        let (px, py, _, ph) = l.preview.unwrap();
        assert_eq!((px, py, ph), (lx + lw, ly, lh));
    }

    #[test]
    fn a_narrow_terminal_stacks_the_preview_under_the_list() {
        let l = layout(80, 40, 60, 5, true);
        let (lx, ly, lw, lh) = l.list;
        let (px, py, pw, ph) = l.preview.unwrap();
        assert_eq!(lw, 76, "full-width rows");
        assert_eq!((px, pw), (lx, lw));
        assert_eq!(py, ly + lh, "preview directly below");
        assert!(ph >= 6 && lh >= 8);
        // too short to stack: list only, as before
        assert_eq!(layout(80, 16, 60, 5, true).preview, None);
        // nothing to preview: list only, whatever the size
        assert_eq!(layout(140, 40, 60, 5, false).preview, None);
    }

    #[test]
    fn fit_row_keeps_the_first_segment_and_the_whole_name() {
        let item = "grammars/asm6502/src/tree_sitter/alloc.h";
        let (s, o) = fit_row(item, 30, true);
        assert_eq!(s, "grammars/…/tree_sitter/alloc.h");
        assert_eq!(s.chars().count(), 30);
        assert_eq!(o[0], Some(0));
        assert_eq!(o[9], None, "the ellipsis maps to nothing");
        assert_eq!(o[10], Some(20), "the tail starts at a slash");
        assert_eq!(*o.last().unwrap(), Some(39));
        // too tight for a head: the tail alone, name still whole
        assert_eq!(fit_row(item, 12, true).0, "…ter/alloc.h");
        // fits: untouched, identity map
        let (s, o) = fit_row("src/main.rs", 30, true);
        assert_eq!(s, "src/main.rs");
        assert_eq!(o, (0..11).map(Some).collect::<Vec<_>>());
        // location rows keep their head
        assert_eq!(
            fit_row("src/a.rs:12: let x = 1;", 10, false).0,
            "src/a.rs:…"
        );
        // no slash at all: plain tail
        assert_eq!(fit_row("abcdefghij", 5, true).0, "…ghij");
    }

    #[test]
    fn dir_range_finds_the_path_in_every_row_shape() {
        assert_eq!(dir_range("src/editor/mod.rs"), Some((0, 11)));
        assert_eq!(dir_range("README.md"), None, "no directory");
        assert_eq!(dir_range("src/a.rs:12: let x = 1;"), Some((0, 4)));
        assert_eq!(dir_range("E src/main.rs:1: bad thing"), Some((2, 6)));
        assert_eq!(dir_range(" 1 % src/core/text.rs [+]"), Some((5, 14)));
        assert_eq!(dir_range("fn     name"), None);
    }
}
