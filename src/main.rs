use crossterm::{
    event::{Event::Key, KeyCode, KeyModifiers, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    loop {
        let event = read()?;

        if let Key(key_event) = event {
            if key_event.code == KeyCode::Char('c') && key_event.modifiers == KeyModifiers::CONTROL
            {
                break;
            }
            if let KeyCode::Char(character) = key_event.code {
                print!("{character}");
                io::stdout().flush()?;
            }
        }
    }
    disable_raw_mode()?;
    Ok(())
}
