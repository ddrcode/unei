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
    /// The content state this snapshot holds (see `Buffer::state`).
    state: u64,
}

/// What the buffer knows about its file on disk (#80, #82).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DiskState {
    /// Not bound to a real file yet (a scratch buffer): nothing to compare.
    NoBaseline,
    /// Bound to a path that did not exist when opened. If it appears, some
    /// other process wrote it — that is an external change too.
    Absent,
    /// The file as last read or written: (mtime, size). Size rides along so
    /// a rewrite within one mtime tick still registers.
    Present(SystemTime, u64),
}

/// A buffer's file binding, so a failed `:w {path}` can put it back.
pub(crate) struct Binding(Option<PathBuf>, DiskState);

pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    /// A view of something not meant to be edited (a binary opened as hex,
    /// #69): mutations are inert and `save` is refused, so the source on disk
    /// can't be clobbered.
    pub read_only: bool,
    disk: DiskState,
    /// Set by the poll when the file changed on disk while this buffer holds
    /// unsaved edits; shown in the statusline, cleared by a reload or `:w!`.
    pub disk_conflict: bool,
    /// Bumps on every change to the rope, undo and redo included, and never
    /// runs backwards — the key every cache and the LSP synchronise on.
    revision: u64,
    /// Identifies the *content*: a fresh id per mutation, restored by
    /// undo/redo to the id that content had before. Comparing it to
    /// `saved_state` answers "is this exactly what was saved?" even across
    /// undo branches (#82 §1 — a plain counter that undo rewound let a
    /// different text reuse the saved number).
    state: u64,
    saved_state: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// State when the outermost open change transaction started.
    open_change: Option<u64>,
    /// Nesting depth of `begin_change`/`end_change`: only the outermost pair
    /// snapshots and commits, so a helper (completion, an LSP edit) inside an
    /// insert session can't close the session's transaction (#82 §12).
    change_depth: u32,
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
            disk: DiskState::NoBaseline,
            disk_conflict: false,
            revision: 0,
            state: 0,
            saved_state: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            open_change: None,
            change_depth: 0,
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
        buf.disk = match disk_stamp_of(path) {
            Some((t, n)) if existed => DiskState::Present(t, n),
            _ => DiskState::Absent,
        };
        Ok((buf, existed))
    }

    /// Whether the file on disk differs from what this buffer last read or
    /// wrote (#80): the mtime or size moved, or a file that was absent when
    /// opened has since appeared (#82 §5). A scratch buffer, or a file since
    /// deleted, reports `false`.
    pub fn disk_changed(&self) -> bool {
        let Some(p) = self.path.as_deref() else {
            return false;
        };
        match self.disk {
            DiskState::NoBaseline => false,
            DiskState::Absent => disk_stamp_of(p).is_some(),
            DiskState::Present(t, n) => disk_stamp_of(p).is_some_and(|now| now != (t, n)),
        }
    }

    /// The current file binding, to restore if a `:w {path}` fails.
    pub(crate) fn binding(&self) -> Binding {
        Binding(self.path.clone(), self.disk)
    }

    /// Rebinds to `path` for a `:w {path}`: no baseline yet — the new file's
    /// identity is recorded by the write itself.
    pub(crate) fn rebind(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.disk = DiskState::NoBaseline;
        self.disk_conflict = false;
    }

    pub(crate) fn restore_binding(&mut self, b: Binding) {
        self.path = b.0;
        self.disk = b.1;
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
        use std::io::Read;
        const HEX_VIEW_BYTES: u64 = 1 << 20; // 1 MiB — plenty for any ROM
        // read only up to the cap (#82: a multi-GB file must not be slurped
        // just to show its first megabyte); the size comes from metadata
        let total = fs::metadata(path)
            .with_context(|| format!("cannot open {}", path.display()))?
            .len();
        let mut bytes = Vec::new();
        fs::File::open(path)
            .with_context(|| format!("cannot open {}", path.display()))?
            .take(HEX_VIEW_BYTES)
            .read_to_end(&mut bytes)?;
        let mut lines = crate::core::hex::hex_dump(&bytes, usize::MAX);
        if total > bytes.len() as u64 {
            lines.push(format!(
                "… {} more bytes not shown (hex view capped at {} KiB)",
                total - bytes.len() as u64,
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
        self.saved_state = self.state;
        self.disk = match self.path.as_deref().and_then(disk_stamp_of) {
            Some((t, n)) => DiskState::Present(t, n),
            None => DiskState::NoBaseline,
        };
        self.disk_conflict = false;
    }

    /// Whether the content differs from what was last saved or loaded. Exact
    /// across undo: returning to the saved text reads as clean, and a fresh
    /// edit after an undo reads as modified (#82 §1).
    pub fn is_modified(&self) -> bool {
        self.state != self.saved_state
    }

    /// Monotonic change counter (undo/redo bump it too) — a cache key, never
    /// an identity of content.
    pub fn version(&self) -> u64 {
        self.revision
    }

    /// Every rope mutation goes through here: a new revision, a fresh
    /// content state, and — because editing after an undo forks history —
    /// the redo stack dropped. Dropping it *here* rather than in
    /// `begin_change` keeps a no-op insert session from destroying redo
    /// (#82 §12).
    fn touch(&mut self) {
        self.revision += 1;
        self.state = self.revision;
        self.redo.clear();
    }

    /// Writes atomically — a unique, exclusively created sibling temp file,
    /// then a rename over the target — and safely (#82 §2–4): the write goes
    /// *through* a symlink to its real target rather than replacing the link,
    /// the target's permission bits are preserved, and the temp name can't
    /// collide with (or be pre-planted as a symlink to) anything. Refuses
    /// when the file changed on disk since it was read or written (#80)
    /// unless `force` — an agent's or formatter's edits must never be
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
                self.touch();
            }
        }
        // the real file: an existing symlink resolves to its referent
        let target = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let perms = fs::metadata(&target).ok().map(|m| m.permissions());
        let dir = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let stem = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let (tmp, mut file) = open_unique_tmp(&dir, stem)?;
        let write = (|| -> Result<()> {
            for chunk in self.rope.chunks() {
                file.write_all(chunk.as_bytes())?;
            }
            file.sync_all()?;
            drop(file);
            if let Some(p) = perms {
                fs::set_permissions(&tmp, p)?;
            }
            fs::rename(&tmp, &target)?;
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
        self.touch();
    }

    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        if self.read_only || range.is_empty() {
            return;
        }
        self.rope.remove(range);
        self.touch();
    }

    /// Starts an undoable change transaction. Only the outermost call
    /// snapshots; nested calls (`cw` opening one that the insert session
    /// extends, a completion or LSP edit applied mid-insert) just deepen it
    /// and are closed by their own `end_change` without committing.
    pub fn begin_change(&mut self, cursor: Cursor) {
        if self.read_only {
            return;
        }
        self.change_depth += 1;
        if self.change_depth > 1 {
            return;
        }
        self.open_change = Some(self.state);
        self.undo.push(Snapshot {
            rope: self.rope.clone(),
            cursor,
            state: self.state,
        });
    }

    /// Ends one level of transaction. The outermost `end_change` returns
    /// whether the buffer actually changed (discarding the snapshot if not);
    /// an inner one returns false and leaves the transaction open.
    pub fn end_change(&mut self) -> bool {
        if self.change_depth == 0 {
            return false;
        }
        self.change_depth -= 1;
        if self.change_depth > 0 {
            return false;
        }
        match self.open_change.take() {
            Some(s0) if s0 == self.state => {
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
            state: self.state,
        });
        self.state = snap.state; // back to a state that existed before
        self.revision += 1; // …but every cache must still notice
        Some(snap.cursor)
    }

    pub fn redo(&mut self, cursor: Cursor) -> Option<Cursor> {
        let snap = self.redo.pop()?;
        self.undo.push(Snapshot {
            rope: std::mem::replace(&mut self.rope, snap.rope),
            cursor,
            state: self.state,
        });
        self.state = snap.state;
        self.revision += 1;
        Some(snap.cursor)
    }
}

