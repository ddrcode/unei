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

    let mut editor = if args.is_empty() {
        Editor::new(Buffer::from_text(""))
    } else {
        let mut buffers = Vec::new();
        let mut new_file = None;
        for arg in &args {
            let (buffer, existed) = Buffer::from_path(Path::new(arg))?;
            if !existed && new_file.is_none() {
                new_file = Some(arg.clone());
            }
            buffers.push(buffer);
        }
        let mut ed = Editor::with_buffers(buffers);
        match new_file {
            // matches the displayed (first) buffer only
            Some(arg) if arg == args[0] => ed.msg(format!("\"{arg}\" [New File]")),
            _ => {}
        }
        ed
    };

    term::install_panic_hook();
    let mut terminal = term::init()?;
    let result = run(&mut terminal, &mut editor);
    term::restore();
    result
}

fn run(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    editor: &mut Editor,
) -> Result<()> {
    let mut last_mode = None;
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

        terminal.draw(|f| ui::render(f, editor))?;

        match event::read()? {
            Event::Key(k) => {
                if let Some(key) = term::convert(k) {
                    editor.handle_key(key);
                }
            }
            Event::Resize(..) => {}
            _ => {}
        }

        if editor.should_quit {
            return Ok(());
        }
    }
}
