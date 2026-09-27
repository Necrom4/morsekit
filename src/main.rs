use crossterm::{
    event::{
        Event::Key, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

// INFO: following https://morsecode.world/international/timing
const WPM: u64 = 12; // NOTE: sole varying variable.
const DIT_MS: u64 = 60_000 / (50 * WPM);
const DIT_DAH_THRESHOLD: Duration = Duration::from_millis(2 * DIT_MS);

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
    let mut press_instant: Option<Instant> = None;

    execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    )?;

    loop {
        let Key(key) = read()? else {
            continue;
        };

        match key.kind {
            KeyEventKind::Press => match key.code {
                KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char(' ') if press_instant.is_none() => {
                    press_instant = Some(Instant::now())
                }
                _ => {}
            },
            KeyEventKind::Release => {
                if let Some(pressed_at) = press_instant
                    && key.code == KeyCode::Char(' ')
                {
                    let duration = Instant::now() - pressed_at;
                    if duration < DIT_DAH_THRESHOLD {
                        write!(stdout, ".")?;
                    } else {
                        write!(stdout, "-")?;
                    }
                    stdout.flush()?;
                    press_instant.take();
                }
            }
            _ => {}
        }
    }
    execute!(stdout, PopKeyboardEnhancementFlags)?;

    Ok(())
}