/// Creates a temp file next to the target that no one else can have planted:
/// a name unique to this process and attempt, opened with `create_new`
/// (O_EXCL — fails on anything already there, and never follows a symlink).
fn open_unique_tmp(dir: &Path, stem: &str) -> Result<(PathBuf, fs::File)> {
    let pid = std::process::id();
    for attempt in 0..64u32 {
        let tmp = dir.join(format!(".{stem}.unei.{pid}.{attempt}.tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("cannot create {}", tmp.display()));
            }
        }
    }
    anyhow::bail!("cannot find a free temp name in {}", dir.display())
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
    fn nested_transactions_commit_as_one_undo_unit() {
        // an insert session (outer) with two completions (inner pairs) inside
        // — #82 §12b: the inner end_change must not close the session
        let mut b = Buffer::from_text("");
        b.begin_change(Cursor::default()); // insert session opens
        b.begin_change(Cursor::default()); // completion #1
        b.insert(0, "vec");
        assert!(!b.end_change(), "inner end must not commit");
        b.insert(3, " ");
        b.begin_change(Cursor::default()); // completion #2
        b.insert(4, "vec");
        assert!(!b.end_change());
        assert!(b.end_change(), "outer end commits the whole session");
        assert_eq!(b.rope.to_string(), "vec vec\n");
        b.undo(Cursor::default());
        assert_eq!(b.rope.to_string(), "\n", "one undo restores the original");
        assert!(b.undo(Cursor::default()).is_none());
    }

    #[test]
    fn saved_state_survives_undo_branching() {
        let mut b = Buffer::from_text("base");
        b.begin_change(Cursor::default());
        b.insert(4, "x");
        b.end_change();
        b.mark_saved(); // "basex" is on disk
        b.undo(Cursor::default()); // back to "base"
        assert!(b.is_modified(), "\"base\" was never saved");
        b.begin_change(Cursor::default());
        b.insert(4, "y");
        b.end_change();
        assert!(b.is_modified(), "\"basey\" ≠ saved \"basex\"");
        b.undo(Cursor::default());
        b.redo(Cursor::default());
        assert_eq!(b.rope.to_string(), "basey\n");
        assert!(b.is_modified());
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
