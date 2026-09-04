mod cmdline;
mod insert;
mod normal;
pub mod testing;

use crate::config::OPTIONS;
use crate::config::keymap::Key;
use crate::core::buffer::{Buffer, Cursor};
use crate::core::commands::{FindKind, Op, Register};
use crate::core::text::{
    cell_at_col, first_non_blank, gr_index_at_col, line_content, line_graphemes, line_len,
    max_normal_col, text_lines,
};

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
    recording: Vec<Key>,
    last_change: Option<Vec<Key>>,
    replaying: bool,
    change_started: bool,
}

impl Editor {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            buffer,
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
            recording: Vec::new(),
            last_change: None,
            replaying: false,
            change_started: false,
        }
    }

    pub fn handle_key(&mut self, key: Key) {
        if self.mode != Mode::Command {
            self.message = None;
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
        if !force && self.buffer.is_modified() {
            self.err("E37: No write since last change (add ! to override)");
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
        }
        s
    }
}
