use crossterm::{
    event::{Event::Key, KeyCode, KeyEventKind, KeyModifiers, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn main() -> io::Result<()> {
    let _raw_mode = RawModeGuard::enable()?;
    let mut stdout = io::stdout().lock();

    loop {
        let Key(key) = read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char(character) => {
                write!(stdout, "{character}")?;
                stdout.flush()?;
            }
            _ => {}
        }
    }

    Ok(())
}
