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
    cell_at_col, first_non_blank, gr_index_at_col, line_content, line_graphemes, line_len,
    max_normal_col, text_lines,
};
use windows::{MIN_WIN_HEIGHT, MIN_WIN_WIDTH, Node, SplitDir, WinId, WinState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
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
    left_cell: usize,
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

pub struct Editor {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub mode: Mode,
    pub goal: Option<usize>,
    pub pending: Pending,
    pub register: Option<Register>,
    pub last_find: Option<(FindKind, char)>,
    pub cmdline: String,
    pub message: Option<Message>,
    pub top_line: usize,
    pub left_cell: usize,
    pub should_quit: bool,
    pub view: View,
    pub buffer_list: Option<BufferList>,
    pub file_picker: Option<file_picker::FilePicker>,
    pub syntax: crate::syntax::Syntax,
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
            left_cell: 0,
        }];
        for (id, buffer) in (2..).zip(buffers) {
            slots.push(Slot {
                id,
                buffer: Some(buffer),
                cursor: Cursor::default(),
                goal: None,
                top_line: 0,
                left_cell: 0,
            });
        }
        Self {
            buffer: first,
            cursor: Cursor::default(),
            mode: Mode::Normal,
            goal: None,
            pending: Pending::default(),
            register: None,
            last_find: None,
            cmdline: String::new(),
            message: None,
            top_line: 0,
            left_cell: 0,
            should_quit: false,
            view: View {
                width: 80,
                height: 22,
            },
            buffer_list: None,
            file_picker: None,
            syntax: crate::syntax::Syntax::default(),
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
            left_cell: self.left_cell,
        };
        let slot = &self.slots[to];
        self.cursor = slot.cursor;
        self.goal = slot.goal;
        self.top_line = slot.top_line;
        self.left_cell = slot.left_cell;
        self.current = id;
    }

    /// Switches the focused window to buffer `id` (sets the alternate).
    pub(crate) fn switch_to(&mut self, id: BufId) {
        if id == self.current || self.slot_index(id).is_none() {
            return;
        }
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
                state.left_cell = 0;
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
        let abs = self.root.join(rel);
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
                    let id = self.slots.iter().map(|s| s.id).max().unwrap_or(0) + 1;
                    self.slots.push(Slot {
                        id,
                        buffer: Some(buffer),
                        cursor: Cursor::default(),
                        goal: None,
                        top_line: 0,
                        left_cell: 0,
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
            left_cell: self.left_cell,
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
        self.left_cell = state.left_cell;
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
                top + (self.cursor.line.saturating_sub(self.top_line)) as u16
            }
            WinDir::Up | WinDir::Down => {
                let left = rects
                    .iter()
                    .find(|(id, _)| *id == self.focused_win)
                    .map(|(_, r)| r.x)
                    .unwrap_or(0);
                left + self.cursor.col.saturating_sub(self.left_cell) as u16
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
            return;
        }
        let state = if with_new_buffer {
            let buf_id = self.slots.iter().map(|s| s.id).max().unwrap_or(0) + 1;
            self.slots.push(Slot {
                id: buf_id,
                buffer: Some(Buffer::from_text("")),
                cursor: Cursor::default(),
                goal: None,
                top_line: 0,
                left_cell: 0,
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
        self.focus_window(id);
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

        match self.mode {
            Mode::Normal => normal::handle_key(self, key),
            Mode::Insert => insert::handle_key(self, key),
            Mode::Command => cmdline::handle_key(self, key),
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
            Mode::Insert => line_len(&self.buffer.rope, self.cursor.line),
            _ => max_normal_col(&self.buffer.rope, self.cursor.line, OPTIONS.tabstop),
        };
        if self.cursor.col > limit {
            self.cursor.col = limit;
        }
    }

    pub fn scroll_to_cursor(&mut self) {
        let h = self.view.height;
        let last = text_lines(&self.buffer.rope) - 1;
        let so = OPTIONS.scrolloff.min(h.saturating_sub(1) / 2);
        if self.cursor.line < self.top_line + so {
            self.top_line = self.cursor.line.saturating_sub(so);
        }
        let bottom_so = so.min(last.saturating_sub(self.cursor.line));
        if self.cursor.line + bottom_so >= self.top_line + h {
            self.top_line = (self.cursor.line + bottom_so + 1).saturating_sub(h);
        }
        self.top_line = self.top_line.min(last);

        let grs = line_graphemes(
            &line_content(&self.buffer.rope, self.cursor.line),
            OPTIONS.tabstop,
        );
        let cell = cell_at_col(&grs, self.cursor.col);
        let cur_w = gr_index_at_col(&grs, self.cursor.col)
            .map(|i| grs[i].width)
            .unwrap_or(1);
        let w = self.view.width;
        if cell < self.left_cell {
            self.left_cell = cell;
        }
        if cell + cur_w > self.left_cell + w {
            self.left_cell = (cell + cur_w).saturating_sub(w);
        }
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

    pub(crate) fn save(&mut self) -> bool {
        match self.buffer.save() {
            Ok((path, lines)) => {
                self.msg(format!("\"{}\" {}L written", path.display(), lines));
                true
            }
            Err(e) => {
                self.err(format!("E212: {e:#}"));
                false
            }
        }
    }

    pub(crate) fn save_and_quit(&mut self, only_if_modified: bool) {
        if (!only_if_modified || self.buffer.is_modified()) && !self.save() {
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
        let last = text_lines(&self.buffer.rope) - 1;
        self.cursor.line = line.min(last);
        self.cursor.col = first_non_blank(&self.buffer.rope, self.cursor.line);
        self.goal = None;
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
            Awaiting::Z => s.push('z'),
            Awaiting::ZUpper => s.push('Z'),
            Awaiting::Leader => s.push('␣'),
            Awaiting::Window => s.push_str("^W"),
        }
        s
    }
}
