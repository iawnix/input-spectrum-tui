use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::global_input::GlobalKeyEvent;

const EVENT_WINDOW: Duration = Duration::from_secs(1);
const MAX_EVENTS: usize = 512;
const PEAK_DECAY: f32 = 0.90;
const ENERGY_DECAY_BASE: f32 = 0.82;
const KEY_EVENT_SCALE: f32 = 0.92;
const WAVE_SPREAD: isize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Bars,
    Wave,
    Peaks,
}

impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Bars => Self::Wave,
            Self::Wave => Self::Peaks,
            Self::Peaks => Self::Bars,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Nord,
    Mono,
    Amber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Key,
    SpecialKey,
}

#[derive(Debug, Clone)]
pub struct Band {
    pub energy: f32,
    pub peak: f32,
    pub last_event: Option<EventKind>,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            energy: 0.0,
            peak: 0.0,
            last_event: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bars: usize,
    pub fps: u16,
    pub theme: Theme,
    pub mode: Mode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bars: 120,
            fps: 30,
            theme: Theme::Nord,
            mode: Mode::Bars,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppCommand {
    None,
    ControlHandled,
    Quit,
}

#[derive(Debug)]
pub struct AppState {
    pub bands: Vec<Band>,
    pub mode: Mode,
    pub theme: Theme,
    pub paused: bool,
    pub sensitivity: f32,
    pub event_count: u64,
    pub key_count: u64,
    pub last_event: Option<EventKind>,
    pub last_key_label: String,
    phase: usize,
    events: VecDeque<Instant>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let bars = config.bars.clamp(8, 240);
        Self {
            bands: vec![Band::default(); bars],
            mode: config.mode,
            theme: config.theme,
            paused: false,
            sensitivity: 1.0,
            event_count: 0,
            key_count: 0,
            last_event: None,
            last_key_label: String::from("-"),
            phase: 0,
            events: VecDeque::with_capacity(MAX_EVENTS),
        }
    }

    pub fn tick(&mut self, delta: Duration) {
        self.prune_events(Instant::now());
        if self.paused {
            return;
        }

        let delta_factor = (delta.as_secs_f32() * 30.0).clamp(0.25, 4.0);
        let decay = ENERGY_DECAY_BASE.powf(delta_factor);
        let peak_decay = PEAK_DECAY.powf(delta_factor);

        for band in &mut self.bands {
            band.energy = sanitize_level(band.energy * decay);
            band.peak = sanitize_level((band.peak * peak_decay).max(band.energy));
            if band.energy < 0.015 {
                band.energy = 0.0;
            }
            if band.peak < 0.02 {
                band.peak = 0.0;
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return AppCommand::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return AppCommand::Quit;
            }
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                self.last_key_label = String::from("space");
                return AppCommand::ControlHandled;
            }
            KeyCode::Tab => {
                self.mode = self.mode.next();
                self.last_key_label = String::from("tab");
                return AppCommand::ControlHandled;
            }
            KeyCode::Char('1') => {
                self.theme = Theme::Nord;
                self.last_key_label = String::from("1");
                return AppCommand::ControlHandled;
            }
            KeyCode::Char('2') => {
                self.theme = Theme::Mono;
                self.last_key_label = String::from("2");
                return AppCommand::ControlHandled;
            }
            KeyCode::Char('3') => {
                self.theme = Theme::Amber;
                self.last_key_label = String::from("3");
                return AppCommand::ControlHandled;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.sensitivity = (self.sensitivity + 0.1).min(3.0);
                self.last_key_label = String::from("+");
                return AppCommand::ControlHandled;
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.sensitivity = (self.sensitivity - 0.1).max(0.2);
                self.last_key_label = String::from("-");
                return AppCommand::ControlHandled;
            }
            _ => {
                let band = key_to_band(key.code, self.bands.len());
                let event_kind = if is_special_key(key.code) {
                    EventKind::SpecialKey
                } else {
                    EventKind::Key
                };
                let amount = if event_kind == EventKind::SpecialKey {
                    1.35
                } else {
                    KEY_EVENT_SCALE
                };
                self.inject_at(band, event_kind, amount);
                self.key_count += 1;
                self.last_key_label = key_label(key.code);
            }
        }
        AppCommand::None
    }

    pub fn handle_global_key(&mut self, event: GlobalKeyEvent) {
        let band = dynamic_key_band(event.code, self.bands.len(), self.phase);
        self.inject_wave_packet(band, EventKind::Key, KEY_EVENT_SCALE);
        self.key_count += 1;
        self.last_key_label = format!("key:{}", event.code);
    }

    pub fn inject_at(&mut self, band_index: usize, kind: EventKind, amount: f32) {
        self.inject_wave_packet(band_index, kind, amount);
    }

    fn inject_wave_packet(&mut self, band_index: usize, kind: EventKind, amount: f32) {
        if self.bands.is_empty() {
            return;
        }

        let now = Instant::now();
        self.record_event(now);
        self.phase = self.phase.wrapping_add(5 + self.events.len());

        let rate_boost = (self.events.len() as f32 / 22.0).min(1.6);
        let amount = amount * self.sensitivity * (1.0 + rate_boost);
        let center = band_index.min(self.bands.len() - 1) as isize;
        let len = self.bands.len() as isize;
        let drift = ((self.phase % 11) as isize) - 5;

        for offset in -WAVE_SPREAD..=WAVE_SPREAD {
            let idx = (center + offset + drift).rem_euclid(len) as usize;
            let distance = offset.unsigned_abs() as f32;
            let bell = (-0.5 * (distance / 3.1).powi(2)).exp();
            let shimmer = 0.82 + (((self.phase + idx) % 7) as f32 * 0.035);
            self.raise_band(idx, kind, amount * bell * shimmer);
        }

        let echo = (center + drift * 3 + (len / 3)).rem_euclid(len) as usize;
        self.raise_band(echo, kind, amount * 0.32);

        self.event_count += 1;
        self.last_event = Some(kind);
    }

    fn raise_band(&mut self, index: usize, kind: EventKind, amount: f32) {
        let band = &mut self.bands[index];
        band.energy = sanitize_level((band.energy + amount).min(1.0));
        band.peak = band.peak.max(band.energy);
        band.last_event = Some(kind);
    }

    fn record_event(&mut self, now: Instant) {
        self.events.push_back(now);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
        self.prune_events(now);
    }

    fn prune_events(&mut self, now: Instant) {
        while self
            .events
            .front()
            .is_some_and(|event_time| now.duration_since(*event_time) > EVENT_WINDOW)
        {
            self.events.pop_front();
        }
    }
}

