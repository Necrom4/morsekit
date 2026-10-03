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
    collections::HashMap,
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

fn translate(line: &str) -> String {
    // TODO: this shouldn't be reconstructured on every translation.
    let lookup: HashMap<&str, char> = HashMap::from([
        (".-", 'A'),
        ("-...", 'B'),
        ("-.-.", 'C'),
        ("-..", 'D'),
        (".", 'E'),
        ("..-.", 'F'),
        ("--.", 'G'),
        ("....", 'H'),
        ("..", 'I'),
        (".---", 'J'),
        ("-.-", 'K'),
        (".-..", 'L'),
        ("--", 'M'),
        ("-.", 'N'),
        ("---", 'O'),
        (".--.", 'P'),
        ("--.-", 'Q'),
        (".-.", 'R'),
        ("...", 'S'),
        ("-", 'T'),
        ("..-", 'U'),
        ("...-", 'V'),
        (".--", 'W'),
        ("-..-", 'X'),
        ("-.--", 'Y'),
        ("--..", 'Z'),
        ("-----", '0'),
        (".----", '1'),
        ("..---", '2'),
        ("...--", '3'),
        ("....-", '4'),
        (".....", '5'),
        ("-....", '6'),
        ("--...", '7'),
        ("---..", '8'),
        ("----.", '9'),
        ("/", ' '),
    ]);

    let mut translation = String::new();

    for morse_group in line.split_whitespace() {
        let translated_char = lookup.get(morse_group).copied().unwrap_or('?');
        translation.push(translated_char);
    }

    translation
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    let (start_column, start_row) = crossterm::cursor::position()?;
    let _raw_mode = RawModeGuard::enable()?;

    let mut audio_output = DeviceSinkBuilder::open_default_sink()?;
    audio_output.log_on_drop(false);
    let mut tone: Option<Player> = None;

    let mut press_instant: Option<Instant> = None;
    let mut release_instant: Option<Instant> = None;
    let mut letter_ended = false;

    let mut morse_line = String::new();
    let mut redraw_morse_line = false;
    let mut redraw_translation_line = false;

    loop {
        if let Some(released_at) = release_instant {
            let gap = released_at.elapsed();
            if !letter_ended && gap >= LETTER_GAP_THRESHOLD {
                morse_line.push(' ');
                redraw_morse_line = true;
                redraw_translation_line = true;
                letter_ended = true;
            }
            if gap >= WORD_GAP_THRESHOLD {
                morse_line.push_str("/ ");
                redraw_morse_line = true;
                redraw_translation_line = true;
                release_instant = None;
            }
        }

        if redraw_morse_line {
            execute!(
                stdout,
                crossterm::cursor::MoveTo(start_column, start_row),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine),
            )?;
            write!(stdout, "{morse_line}")?;
            stdout.flush()?;
            redraw_morse_line = false;
        }

        if redraw_translation_line {
            execute!(
                stdout,
                crossterm::cursor::MoveTo(start_column, start_row + 1),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine),
            )?;
            // TODO: shouldn't translate the whole line on each redraw.
            write!(stdout, "{}", translate(&morse_line))?;
            stdout.flush()?;
            redraw_translation_line = false;
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
                        morse_line.push('.');
                    } else {
                        morse_line.push('-');
                    }
                    redraw_morse_line = true;
                    press_instant.take();
                    release_instant = Some(released_at);
                }
            }
            _ => {}
        }
    }

    Ok(())
}
