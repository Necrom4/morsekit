use clap::Parser;
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm::{
        event::{
            self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
            PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags, poll,
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
    #[arg(short, long, default_value_t = 600.0)]
    pitch: f32,
    /// Morse speed in words per minute
    #[arg(short, long, default_value_t = 12, value_parser = clap::value_parser!(u64).range(1..=50))]
    wpm: u64,
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
    // TODO: add alphabetical->morse mode (choose good and constant name for modes/buffers)
    // TODO: config if naming should be `_buffer` and if `_translation` makes sense as an opposite,
    // even in other mode.
    current_letter: String,
    morse_buffer: String,
    translation_buffer: String,
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
            current_letter: String::new(),
            morse_buffer: String::new(),
            translation_buffer: String::new(),
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
            && let Event::Key(key_event) = event::read()?
        {
            self.handle_key_event(key_event)
        };
        Ok(())
    }

    fn tick(&mut self) {
        if let Some(signal_ended_at) = self.last_signal_ended_at {
            let gap = signal_ended_at.elapsed();
            if !self.letter_gap_added && gap >= self.timing.letter_gap_threshold {
                self.translation_buffer
                    .push(translate_letter(&self.current_letter));
                self.current_letter.clear();
                self.morse_buffer.push(' ');
                self.letter_gap_added = true;
            }
            if gap >= self.timing.word_gap_threshold {
                self.morse_buffer.push_str("/ ");
                self.translation_buffer.push(' ');
                self.last_signal_ended_at = None;
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.kind {
            KeyEventKind::Press => match key_event.code {
                // TODO: enable quitting on `q` in alphabetical->morse mode.
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
                // TODO: `delete` last word.
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
                    self.current_letter.push(signal);
                    self.morse_buffer.push(signal);
                    self.key_pressed_at.take();
                    self.last_signal_ended_at = Some(signal_ended_at);
                }
                _ => {}
            },
            _ => {}
        }
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
        Paragraph::new(self.morse_buffer.as_str())
            .left_aligned()
            .wrap(Wrap { trim: true })
            .block(block_top)
            .render(top, buf);

        let translation_title = Line::from(" TRANSLATION ".bold());
        let instructions = Line::from(vec![
            " Space".blue().bold(),
            " to type | ".into(),
            "Esc".red().bold(),
            " to quit ".into(),
        ]);
        let block_bottom = Block::bordered()
            .title(translation_title.left_aligned())
            .title_bottom(instructions.centered())
            .border_set(border::ROUNDED)
            .padding(Padding::uniform(1));
        let mut translation_line = self.translation_buffer.clone();
        if !self.current_letter.is_empty() {
            translation_line.push(translate_letter(&self.current_letter));
        }
        Paragraph::new(translation_line)
            .left_aligned()
            .wrap(Wrap { trim: true })
            .block(block_bottom)
            .render(bottom, buf);
    }
}

fn translate_letter(morse: &str) -> char {
    MORSE_TABLE.get(morse).copied().unwrap_or('_')
}

// TODO: print to stdout the result of the translation.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
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
        App::new(timing, audio_device, args.pitch).run(terminal)
    })?;

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
