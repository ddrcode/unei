use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    /// A view of something not meant to be edited (a binary opened as hex,
    /// #69): mutations are inert and `save` is refused, so the source on disk
    /// can't be clobbered.
    pub read_only: bool,
    /// The file as last read or written: (mtime, size). A mismatch with the
    /// file now means something else changed it (#80) — `save` refuses
    /// unless forced, and the editor's poll reloads or warns.
    disk_stamp: Option<(SystemTime, u64)>,
    /// Set by the poll when the file changed on disk while this buffer holds
    /// unsaved edits; shown in the statusline, cleared by a reload or `:w!`.
    pub disk_conflict: bool,
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

/// The file's (mtime, size) — the identity of "what's on disk right now".
/// Size rides along so a rewrite within one mtime tick still registers.
fn disk_stamp_of(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: normalized(text),
            path: None,
            read_only: false,
            disk_stamp: None,
            disk_conflict: false,
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
        if existed {
            buf.disk_stamp = disk_stamp_of(path);
        }
        Ok((buf, existed))
    }

    /// Whether the file on disk differs from what this buffer last read or
    /// wrote (#80): the mtime or size moved. A buffer never on disk, or a
    /// file since deleted, reports `false`.
    pub fn disk_changed(&self) -> bool {
        match (self.path.as_deref(), self.disk_stamp) {
            (Some(p), Some(recorded)) => disk_stamp_of(p).is_some_and(|now| now != recorded),
            _ => false,
        }
    }

    /// Drops the recorded disk stamp — after `:w {path}` rebinds the buffer,
    /// the previous file's identity must not veto the first write.
    pub(crate) fn forget_disk_stamp(&mut self) {
        self.disk_stamp = None;
        self.disk_conflict = false;
    }

    /// Reads the file's current content from disk (for a reload).
    pub fn read_disk(&self) -> Result<String> {
        let path = self.path.as_deref().context("no file name")?;
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
    }

    /// Opens a compiled `.prg` (a Commodore/X16 program: 2-byte little-endian
    /// load address, then the 65C02 code image) as its disassembly — a
    /// read-only ACME listing produced by the machine lens's opcode table
    /// (#65). Bound to a synthetic `.s` path so it highlights and answers `K`
    /// like source, and a `:w` saves that source rather than the binary.
    pub fn from_prg(path: &Path) -> Result<Buffer> {
        let bytes = fs::read(path).with_context(|| format!("cannot open {}", path.display()))?;
        if bytes.len() < 3 {
            anyhow::bail!("{}: not a .prg (need at least 3 bytes)", path.display());
        }
        let org = u16::from_le_bytes([bytes[0], bytes[1]]);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "prg".to_string());
        let text = crate::lens::disassemble(&bytes[2..], org, &name, bytes.len());
        let mut buf = Self::from_text(&text);
        buf.path = Some(PathBuf::from(format!("{name}.disasm.s")));
        Ok(buf)
    }

    /// Opens any binary file as a read-only hex view (#69): a full hex dump
    /// in a buffer that refuses edits and saves, so the file on disk can't be
    /// clobbered. Large files are dumped up to a cap with a truncation note.
    pub fn from_hex(path: &Path) -> Result<Buffer> {
        const HEX_VIEW_BYTES: usize = 1 << 20; // 1 MiB — plenty for any ROM
        let bytes = fs::read(path).with_context(|| format!("cannot open {}", path.display()))?;
        let shown = bytes.len().min(HEX_VIEW_BYTES);
        let mut lines = crate::core::hex::hex_dump(&bytes[..shown], usize::MAX);
        if shown < bytes.len() {
            lines.push(format!(
                "… {} more bytes not shown (hex view capped at {} KiB)",
                bytes.len() - shown,
                HEX_VIEW_BYTES / 1024
            ));
        }
        let mut buf = Self::from_text(&lines.join("\n"));
        buf.path = Some(path.to_path_buf());
        buf.read_only = true;
        Ok(buf)
    }

    /// Declares the current content identical to what is on disk (used
    /// after reloading an externally formatted file), and re-stamps the file
    /// so that write isn't later mistaken for an external change.
    pub(crate) fn mark_saved(&mut self) {
        self.saved_version = self.version;
        self.disk_stamp = self.path.as_deref().and_then(disk_stamp_of);
        self.disk_conflict = false;
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Writes atomically: temp file in the same directory, then rename.
    /// Refuses when the file changed on disk since it was read or written
    /// (#80) unless `force` — an agent's or formatter's edits must never be
    /// silently overwritten.
    pub fn save(&mut self, force: bool) -> Result<(PathBuf, usize)> {
        if self.read_only {
            anyhow::bail!("read-only buffer");
        }
        if !force && self.disk_changed() {
            anyhow::bail!("file changed on disk — :w! overwrites, :e reloads");
        }
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
                ".{}.unei.tmp",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
            )),
            None => PathBuf::from(format!(".{}.unei.tmp", path.display())),
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
        self.mark_saved(); // also re-stamps: this write is ours, not external
        Ok((path, crate::core::text::text_lines(&self.rope)))
    }

    pub fn insert(&mut self, char_idx: usize, text: &str) {
        if self.read_only || text.is_empty() {
            return;
        }
        self.rope.insert(char_idx, text);
        self.version += 1;
    }

    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        if self.read_only || range.is_empty() {
            return;
        }
        self.rope.remove(range);
        self.version += 1;
    }

    /// Starts an undoable change transaction; a snapshot of the current state
    /// is pushed and redo history is dropped. Nested calls are folded into the
    /// open transaction (e.g. `cw` opens one that the insert session extends).
    pub fn begin_change(&mut self, cursor: Cursor) {
        if self.read_only || self.open_change.is_some() {
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

    #[test]
    fn from_hex_is_read_only_and_bound_to_the_file() {
        let dir = std::env::temp_dir().join(format!("unei-hex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rom.bin");
        std::fs::write(&f, [0x01u8, 0x08, 0xA9, 0x02, 0x8D, 0x20, 0xD0]).unwrap();
        let buf = Buffer::from_hex(&f).unwrap();
        assert!(buf.read_only);
        assert_eq!(buf.path.as_deref(), Some(f.as_path())); // keeps the real name
        assert!(buf.rope.to_string().contains("00000000  01 08 a9 02"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_only_buffer_rejects_edits_and_save() {
        let mut b = Buffer::from_text("hello");
        b.read_only = true;
        b.begin_change(Cursor::default());
        b.insert(0, "X");
        b.remove(0..1);
        assert_eq!(b.rope.to_string(), "hello\n"); // untouched
        assert!(!b.is_modified());
        b.path = Some(PathBuf::from("/nonexistent/does-not-matter"));
        assert!(b.save(false).is_err()); // refuses, so the source can't be clobbered
    }
}
