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
use crate::core::text::first_non_blank;
use crate::editor::windows::SplitDir;

use super::Editor;

/// What the picker is picking (#62): files to open, or symbols in the
/// current buffer to jump to. The fuzzy list and rendering are shared.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PickerKind {
    Files,
    Symbols,
}

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
}

impl FilePicker {
    pub fn item(&self, m: &Match) -> &str {
        &self.items[m.item]
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// Absolute path of the currently selected match.
    fn selected_path(&self) -> Option<PathBuf> {
        let m = self.matches.get(self.selected)?;
        Some(self.root.join(self.item(m)))
    }

    /// Rebuilds the preview for the selected file, unless it hasn't changed.
    /// Only files have a preview; the symbol picker is a plain list.
    fn refresh_preview(&mut self) {
        if self.kind != PickerKind::Files {
            return;
        }
        let path = self.selected_path();
        if path == self.preview_for {
            return;
        }
        self.preview = path.as_deref().map(build_preview);
        self.preview_for = path;
    }

    fn refilter(&mut self) {
        self.selected = 0;
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
    }
}

/// Whether a file's head is binary rather than text: a NUL byte, bytes that
/// don't decode as UTF-8 (a multibyte char clipped by the sample boundary
/// doesn't count), or valid text littered with control chars. A plain
/// NUL test misses short 6502 images whose sampled head carries no NUL.
fn is_binary(head: &[u8]) -> bool {
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(text) => {
            let ctrl = text
                .chars()
                .filter(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{c}'))
                .count();
            ctrl * 20 > text.chars().count().max(1) // >5% control ⇒ binary
        }
        // an invalid byte within the last 3 is a char cut by the sample edge,
        // not real binary; anything earlier means binary
        Err(e) => e.valid_up_to() < head.len().saturating_sub(3),
    }
}

/// A classic hex dump of the first `max_lines` rows of 16 bytes: offset,
/// the bytes in two octets, then an ASCII gutter (non-printable → `.`).
fn hex_dump(data: &[u8], max_lines: usize) -> Vec<String> {
    data.chunks(16)
        .take(max_lines)
        .enumerate()
        .map(|(row, chunk)| {
            let mut hex = String::new();
            for (i, b) in chunk.iter().enumerate() {
                if i == 8 {
                    hex.push(' '); // gap between the two octets
                }
                hex.push_str(&format!("{b:02x} "));
            }
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            // pad the hex column (16×"xx " + 1 gap = 49) so gutters align
            format!("{:08x}  {hex:<49}|{ascii}|", row * 16)
        })
        .collect()
}

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
    use super::{build_preview, hex_dump, is_binary, valid_new_path};

    #[test]
    fn detects_binary_without_a_nul_byte() {
        // a small 6502 image: high opcode bytes, no NUL in the sampled head —
        // the old `contains(&0)` check let this through as garbage text
        let prg = [0x01u8, 0x08, 0xA9, 0x02, 0x8D, 0x20, 0xD0, 0xA2, 0xFF, 0xCA];
        assert!(is_binary(&prg));
        assert!(!is_binary(b"fn main() {}\n// plain rust\n"));
        // a multibyte char clipped by the sample boundary is still text
        let mut cut = "café au lait ".repeat(4).into_bytes();
        cut.push(0xC3); // dangling lead byte of a 2-byte sequence
        assert!(!is_binary(&cut));
    }

    #[test]
    fn hex_dump_has_offset_bytes_and_ascii_gutter() {
        let rows = hex_dump(&[0x00, 0xC0, 0x41, 0x42], 8);
        assert_eq!(rows.len(), 1);
        // offset, the bytes, then printable ascii ('.' for non-printable)
        assert!(rows[0].starts_with("00000000  00 c0 41 42 "), "{}", rows[0]);
        assert!(rows[0].ends_with("|..AB|"), "{}", rows[0]);
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
