//! The buffer-list overlay: pick a buffer or close one.

use crate::config::keymap::{self, Key};
use crate::core::commands::ListCmd;

use super::{BufferList, Editor};

pub fn open(ed: &mut Editor) {
    let selected = ed
        .buffer_entries()
        .iter()
        .position(|e| e.current)
        .unwrap_or(0);
    ed.buffer_list = Some(BufferList { selected });
}

pub fn handle_key(ed: &mut Editor, key: Key) {
    let Some(cmd) = keymap::list_token(key) else {
        return;
    };
    let Some(selected) = ed.buffer_list.as_ref().map(|l| l.selected) else {
        return;
    };
    let count = ed.buffer_count();
    match cmd {
        ListCmd::Up => {
            ed.buffer_list = Some(BufferList {
                selected: selected.saturating_sub(1),
            });
        }
        ListCmd::Down => {
            ed.buffer_list = Some(BufferList {
                selected: (selected + 1).min(count - 1),
            });
        }
        ListCmd::Dismiss => ed.buffer_list = None,
        ListCmd::Select => {
            let id = ed.buffer_entries()[selected].id;
            ed.buffer_list = None;
            ed.switch_to(id);
        }
        ListCmd::CloseBuffer => {
            let id = ed.buffer_entries()[selected].id;
            ed.close_buffer(id, false);
            // on refusal (modified buffer) the count is unchanged and the
            // error message is already set; the list stays open either way
            let count = ed.buffer_count();
            ed.buffer_list = Some(BufferList {
                selected: selected.min(count - 1),
            });
        }
    }
}
