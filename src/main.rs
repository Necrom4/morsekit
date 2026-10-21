use clap::{CommandFactory, Parser, error::ErrorKind};
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm::{
        event::{
            Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
            PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags, poll, read,
        },
        execute,
        terminal::supports_keyboard_enhancement,
    },
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    symbols::border,
    text::Line,
    widgets::{Block, Padding, Paragraph, Widget, Wrap},
};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source, source::SineWave};
use std::{
    collections::HashMap,
    io,
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
    #[arg(short, long, default_value_t = 600, value_parser = clap::value_parser!(u16).range(220..=880))]
    pitch: u16,
    /// Morse speed in words per minute
    #[arg(short, long, default_value_t = 12, value_parser = clap::value_parser!(u64).range(1..=50))]
    wpm: u64,
}

impl Args {
    fn validate(&self) -> Result<(), clap::Error> {
        if self.word_gap_threshold < self.letter_gap_threshold {
            return Err(Args::command().error(
                ErrorKind::ArgumentConflict,
                format!(
                    "`--word-gap-threshold` ({}) must not be smaller than `--letter-gap-threshold` ({})",
                    self.word_gap_threshold, self.letter_gap_threshold
                ),
            ));
        }
        Ok(())
    }
}

struct KeyboardEnhancementGuard {
    keyboard_enhancement_enabled: bool,
}

impl KeyboardEnhancementGuard {
    fn enable() -> io::Result<Self> {
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

impl Drop for KeyboardEnhancementGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        while let Ok(true) = poll(Duration::ZERO)
            && read().is_ok()
        {}
    }
}

#[derive(Debug, Clone, Copy)]
struct Timing {
    dah_threshold: Duration,
    letter_gap_threshold: Duration,
    word_gap_threshold: Duration,
}

impl Timing {
    fn from_args(args: &Args) -> Self {
        // Following https://morsecode.world/international/timing.
        let dit_ms = 60_000 / (50 * args.wpm);
        Self {
            dah_threshold: Duration::from_millis(args.dah_threshold * dit_ms),
            letter_gap_threshold: Duration::from_millis(args.letter_gap_threshold * dit_ms),
            word_gap_threshold: Duration::from_millis(args.word_gap_threshold * dit_ms),
        }
    }
}

struct App {
    timing: Timing,
    // TODO: add a text->morse mode (`Mode::TextToMorse`).
    pending_morse: String,
    morse: String,
    text: String,
    // keying state
    key_pressed_at: Option<Instant>,
    last_signal_ended_at: Option<Instant>,
    letter_gap_added: bool,
    // audio
    active_tone: Option<Player>,
    audio_device: Option<MixerDeviceSink>,
    pitch: f32,
    exit: bool,
}

