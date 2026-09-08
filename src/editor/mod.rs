pub mod analyzer;
mod buffer_list;
mod cmdline;
pub mod file_picker;
mod insert;
mod normal;
pub mod testing;
pub mod windows;

use ratatui::layout::Rect;

use crate::config::OPTIONS;
use crate::config::keymap::Key;
use crate::core::buffer::{Buffer, Cursor};
use crate::core::commands::{FindKind, Op, Register, WinDir};
use crate::core::text::{
    WrapRow, cell_at_col, first_non_blank, line_content, line_graphemes, line_len, max_normal_col,
    row_of_col, text_lines, wrap_rows,
};
use windows::{MIN_WIN_HEIGHT, MIN_WIN_WIDTH, Node, SplitDir, WinId, WinState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    /// `R` — overtype: typed characters replace the ones under the cursor
    /// so the line keeps its length (tab-aligned trailing comments stay put).
    Replace,
    Command,
    Visual(crate::core::commands::VisualKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Awaiting {
    #[default]
    None,
    Find(FindKind),
    Replace,
    G,
    Z,
    ZUpper,
    Leader,
    /// `Ctrl+w` / `<leader>w` pressed, second key pending.
    Window,
    /// `<leader>c…` — code chord.
    LeaderC,
    /// `<leader>r…` — rust chord.
    LeaderR,
    /// `<leader>d…` — diagnostics chord.
    LeaderD,
    /// `gc…` — comment chord (awaiting the second `c` of `gcc`).
    GComment,
    /// `m` pressed — awaiting the mark letter to set.
    SetMark,
    /// `` ` `` / `'` pressed — awaiting the mark letter to jump to.
    JumpMark {
        exact: bool,
    },
    /// `a`/`n` pressed as an operator target or in visual mode (#12) —
    /// awaiting the object key. `around` distinguishes `a…` from `n…`.
    TextObject {
        around: bool,
    },
}

#[derive(Debug, Default)]
pub struct Pending {
    pub count1: Option<usize>,
    pub op: Option<Op>,
    pub count2: Option<usize>,
    pub awaiting: Awaiting,
}

impl Pending {
    pub fn is_idle(&self) -> bool {
        self.count1.is_none()
            && self.op.is_none()
            && self.count2.is_none()
            && self.awaiting == Awaiting::None
    }

    /// Combined count: `2d3w` deletes six words.
    pub fn take_count(&mut self) -> Option<usize> {
        let (c1, c2) = (self.count1.take(), self.count2.take());
        if c1.is_none() && c2.is_none() {
            None
        } else {
            Some(c1.unwrap_or(1).saturating_mul(c2.unwrap_or(1)).max(1))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct View {
    pub width: usize,
    pub height: usize,
}

pub struct Message {
    pub text: String,
    pub error: bool,
}

/// Stable buffer number, shown in the buffer list (vim-style: numbers are
/// assigned at creation and never reused).
pub type BufId = usize;

/// A buffer that is not currently displayed, parked with its view state.
/// The displayed buffer lives checked out in `Editor::buffer` and its slot
/// holds `buffer: None`.
struct Slot {
    id: BufId,
    buffer: Option<Buffer>,
    cursor: Cursor,
    goal: Option<usize>,
    top_line: usize,
}

/// One row of the buffer list, for rendering and tests.
pub struct BufEntry {
    pub id: BufId,
    pub name: String,
    pub modified: bool,
    pub current: bool,
    pub alternate: bool,
}

/// State of the buffer-list overlay.
pub struct BufferList {
    pub selected: usize,
}

/// What the command line is currently for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    Command,
    Search { forward: bool },
}

/// The yanked region, briefly highlighted (nvim's on_yank flash).
#[derive(Clone, Copy)]
pub enum FlashRegion {
    /// Absolute char range.
    Char(usize, usize),
    /// Inclusive line range.
    Line(usize, usize),
    /// Lines l1..=l2, display cells left..=right.
    Block(usize, usize, usize, usize),
}

/// See `Editor::block_change`.
pub(crate) struct BlockChange {
    /// Line the insert session runs on.
    pub top_line: usize,
    /// The other lines of the block, receiving the replica.
    pub lines: Vec<usize>,
    /// Char column where the inserted text starts on every line.
    pub col: usize,
}

pub struct Editor {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub mode: Mode,
    pub goal: Option<usize>,
    pub pending: Pending,
    /// The yank register: written by y, read by p/P (and visual paste).
    pub yank_reg: Option<Register>,
    /// The cut register: written by deletes/changes, read by <leader>p/P.
    pub cut_reg: Option<Register>,
    /// Last text mirrored to the system clipboard (OSC 52) — test hook.
    pub last_clipboard: Option<String>,
    pub last_find: Option<(FindKind, char)>,
    pub cmdline: String,
    pub message: Option<Message>,
    pub top_line: usize,
    pub should_quit: bool,
    /// When the external-change poll last ran (#80); `None` = never.
    last_disk_check: Option<std::time::Instant>,
    /// Next buffer id to hand out. Ids are never reused (closing the highest
    /// buffer must not let its id — and any mark or cache keyed by it — be
    /// inherited by the next one, #82).
    next_buf_id: BufId,
    pub view: View,
    pub buffer_list: Option<BufferList>,
    pub file_picker: Option<file_picker::FilePicker>,
    pub syntax: crate::syntax::Syntax,
    /// rust-analyzer session (ticket #6), started lazily.
    pub(crate) lsp: Option<crate::lsp::Client>,
    lsp_broken: bool,
    /// Buffers with edits the server hasn't seen yet, and when they were
    /// last touched. Per buffer, not one global timer: an edit in A followed
    /// by a switch to B used to let B consume the debounce and leave A stale
    /// on the server for good (#82 §9).
    lsp_dirty: std::collections::HashMap<BufId, std::time::Instant>,
    /// Hover / annotated-line float (any key dismisses).
    pub info_float: Option<Vec<String>>,
    pub actions_menu: Option<analyzer::ActionsMenu>,
    /// Active completion popup (#22); lives only during an insert session.
    pub completion: Option<analyzer::CompletionMenu>,
    /// In-flight completion request context: (line, prefix-start char col).
    completion_req: Option<(usize, usize)>,
    /// Per-buffer marks (#14): (buffer, letter) → position. The `` ` ``
    /// letter holds the pre-jump position for the `` `` `` / `''` toggle.
    marks: std::collections::HashMap<(BufId, char), Cursor>,
    /// Replace-mode overtype history: per typed char, the original it
    /// covered (`Some`) or nothing when appending past line end (`None`),
    /// so Backspace can restore it.
    pub(crate) replace_stack: Vec<Option<char>>,
    /// End-of-line diagnostic ghost text toggle (<leader>dh).
    pub ghost_text: bool,
    diag_views: std::collections::HashMap<std::path::PathBuf, analyzer::DiagView>,
    /// Jump history (Ctrl+o / Ctrl+i), editor-global. `jump_index == len`
    /// means "at the live end".
    jumplist: Vec<(BufId, Cursor)>,
    jump_index: usize,
    /// The fixed end of the visual selection (cursor is the moving end).
    pub visual_anchor: Cursor,
    /// Last selection, for `gv` (kind, anchor, cursor).
    last_visual: Option<(crate::core::commands::VisualKind, Cursor, Cursor)>,
    /// Pending blockwise change: after `c` on a block, the insert session's
    /// text replicates to these lines at the given char column on Esc.
    block_change: Option<BlockChange>,
    /// Active yank flash: start time + region.
    pub yank_flash: Option<(std::time::Instant, FlashRegion)>,
    /// Buffer search state (pattern, matches, hlsearch).
    pub search: crate::search::Search,
    /// Windows currently projecting their buffer as a preview (#39).
    preview_windows: std::collections::HashSet<WinId>,
    /// Per preview window: (cursor line, top line) in RENDERED space.
    preview_nav: std::collections::HashMap<WinId, (usize, usize)>,
    /// Rendered documents, cached per buffer (version+width checked).
    preview_docs: std::collections::HashMap<BufId, crate::preview::PreviewDoc>,
    /// rust-analyzer's hints for a buffer, with the revision they describe
    /// (#93): only shown while the buffer is still at that revision.
    inlay_hints: std::collections::HashMap<BufId, (u64, Vec<crate::lsp::InlayHint>)>,
    /// Bumped whenever diagnostics change, so projections that print them
    /// re-render.
    diag_epoch: u64,
    /// What the command line is prompting for.
    pub prompt: Prompt,
    /// Position to restore when an incremental search is cancelled.
    pub(crate) search_origin: Option<(Cursor, usize)>,
    /// Search state to restore on cancel (pattern, direction, highlight).
    pub(crate) search_saved: Option<(String, crate::search::Direction, bool)>,
    /// Line range captured when `:` was pressed in visual mode (for `:s`).
    pub(crate) cmd_selection: Option<(usize, usize)>,
    /// Working folder: picker listings and relative opens resolve against it.
    root: std::path::PathBuf,
    /// All buffers in creation order; the current one is checked out into
    /// `buffer` and its slot holds `None`.
    slots: Vec<Slot>,
    current: BufId,
    /// The focused window's alternate buffer (parked windows keep theirs
    /// in their `WinState`).
    alternate: Option<BufId>,
    /// Split-window tree; the focused window's view state is checked out
    /// into the flat fields above.
    win_root: Node,
    focused_win: WinId,
    next_win_id: WinId,
    zoomed: bool,
    /// Screen region the window tree occupies (updated by the renderer).
    win_area: Rect,
    recording: Vec<Key>,
    last_change: Option<Vec<Key>>,
    replaying: bool,
    change_started: bool,
}

impl Editor {
    pub fn new(buffer: Buffer) -> Self {
        Self::with_buffers(vec![buffer])
    }

    /// Editor with one or more buffers; the first is displayed.
    pub fn with_buffers(buffers: Vec<Buffer>) -> Self {
        assert!(!buffers.is_empty());
        let mut buffers = buffers.into_iter();
        let first = buffers.next().unwrap();
        let mut slots = vec![Slot {
            id: 1,
            buffer: None,
            cursor: Cursor::default(),
            goal: None,
            top_line: 0,
        }];
        for (id, buffer) in (2..).zip(buffers) {
            slots.push(Slot {
                id,
                buffer: Some(buffer),
                cursor: Cursor::default(),
                goal: None,
                top_line: 0,
            });
        }
        Self {
            buffer: first,
            cursor: Cursor::default(),
            mode: Mode::Normal,
            goal: None,
            pending: Pending::default(),
            yank_reg: None,
            cut_reg: None,
            last_clipboard: None,
            last_find: None,
            cmdline: String::new(),
            message: None,
            top_line: 0,
            should_quit: false,
            last_disk_check: None,
            next_buf_id: slots.len() + 1, // ids seeded 1..=n above
            view: View {
                width: 80,
                height: 22,
            },
            buffer_list: None,
            file_picker: None,
            syntax: crate::syntax::Syntax::default(),
            lsp: None,
            lsp_broken: false,
            lsp_dirty: std::collections::HashMap::new(),
            info_float: None,
            actions_menu: None,
            completion: None,
            completion_req: None,
            marks: std::collections::HashMap::new(),
            replace_stack: Vec::new(),
            ghost_text: true,
            diag_views: std::collections::HashMap::new(),
            jumplist: Vec::new(),
            jump_index: 0,
            visual_anchor: Cursor::default(),
            last_visual: None,
            block_change: None,
            yank_flash: None,
            search: crate::search::Search::default(),
            preview_windows: std::collections::HashSet::new(),
            preview_nav: std::collections::HashMap::new(),
            preview_docs: std::collections::HashMap::new(),
            inlay_hints: std::collections::HashMap::new(),
            diag_epoch: 0,
            prompt: Prompt::Command,
            search_origin: None,
            search_saved: None,
            cmd_selection: None,
            root: std::path::PathBuf::from("."),
            slots,
            current: 1,
            alternate: None,
            win_root: Node::leaf(1, None),
            focused_win: 1,
            next_win_id: 2,
            zoomed: false,
            win_area: Rect::new(0, 0, 80, 23),
            recording: Vec::new(),
            last_change: None,
            replaying: false,
            change_started: false,
        }
    }

    pub fn buffer_count(&self) -> usize {
        self.slots.len()
    }

    pub fn current_buffer_id(&self) -> BufId {
        self.current
    }

    fn buffer_name(buffer: &Buffer) -> String {
        buffer
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[No Name]".to_string())
    }

    /// The buffer list in creation order, for rendering and tests.
    pub fn buffer_entries(&self) -> Vec<BufEntry> {
        self.slots
            .iter()
            .map(|slot| {
                let buffer = slot.buffer.as_ref().unwrap_or(&self.buffer);
                BufEntry {
                    id: slot.id,
                    name: Self::buffer_name(buffer),
                    modified: buffer.is_modified(),
                    current: slot.id == self.current,
                    alternate: Some(slot.id) == self.alternate,
                }
            })
            .collect()
    }

    fn slot_index(&self, id: BufId) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    /// Checks the current buffer back into its slot (remembering its view)
    /// and checks `id` out, loading that buffer's remembered view.
    fn checkout_buffer(&mut self, id: BufId) {
        if id == self.current {
            return;
        }
        let Some(to) = self.slot_index(id) else {
            return;
        };
        let incoming = self.slots[to].buffer.take().expect("parked buffer");
        let outgoing = std::mem::replace(&mut self.buffer, incoming);
        let cur = self.slot_index(self.current).expect("current slot exists");
        self.slots[cur] = Slot {
            id: self.current,
            buffer: Some(outgoing),
            cursor: self.cursor,
            goal: self.goal,
            top_line: self.top_line,
        };
        let slot = &self.slots[to];
        self.cursor = slot.cursor;
        self.goal = slot.goal;
        self.top_line = slot.top_line;
        self.current = id;
    }

    /// Remembers the current position on the jumplist (vim-style: recorded
    /// at the source of a jump, before it happens).
    /// `m{a-z}` — set a mark at the cursor in the current buffer.
    pub(crate) fn set_mark(&mut self, ch: char) {
        if ch.is_ascii_alphanumeric() {
            self.marks.insert((self.current, ch), self.cursor);
        }
    }

    /// `` `x `` (exact) / `'x` (first non-blank of the line) — jump to a
    /// mark, recording a jumplist entry. `` ` ``/`'` mean the pre-jump spot.
    pub(crate) fn jump_mark(&mut self, ch: char, exact: bool) {
        let key = if ch == '`' || ch == '\'' { '`' } else { ch };
        let Some(&target) = self.marks.get(&(self.current, key)) else {
            self.err("E20: Mark not set");
            return;
        };
        self.record_jump();
        // marks aren't adjusted as text is edited: the remembered line may no
        // longer exist, so clamp before indexing the rope (#82 §6)
        let line = target
            .line
            .min(text_lines(&self.buffer.rope).saturating_sub(1));
        self.cursor = if exact {
            Cursor::new(line, target.col)
        } else {
            Cursor::new(line, first_non_blank(&self.buffer.rope, line))
        };
        self.goal = None;
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    /// `gf` — open the file named under the cursor, resolved against the
    /// current file's directory, then the project root (first existing
    /// wins; a bare name also tries a `.rs` suffix).
    pub(crate) fn goto_file(&mut self) {
        let Some(token) = self.file_token_under_cursor() else {
            self.err("E446: No file name under the cursor");
            return;
        };
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        let tp = std::path::Path::new(&token);
        if tp.is_absolute() {
            candidates.push(tp.to_path_buf());
        }
        let cur_dir = self
            .buffer
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf);
        for base in cur_dir.iter().chain(std::iter::once(&self.root)) {
            candidates.push(base.join(&token));
            candidates.push(base.join(format!("{token}.rs")));
        }
        for c in candidates {
            if c.is_file() {
                let path = c.to_string_lossy().into_owned();
                self.open_path(&path, None);
                return;
            }
        }
        self.err(format!("E447: Can't find file \"{token}\""));
    }

    /// The path-like token under the cursor (letters, digits, and `/._-~`).
    fn file_token_under_cursor(&self) -> Option<String> {
        let chars: Vec<char> = line_content(&self.buffer.rope, self.cursor.line)
            .chars()
            .collect();
        let is_path =
            |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~');
        let col = self.cursor.col.min(chars.len().saturating_sub(1));
        if chars.is_empty() || !is_path(chars[col]) {
            return None;
        }
        let mut start = col;
        while start > 0 && is_path(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col + 1;
        while end < chars.len() && is_path(chars[end]) {
            end += 1;
        }
        let token: String = chars[start..end].iter().collect();
        (!token.is_empty()).then_some(token)
    }

    /// `:w {path}` — write the buffer to a path and bind it there (vim
    /// semantics), the way a bare-launch scratch buffer gets a home.
    /// `:w {path}` (`force` = `:w! {path}`). Like vim, refuses to replace a
    /// file that already exists unless forced (E13), and only keeps the new
    /// binding if the write succeeds (#82 §5).
    pub(crate) fn save_as(&mut self, path: &str, force: bool) {
        let path = path.trim();
        if path.is_empty() {
            self.err("E32: No file name");
            return;
        }
        let p = std::path::Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(path)
        };
        if self.buffer.path.as_deref() == Some(abs.as_path()) {
            self.save(force); // writing to our own file: a plain :w
            return;
        }
        if !force && abs.exists() {
            self.err("E13: File exists (add ! to override)");
            return;
        }
        let previous = self.buffer.binding();
        self.buffer.rebind(abs);
        self.syntax.invalidate(self.current); // extension may have changed
        if !self.save(force) {
            self.buffer.restore_binding(previous);
            self.syntax.invalidate(self.current);
        }
    }

    pub(crate) fn record_jump(&mut self) {
        // the pre-jump position is the `` ` `` mark (powers `` `` ``/`''`)
        self.marks.insert((self.current, '`'), self.cursor);
        let entry = (self.current, self.cursor);
        self.jumplist.truncate(self.jump_index);
        if self.jumplist.last() != Some(&entry) {
            self.jumplist.push(entry);
        }
        if self.jumplist.len() > 100 {
            self.jumplist.remove(0);
        }
        self.jump_index = self.jumplist.len();
    }

    fn goto_jump_entry(&mut self, index: usize) {
        let (buf, cursor) = self.jumplist[index];
        if self.slot_index(buf).is_none() {
            // buffer no longer exists; drop the stale entry
            self.jumplist.remove(index);
            if self.jump_index > index {
                self.jump_index -= 1;
            }
            return;
        }
        if buf != self.current {
            let old = self.current;
            self.checkout_buffer(buf);
            self.alternate = Some(old);
        }
        self.cursor = cursor;
        self.goal = None;
        self.clamp_cursor();
        self.refresh_focused_view();
    }

    /// `Ctrl+o` — walk back through the jumplist.
    pub(crate) fn jump_back(&mut self) {
        if self.jump_index == 0 {
            self.msg("at oldest jump");
            return;
        }
        if self.jump_index == self.jumplist.len() {
            // entering history: remember where we are so Ctrl+i returns
            let entry = (self.current, self.cursor);
            if self.jumplist.last() != Some(&entry) {
                self.jumplist.push(entry);
            }
        }
        self.jump_index -= 1;
        self.goto_jump_entry(self.jump_index);
    }

    /// `Ctrl+i` / `Tab` — walk forward again.
    pub(crate) fn jump_forward(&mut self) {
        if self.jump_index + 1 >= self.jumplist.len() {
            self.msg("at newest jump");
            return;
        }
        self.jump_index += 1;
        self.goto_jump_entry(self.jump_index);
    }

    /// `Ctrl+w Ctrl+w` — cycle focus through windows in layout order.
    pub(crate) fn focus_next_window(&mut self) {
        let ids = self.win_root.window_ids();
        if ids.len() < 2 {
            return;
        }
        let pos = ids.iter().position(|w| *w == self.focused_win).unwrap_or(0);
        self.focus_window(ids[(pos + 1) % ids.len()]);
    }

    /// Switches the focused window to buffer `id` (sets the alternate).
    pub(crate) fn switch_to(&mut self, id: BufId) {
        if id == self.current || self.slot_index(id).is_none() {
            return;
        }
        self.record_jump();
        let old = self.current;
        self.checkout_buffer(id);
        self.alternate = Some(old);
        self.clamp_cursor();
        self.refresh_focused_view();
        self.msg(Self::buffer_name(&self.buffer));
    }

    /// `Ctrl+^` — the previously displayed buffer.
    pub(crate) fn switch_alternate(&mut self) {
        match self.alternate {
            Some(id) if self.slot_index(id).is_some() => self.switch_to(id),
            _ => self.err("E23: No alternate file"),
        }
    }

    /// Closes a buffer (from the list or `:bd`). Closing the last buffer
    /// quits the editor; a modified buffer refuses without `force`.
    pub(crate) fn close_buffer(&mut self, id: BufId, force: bool) {
        let Some(idx) = self.slot_index(id) else {
            return;
        };
        let modified = if id == self.current {
            self.buffer.is_modified()
        } else {
            self.slots[idx]
                .buffer
                .as_ref()
                .expect("parked")
                .is_modified()
        };
        if modified && !force {
            let name = if id == self.current {
                Self::buffer_name(&self.buffer)
            } else {
                Self::buffer_name(self.slots[idx].buffer.as_ref().expect("parked"))
            };
            self.err(format!(
                "E89: No write since last change for buffer \"{name}\" (add ! to override)"
            ));
            return;
        }
        if self.slots.len() == 1 {
            self.should_quit = true;
            return;
        }
        let closing_path = if id == self.current {
            self.buffer.path.clone()
        } else {
            self.slots[idx].buffer.as_ref().and_then(|b| b.path.clone())
        };
        if let Some(p) = closing_path {
            self.lsp_did_close(&p);
        }
        self.lsp_forget_buffer(id);
        if id == self.current {
            let target = self
                .alternate
                .filter(|a| *a != id && self.slot_index(*a).is_some())
                .unwrap_or_else(|| {
                    let next = (idx + 1) % self.slots.len();
                    self.slots[next].id
                });
            self.switch_to(target);
        }
        let idx = self.slot_index(id).expect("still present");
        self.slots.remove(idx);
        self.syntax.invalidate(id);
        if self.alternate == Some(id) {
            self.alternate = None;
        }
        // parked windows showing the closed buffer move to their alternate
        // (falling back to the first remaining buffer), at that buffer's
        // remembered view
        let fallback = self.slots.first().map(|s| s.id).unwrap_or(self.current);
        let views: Vec<(BufId, Cursor, usize)> = self
            .slots
            .iter()
            .map(|s| (s.id, s.cursor, s.top_line))
            .collect();
        let mut states = Vec::new();
        self.win_root.states_mut(&mut states);
        for (_, state) in states {
            if state.alternate == Some(id) {
                state.alternate = None;
            }
            if state.buf_id == id {
                let new_buf = state
                    .alternate
                    .filter(|a| views.iter().any(|(b, ..)| b == a))
                    .unwrap_or(fallback);
                let (cursor, top) = views
                    .iter()
                    .find(|(b, ..)| *b == new_buf)
                    .map(|(_, c, t)| (*c, *t))
                    .unwrap_or_default();
                state.buf_id = new_buf;
                state.cursor = cursor;
                state.top_line = top;
                state.goal = None;
                state.alternate = None;
            }
        }
    }

    // ------------------------------------------------------------------
    // Windows
    // ------------------------------------------------------------------

    pub fn window_count(&self) -> usize {
        self.win_root.window_count()
    }

    pub fn focused_window_id(&self) -> WinId {
        self.focused_win
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed
    }

    /// Called by the renderer (and tests) with the region windows occupy.
    pub fn set_window_area(&mut self, area: Rect) {
        self.win_area = area;
        self.refresh_focused_view();
    }

    /// Points `view` at the focused window's actual text dimensions (from
    /// the current layout) and re-scrolls. Must run after anything that
    /// changes focus or geometry — scrolling against the previous window's
    /// dimensions clobbers the restored scroll position.
    pub(crate) fn refresh_focused_view(&mut self) {
        let Some((_, rect)) = self
            .window_rects()
            .into_iter()
            .find(|(id, _)| *id == self.focused_win)
        else {
            return;
        };
        let gutter = crate::config::gutter_width(text_lines(&self.buffer.rope));
        self.set_view(
            rect.width.saturating_sub(gutter) as usize,
            rect.height.saturating_sub(1) as usize,
        );
    }

    /// Screen rectangles of all windows (each includes its statusline row).
    /// While zoomed only the focused window is visible, full-area.
    pub fn window_rects(&self) -> Vec<(WinId, Rect)> {
        if self.zoomed {
            return vec![(self.focused_win, self.win_area)];
        }
        let mut out = Vec::new();
        self.win_root.layout(self.win_area, &mut out);
        out
    }

    /// Separator columns between vertical splits (empty while zoomed).
    pub fn window_separators(&self) -> Vec<Rect> {
        if self.zoomed {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.win_root.separators(self.win_area, &mut out);
        out
    }

    /// View state of a parked window (the focused one lives in the editor
    /// fields). Cloned, for rendering.
    pub fn parked_window(&self, id: WinId) -> Option<WinState> {
        fn find(node: &Node, id: WinId) -> Option<&Option<WinState>> {
            match node {
                Node::Leaf { id: nid, state } if *nid == id => Some(state),
                Node::Leaf { .. } => None,
                Node::Split { children, .. } => children.iter().find_map(|c| find(c, id)),
            }
        }
        find(&self.win_root, id).and_then(|s| s.clone())
    }

    /// The buffer shown by window `id` — the checked-out one or a parked slot.
    pub fn window_buffer(&self, id: WinId) -> &Buffer {
        let buf_id = if id == self.focused_win {
            self.current
        } else {
            self.parked_window(id)
                .map(|s| s.buf_id)
                .unwrap_or(self.current)
        };
        self.buffer_ref(buf_id)
    }

    pub(crate) fn buffer_mut_by_id(&mut self, buf_id: BufId) -> Option<&mut Buffer> {
        if buf_id == self.current {
            return Some(&mut self.buffer);
        }
        self.slots
            .iter_mut()
            .find(|s| s.id == buf_id)
            .and_then(|s| s.buffer.as_mut())
    }

    pub fn buffer_ref(&self, buf_id: BufId) -> &Buffer {
        if buf_id == self.current {
            return &self.buffer;
        }
        self.slot_index(buf_id)
            .and_then(|i| self.slots[i].buffer.as_ref())
            .unwrap_or(&self.buffer)
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn set_root(&mut self, root: std::path::PathBuf) {
        self.root = root;
    }

    /// Opens `rel` (relative to the working folder) in the focused window,
    /// optionally splitting first. Reuses an existing buffer for the same
    /// file; otherwise loads it (a missing file becomes a "[New File]"
    /// buffer that materializes on :w).
    pub(crate) fn open_path(&mut self, rel: &str, split: Option<SplitDir>) {
        self.record_jump();
        let abs = self.root.join(rel);
        // A `.prg` is a compiled 6502 image, not text — open it disassembled.
        if abs.extension().and_then(|e| e.to_str()) == Some("prg") {
            self.open_prg_disassembly(&abs, split);
            return;
        }
        // Any other binary opens as a read-only hex view, not garbage (#69).
        if crate::core::hex::file_looks_binary(&abs) {
            self.open_hex_view(&abs, split);
            return;
        }
        let canon = std::fs::canonicalize(&abs).unwrap_or_else(|_| abs.clone());
        let existing = self.slots.iter().find_map(|slot| {
            let buffer = slot.buffer.as_ref().unwrap_or(&self.buffer);
            let path = buffer.path.as_ref()?;
            let buf_canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            (buf_canon == canon).then_some(slot.id)
        });
        if let Some(dir) = split {
            self.split_window(dir, false);
        }
        match existing {
            Some(id) => self.switch_to(id),
            None => match Buffer::from_path(&abs) {
                Ok((buffer, existed)) => {
                    let id = self.alloc_buf_id();
                    self.slots.push(Slot {
                        id,
                        buffer: Some(buffer),
                        cursor: Cursor::default(),
                        goal: None,
                        top_line: 0,
                    });
                    self.switch_to(id);
                    if !existed {
                        self.msg(format!("\"{}\" [New File]", abs.display()));
                    }
                }
                Err(e) => self.err(format!("{e:#}")),
            },
        }
    }

    /// Opens a compiled `.prg` as its 65C02 disassembly (#65). The file is a
    /// 2-byte little-endian load address followed by the code image; the
    /// machine lens's opcode table decodes the image into a read-only ACME
    /// listing that highlights and answers `K` like hand-written source.
    fn open_prg_disassembly(&mut self, abs: &std::path::Path, split: Option<SplitDir>) {
        let buffer = match Buffer::from_prg(abs) {
            Ok(b) => b,
            Err(e) => {
                self.err(format!("{e:#}"));
                return;
            }
        };
        if let Some(dir) = split {
            self.split_window(dir, false);
        }
        let id = self.alloc_buf_id();
        let name = abs.file_name().map(|n| n.to_string_lossy().into_owned());
        self.slots.push(Slot {
            id,
            buffer: Some(buffer),
            cursor: Cursor::default(),
            goal: None,
            top_line: 0,
        });
        self.switch_to(id);
        if let Some(name) = name {
            self.msg(format!("disassembled {name}"));
        }
    }

    /// Opens a binary file as a read-only hex view (#69). Everything the
    /// editor can't render as text — ROMs, `.bin`, object files — becomes a
    /// hex dump you can scroll and search but not edit, so the file is safe.
    fn open_hex_view(&mut self, abs: &std::path::Path, split: Option<SplitDir>) {
        let buffer = match Buffer::from_hex(abs) {
            Ok(b) => b,
            Err(e) => {
                self.err(format!("{e:#}"));
                return;
            }
        };
        if let Some(dir) = split {
            self.split_window(dir, false);
        }
        let id = self.alloc_buf_id();
        let name = abs.file_name().map(|n| n.to_string_lossy().into_owned());
        self.slots.push(Slot {
            id,
            buffer: Some(buffer),
            cursor: Cursor::default(),
            goal: None,
            top_line: 0,
        });
        self.switch_to(id);
        if let Some(name) = name {
            self.msg(format!("{name}: binary — read-only hex view"));
        }
    }

    /// A never-before-used buffer id.
    fn alloc_buf_id(&mut self) -> BufId {
        let id = self.next_buf_id;
        self.next_buf_id += 1;
        id
    }

    /// Lazily refreshes syntax highlights for a buffer (render prefetch).
    pub fn ensure_syntax(&mut self, buf_id: BufId) -> bool {
        let buffer = if buf_id == self.current {
            &self.buffer
        } else {
            match self
                .slots
                .iter()
                .find(|s| s.id == buf_id)
                .and_then(|s| s.buffer.as_ref())
            {
                Some(b) => b,
                None => return false,
            }
        };
        self.syntax.ensure(buf_id, buffer)
    }

    fn live_win_state(&self) -> WinState {
        WinState {
            buf_id: self.current,
            cursor: self.cursor,
            goal: self.goal,
            top_line: self.top_line,
            alternate: self.alternate,
        }
    }

    /// Moves focus to window `id`, checking view state (and, when needed,
    /// the buffer) in and out.
    pub(crate) fn focus_window(&mut self, id: WinId) {
        if id == self.focused_win || !self.win_root.contains(id) {
            return;
        }
        self.zoomed = false;
        let parked = self.live_win_state();
        if let Some(slot) = self.win_root.state_mut(self.focused_win) {
            *slot = Some(parked);
        }
        let state = self
            .win_root
            .state_mut(id)
            .and_then(Option::take)
            .expect("parked window state");
        if state.buf_id != self.current {
            self.checkout_buffer(state.buf_id);
        }
        self.cursor = state.cursor;
        self.goal = state.goal;
        self.top_line = state.top_line;
        self.alternate = state.alternate;
        self.focused_win = id;
        self.clamp_cursor();
        self.refresh_focused_view();
    }

    pub(crate) fn focus_direction(&mut self, dir: WinDir) {
        let was_zoomed = std::mem::replace(&mut self.zoomed, false);
        let rects = self.window_rects();
        let pref = match dir {
            WinDir::Left | WinDir::Right => {
                // cursor's screen row within the focused window
                let top = rects
                    .iter()
                    .find(|(id, _)| *id == self.focused_win)
                    .map(|(_, r)| r.y)
                    .unwrap_or(0);
                top + self.cursor_display_pos().0 as u16
            }
            WinDir::Up | WinDir::Down => {
                let left = rects
                    .iter()
                    .find(|(id, _)| *id == self.focused_win)
                    .map(|(_, r)| r.x)
                    .unwrap_or(0);
                left + self.cursor_display_pos().1 as u16
            }
        };
        match windows::neighbor(&rects, self.focused_win, dir, pref) {
            Some(id) => self.focus_window(id),
            None => self.zoomed = was_zoomed,
        }
    }

    /// `Ctrl+w s/v/n` — splits the focused window. `with_new_buffer` creates
    /// an empty buffer for the new window (vim `Ctrl+w n`).
    pub(crate) fn split_window(&mut self, dir: SplitDir, with_new_buffer: bool) {
        if let Some(id) = self.split_window_into(dir, with_new_buffer) {
            self.focus_window(id);
        }
    }

    /// Splits the focused window and returns the new one *without* moving
    /// focus (`Ctrl+w V`/`X`): it shows the same buffer at the same
    /// position, parked.
    pub(crate) fn split_window_into(
        &mut self,
        dir: SplitDir,
        with_new_buffer: bool,
    ) -> Option<WinId> {
        self.zoomed = false;
        let rect = self
            .window_rects()
            .into_iter()
            .find(|(id, _)| *id == self.focused_win)
            .map(|(_, r)| r)
            .unwrap_or(self.win_area);
        let fits = match dir {
            SplitDir::Horizontal => rect.height >= 2 * MIN_WIN_HEIGHT,
            SplitDir::Vertical => rect.width > 2 * MIN_WIN_WIDTH,
        };
        if !fits {
            self.err("E36: Not enough room");
            return None;
        }
        let state = if with_new_buffer {
            let buf_id = self.alloc_buf_id();
            self.slots.push(Slot {
                id: buf_id,
                buffer: Some(Buffer::from_text("")),
                cursor: Cursor::default(),
                goal: None,
                top_line: 0,
            });
            WinState {
                buf_id,
                ..Default::default()
            }
        } else {
            // same buffer, same view — vim keeps the position in both windows
            let mut s = self.live_win_state();
            s.alternate = None;
            s
        };
        let id = self.next_win_id;
        self.next_win_id += 1;
        self.win_root
            .split(self.focused_win, dir, Node::leaf(id, Some(state)));
        Some(id)
    }

    /// Text area of a window: (width, rows) as the renderer will lay it out.
    fn window_text_size(&self, id: WinId) -> (usize, usize) {
        self.window_rects()
            .into_iter()
            .find(|(w, _)| *w == id)
            .map(|(_, r)| {
                (
                    (r.width as usize).saturating_sub(2),
                    (r.height as usize).saturating_sub(1).max(1),
                )
            })
            .unwrap_or((self.view.width.saturating_sub(2), self.view.height.max(1)))
    }

    /// `Ctrl+w q` — closes the focused window; the last one quits the editor.
    pub(crate) fn close_window(&mut self, force: bool) {
        self.zoomed = false;
        if self.window_count() == 1 {
            self.quit(force);
            return;
        }
        let closing = self.focused_win;
        let ids = self.win_root.window_ids();
        let pos = ids.iter().position(|w| *w == closing).unwrap_or(0);
        let target = if pos + 1 < ids.len() {
            ids[pos + 1]
        } else {
            ids[pos - 1]
        };
        self.focus_window(target);
        self.win_root.close(closing);
        self.preview_windows.remove(&closing);
        self.preview_nav.remove(&closing);
        self.refresh_focused_view();
    }

    /// `Ctrl+w o` — the focused window becomes the only one. Buffers shown
    /// elsewhere stay open (hidden), like vim with 'hidden' set.
    pub(crate) fn only_window(&mut self) {
        self.zoomed = false;
        for id in self.win_root.window_ids() {
            if id != self.focused_win {
                self.win_root.close(id);
            }
        }
        self.refresh_focused_view();
    }

    pub(crate) fn equalize_windows(&mut self) {
        self.zoomed = false;
        self.win_root.equalize();
        self.refresh_focused_view();
    }

    pub(crate) fn rotate_windows(&mut self) {
        self.zoomed = false;
        self.win_root.rotate(self.focused_win);
        self.refresh_focused_view();
    }

    pub(crate) fn flip_layout(&mut self) {
        self.zoomed = false;
        self.win_root.flip(self.focused_win);
        self.refresh_focused_view();
    }

    pub(crate) fn zoom_toggle(&mut self) {
        if self.window_count() > 1 {
            self.zoomed = !self.zoomed;
            self.refresh_focused_view();
        }
    }

    pub(crate) fn resize_window(&mut self, dir: WinDir) {
        self.zoomed = false;
        let step = match dir {
            WinDir::Left | WinDir::Right => OPTIONS.resize_step_cols,
            WinDir::Up | WinDir::Down => OPTIONS.resize_step_rows,
        };
        self.win_root
            .resize(self.focused_win, dir, step, self.win_area);
        self.refresh_focused_view();
    }

    /// `:q`-family behavior: with splits open, closes the window; the last
    /// window quits the editor (with the modified-buffer checks).
    pub(crate) fn close_window_or_quit(&mut self, force: bool) {
        if self.window_count() > 1 {
            self.close_window(force);
        } else {
            self.quit(force);
        }
    }

    /// First modified buffer anywhere (for quit commands), by name.
    fn any_modified_buffer(&self) -> Option<String> {
        self.slots.iter().find_map(|slot| {
            let buffer = slot.buffer.as_ref().unwrap_or(&self.buffer);
            buffer.is_modified().then(|| Self::buffer_name(buffer))
        })
    }

    pub fn handle_key(&mut self, key: Key) {
        if self.mode != Mode::Command {
            self.message = None;
        }
        // the info float closes on any key; Esc stops there, others act
        if self.info_float.is_some() {
            self.info_float = None;
            if matches!(key, Key::Esc | Key::Char('q') | Key::Char('K')) {
                return;
            }
        }
        if self.actions_menu.is_some() {
            analyzer_menu_key(self, key);
            return;
        }
        if self.focused_is_preview()
            && self.mode == Mode::Normal
            && self.pending.awaiting == Awaiting::None
        {
            self.preview_key(key);
            self.clamp_cursor();
            return;
        }
        if self.file_picker.is_some() {
            file_picker::handle_key(self, key);
            self.clamp_cursor();
            self.scroll_to_cursor();
            return;
        }
        if self.buffer_list.is_some() {
            buffer_list::handle_key(self, key);
            self.clamp_cursor();
            self.scroll_to_cursor();
            return;
        }
        let record = !self.replaying && self.mode != Mode::Command;
        if record {
            if self.recording.is_empty() {
                self.change_started = false;
            }
            self.recording.push(key);
        }

        let version_before = self.buffer.version();
        match self.mode {
            Mode::Normal | Mode::Visual(_) => normal::handle_key(self, key),
            Mode::Insert | Mode::Replace => insert::handle_key(self, key),
            Mode::Command => cmdline::handle_key(self, key),
        }
        if self.buffer.version() != version_before {
            self.lsp_note_edit();
        }

        // A completed command that changed the buffer becomes the dot-repeat.
        if !self.replaying
            && self.mode == Mode::Normal
            && self.pending.is_idle()
            && !self.recording.is_empty()
        {
            if self.change_started {
                self.last_change = Some(std::mem::take(&mut self.recording));
            } else {
                self.recording.clear();
            }
            self.change_started = false;
        }

        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    pub(crate) fn repeat_last_change(&mut self, count: Option<usize>) {
        // drop the '.' itself from the recording so it never becomes
        // the next last-change
        self.recording.clear();
        let Some(mut keys) = self.last_change.clone() else {
            return;
        };
        // a count on '.' replaces the leading count of the recorded change
        // (vim keeps any count typed after an operator)
        if let Some(n) = count {
            let lead = if matches!(keys.first(), Some(Key::Char('1'..='9'))) {
                keys.iter()
                    .take_while(|k| matches!(k, Key::Char('0'..='9')))
                    .count()
            } else {
                0
            };
            let mut with_count: Vec<Key> = n.to_string().chars().map(Key::Char).collect();
            with_count.extend_from_slice(&keys[lead..]);
            keys = with_count;
        }
        self.replaying = true;
        for key in keys {
            self.handle_key(key);
        }
        self.replaying = false;
        self.recording.clear();
        self.change_started = false;
    }

    pub(crate) fn drop_recording(&mut self) {
        self.recording.clear();
        self.change_started = false;
    }

    pub(crate) fn note_change_committed(&mut self, committed: bool) {
        if committed {
            self.change_started = true;
        }
    }

    pub fn msg(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
        });
    }

    pub fn err(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
        });
    }

    pub fn set_view(&mut self, width: usize, height: usize) {
        self.view = View {
            width: width.max(1),
            height: height.max(1),
        };
        self.scroll_to_cursor();
    }

    /// Keeps the cursor on a valid position for the current mode.
    pub fn clamp_cursor(&mut self) {
        let last = text_lines(&self.buffer.rope) - 1;
        if self.cursor.line > last {
            self.cursor.line = last;
        }
        let limit = match self.mode {
            Mode::Insert | Mode::Replace => line_len(&self.buffer.rope, self.cursor.line),
            _ => max_normal_col(&self.buffer.rope, self.cursor.line, OPTIONS.tabstop),
        };
        if self.cursor.col > limit {
            self.cursor.col = limit;
        }
    }

    /// Display rows of a buffer line at the focused window's text width —
    /// soft wrap at word boundaries (#9).
    fn wrap_rows_of(&self, line: usize) -> Vec<WrapRow> {
        let grs = line_graphemes(&line_content(&self.buffer.rope, line), OPTIONS.tabstop);
        wrap_rows(&grs, self.view.width)
    }

    /// How many display rows `line` takes — exact, and allocation-free for
    /// the common case (#89). No grapheme is wider than its byte length
    /// except a tab: `line_graphemes` gives every cluster `width().max(1)`,
    /// a 1-byte cluster is ASCII, and nothing below U+1100 is East Asian
    /// wide, so bytes plus the tabs' extra reach bound the cells. A line
    /// under that bound is one row without segmenting it; only lines over
    /// it go through the wrapper. This runs for every line between the
    /// window top and the cursor on each keystroke.
    fn rows_of_line(&self, line: usize) -> usize {
        let slice = self.buffer.rope.line(line);
        let mut bytes = slice.len_bytes();
        if bytes > 0 && slice.char(slice.len_chars() - 1) == '\n' {
            bytes -= 1;
        }
        if bytes <= self.view.width {
            let tabs: usize = slice
                .chunks()
                .map(|c| c.bytes().filter(|&b| b == b'\t').count())
                .sum();
            if bytes + tabs * (OPTIONS.tabstop - 1) <= self.view.width {
                return 1;
            }
        }
        self.wrap_rows_of(line).len()
    }

    /// Display rows occupied by the lines `from..to`.
    fn rows_between(&self, from: usize, to: usize) -> usize {
        (from..to).map(|l| self.rows_of_line(l)).sum()
    }

    /// The cursor's row index within its line, and that row.
    fn cursor_row(&self) -> (usize, WrapRow) {
        let rows = self.wrap_rows_of(self.cursor.line);
        let i = row_of_col(&rows, self.cursor.col);
        (i, rows[i])
    }

    /// The largest top line that leaves at least `rows_above` display rows
    /// between the window's top and the cursor's row — line 0 if the buffer
    /// runs out first. Walks back from the cursor, so it costs a window's
    /// worth of lines, never the whole buffer.
    pub(crate) fn top_line_with_rows_above(&self, rows_above: usize) -> usize {
        let (row_in_line, _) = self.cursor_row();
        let mut acc = row_in_line;
        let mut l = self.cursor.line;
        while l > 0 && acc < rows_above {
            l -= 1;
            acc += self.rows_of_line(l);
        }
        l
    }

    /// The smallest top line whose rows still *fit* at most `limit` display
    /// rows above the cursor's row — as much context above as the window
    /// allows without pushing the cursor off the bottom. Tops are whole
    /// lines, so a wrapped line just above may leave the window short.
    pub(crate) fn top_line_fitting_rows_above(&self, limit: usize) -> usize {
        let (row_in_line, _) = self.cursor_row();
        let mut acc = row_in_line;
        let mut l = self.cursor.line;
        while l > 0 {
            let above = self.rows_of_line(l - 1);
            if acc + above > limit {
                break;
            }
            l -= 1;
            acc += above;
        }
        l
    }

    /// Display rows below the cursor's row, counting at most `limit`.
    fn rows_below_cursor(&self, limit: usize) -> usize {
        let last = text_lines(&self.buffer.rope) - 1;
        let rows = self.wrap_rows_of(self.cursor.line);
        let mut acc = rows.len() - 1 - row_of_col(&rows, self.cursor.col);
        let mut l = self.cursor.line;
        while acc < limit && l < last {
            l += 1;
            acc += self.rows_of_line(l);
        }
        acc.min(limit)
    }

    /// The cursor's screen position inside the focused window's text area:
    /// (display row below the top line, cell within that row).
    pub fn cursor_display_pos(&self) -> (usize, usize) {
        let (row_in_line, row) = self.cursor_row();
        let y = if self.cursor.line < self.top_line {
            0
        } else {
            self.rows_between(self.top_line, self.cursor.line) + row_in_line
        };
        let grs = line_graphemes(
            &line_content(&self.buffer.rope, self.cursor.line),
            OPTIONS.tabstop,
        );
        let cell = cell_at_col(&grs, self.cursor.col);
        (y, cell.saturating_sub(row.start_cell))
    }

    /// Keeps the cursor's display row inside the window with `scrolloff` rows
    /// of margin, counting soft-wrapped rows (#9). Soft wrap replaced
    /// horizontal scrolling, so there is no horizontal component.
    pub fn scroll_to_cursor(&mut self) {
        let h = self.view.height.max(1);
        let last = text_lines(&self.buffer.rope) - 1;
        let so = OPTIONS.scrolloff.min(h.saturating_sub(1) / 2);
        if self.cursor.line < self.top_line {
            self.top_line = self.cursor.line;
        }
        let (row_in_line, _) = self.cursor_row();
        // a cursor a whole window's worth of lines below the top is off the
        // bottom regardless of wrapping — skip counting every row in between
        let cur_row = |ed: &Self| {
            if ed.cursor.line >= ed.top_line + h {
                usize::MAX / 2
            } else {
                ed.rows_between(ed.top_line, ed.cursor.line) + row_in_line
            }
        };
        if cur_row(self) < so {
            self.top_line = self.top_line_with_rows_above(so);
        }
        // near the end of the buffer the margin shrinks so the last line can
        // sit at the bottom, as in vim
        let bottom_so = self.rows_below_cursor(so);
        if cur_row(self) + bottom_so >= h {
            self.top_line = self.top_line_fitting_rows_above(h - 1 - bottom_so);
        }
        self.top_line = self.top_line.min(last);
    }

    /// Vertical cursor move that clamps at the edges (used by scrolling).
    pub(crate) fn move_vertical(&mut self, delta: isize) {
        let last = text_lines(&self.buffer.rope) - 1;
        let line = if delta < 0 {
            self.cursor.line.saturating_sub(delta.unsigned_abs())
        } else {
            (self.cursor.line + delta as usize).min(last)
        };
        let grs = line_graphemes(
            &line_content(&self.buffer.rope, self.cursor.line),
            OPTIONS.tabstop,
        );
        let goal = self
            .goal
            .unwrap_or_else(|| cell_at_col(&grs, self.cursor.col));
        let tgrs = line_graphemes(&line_content(&self.buffer.rope, line), OPTIONS.tabstop);
        let col = if goal == usize::MAX {
            tgrs.last().map(|g| g.char_off).unwrap_or(0)
        } else {
            crate::core::text::col_at_cell(&tgrs, goal)
        };
        self.cursor = Cursor::new(line, col);
        self.goal = Some(goal);
    }

    pub(crate) fn quit(&mut self, force: bool) {
        if force {
            self.should_quit = true;
            return;
        }
        if self.buffer.is_modified() {
            self.err("E37: No write since last change (add ! to override)");
        } else if let Some(name) = self.any_modified_buffer() {
            self.err(format!(
                "E162: No write since last change for buffer \"{name}\""
            ));
        } else {
            self.should_quit = true;
        }
    }

    /// `:w` (`force` = `:w!`, which overwrites a file that changed on disk).
    pub(crate) fn save(&mut self, force: bool) -> bool {
        let (path, lines) = match self.buffer.save(force) {
            Ok(ok) => ok,
            Err(e) => {
                self.err(format!("E212: {e:#}"));
                return false;
            }
        };
        self.lsp_did_save(&path);
        match crate::format::format_file(&path) {
            crate::format::Outcome::NoConfig | crate::format::Outcome::Unchanged => {
                self.msg(format!("\"{}\" {}L written", path.display(), lines));
            }
            crate::format::Outcome::Reformatted { text } => {
                self.reload_current_buffer(&text);
                self.msg(format!(
                    "\"{}\" {}L written, formatted",
                    path.display(),
                    lines
                ));
            }
            crate::format::Outcome::Failed(e) => {
                self.err(format!("\"{}\" written; {e}", path.display()));
            }
        }
        true
    }

    /// Replaces the current buffer's content with externally formatted text,
    /// as a single undoable change; the buffer stays marked clean.
    fn reload_current_buffer(&mut self, text: &str) {
        self.buffer.begin_change(self.cursor);
        let len = self.buffer.rope.len_chars();
        self.buffer.remove(0..len);
        self.buffer.insert(0, text);
        if self.buffer.rope.len_chars() == 0
            || self.buffer.rope.char(self.buffer.rope.len_chars() - 1) != '\n'
        {
            let at = self.buffer.rope.len_chars();
            self.buffer.insert(at, "\n");
        }
        let committed = self.buffer.end_change();
        self.note_change_committed(committed);
        self.buffer.mark_saved();
        self.lsp_note_edit(); // a reload changes the text the server holds too
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    /// Reloads the current buffer from disk as one undoable change (`:e`;
    /// `force` = `:e!`, which discards unsaved edits). Clears any
    /// changed-on-disk conflict.
    pub(crate) fn reload_from_disk(&mut self, force: bool) {
        if self.buffer.path.is_none() {
            self.err("E32: No file name");
            return;
        }
        if !force && self.buffer.is_modified() {
            self.err("E37: unsaved changes — :e! discards them and reloads");
            return;
        }
        match self.buffer.read_disk() {
            Ok(text) => {
                self.reload_current_buffer(&text);
                self.msg("reloaded from disk");
            }
            Err(e) => self.err(format!("{e:#}")),
        }
    }

    /// The external-change poll (#80), throttled to about once a second:
    /// when the current file changed on disk, a clean buffer reloads itself
    /// (an agent's or formatter's edit simply appears), a dirty one is
    /// flagged and warned — never silently overwritten. Returns whether a
    /// redraw is needed.
    pub fn disk_tick(&mut self) -> bool {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(1);
        if self.last_disk_check.is_some_and(|t| t.elapsed() < EVERY) {
            return false;
        }
        self.last_disk_check = Some(std::time::Instant::now());
        self.check_disk()
    }

    /// The unthrottled body of [`disk_tick`].
    pub fn check_disk(&mut self) -> bool {
        if !self.buffer.disk_changed() {
            return false;
        }
        if !self.buffer.is_modified() && self.mode == Mode::Normal {
            match self.buffer.read_disk() {
                Ok(text) => {
                    self.reload_current_buffer(&text);
                    self.msg("reloaded: file changed on disk");
                }
                Err(e) => self.err(format!("{e:#}")),
            }
            return true;
        }
        if !self.buffer.disk_conflict {
            self.buffer.disk_conflict = true;
            self.err(
                "WARNING: file changed on disk — :e! reloads (drops your edits), :w! overwrites",
            );
            return true;
        }
        false
    }

    pub(crate) fn save_and_quit(&mut self, only_if_modified: bool, force: bool) {
        if (!only_if_modified || self.buffer.is_modified()) && !self.save(force) {
            return;
        }
        if self.window_count() > 1 {
            self.close_window(false);
            return;
        }
        if let Some(name) = self.any_modified_buffer() {
            self.err(format!(
                "E162: No write since last change for buffer \"{name}\""
            ));
            return;
        }
        self.should_quit = true;
    }

    pub(crate) fn goto_line(&mut self, line: usize) {
        self.record_jump();
        let last = text_lines(&self.buffer.rope) - 1;
        self.cursor.line = line.min(last);
        self.cursor.col = first_non_blank(&self.buffer.rope, self.cursor.line);
        self.goal = None;
    }

    /// Applies a chosen code action (or asks the server to resolve it).
    fn run_selected_action(&mut self) {
        let Some(menu) = self.actions_menu.take() else {
            return;
        };
        let Some(action) = menu.actions.into_iter().nth(menu.selected) else {
            return;
        };
        let edit = action.raw.get("edit").cloned();
        match edit {
            Some(e) if e.is_object() => self.apply_workspace_edit(&e),
            _ => {
                if let Some(client) = &mut self.lsp {
                    client.resolve_action(action.raw);
                }
            }
        }
    }

    pub(crate) fn enter_visual(&mut self, kind: crate::core::commands::VisualKind) {
        match self.mode {
            Mode::Visual(k) if k == kind => self.leave_visual(),
            Mode::Visual(_) => self.mode = Mode::Visual(kind),
            _ => {
                self.visual_anchor = self.cursor;
                self.mode = Mode::Visual(kind);
            }
        }
    }

    pub(crate) fn leave_visual(&mut self) {
        if let Mode::Visual(kind) = self.mode {
            self.last_visual = Some((kind, self.visual_anchor, self.cursor));
            self.mode = Mode::Normal;
        }
    }

    /// `gv` — reselect the last visual selection.
    pub(crate) fn reselect_visual(&mut self) {
        if let Some((kind, anchor, cursor)) = self.last_visual {
            self.visual_anchor = anchor;
            self.cursor = cursor;
            self.mode = Mode::Visual(kind);
            self.clamp_cursor();
            self.scroll_to_cursor();
        }
    }

    /// Mirrors yanked text to the system clipboard via OSC 52 (kitty's
    /// clipboard_control allows writes). Yanks only — deletes never touch
    /// the system clipboard.
    pub(crate) fn mirror_yank_to_clipboard(&mut self, text: &str) {
        const MAX: usize = 5 * 1024 * 1024;
        if text.is_empty() || text.len() > MAX {
            return;
        }
        self.last_clipboard = Some(text.to_string());
        use std::io::{IsTerminal, Write};
        let mut out = std::io::stdout();
        if out.is_terminal() {
            let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
            let _ = out.flush();
        }
    }

    pub(crate) fn start_yank_flash(&mut self, region: FlashRegion) {
        if crate::config::OPTIONS.yank_flash_ms > 0 {
            self.yank_flash = Some((std::time::Instant::now(), region));
        }
    }

    /// Main-loop pump: true while the flash needs redraws (and once more
    /// when it expires).
    pub fn yank_flash_active(&mut self) -> bool {
        match self.yank_flash {
            Some((t0, _)) => {
                if t0.elapsed().as_millis() as u64 > crate::config::OPTIONS.yank_flash_ms {
                    self.yank_flash = None;
                }
                true
            }
            None => false,
        }
    }

    pub(crate) fn set_block_change(&mut self, change: BlockChange) {
        self.block_change = Some(change);
    }

    /// On leaving a block-change insert session: replicate the text typed on
    /// the top line to every other line of the block, at the block column.
    pub(crate) fn finish_block_change(&mut self) {
        let Some(change) = self.block_change.take() else {
            return;
        };
        // the replica is what now sits between the block column and the
        // cursor on the top line; a multi-line insert bails, like vim
        if self.cursor.line != change.top_line || self.cursor.col < change.col {
            return;
        }
        let content = line_content(&self.buffer.rope, change.top_line);
        let typed: String = content
            .chars()
            .skip(change.col)
            .take(self.cursor.col - change.col)
            .collect();
        if typed.is_empty() {
            return;
        }
        for line in change.lines {
            if line >= text_lines(&self.buffer.rope) {
                continue;
            }
            let len = line_len(&self.buffer.rope, line);
            if len < change.col {
                continue; // line never reached the block column
            }
            let at = self.buffer.rope.line_to_char(line) + change.col;
            self.buffer.insert(at, &typed);
        }
    }

    /// Render prefetch: keep the match cache fresh for the current buffer.
    pub fn search_ensure_current(&mut self) {
        if self.search.is_active() {
            self.search
                .ensure(&self.buffer.rope, self.current, self.buffer.version());
        }
    }

    /// `n` / `N` — repeat the search in (or against) its direction.
    pub(crate) fn search_step(&mut self, same_direction: bool) {
        if !self.search.is_active() {
            self.err("E35: No previous search");
            return;
        }
        self.search
            .ensure(&self.buffer.rope, self.current, self.buffer.version());
        self.search.highlight = true;
        let forward =
            (self.search.direction == crate::search::Direction::Forward) == same_direction;
        let found = if forward {
            self.search
                .next_after(self.cursor.line, self.cursor.col as u32)
        } else {
            self.search
                .prev_before(self.cursor.line, self.cursor.col as u32)
        };
        match found {
            Some(((line, col, _), wrapped)) => {
                self.cursor = Cursor::new(line, col as usize);
                self.goal = None;
                if wrapped {
                    self.msg(if forward {
                        "search hit BOTTOM, continuing at TOP"
                    } else {
                        "search hit TOP, continuing at BOTTOM"
                    });
                }
                self.clamp_cursor();
                self.scroll_to_cursor();
            }
            None => self.err(format!("E486: Pattern not found: {}", self.search.pattern)),
        }
    }

    /// `*` — whole-word search for the word under the cursor.
    pub(crate) fn search_word_under_cursor(&mut self) {
        use crate::core::text::{CharClass, char_class};
        let content = line_content(&self.buffer.rope, self.cursor.line);
        let chars: Vec<char> = content.chars().collect();
        let mut start = self.cursor.col.min(chars.len().saturating_sub(1));
        // sit on a non-word char: scan forward to the next word on the line
        while start < chars.len() && char_class(chars[start], false) != CharClass::Word {
            start += 1;
        }
        if start >= chars.len() {
            self.err("E348: No word under cursor");
            return;
        }
        let mut s = start;
        while s > 0 && char_class(chars[s - 1], false) == CharClass::Word {
            s -= 1;
        }
        let mut e = start;
        while e < chars.len() && char_class(chars[e], false) == CharClass::Word {
            e += 1;
        }
        let word: String = chars[s..e].iter().collect();
        let pattern = format!(r"\b{}\b", regex::escape(&word));
        self.record_jump();
        self.search
            .set_pattern(&pattern, crate::search::Direction::Forward);
        self.search
            .ensure(&self.buffer.rope, self.current, self.buffer.version());
        // jump to the next occurrence after the current word
        if let Some(((line, col, _), _)) = self
            .search
            .next_after(self.cursor.line, e.saturating_sub(1) as u32)
        {
            self.cursor = Cursor::new(line, col as usize);
            self.goal = None;
            self.clamp_cursor();
            self.scroll_to_cursor();
        }
        self.msg(format!("/{pattern}"));
    }

    /// `:s/pat/rep/[g]` — file scope by default, selection scope when the
    /// prompt was opened from visual mode. Never line ranges (per #8).
    pub(crate) fn substitute(&mut self, spec: &str) {
        let Some((pattern, replacement, global)) = parse_substitute(spec) else {
            self.err("E486: Malformed :s — expected :s/pattern/replacement/[g]");
            return;
        };
        let Some(re) = crate::search::compile(&pattern) else {
            self.err(format!("E383: Invalid pattern: {pattern}"));
            return;
        };
        let (l1, l2) = self
            .cmd_selection
            .take()
            .unwrap_or((0, text_lines(&self.buffer.rope) - 1));
        let mut replaced = 0usize;
        let mut lines_hit = 0usize;
        self.buffer.begin_change(self.cursor);
        for line in l1..=l2.min(text_lines(&self.buffer.rope) - 1) {
            let content = line_content(&self.buffer.rope, line);
            let new = if global {
                let n = re.find_iter(&content).count();
                if n == 0 {
                    continue;
                }
                replaced += n;
                re.replace_all(&content, replacement.as_str()).into_owned()
            } else {
                if re.find(&content).is_none() {
                    continue;
                }
                replaced += 1;
                re.replace(&content, replacement.as_str()).into_owned()
            };
            lines_hit += 1;
            let base = self.buffer.rope.line_to_char(line);
            let len = line_len(&self.buffer.rope, line);
            self.buffer.remove(base..base + len);
            self.buffer.insert(base, &new);
        }
        let committed = self.buffer.end_change();
        self.note_change_committed(committed);
        if replaced == 0 {
            self.err(format!("E486: Pattern not found: {pattern}"));
        } else {
            // hlsearch follows the substitution pattern, vim-style
            self.search
                .set_pattern(&pattern, crate::search::Direction::Forward);
            self.msg(format!(
                "{replaced} substitution{} on {lines_hit} line{}",
                if replaced == 1 { "" } else { "s" },
                if lines_hit == 1 { "" } else { "s" }
            ));
        }
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    // ------------------------------------------------------------------
    // Preview (#39): per-window projection + PREVIEW keyboard on focus
    // ------------------------------------------------------------------

    pub fn is_preview_window(&self, id: WinId) -> bool {
        self.preview_windows.contains(&id)
    }

    pub fn focused_is_preview(&self) -> bool {
        self.is_preview_window(self.focused_win)
    }

    /// `gp` — toggles the focused window between source and preview.
    pub(crate) fn toggle_preview(&mut self) {
        let id = self.focused_win;
        if self.preview_windows.remove(&id) {
            // back to source: land on the line the preview was reading
            if let Some((view_line, _)) = self.preview_nav.remove(&id)
                && let Some(doc) = self.preview_docs.get(&self.current)
            {
                let source = doc.source_line_for_view(view_line);
                self.cursor = Cursor::new(source, 0);
                self.goal = None;
                self.clamp_cursor();
                self.refresh_focused_view();
            }
            return;
        }
        if !self.announce_projection() {
            return;
        }
        self.project_window(id);
    }

    /// Whether the current buffer has a projection (`gp` and the preview
    /// splits share the gate); says why not otherwise.
    fn announce_projection(&mut self) -> bool {
        match crate::config::languages::detect(self.buffer.path.as_deref()) {
            Some("markdown") => true,
            Some("rust") => {
                // the compiler's-eye view (#93): annotations arrive from
                // rust-analyzer a round-trip later; say so when it's absent
                if self.lsp.is_none() {
                    self.msg("compiler's-eye view — no rust-analyzer here, source only");
                } else {
                    self.msg("compiler's-eye view");
                }
                true
            }
            _ => {
                self.msg("no preview for this file type (markdown and rust)");
                false
            }
        }
    }

    /// Turns window `id` (showing the current buffer) into its projection,
    /// reading line on the screen row the source cursor is on — the window
    /// changes what it shows, not where the eye is.
    fn project_window(&mut self, id: WinId) {
        self.preview_windows.insert(id);
        self.request_inlay_hints(self.current);
        let (width, rows) = self.window_text_size(id);
        let source_line = self.cursor.line;
        let row = self.cursor_display_pos().0.min(rows - 1);
        let view_line = self
            .preview_doc_for(self.current, width)
            .view_line_for_source(source_line);
        let top = view_line.saturating_sub(row);
        self.preview_nav.insert(id, (view_line, top));
    }

    /// `Ctrl+w Alt+v` / `Alt+x`: split, project the new window, stay — the
    /// side-by-side editing setup in one chord.
    pub(crate) fn preview_split(&mut self, dir: SplitDir) {
        if !self.announce_projection() {
            return;
        }
        if let Some(id) = self.split_window_into(dir, false) {
            self.project_window(id);
        }
    }

    /// The cached rendered document for a buffer, rebuilt when stale:
    /// markdown as the reader sees it, Rust as the compiler sees it (#93).
    pub fn preview_doc_for(&mut self, buf: BufId, width: usize) -> &crate::preview::PreviewDoc {
        let version = self.buffer_ref(buf).version();
        let rust =
            crate::config::languages::detect(self.buffer_ref(buf).path.as_deref()) == Some("rust");
        let (width, stamp) = if rust {
            (width.max(20), self.projection_stamp(buf))
        } else {
            (width.clamp(20, 100), 0) // prose width
        };
        let fresh = self
            .preview_docs
            .get(&buf)
            .is_some_and(|d| d.is_fresh(version, width, stamp));
        if !fresh {
            let doc = if rust {
                let hints = self.fresh_inlay_hints(buf);
                let diags = self.buffer_diagnostics(buf);
                crate::preview::rust::render(
                    &self.buffer_ref(buf).rope,
                    version,
                    width,
                    &hints,
                    &diags,
                    stamp,
                )
            } else {
                crate::preview::markdown::render(&self.buffer_ref(buf).rope, version, width)
            };
            self.preview_docs.insert(buf, doc);
        }
        self.preview_docs.get(&buf).expect("just inserted")
    }

    /// Buffers some window is currently projecting.
    pub(crate) fn projected_buffers(&self) -> Vec<BufId> {
        let mut out: Vec<BufId> = self
            .preview_windows
            .iter()
            .filter_map(|&id| {
                if id == self.focused_win {
                    Some(self.current)
                } else {
                    self.parked_window(id).map(|s| s.buf_id)
                }
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn preview_nav_state(&self, id: WinId) -> (usize, usize) {
        self.preview_nav.get(&id).copied().unwrap_or((0, 0))
    }

    /// Follow: previews of the focused source buffer track its cursor.
    /// Called by the renderer each frame.
    pub fn sync_preview_follow(&mut self) {
        if self.focused_is_preview() {
            return;
        }
        let src_line = self.cursor.line;
        let buf = self.current;
        // the reading line sits on the same screen row as the source
        // cursor, so side by side the eye moves straight across (#94)
        let (row, _) = self.cursor_display_pos();
        let ids: Vec<WinId> = self
            .preview_windows
            .iter()
            .copied()
            .filter(|id| *id != self.focused_win)
            .collect();
        for id in ids {
            // only previews of the buffer being edited follow
            let shows = self
                .parked_window(id)
                .map(|s| s.buf_id == buf)
                .unwrap_or(false);
            if !shows {
                continue;
            }
            // the projection at *that* window's width — looking it up at
            // the source window's would re-render it every frame whenever
            // the two differ by a column
            let (width, rows) = self.window_text_size(id);
            let view_line = self
                .preview_doc_for(buf, width)
                .view_line_for_source(src_line);
            let top = view_line.saturating_sub(row.min(rows - 1));
            self.preview_nav.insert(id, (view_line, top));
        }
    }

    /// `gcc` — toggle line comment on the current line and `count-1` below.
    pub(crate) fn toggle_comment_lines(&mut self, count: usize) {
        let last = self.buffer.rope.len_lines().saturating_sub(1);
        let from = self.cursor.line.min(last);
        let to = (from + count.saturating_sub(1)).min(last);
        self.toggle_comment(from, to);
    }

    /// `gc` in visual mode — toggle line comments over the selected lines.
    pub(crate) fn toggle_comment_visual(&mut self) {
        let (a, b) = (self.visual_anchor.line, self.cursor.line);
        let (from, to) = (a.min(b), a.max(b));
        self.leave_visual();
        self.toggle_comment(from, to);
    }

    /// Toggles line comments over `from..=to`: uncomments when every
    /// non-blank line is already commented, comments otherwise. Comment
    /// markers align to the shallowest indentation; blank lines are left be.
    pub(crate) fn toggle_comment(&mut self, from: usize, to: usize) {
        use crate::core::text::{first_non_blank, line_content};
        let Some(token) = crate::comment::line_comment(&self.buffer) else {
            self.err("no line-comment syntax for this file");
            return;
        };
        let last = self.buffer.rope.len_lines().saturating_sub(1);
        let to = to.min(last);
        let targets: Vec<usize> = (from..=to)
            .filter(|&l| !line_content(&self.buffer.rope, l).trim().is_empty())
            .collect();
        if targets.is_empty() {
            return;
        }
        let all_commented = targets
            .iter()
            .all(|&l| crate::comment::is_commented(&line_content(&self.buffer.rope, l), &token));
        let tok_len = token.chars().count();
        self.buffer.begin_change(self.cursor);
        if all_commented {
            for &l in targets.iter().rev() {
                let col = first_non_blank(&self.buffer.rope, l);
                let start = self.buffer.rope.line_to_char(l) + col;
                self.buffer.remove(start..start + tok_len);
                if self.buffer.rope.get_char(start) == Some(' ') {
                    self.buffer.remove(start..start + 1);
                }
            }
        } else {
            let indent = targets
                .iter()
                .map(|&l| first_non_blank(&self.buffer.rope, l))
                .min()
                .unwrap_or(0);
            let insertion = format!("{token} ");
            for &l in targets.iter().rev() {
                let at = self.buffer.rope.line_to_char(l) + indent;
                self.buffer.insert(at, &insertion);
            }
        }
        self.buffer.end_change();
        self.clamp_cursor();
    }

    /// `K` — the one "tell me about this" verb: LSP hover where a server
    /// exists, the machine lens (#45) everywhere else.
    pub(crate) fn hover(&mut self) {
        if self.current_rust_file().is_some() {
            self.analyzer_hover();
        } else {
            self.lens_hover();
        }
    }

    /// The asm dialect this buffer declares in its modeline, if any.
    fn lens_family(&self) -> Option<crate::lens::Family> {
        crate::lens::modeline(self.buffer.rope.lines().take(5).map(|l| l.to_string()))
    }

    /// The machine lens: a numeric literal under the cursor answers as a
    /// number; otherwise, in a dialect-declared asm file, the line answers
    /// as an instruction.
    fn lens_hover(&mut self) {
        let line = crate::core::text::line_content(&self.buffer.rope, self.cursor.line);
        if let Some(word) = crate::lens::literal_at(&line, self.cursor.col)
            && let Some(float) = crate::lens::number_hover(&word)
        {
            self.info_float = Some(float);
            return;
        }
        if let Some(family) = self.lens_family()
            && let Some(float) = crate::lens::opcode_hover(&line, &family)
        {
            self.info_float = Some(float);
            return;
        }
        self.msg("K: nothing to tell about this");
    }

    /// Visual `K` in a 6502-family file: cycle sum over the selected lines.
    pub(crate) fn lens_cycle_sum(&mut self) {
        let Some(family) = self.lens_family() else {
            return;
        };
        if matches!(family, crate::lens::Family::Other(_)) {
            return;
        }
        let (a, b) = (self.visual_anchor.line, self.cursor.line);
        let (from, to) = (a.min(b), a.max(b));
        let lines: Vec<String> = (from..=to)
            .map(|i| crate::core::text::line_content(&self.buffer.rope, i))
            .collect();
        self.info_float = Some(crate::lens::cycle_sum(&lines, &family));
    }

    /// Leaves preview on this window, landing the cursor on the source
    /// line mapped from the given rendered line.
    fn preview_jump_to_source(&mut self, id: windows::WinId, view_line: usize, width: usize) {
        let source = self
            .preview_doc_for(self.current, width)
            .source_line_for_view(view_line);
        self.preview_windows.remove(&id);
        self.preview_nav.remove(&id);
        self.cursor = Cursor::new(source, 0);
        self.goal = None;
        self.clamp_cursor();
        self.refresh_focused_view();
    }

    /// Keyboard when a preview window has focus: navigation over the
    /// rendered document; Enter/gd jumps to source; gp/Esc projects back.
    pub(crate) fn preview_key(&mut self, key: Key) {
        use crate::config::keymap::Key as K;
        let id = self.focused_win;
        let width = self.view.width.saturating_sub(2);
        let height = self.view.height.max(2);
        let total = self.preview_doc_for(self.current, width).line_count();
        let (mut line, mut top) = self.preview_nav_state(id);
        let last = total.saturating_sub(1);
        match key {
            K::Char('k') | K::Down => line = (line + 1).min(last),
            K::Char('i') | K::Up => line = line.saturating_sub(1),
            K::Ctrl('d') => line = (line + height / 2).min(last),
            K::Ctrl('u') => line = line.saturating_sub(height / 2),
            K::Ctrl('f') | K::PageDown => line = (line + height).min(last),
            K::Ctrl('b') | K::PageUp => line = line.saturating_sub(height),
            K::Char('g') => line = 0, // gg-lite: single g goes to top
            K::Char('G') => line = last,
            K::Enter => {
                // jump to source at the mapped line
                self.preview_jump_to_source(id, line, width);
                return;
            }
            K::Char('h') => {
                // start editing right here: source at the mapped line, insert
                self.preview_jump_to_source(id, line, width);
                normal::enter_insert(self, crate::core::commands::InsertEntry::Before);
                return;
            }
            K::Char(':') => {
                // the command line works from a preview (`:q` closes the panel)
                self.cmdline.clear();
                self.prompt = Prompt::Command;
                self.mode = Mode::Command;
                return;
            }
            K::Char('p') | K::Esc => {
                self.toggle_preview();
                return;
            }
            K::Ctrl('w') => {
                // window chord still works from a preview
                self.pending.awaiting = Awaiting::Window;
                return;
            }
            _ => return,
        }
        // scrolloff-ish: keep the reading line comfortably framed
        let margin = (height / 4).min(5);
        if line < top + margin {
            top = line.saturating_sub(margin);
        }
        if line + margin >= top + height {
            top = (line + margin + 1).saturating_sub(height);
        }
        top = top.min(last);
        self.preview_nav.insert(id, (line, top));
    }

    /// Terminal paste (Cmd+V via bracketed paste): literal text in insert
    /// mode, a charwise put in normal mode, replaces the selection in
    /// visual mode. Never interpreted as keys.
    pub fn paste_external(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        // whoever owns the keyboard owns the paste (#82 §11): an open picker
        // takes it as query text, the buffer list and a focused preview
        // (navigation-only) swallow it — the source buffer is never touched
        if let Some(p) = &mut self.file_picker {
            p.push_query(&normalized);
            return;
        }
        if self.buffer_list.is_some() || self.focused_is_preview() {
            return;
        }
        match self.mode {
            Mode::Insert | Mode::Replace => {
                insert::insert_text(self, &normalized);
                self.lsp_note_edit();
            }
            Mode::Visual(_) => {
                normal::visual_paste_with(self, Register::Char(normalized));
                self.lsp_note_edit();
            }
            Mode::Normal => {
                self.buffer.begin_change(self.cursor);
                let at = self.buffer.rope.line_to_char(self.cursor.line) + self.cursor.col;
                self.buffer.insert(at, &normalized);
                let end = at + normalized.chars().count();
                self.cursor = {
                    let rope = &self.buffer.rope;
                    let last = text_lines(rope) - 1;
                    let line = rope
                        .char_to_line(
                            end.saturating_sub(1)
                                .min(rope.len_chars().saturating_sub(1)),
                        )
                        .min(last);
                    let col = end
                        .saturating_sub(1)
                        .saturating_sub(rope.line_to_char(line));
                    Cursor::new(line, col)
                };
                let committed = self.buffer.end_change();
                self.note_change_committed(committed);
                self.clamp_cursor();
                self.scroll_to_cursor();
                self.lsp_note_edit();
            }
            Mode::Command => {
                // paste into the prompt (single-line: newlines become spaces)
                let flat = normalized.replace('\n', " ");
                self.cmdline.push_str(flat.trim_end());
            }
        }
    }

    /// Compact pending-state hint for the message line (vim's 'showcmd').
    pub fn pending_display(&self) -> String {
        let mut s = String::new();
        if let Some(c) = self.pending.count1 {
            s.push_str(&c.to_string());
        }
        match self.pending.op {
            Some(Op::Delete) => s.push('d'),
            Some(Op::Change) => s.push('c'),
            Some(Op::Yank) => s.push('y'),
            Some(Op::Indent) => s.push('>'),
            Some(Op::Dedent) => s.push('<'),
            None => {}
        }
        if let Some(c) = self.pending.count2 {
            s.push_str(&c.to_string());
        }
        match self.pending.awaiting {
            Awaiting::None => {}
            Awaiting::Find(FindKind::To) => s.push('f'),
            Awaiting::Find(FindKind::ToBack) => s.push('F'),
            Awaiting::Find(FindKind::Till) => s.push('t'),
            Awaiting::Find(FindKind::TillBack) => s.push('T'),
            Awaiting::Replace => s.push('r'),
            Awaiting::G => s.push('g'),
            Awaiting::GComment => s.push_str("gc"),
            Awaiting::SetMark => s.push('m'),
            Awaiting::JumpMark { exact } => s.push(if exact { '`' } else { '\'' }),
            Awaiting::TextObject { around } => s.push(if around { 'a' } else { 'n' }),
            Awaiting::Z => s.push('z'),
            Awaiting::ZUpper => s.push('Z'),
            Awaiting::Leader => s.push('␣'),
            Awaiting::Window => s.push_str("^W"),
            Awaiting::LeaderC => s.push_str("␣c"),
            Awaiting::LeaderR => s.push_str("␣r"),
            Awaiting::LeaderD => s.push_str("␣d"),
        }
        s
    }
}

/// Keys inside the code-actions menu: i/k navigate, Enter applies, Esc quits.
fn analyzer_menu_key(ed: &mut Editor, key: Key) {
    let Some(menu) = &mut ed.actions_menu else {
        return;
    };
    match key {
        Key::Char('i') | Key::Up => menu.selected = menu.selected.saturating_sub(1),
        Key::Char('k') | Key::Down => {
            menu.selected = (menu.selected + 1).min(menu.actions.len().saturating_sub(1));
        }
        Key::Enter => ed.run_selected_action(),
        Key::Esc | Key::Char('q') | Key::Ctrl('c') => ed.actions_menu = None,
        _ => {}
    }
}

/// Parses `s/pat/rep/[g]` (or `%s/…`), honoring backslash-escaped slashes.
fn parse_substitute(spec: &str) -> Option<(String, String, bool)> {
    let rest = spec
        .strip_prefix("%s")
        .or_else(|| spec.strip_prefix('s'))?
        .strip_prefix('/')?;
    let mut parts: Vec<String> = vec![String::new()];
    let mut escaped = false;
    for c in rest.chars() {
        if escaped {
            if c != '/' {
                parts.last_mut().unwrap().push('\\');
            }
            parts.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '/' {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    if escaped {
        parts.last_mut().unwrap().push('\\');
    }
    let pattern = parts.first().cloned().unwrap_or_default();
    if pattern.is_empty() {
        return None;
    }
    let replacement = parts.get(1).cloned().unwrap_or_default();
    let flags = parts.get(2).cloned().unwrap_or_default();
    Some((pattern, replacement, flags.contains('g')))
}

/// Minimal standard base64 (no padding shortcuts), for OSC 52 payloads.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::testing::editor_with_buffers;

    #[test]
    fn buffer_ids_are_never_reused() {
        // #82: allocating as max(live)+1 let a closed top id come back, and
        // marks/caches keyed by it with it
        let mut ed = editor_with_buffers(&[("a.rs", ""), ("b.rs", "")]);
        ed.close_buffer(2, true);
        assert_eq!(ed.alloc_buf_id(), 3, "closed id 2 must not be recycled");
    }

    #[test]
    fn row_count_fast_path_agrees_with_the_wrapper() {
        // #89: the byte bound must never claim one row for a line the
        // wrapper would fold — sweep every width across the awkward cases
        let lines = [
            "",
            "short",
            "\tindented\twith\ttabs",
            "a\t\t\t\tb",
            "tab\tafter wide 日本\tmixed",
            "日本語のテキストは広い",
            "café naïve résumé façade",
            "e\u{301}e\u{301}e\u{301} combining",
            "\u{301}lone combining mark first",
            "👨\u{200d}👩\u{200d}👧 family 🇵🇱 flag ©\u{fe0f} mark",
            "क्षि क्षि क्षि devanagari",
            "ctrl\u{1}chars\u{7f}here",
            "a very long line of ordinary prose that certainly wraps",
            "loooooooooooooooooooooooooooooooooooooooooongword",
        ];
        let text = lines.join("\n") + "\n";
        let mut ed = editor_with_buffers(&[("t.md", &text)]);
        for width in 1..=48 {
            ed.set_view(width, 10);
            for (l, line) in lines.iter().enumerate() {
                assert_eq!(
                    ed.rows_of_line(l),
                    ed.wrap_rows_of(l).len(),
                    "line {line:?} at width {width}"
                );
            }
        }
    }
}
