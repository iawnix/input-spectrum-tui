mod app;
mod global_input;
mod ui;

use std::env;
use std::io::{self, stdout, Stdout};
use std::time::{Duration, Instant};

use app::{AppCommand, AppConfig, AppState, Mode, Theme};
use crossterm::event::{
    self, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::global_input::GlobalInput;

type Tui = Terminal<CrosstermBackend<Stdout>>;
const CONTROL_GLOBAL_SUPPRESS: Duration = Duration::from_millis(90);

fn main() -> io::Result<()> {
    let config = match parse_args(env::args().skip(1)) {
        Ok(config) => config,
        Err(message) if message == "__help__" => {
            print_help();
            return Ok(());
        }
        Err(message) => {
            eprintln!("{message}");
            eprintln!("Run `inputspectrum --help` for usage.");
            std::process::exit(2);
        }
    };

    let mut terminal = setup_terminal()?;
    let terminal_guard = TerminalGuard;
    let result = run(&mut terminal, config);
    drop(terminal_guard);
    result
}

fn run(terminal: &mut Tui, config: AppConfig) -> io::Result<()> {
    let fps = config.fps.clamp(10, 120);
    let tick_rate = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut app = AppState::new(config);
    let global_input = GlobalInput::start();
    let mut last_tick = Instant::now();
    let mut suppress_global_until: Option<Instant> = None;

    loop {
        let elapsed = last_tick.elapsed();
        let timeout = tick_rate.saturating_sub(elapsed);
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => match app.handle_key(key) {
                    AppCommand::Quit => break,
                    AppCommand::ControlHandled => {
                        suppress_global_until = Some(Instant::now() + CONTROL_GLOBAL_SUPPRESS);
                    }
                    AppCommand::None => {}
                },
                Event::Resize(_, _) => {}
                Event::Mouse(_) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }

        let now = Instant::now();
        for input_event in global_input.drain() {
            if suppress_global_until.is_some_and(|until| now < until) {
                continue;
            }
            app.handle_global_key(input_event);
        }

        terminal.draw(|frame| ui::draw(frame, &app))?;

        if last_tick.elapsed() >= tick_rate {
            let now = Instant::now();
            let delta = now.duration_since(last_tick);
            app.tick(delta);
            last_tick = now;
        }
    }

    Ok(())
}

fn setup_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let terminal_guard = TerminalGuard;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;
    let terminal = Terminal::new(CrosstermBackend::new(out))?;
    terminal_guard.disarm();
    Ok(terminal)
}

struct TerminalGuard;

impl TerminalGuard {
    fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen
        );
    }
}

fn parse_args<I>(args: I) -> Result<AppConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut config = AppConfig::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(String::from("__help__")),
            "--bars" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--bars requires a value"))?;
                config.bars = parse_range(&value, "--bars", 8, 240)? as usize;
            }
            "--fps" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--fps requires a value"))?;
                config.fps = parse_range(&value, "--fps", 10, 120)? as u16;
            }
            "--theme" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--theme requires a value"))?;
                config.theme = parse_theme(&value)?;
            }
            "--mode" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--mode requires a value"))?;
                config.mode = parse_mode(&value)?;
            }
            _ if arg.starts_with("--bars=") => {
                config.bars = parse_range(&arg[7..], "--bars", 8, 240)? as usize;
            }
            _ if arg.starts_with("--fps=") => {
                config.fps = parse_range(&arg[6..], "--fps", 10, 120)? as u16;
            }
            _ if arg.starts_with("--theme=") => {
                config.theme = parse_theme(&arg[8..])?;
            }
            _ if arg.starts_with("--mode=") => {
                config.mode = parse_mode(&arg[7..])?;
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(config)
}

fn parse_range(value: &str, flag: &str, min: u16, max: u16) -> Result<u16, String> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| format!("{flag} must be a number"))?;
    if (min..=max).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!("{flag} must be between {min} and {max}"))
    }
}

fn parse_theme(value: &str) -> Result<Theme, String> {
    match value {
        "nord" => Ok(Theme::Nord),
        "mono" => Ok(Theme::Mono),
        "amber" => Ok(Theme::Amber),
        _ => Err(String::from(
            "--theme must be one of: nord, mono, amber",
        )),
    }
}

fn parse_mode(value: &str) -> Result<Mode, String> {
    match value {
        "bars" => Ok(Mode::Bars),
        "wave" => Ok(Mode::Wave),
        "peaks" => Ok(Mode::Peaks),
        _ => Err(String::from("--mode must be one of: bars, wave, peaks")),
    }
}

fn print_help() {
    println!(
        "inputspectrum\n\nUSAGE:\n    inputspectrum [--fps 60] [--bars 120] [--theme nord|mono|amber] [--mode bars|wave|peaks]\n\nCONTROLS:\n    q/Esc      quit\n    space      pause/resume\n    tab        switch mode\n    1/2/3      switch theme\n    +/-        sensitivity\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let config = parse_args(Vec::<String>::new()).unwrap();
        assert_eq!(config.bars, 120);
        assert_eq!(config.fps, 30);
        assert_eq!(config.theme, Theme::Nord);
        assert_eq!(config.mode, Mode::Bars);
    }

    #[test]
    fn parses_long_options() {
        let config = parse_args([
            "--fps=60".to_string(),
            "--bars".to_string(),
            "96".to_string(),
            "--theme".to_string(),
            "amber".to_string(),
            "--mode=wave".to_string(),
        ])
        .unwrap();

        assert_eq!(config.fps, 60);
        assert_eq!(config.bars, 96);
        assert_eq!(config.theme, Theme::Amber);
        assert_eq!(config.mode, Mode::Wave);
    }

    #[test]
    fn rejects_out_of_range_values() {
        assert!(parse_args(["--fps=5".to_string()]).is_err());
        assert!(parse_args(["--bars=400".to_string()]).is_err());
    }

    #[test]
    fn rejects_unknown_theme_aliases() {
        assert!(parse_args(["--theme=cyber".to_string()]).is_err());
    }

    #[test]
    fn rejects_unknown_mode_aliases() {
        assert!(parse_args(["--mode=cyber".to_string()]).is_err());
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(parse_args(["--backend=evdev".to_string()]).is_err());
    }
}
