//! The fuzzy file picker (snacks.nvim-inspired, zero configuration).
//!
//! Listing walks the working folder with ripgrep's `ignore` rules (gitignore
//! respected, hidden files skipped); matching and ranking use nucleo, the
//! fuzzy engine behind Helix. Typed characters edit the query; commands sit
//! on control keys (`config::keymap::picker_token`).

use std::path::Component;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::config::keymap::{self, Key};
use crate::core::commands::PickerCmd;
use crate::editor::windows::SplitDir;

use super::Editor;

/// Hard cap on the number of files walked (keeps huge trees responsive).
const MAX_FILES: usize = 50_000;

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
}

impl FilePicker {
    pub fn item(&self, m: &Match) -> &str {
        &self.items[m.item]
    }

    pub fn total(&self) -> usize {
        self.items.len()
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
        PickerCmd::Up => p.selected = p.selected.saturating_sub(1),
        PickerCmd::Down => {
            p.selected = (p.selected + 1).min(p.matches.len().saturating_sub(1));
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
    use super::valid_new_path;

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
