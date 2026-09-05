use std::io;
use std::path::Path;

use anyhow::Result;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;

use tailored::core::buffer::Buffer;
use tailored::editor::{Editor, Mode};
use tailored::{term, ui};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let launch = tailored::launch::resolve(&args, cwd);

    let mut editor = if launch.files.is_empty() {
        Editor::new(Buffer::from_text(""))
    } else {
        let mut buffers = Vec::new();
        let mut new_file = None;
        for path in &launch.files {
            let (buffer, existed) = Buffer::from_path(path)?;
            if !existed && new_file.is_none() {
                new_file = Some(path.clone());
            }
            buffers.push(buffer);
        }
        let mut ed = Editor::with_buffers(buffers);
        match new_file {
            // matches the displayed (first) buffer only
            Some(p) if p == launch.files[0] => {
                ed.msg(format!("\"{}\" [New File]", p.display()));
            }
            _ => {}
        }
        ed
    };
    editor.set_root(launch.root);
    if launch.open_picker {
        tailored::editor::file_picker::open(&mut editor);
    }

    term::install_panic_hook();
    let mut terminal = term::init()?;
    let result = run(&mut terminal, &mut editor);
    term::restore();
    result
}

fn run(
    terminal: &mut ratatui::Terminal<term::KittyBackend<io::Stdout>>,
    editor: &mut Editor,
) -> Result<()> {
    let mut last_mode = None;
    let mut needs_redraw = true;
    loop {
        let mode = editor.mode;
        if last_mode != Some(mode) {
            let style = match mode {
                Mode::Insert => SetCursorStyle::SteadyBar,
                _ => SetCursorStyle::SteadyBlock,
            };
            execute!(io::stdout(), style)?;
            last_mode = Some(mode);
        }

        if needs_redraw {
            terminal.draw(|f| ui::render(f, editor))?;
            needs_redraw = false;
        }

        // poll so rust-analyzer messages are handled while idle
        if event::poll(std::time::Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(k) => {
                    if let Some(key) = term::convert(k) {
                        editor.handle_key(key);
                        needs_redraw = true;
                    }
                }
                Event::Resize(..) => needs_redraw = true,
                _ => {}
            }
            // drain any burst of input before redrawing
            while event::poll(std::time::Duration::ZERO)? {
                match event::read()? {
                    Event::Key(k) => {
                        if let Some(key) = term::convert(k) {
                            editor.handle_key(key);
                        }
                    }
                    Event::Resize(..) => {}
                    _ => {}
                }
            }
        }

        if editor.lsp_tick() {
            needs_redraw = true;
        }

        if editor.should_quit {
            editor.lsp_shutdown();
            return Ok(());
        }
    }
}