pub fn sanitize_level(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn key_to_band(code: KeyCode, band_count: usize) -> usize {
    if band_count == 0 {
        return 0;
    }

    let value = match code {
        KeyCode::Char(ch) => ch as usize,
        KeyCode::Enter => 13,
        KeyCode::Backspace => 8,
        KeyCode::Left => 17,
        KeyCode::Right => 19,
        KeyCode::Up => 23,
        KeyCode::Down => 29,
        KeyCode::Home => 31,
        KeyCode::End => 37,
        KeyCode::PageUp => 41,
        KeyCode::PageDown => 43,
        KeyCode::Delete => 47,
        KeyCode::Insert => 53,
        KeyCode::F(n) => 60 + n as usize,
        KeyCode::Tab => 71,
        KeyCode::BackTab => 73,
        KeyCode::Esc => 79,
        _ => 89,
    };

    stable_hash(value) % band_count
}

pub fn dynamic_key_band(code: u16, band_count: usize, phase: usize) -> usize {
    if band_count == 0 {
        return 0;
    }
    stable_hash((code as usize).wrapping_add(phase.rotate_left(3))) % band_count
}

fn stable_hash(value: usize) -> usize {
    value
        .wrapping_mul(2_654_435_761usize)
        .rotate_left(7)
        .wrapping_add(0x9e37_79b9usize)
}

fn is_special_key(code: KeyCode) -> bool {
    !matches!(code, KeyCode::Char(_))
}

fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Enter => String::from("enter"),
        KeyCode::Backspace => String::from("backspace"),
        KeyCode::Left => String::from("left"),
        KeyCode::Right => String::from("right"),
        KeyCode::Up => String::from("up"),
        KeyCode::Down => String::from("down"),
        KeyCode::Home => String::from("home"),
        KeyCode::End => String::from("end"),
        KeyCode::PageUp => String::from("pageup"),
        KeyCode::PageDown => String::from("pagedown"),
        KeyCode::Delete => String::from("delete"),
        KeyCode::Insert => String::from("insert"),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Tab => String::from("tab"),
        KeyCode::BackTab => String::from("backtab"),
        KeyCode::Esc => String::from("esc"),
        _ => String::from("key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_mapping_stays_in_range() {
        for band_count in [1, 2, 8, 80, 240] {
            for code in [
                KeyCode::Char('a'),
                KeyCode::Char('Z'),
                KeyCode::Enter,
                KeyCode::Backspace,
                KeyCode::Left,
                KeyCode::F(12),
            ] {
                assert!(key_to_band(code, band_count) < band_count);
            }
        }
    }

    #[test]
    fn global_code_mapping_stays_in_range() {
        for band_count in [1, 2, 8, 80, 240] {
            for code in [0, 1, 30, 272, u16::MAX] {
                assert!(dynamic_key_band(code, band_count, 37) < band_count);
            }
        }
    }

    #[test]
    fn dynamic_mapping_moves_repeated_keys() {
        let first = dynamic_key_band(30, 80, 0);
        let second = dynamic_key_band(30, 80, 7);
        assert_ne!(first, second);
    }

    #[test]
    fn sanitize_rejects_invalid_values() {
        assert_eq!(sanitize_level(f32::NAN), 0.0);
        assert_eq!(sanitize_level(f32::INFINITY), 0.0);
        assert_eq!(sanitize_level(-0.4), 0.0);
        assert_eq!(sanitize_level(1.4), 1.0);
    }

    #[test]
    fn injection_and_decay_remain_bounded() {
        let mut app = AppState::new(AppConfig {
            bars: 16,
            ..AppConfig::default()
        });
        app.inject_at(4, EventKind::Key, 8.0);
        assert!(app.bands.iter().all(|band| (0.0..=1.0).contains(&band.energy)));

        app.tick(Duration::from_millis(16));
        assert!(app.bands.iter().all(|band| (0.0..=1.0).contains(&band.energy)));
        assert!(app.bands.iter().all(|band| (0.0..=1.0).contains(&band.peak)));
    }

    #[test]
    fn input_rate_prunes_old_events() {
        let mut app = AppState::new(AppConfig::default());
        let now = Instant::now();
        app.record_event(now - Duration::from_millis(1_500));
        app.record_event(now);
        app.prune_events(now);
        assert_eq!(app.events.len(), 1);
    }

    #[test]
    fn control_keys_change_settings_without_injecting_energy() {
        let mut app = AppState::new(AppConfig::default());

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            AppCommand::ControlHandled
        );
        assert_eq!(app.mode, Mode::Wave);
        assert_eq!(app.event_count, 0);
        assert_eq!(app.key_count, 0);
        assert!(app
            .bands
            .iter()
            .all(|band| band.energy == 0.0 && band.peak == 0.0));
    }
}
