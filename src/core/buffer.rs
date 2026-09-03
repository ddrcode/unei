use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ropey::Rope;

use crate::config::OPTIONS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

impl Cursor {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

struct Snapshot {
    rope: Rope,
    cursor: Cursor,
    version: u64,
}

pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    version: u64,
    saved_version: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Version at the moment the currently open change transaction started.
    open_change: Option<u64>,
}

/// The buffer invariant mirrors vim's line model: every line has a newline
/// terminator, so the rope always ends with '\n' (an empty buffer is "\n" —
/// one empty line). Files are normalized on load; editorconfig mandates the
/// final newline on save anyway.
fn normalized(text: &str) -> Rope {
    if text.ends_with('\n') {
        Rope::from_str(text)
    } else {
        let mut rope = Rope::from_str(text);
        rope.insert(rope.len_chars(), "\n");
        rope
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: normalized(text),
            path: None,
            version: 0,
            saved_version: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            open_change: None,
        }
    }

    /// Opens `path`, or starts an empty buffer bound to it when the file does
    /// not exist yet (vim's "[New File]").
    pub fn from_path(path: &Path) -> Result<(Self, bool)> {
        let (rope, existed) = match fs::read_to_string(path) {
            Ok(text) => (normalized(&text), true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (normalized(""), false),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot open {}", path.display()));
            }
        };
        let mut buf = Self::from_text("");
        buf.rope = rope;
        buf.path = Some(path.to_path_buf());
        Ok((buf, existed))
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Writes atomically: temp file in the same directory, then rename.
    pub fn save(&mut self) -> Result<(PathBuf, usize)> {
        let path = self.path.clone().context("no file name")?;
        if OPTIONS.final_newline {
            let n = self.rope.len_chars();
            if n > 0 && self.rope.char(n - 1) != '\n' {
                self.rope.insert(n, "\n");
                self.version += 1;
            }
        }
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
        let tmp = match dir {
            Some(d) => d.join(format!(
                ".{}.tailored.tmp",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
            )),
            None => PathBuf::from(format!(".{}.tailored.tmp", path.display())),
        };
        let write = (|| -> Result<()> {
            let mut f = fs::File::create(&tmp)?;
            for chunk in self.rope.chunks() {
                f.write_all(chunk.as_bytes())?;
            }
            f.sync_all()?;
            fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if write.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write.with_context(|| format!("cannot write {}", path.display()))?;
        self.saved_version = self.version;
        Ok((path, crate::core::text::text_lines(&self.rope)))
    }

    pub fn insert(&mut self, char_idx: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        self.rope.insert(char_idx, text);
        self.version += 1;
    }

    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.rope.remove(range);
        self.version += 1;
    }

    /// Starts an undoable change transaction; a snapshot of the current state
    /// is pushed and redo history is dropped. Nested calls are folded into the
    /// open transaction (e.g. `cw` opens one that the insert session extends).
    pub fn begin_change(&mut self, cursor: Cursor) {
        if self.open_change.is_some() {
            return;
        }
        self.open_change = Some(self.version);
        self.redo.clear();
        self.undo.push(Snapshot {
            rope: self.rope.clone(),
            cursor,
            version: self.version,
        });
    }

    /// Ends the open transaction. Returns true when the buffer actually
    /// changed; otherwise the snapshot is discarded.
    pub fn end_change(&mut self) -> bool {
        match self.open_change.take() {
            Some(v0) if v0 == self.version => {
                self.undo.pop();
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    pub fn undo(&mut self, cursor: Cursor) -> Option<Cursor> {
        let snap = self.undo.pop()?;
        self.redo.push(Snapshot {
            rope: std::mem::replace(&mut self.rope, snap.rope),
            cursor,
            version: self.version,
        });
        self.version = snap.version;
        Some(snap.cursor)
    }

    pub fn redo(&mut self, cursor: Cursor) -> Option<Cursor> {
        let snap = self.redo.pop()?;
        self.undo.push(Snapshot {
            rope: std::mem::replace(&mut self.rope, snap.rope),
            cursor,
            version: self.version,
        });
        self.version = snap.version;
        Some(snap.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_transaction_leaves_no_undo_entry() {
        let mut b = Buffer::from_text("abc");
        b.begin_change(Cursor::new(0, 0));
        assert!(!b.end_change());
        assert!(b.undo(Cursor::default()).is_none());
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut b = Buffer::from_text("abc");
        b.begin_change(Cursor::new(0, 0));
        b.insert(3, "def");
        assert!(b.end_change());
        assert!(b.is_modified());

        let cur = b.undo(Cursor::new(0, 5)).unwrap();
        assert_eq!(b.rope.to_string(), "abc\n");
        assert_eq!(cur, Cursor::new(0, 0));
        assert!(!b.is_modified());

        let cur = b.redo(Cursor::new(0, 0)).unwrap();
        assert_eq!(b.rope.to_string(), "abcdef\n");
        assert_eq!(cur, Cursor::new(0, 5));
    }

    #[test]
    fn new_change_drops_redo() {
        let mut b = Buffer::from_text("");
        b.begin_change(Cursor::default());
        b.insert(0, "one");
        b.end_change();
        b.undo(Cursor::default());
        b.begin_change(Cursor::default());
        b.insert(0, "two");
        b.end_change();
        assert!(b.redo(Cursor::default()).is_none());
        assert_eq!(b.rope.to_string(), "two\n");
    }

    #[test]
    fn buffers_are_newline_terminated() {
        assert_eq!(Buffer::from_text("").rope.to_string(), "\n");
        assert_eq!(Buffer::from_text("a").rope.to_string(), "a\n");
        assert_eq!(Buffer::from_text("a\n").rope.to_string(), "a\n");
    }
}
