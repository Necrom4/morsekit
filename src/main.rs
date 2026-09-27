use crossterm::{
    event::{Event::Key, KeyCode, KeyEventKind, KeyModifiers, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    loop {
        let Key(key) = read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => break,
            KeyCode::Char(character) => {
                print!("{character}");
                io::stdout().flush()?;
            }
            _ => {}
        }
    }
    disable_raw_mode()?;
    Ok(())
}