impl App {
    fn new(timing: Timing, audio_device: Option<MixerDeviceSink>, pitch: f32) -> Self {
        Self {
            timing,
            pending_morse: String::new(),
            morse: String::new(),
            text: String::new(),
            key_pressed_at: None,
            last_signal_ended_at: None,
            letter_gap_added: false,
            active_tone: None,
            audio_device,
            pitch,
            exit: false,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
            self.tick();
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if poll(POLL_INTERVAL)?
            && let Event::Key(key_event) = read()?
        {
            self.handle_key_event(key_event)
        };
        Ok(())
    }

    fn tick(&mut self) {
        if let Some(signal_ended_at) = self.last_signal_ended_at {
            let gap = signal_ended_at.elapsed();
            if !self.letter_gap_added && gap >= self.timing.letter_gap_threshold {
                self.text.push(decode(&self.pending_morse));
                self.pending_morse.clear();
                self.morse.push(' ');
                self.letter_gap_added = true;
            }
            if gap >= self.timing.word_gap_threshold {
                self.morse.push_str("/ ");
                self.text.push(' ');
                self.last_signal_ended_at = None;
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.kind {
            KeyEventKind::Press => match key_event.code {
                // TODO: enable quitting on `q` in text->morse mode.
                KeyCode::Esc => self.exit(),
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.exit()
                }
                KeyCode::Char(' ') if self.key_pressed_at.is_none() => {
                    self.key_pressed_at = Some(Instant::now());
                    if let Some(audio_device) = &self.audio_device {
                        let player = Player::connect_new(audio_device.mixer());
                        let source = SineWave::new(self.pitch).amplify(0.20);
                        player.append(source);
                        self.active_tone = Some(player);
                    }
                    self.last_signal_ended_at = None;
                    self.letter_gap_added = false;
                }
                KeyCode::Backspace if self.key_pressed_at.is_none() => self.delete_last_word(),
                _ => {}
            },
            KeyEventKind::Release => match key_event.code {
                KeyCode::Char(' ') if let Some(signal_started_at) = self.key_pressed_at => {
                    let signal_ended_at = Instant::now();
                    if let Some(player) = self.active_tone.take() {
                        player.stop();
                    }
                    let duration = signal_ended_at - signal_started_at;
                    let signal = if duration < self.timing.dah_threshold {
                        '.'
                    } else {
                        '-'
                    };
                    self.pending_morse.push(signal);
                    self.morse.push(signal);
                    self.key_pressed_at.take();
                    self.last_signal_ended_at = Some(signal_ended_at);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn delete_last_word(&mut self) {
        delete_last_word(&mut self.morse, "/ ");
        delete_last_word(&mut self.text, " ");
        self.pending_morse.clear();
        self.last_signal_ended_at = None;
        self.letter_gap_added = false;
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::vertical([Constraint::Percentage(50), Constraint::Fill(1)]);
        let [top, bottom] = layout.areas(area);

        let title = Line::from(vec![" ".into(), " morsekit ".bold().on_red(), " ".into()]);
        let morse_title = Line::from(" MORSE ".bold());
        let block_top = Block::bordered()
            .title(morse_title.left_aligned())
            .title(title.centered())
            .border_set(border::ROUNDED)
            .padding(Padding::uniform(1));
        Paragraph::new(self.morse.as_str())
            .left_aligned()
            .wrap(Wrap { trim: true })
            .block(block_top)
            .render(top, buf);

        let text_title = Line::from(" TEXT ".bold());
        let instructions = Line::from(vec![
            " Space".blue().bold(),
            " to sign | ".into(),
            "Backspace".yellow().bold(),
            " to delete a word | ".into(),
            "Esc".red().bold(),
            " to quit ".into(),
        ]);
        let block_bottom = Block::bordered()
            .title(text_title.left_aligned())
            .title_bottom(instructions.centered())
            .border_set(border::ROUNDED)
            .padding(Padding::uniform(1));
        let mut text_line = self.text.clone();
        if !self.pending_morse.is_empty() {
            text_line.push(decode(&self.pending_morse));
        }
        Paragraph::new(text_line)
            .left_aligned()
            .wrap(Wrap { trim: true })
            .block(block_bottom)
            .render(bottom, buf);
    }
}

fn decode(morse: &str) -> char {
    MORSE_TABLE.get(morse).copied().unwrap_or('_')
}
fn delete_last_word(text: &mut String, separator: &str) {
    let trimmed = text.trim_end_matches(|c: char| separator.contains(c));
    let end = trimmed
        .rfind(separator)
        .map_or(0, |index| index + separator.len());
    text.truncate(end);
}

// TODO: print to stdout the result of the translation.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Err(error) = args.validate() {
        error.exit();
    }
    if !supports_keyboard_enhancement()? {
        eprintln!("error: this terminal does not support the Kitty keyboard protocol");
        eprintln!("  your terminal won't pick up `KeyEventKind::Release`");
        eprintln!(
            "  help: consider using one of the listed terminals, see https://sw.kovidgoyal.net/kitty/keyboard-protocol/"
        );
        std::process::exit(1);
    }
    let timing = Timing::from_args(&args);

    let audio_device = if args.mute {
        None
    } else {
        let mut audio_device = DeviceSinkBuilder::open_default_sink()?;
        audio_device.log_on_drop(false);
        Some(audio_device)
    };

    ratatui::run(|terminal| {
        let _guard = KeyboardEnhancementGuard::enable()?;
        App::new(timing, audio_device, f32::from(args.pitch)).run(terminal)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    mod delete_last_word {
        use super::super::delete_last_word;

        fn deleted(text: &str, separator: &str) -> String {
            let mut text = text.to_string();
            delete_last_word(&mut text, separator);
            text
        }

        #[test]
        fn removes_unfinished_last_word() {
            assert_eq!(deleted("HELLO WO", " "), "HELLO ");
        }

        #[test]
        fn removes_finished_last_word() {
            assert_eq!(deleted("HELLO WORLD ", " "), "HELLO ");
        }

        #[test]
        fn removes_only_word() {
            assert_eq!(deleted("HELLO", " "), "");
        }

        #[test]
        fn empty_text_stays_empty() {
            assert_eq!(deleted("", " "), "");
        }

        #[test]
        fn removes_unfinished_morse_word() {
            assert_eq!(deleted(".... .. / .-- --- ", "/ "), ".... .. / ");
        }

        #[test]
        fn removes_finished_morse_word() {
            assert_eq!(deleted(".... .. / .-- --- / ", "/ "), ".... .. / ");
        }

        #[test]
        fn removes_morse_letter_in_progress() {
            assert_eq!(deleted(".... .. / .-", "/ "), ".... .. / ");
        }
    }

    mod decode {
        use super::super::decode;

        #[test]
        fn decodes_single_element_letter() {
            let result = decode(".");
            assert_eq!(result, 'E');
        }

        #[test]
        fn decodes_multi_element_letter() {
            let result = decode(".-");
            assert_eq!(result, 'A');
        }

        #[test]
        fn decodes_number() {
            let result = decode("-----");
            assert_eq!(result, '0');
        }

        #[test]
        fn decodes_punctuation() {
            let result = decode("--..--");
            assert_eq!(result, ',');
        }

        #[test]
        fn incomplete_sequence_returns_placeholder() {
            let result = decode("--..-");
            assert_eq!(result, '_');
        }

        #[test]
        fn unknown_sequence_returns_placeholder() {
            let result = decode("......");
            assert_eq!(result, '_');
        }

        #[test]
        fn empty_input_returns_placeholder() {
            let result = decode("");
            assert_eq!(result, '_');
        }

        #[test]
        fn invalid_symbols_return_placeholder() {
            let result = decode("abc");
            assert_eq!(result, '_');
        }

        #[test]
        fn decodes_word_separator() {
            let result = decode("/");
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
            assert_eq!(args.pitch, 600);
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
            assert_eq!(args.pitch, 750);
            assert_eq!(args.wpm, 20);
        }

        #[test]
        fn accepts_short_flags() {
            let args = Args::try_parse_from(["morsekit", "-m", "-p", "750", "-w", "20"])
                .expect("short flags should parse");

            assert!(args.mute);
            assert_eq!(args.pitch, 750);
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
        fn pitch_rejects_values_outside_range() {
            for value in ["0", "-10", "219", "881", "600.5", "nan", "inf"] {
                assert!(
                    Args::try_parse_from(["morsekit", "--pitch", value]).is_err(),
                    "pitch should reject {value}"
                );
            }
        }

        #[test]
        fn pitch_accepts_range_boundaries() {
            for (value, expected) in [("220", 220), ("880", 880)] {
                let args = Args::try_parse_from(["morsekit", "--pitch", value])
                    .unwrap_or_else(|error| panic!("pitch should accept {value}: {error}"));
                assert_eq!(args.pitch, expected);
            }
        }

        #[test]
        fn word_gap_smaller_threshold_than_letter_gap_threshold_is_rejected() {
            for flags in [
                ["--word-gap-threshold", "1", "--letter-gap-threshold", "3"],
                ["--word-gap-threshold", "1", "--wpm", "12"],
                ["--letter-gap-threshold", "6", "--wpm", "12"],
            ] {
                let args = Args::try_parse_from(["morsekit"].into_iter().chain(flags))
                    .expect("each flag is valid on its own");
                assert!(args.validate().is_err(), "{flags:?} should be rejected");
            }
        }

        #[test]
        fn word_gap_threshold_equal_to_or_larger_than_letter_gap_threshold_is_accepted() {
            for (word, letter) in ["5", "2", "3"].into_iter().zip(["2", "2", "3"]) {
                let args = Args::try_parse_from([
                    "morsekit",
                    "--word-gap-threshold",
                    word,
                    "--letter-gap-threshold",
                    letter,
                ])
                .expect("flags should parse");
                assert!(args.validate().is_ok(), "{word} and {letter} should pass");
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
