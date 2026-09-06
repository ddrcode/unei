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
use crate::core::commands::PickerCmd;
use crate::editor::windows::SplitDir;

use super::Editor;

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
    /// Registry language for highlighting, if detected.
    pub lang: Option<String>,
    /// A one-line note instead of content (binary / empty / unreadable).
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
    fn refresh_preview(&mut self) {
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
            let rel = p.item(m).to_string();
            ed.file_picker = None;
            let split = match cmd {
                PickerCmd::OpenVsplit => Some(SplitDir::Vertical),
                PickerCmd::OpenHsplit => Some(SplitDir::Horizontal),
                _ => None,
            };
            ed.open_path(&rel, split);
        }
        PickerCmd::CreatePath => {
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
    let head = &data[..data.len().min(PREVIEW_BYTES)];
    if head.contains(&0) {
        return note(format!("binary · {}", human_size(data.len())));
    }
    if data.is_empty() {
        return note("empty file".into());
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
    use super::{build_preview, valid_new_path};

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
