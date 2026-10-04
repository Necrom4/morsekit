use clap::Parser;
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
    sync::LazyLock,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

static MORSE_TABLE: LazyLock<HashMap<&str, char>> = LazyLock::new(|| {
    HashMap::from([
        // Letters
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
        // Figures
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
        // Punctuation
        (".-.-.-", '.'),
        ("--..--", ','),
        ("---...", ':'),
        ("..--..", '?'),
        (".----.", '\''),
        ("-....-", '-'),
        ("-..-.", '/'),
        ("-.--.", '('),
        ("-.--.-", ')'),
        (".-..-.", '"'),
        ("-...-", '='),
        (".-.-.", '+'),
        (".--.-.", '@'),
        // Word separator
        ("/", ' '),
    ])
});

#[derive(Parser, Debug)]
#[command(version, about = "Live Morse code translator")]
struct Args {
    /// Morse speed in words per minute
    #[arg(short, long, default_value_t = 12, value_parser = clap::value_parser!(u64).range(1..=50))]
    wpm: u64,

    /// Mute beep sound
    #[arg(short, long)]
    mute: bool,
}

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

fn translate_letter(morse: &str) -> char {
    MORSE_TABLE.get(morse).copied().unwrap_or('_')
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut stdout = io::stdout().lock();
    let _raw_mode = RawModeGuard::enable()?;

    let (start_column, mut start_row) = crossterm::cursor::position()?;
    let (_, rows) = crossterm::terminal::size()?;
    if rows < 2 {
        return Err(io::Error::other("terminal needs at least two rows").into());
    }
    if start_row.saturating_add(1) >= rows {
        execute!(stdout, crossterm::terminal::ScrollUp(1))?;
        start_row = rows - 2;
    }
    let mut current_morse_column = start_column;
    let current_morse_row = start_row;
    let mut current_translation_column = start_column;
    let current_translation_row = start_row + 1;

    // Following https://morsecode.world/international/timing.
    let dit_ms = 60_000 / (50 * args.wpm);
    let dit_dah_threshold = Duration::from_millis(2 * dit_ms);
    let letter_gap_threshold = Duration::from_millis(2 * dit_ms);
    let word_gap_threshold = Duration::from_millis(5 * dit_ms);

    let mut audio_device = DeviceSinkBuilder::open_default_sink()?;
    audio_device.log_on_drop(false);
    let mut active_tone: Option<Player> = None;

    // Keying state
    let mut key_pressed_at: Option<Instant> = None;
    let mut last_signal_ended_at: Option<Instant> = None;
    let mut letter_gap_added = false;

    let mut current_letter = String::new();

    loop {
        if let Some(signal_ended_at) = last_signal_ended_at {
            let gap = signal_ended_at.elapsed();
            if !letter_gap_added && gap >= letter_gap_threshold {
                current_letter.clear();
                execute!(
                    stdout,
                    crossterm::cursor::MoveTo(current_morse_column, current_morse_row),
                )?;
                write!(stdout, " ")?;
                stdout.flush()?;
                current_morse_column += 1;
                current_translation_column += 1;
                letter_gap_added = true;
            }
            if gap >= word_gap_threshold {
                execute!(
                    stdout,
                    crossterm::cursor::MoveTo(current_morse_column, current_morse_row),
                )?;
                write!(stdout, "/ ")?;
                stdout.flush()?;
                current_morse_column += 2;
                execute!(
                    stdout,
                    crossterm::cursor::MoveTo(current_translation_column, current_translation_row,),
                )?;
                write!(stdout, " ")?;
                stdout.flush()?;
                current_translation_column += 1;
                last_signal_ended_at = None;
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
                KeyCode::Char(' ') if key_pressed_at.is_none() => {
                    key_pressed_at = Some(Instant::now());
                    if !args.mute {
                        let player = Player::connect_new(audio_device.mixer());
                        let source = SineWave::new(600.0).amplify(0.20);
                        player.append(source);
                        active_tone = Some(player);
                    }
                    last_signal_ended_at = None;
                    letter_gap_added = false;
                }
                _ => {}
            },
            KeyEventKind::Release => {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }
                if let Some(signal_started_at) = key_pressed_at
                    && key.code == KeyCode::Char(' ')
                {
                    let signal_ended_at = Instant::now();
                    if let Some(player) = active_tone.take() {
                        player.stop();
                    }
                    let duration = signal_ended_at - signal_started_at;
                    let signal = if duration < dit_dah_threshold {
                        '.'
                    } else {
                        '-'
                    };
                    current_letter.push(signal);
                    execute!(
                        stdout,
                        crossterm::cursor::MoveTo(current_morse_column, current_morse_row),
                    )?;
                    write!(stdout, "{signal}")?;
                    stdout.flush()?;
                    current_morse_column += 1;
                    execute!(
                        stdout,
                        crossterm::cursor::MoveTo(
                            current_translation_column,
                            current_translation_row,
                        ),
                    )?;
                    write!(stdout, "{}", translate_letter(&current_letter))?;
                    stdout.flush()?;
                    key_pressed_at.take();
                    last_signal_ended_at = Some(signal_ended_at);
                }
            }
            _ => {}
        }
    }

    Ok(())
}
