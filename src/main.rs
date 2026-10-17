use clap::Parser;
use crossterm::{
    cursor::{MoveTo, MoveUp},
    event::{
        Event::Key, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags, poll, read,
    },
    execute, queue,
    style::Print,
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
    /// Minimum signal duration, in dits, classified as a dah
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=u64::MAX / 1_200))]
    dah_threshold: u64,
    /// Silence threshold, in dits, that ends a letter
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=u64::MAX / 1_200))]
    letter_gap_threshold: u64,
    /// Silence threshold, in dits, that creates a word gap
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=u64::MAX / 1_200))]
    word_gap_threshold: u64,
    /// Mute beep sound
    #[arg(short, long)]
    mute: bool,
    /// Tone frequency in Hz
    #[arg(short, long, default_value_t = 600.0)]
    pitch: f32,
    /// Morse speed in words per minute
    #[arg(short, long, default_value_t = 12, value_parser = clap::value_parser!(u64).range(1..=50))]
    wpm: u64,
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

    let _ = write!(stdout, "morse: \n\rascii: ");
    queue!(stdout, MoveUp(1))?;

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
    let dah_threshold = Duration::from_millis(args.dah_threshold * dit_ms);
    let letter_gap_threshold = Duration::from_millis(args.letter_gap_threshold * dit_ms);
    let word_gap_threshold = Duration::from_millis(args.word_gap_threshold * dit_ms);

    let audio_device = if args.mute {
        None
    } else {
        let mut audio_device = DeviceSinkBuilder::open_default_sink()?;
        audio_device.log_on_drop(false);
        Some(audio_device)
    };
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
                queue!(
                    stdout,
                    MoveTo(current_morse_column, current_morse_row),
                    Print(" ")
                )?;
                stdout.flush()?;
                current_morse_column += 1;
                current_translation_column += 1;
                letter_gap_added = true;
            }
            if gap >= word_gap_threshold {
                queue!(
                    stdout,
                    MoveTo(current_morse_column, current_morse_row),
                    Print("/ ")
                )?;
                stdout.flush()?;
                current_morse_column += 2;
                queue!(
                    stdout,
                    MoveTo(current_translation_column, current_translation_row,),
                    Print(" ")
                )?;
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
                    if let Some(audio_device) = &audio_device {
                        let player = Player::connect_new(audio_device.mixer());
                        let source = SineWave::new(args.pitch).amplify(0.20);
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
                    let signal = if duration < dah_threshold { '.' } else { '-' };
                    current_letter.push(signal);
                    queue!(
                        stdout,
                        MoveTo(current_morse_column, current_morse_row),
                        Print(signal)
                    )?;
                    stdout.flush()?;
                    current_morse_column += 1;
                    queue!(
                        stdout,
                        MoveTo(current_translation_column, current_translation_row,),
                        Print(translate_letter(&current_letter))
                    )?;
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

#[cfg(test)]
mod tests {
    mod translate {
        use super::super::translate_letter;

        #[test]
        fn translates_single_element_letter() {
            let result = translate_letter(".");
            assert_eq!(result, 'E');
        }

        #[test]
        fn translates_multi_element_letter() {
            let result = translate_letter(".-");
            assert_eq!(result, 'A');
        }

        #[test]
        fn translates_number() {
            let result = translate_letter("-----");
            assert_eq!(result, '0');
        }

        #[test]
        fn translates_punctuation() {
            let result = translate_letter("--..--");
            assert_eq!(result, ',');
        }

        #[test]
        fn incomplete_sequence_returns_placeholder() {
            let result = translate_letter("--..-");
            assert_eq!(result, '_');
        }

        #[test]
        fn unknown_sequence_returns_placeholder() {
            let result = translate_letter("......");
            assert_eq!(result, '_');
        }

        #[test]
        fn empty_input_returns_placeholder() {
            let result = translate_letter("");
            assert_eq!(result, '_');
        }

        #[test]
        fn invalid_symbols_return_placeholder() {
            let result = translate_letter("abc");
            assert_eq!(result, '_');
        }

        #[test]
        fn translates_word_separator() {
            let result = translate_letter("/");
            assert_eq!(result, ' ');
        }
    }

    mod cli {
        use super::super::Args;
        use clap::Parser;

        #[test]
        fn uses_defaults_without_flags() {
            let args = Args::try_parse_from(["morsekit"]).expect("defaults should parse");

            assert_eq!(args.dah_threshold, 2);
            assert_eq!(args.letter_gap_threshold, 2);
            assert_eq!(args.word_gap_threshold, 5);
            assert!(!args.mute);
            assert_eq!(args.pitch, 600.0);
            assert_eq!(args.wpm, 12);
        }

        #[test]
        fn supplied_flags_override_defaults() {
            let args = Args::try_parse_from([
                "morsekit",
                "--dah-threshold",
                "3",
                "--letter-gap-threshold",
                "4",
                "--word-gap-threshold",
                "7",
                "--mute",
                "--pitch",
                "750",
                "--wpm",
                "20",
            ])
            .expect("valid flags should parse");

            assert_eq!(args.dah_threshold, 3);
            assert_eq!(args.letter_gap_threshold, 4);
            assert_eq!(args.word_gap_threshold, 7);
            assert!(args.mute);
            assert_eq!(args.pitch, 750.0);
            assert_eq!(args.wpm, 20);
        }

        #[test]
        fn accepts_short_flags() {
            let args = Args::try_parse_from(["morsekit", "-m", "-p", "750", "-w", "20"])
                .expect("short flags should parse");

            assert!(args.mute);
            assert_eq!(args.pitch, 750.0);
            assert_eq!(args.wpm, 20);
        }

        #[test]
        fn thresholds_accept_range_boundaries() {
            let maximum = (u64::MAX / 1_200).to_string();
            for flag in [
                "--dah-threshold",
                "--letter-gap-threshold",
                "--word-gap-threshold",
            ] {
                for value in ["1", maximum.as_str()] {
                    assert!(
                        Args::try_parse_from(["morsekit", flag, value]).is_ok(),
                        "{flag} should accept {value}"
                    );
                }
            }
        }

        #[test]
        fn thresholds_reject_values_outside_range() {
            let above_maximum = (u64::MAX / 1_200 + 1).to_string();
            for flag in [
                "--dah-threshold",
                "--letter-gap-threshold",
                "--word-gap-threshold",
            ] {
                for value in ["0", above_maximum.as_str()] {
                    assert!(
                        Args::try_parse_from(["morsekit", flag, value]).is_err(),
                        "{flag} should reject {value}"
                    );
                }
            }
        }

        #[test]
        fn wpm_accepts_range_boundaries() {
            for value in ["1", "50"] {
                let args = Args::try_parse_from(["morsekit", "--wpm", value])
                    .expect("WPM boundaries should parse");
                assert_eq!(args.wpm.to_string(), value);
            }
        }

        #[test]
        fn wpm_rejects_values_outside_range() {
            for value in ["0", "51"] {
                assert!(
                    Args::try_parse_from(["morsekit", "--wpm", value]).is_err(),
                    "WPM should reject {value}"
                );
            }
        }

        #[test]
        fn numeric_flags_reject_missing_or_invalid_values() {
            for flag in [
                "--dah-threshold",
                "--letter-gap-threshold",
                "--word-gap-threshold",
                "--wpm",
                "--pitch",
            ] {
                assert!(
                    Args::try_parse_from(["morsekit", flag]).is_err(),
                    "{flag} should require a value"
                );
                assert!(
                    Args::try_parse_from(["morsekit", flag, "abc"]).is_err(),
                    "{flag} should reject nonnumeric input"
                );
            }
        }

        #[test]
        fn argument_definitions_are_consistent() {
            use clap::CommandFactory;

            Args::command().debug_assert();
        }
    }
}
