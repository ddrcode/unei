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

use super::Editor;

/// What the picker is picking: files to open (#26), symbols in the current
/// buffer (#62), or lines matching a regex across the project (grep, #72).
/// The list, preview and rendering are shared; only the source differs.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PickerKind {
    Files,
    Symbols,
    Grep,
}

/// One grep result: where to jump when it's chosen (path relative to root,
/// 0-based line and char column of the match).
pub struct GrepHit {
    path: String,
    line: usize,
    col: usize,
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
    /// Grep mode: where each result jumps to, parallel to `items`.
    grep_hits: Vec<GrepHit>,
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
            PickerKind::Grep => {
                // rebuild on every move: the line, not just the file, frames it
                self.preview = self
                    .matches
                    .get(self.selected)
                    .map(|m| &self.grep_hits[m.item])
                    .map(|hit| build_preview_at(&self.root.join(&hit.path), hit.line));
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
        let walker = ignore::WalkBuilder::new(&self.root)
            .require_git(false)
            .follow_links(false)
            .build();
        'walk: for entry in walker.flatten() {
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
                    indices: Vec::new(),
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

/// Walks the working folder and opens the picker.
pub fn open(ed: &mut Editor) {
    let mut items = Vec::new();
    let mut truncated = false;
    let walker = ignore::WalkBuilder::new(ed.root())
        .require_git(false)
        .follow_links(false)
        .build();
    for entry in walker.flatten() {
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
    };
    picker.refilter();
    ed.buffer_list = None;
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
    };
    picker.refilter();
    ed.buffer_list = None;
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
    };
    picker.refilter();
    ed.buffer_list = None;
    ed.file_picker = Some(picker);
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
                PickerKind::Grep => {
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
