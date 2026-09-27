use crossterm::event::{Event::Key, KeyCode, read};
use std::io;

fn main() -> io::Result<()> {
    loop {
        let event = read()?;

        if let Key(key_event) = event
            && let KeyCode::Char(character) = key_event.code
        {
            println!("{character}");
        }
    }
}
