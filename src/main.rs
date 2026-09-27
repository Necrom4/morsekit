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

        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            break;
        }
        if let KeyCode::Char(character) = key.code {
            print!("{character}");
            io::stdout().flush()?;
        }
    }
    disable_raw_mode()?;
    Ok(())
}
