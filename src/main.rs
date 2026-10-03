use crossterm::{
    event::{
        Event::Key, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags, poll, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use rodio::{DeviceSinkBuilder, Player, Source, source::SineWave};
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

// INFO: following https://morsecode.world/international/timing
const WPM: u64 = 12; // NOTE: sole varying variable.
const DIT_MS: u64 = 60_000 / (50 * WPM);
const DIT_DAH_THRESHOLD: Duration = Duration::from_millis(2 * DIT_MS);
const LETTER_GAP_THRESHOLD: Duration = Duration::from_millis(2 * DIT_MS);
const WORD_GAP_THRESHOLD: Duration = Duration::from_millis(5 * DIT_MS);

const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct RawModeGuard {
    keyboard_enhancement_enabled: bool,
}

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut guard = Self {
            keyboard_enhancement_enabled: false,
        };
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )?;
        guard.keyboard_enhancement_enabled = true;

        Ok(guard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();

    let _raw_mode = RawModeGuard::enable()?;

    let audio_output = DeviceSinkBuilder::open_default_sink()?;
    let mut tone: Option<Player> = None;

    let mut press_instant: Option<Instant> = None;
    let mut release_instant: Option<Instant> = None;
    let mut letter_ended = false;

    loop {
        if let Some(released_at) = release_instant {
            let gap = released_at.elapsed();
            if !letter_ended && gap >= LETTER_GAP_THRESHOLD {
                write!(stdout, " ")?;
                stdout.flush()?;
                letter_ended = true;
            }
            if gap >= WORD_GAP_THRESHOLD {
                write!(stdout, "/ ")?;
                stdout.flush()?;
                release_instant = None;
            }
        }

        if !poll(POLL_INTERVAL)? {
            continue;
        }

        let Key(key) = read()? else {
            continue;
        };

        match key.kind {
            KeyEventKind::Press => match key.code {
                KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char(' ') if press_instant.is_none() => {
                    press_instant = Some(Instant::now());
                    let player = Player::connect_new(audio_output.mixer());
                    let source = SineWave::new(600.0).amplify(0.20);
                    player.append(source);
                    tone = Some(player);
                    release_instant = None;
                    letter_ended = false;
                }
                _ => {}
            },
            KeyEventKind::Release => {
                if let Some(pressed_at) = press_instant
                    && key.code == KeyCode::Char(' ')
                {
                    let released_at = Instant::now();
                    if let Some(player) = tone.take() {
                        player.stop();
                    }
                    let duration = released_at - pressed_at;
                    if duration < DIT_DAH_THRESHOLD {
                        write!(stdout, ".")?;
                    } else {
                        write!(stdout, "-")?;
                    }
                    stdout.flush()?;
                    press_instant.take();
                    release_instant = Some(released_at);
                }
            }
            _ => {}
        }
    }

    Ok(())
}
