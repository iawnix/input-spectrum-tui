use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

const EVENT_WINDOW: Duration = Duration::from_secs(1);
const MAX_EVENTS: usize = 512;
const PEAK_DECAY: f32 = 0.86;
const ENERGY_DECAY_BASE: f32 = 0.78;
const MOVE_EVENT_SCALE: f32 = 0.32;
const CLICK_EVENT_SCALE: f32 = 1.25;
const KEY_EVENT_SCALE: f32 = 1.0;
const WHEEL_EVENT_SCALE: f32 = 1.6;

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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bars => "bars",
            Self::Wave => "wave",
            Self::Peaks => "peaks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Cyber,
    Mono,
    Amber,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cyber => "cyber",
            Self::Mono => "mono",
            Self::Amber => "amber",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Key,
    SpecialKey,
    Click,
    Drag,
    Move,
    Wheel,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::SpecialKey => "special",
            Self::Click => "click",
            Self::Drag => "drag",
            Self::Move => "move",
            Self::Wheel => "wheel",
        }
    }
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
            bars: 80,
            fps: 30,
            theme: Theme::Cyber,
            mode: Mode::Bars,
        }
    }
}

#[derive(Debug)]
pub enum AppCommand {
    None,
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
    pub mouse_count: u64,
    pub selected_band: Option<usize>,
    pub mouse_position: Option<(u16, u16)>,
    pub last_event: Option<EventKind>,
    pub last_key_label: String,
    events: VecDeque<Instant>,
    last_mouse_position: Option<(u16, u16)>,
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
            mouse_count: 0,
            selected_band: None,
            mouse_position: None,
            last_event: None,
            last_key_label: String::from("-"),
            events: VecDeque::with_capacity(MAX_EVENTS),
            last_mouse_position: None,
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

    pub fn input_rate(&self) -> usize {
        self.events.len()
    }

    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return AppCommand::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return AppCommand::Quit;
            }
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                self.inject_fixed(4, EventKind::SpecialKey, 1.4);
                self.last_key_label = String::from("space");
            }
            KeyCode::Tab => {
                self.mode = self.mode.next();
                self.inject_fixed(7, EventKind::SpecialKey, 1.4);
                self.last_key_label = String::from("tab");
            }
            KeyCode::Char('1') => {
                self.theme = Theme::Cyber;
                self.inject_fixed(1, EventKind::SpecialKey, 1.2);
                self.last_key_label = String::from("1");
            }
            KeyCode::Char('2') => {
                self.theme = Theme::Mono;
                self.inject_fixed(2, EventKind::SpecialKey, 1.2);
                self.last_key_label = String::from("2");
            }
            KeyCode::Char('3') => {
                self.theme = Theme::Amber;
                self.inject_fixed(3, EventKind::SpecialKey, 1.2);
                self.last_key_label = String::from("3");
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.sensitivity = (self.sensitivity + 0.1).min(3.0);
                self.inject_fixed(5, EventKind::SpecialKey, 1.0);
                self.last_key_label = String::from("+");
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.sensitivity = (self.sensitivity - 0.1).max(0.2);
                self.inject_fixed(6, EventKind::SpecialKey, 1.0);
                self.last_key_label = String::from("-");
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

    pub fn handle_mouse(&mut self, mouse: MouseEvent, width: u16) {
        self.mouse_position = Some((mouse.column, mouse.row));
        let band = mouse_to_band(mouse.column, width, self.bands.len());

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::Down(MouseButton::Right)
            | MouseEventKind::Down(MouseButton::Middle) => {
                self.selected_band = Some(band);
                self.inject_at(band, EventKind::Click, CLICK_EVENT_SCALE);
                self.mouse_count += 1;
            }
            MouseEventKind::Drag(_) => {
                self.selected_band = Some(band);
                self.inject_at(band, EventKind::Drag, 0.85);
                self.inject_neighbors(band, EventKind::Drag, 0.35, 3);
                self.mouse_count += 1;
            }
            MouseEventKind::Moved => {
                let motion = self
                    .last_mouse_position
                    .map(|(last_x, last_y)| {
                        let dx = mouse.column.abs_diff(last_x);
                        let dy = mouse.row.abs_diff(last_y);
                        (dx + dy).min(20) as f32 / 20.0
                    })
                    .unwrap_or(0.15);
                self.inject_at(band, EventKind::Move, MOVE_EVENT_SCALE + motion * 0.65);
            }
            MouseEventKind::ScrollUp => {
                self.inject_sweep(band, -1, WHEEL_EVENT_SCALE);
                self.mouse_count += 1;
            }
            MouseEventKind::ScrollDown => {
                self.inject_sweep(band, 1, WHEEL_EVENT_SCALE);
                self.mouse_count += 1;
            }
            MouseEventKind::ScrollLeft => {
                self.inject_sweep(0, 1, WHEEL_EVENT_SCALE * 0.8);
                self.mouse_count += 1;
            }
            MouseEventKind::ScrollRight => {
                let last = self.bands.len().saturating_sub(1);
                self.inject_sweep(last, -1, WHEEL_EVENT_SCALE * 0.8);
                self.mouse_count += 1;
            }
            MouseEventKind::Up(_) => {}
        }

        self.last_mouse_position = Some((mouse.column, mouse.row));
    }

    fn inject_fixed(&mut self, seed: usize, kind: EventKind, amount: f32) {
        let band = if self.bands.is_empty() {
            0
        } else {
            (seed * self.bands.len() / 10).min(self.bands.len() - 1)
        };
        self.inject_at(band, kind, amount);
    }

    pub fn inject_at(&mut self, band_index: usize, kind: EventKind, amount: f32) {
        if self.bands.is_empty() {
            return;
        }

        let now = Instant::now();
        self.record_event(now);
        let rate_boost = (self.events.len() as f32 / 24.0).min(1.4);
        let amount = amount * self.sensitivity * (1.0 + rate_boost);
        let band_index = band_index.min(self.bands.len() - 1);

        self.raise_band(band_index, kind, amount);
        self.inject_neighbors(band_index, kind, amount * 0.36, 2);

        self.event_count += 1;
        self.last_event = Some(kind);
    }

    fn inject_neighbors(&mut self, center: usize, kind: EventKind, amount: f32, radius: usize) {
        for offset in 1..=radius {
            let attenuated = amount / (offset as f32 + 1.0);
            if center >= offset {
                self.raise_band(center - offset, kind, attenuated);
            }
            if center + offset < self.bands.len() {
                self.raise_band(center + offset, kind, attenuated);
            }
        }
    }

    fn inject_sweep(&mut self, start: usize, direction: i8, amount: f32) {
        let len = self.bands.len();
        if len == 0 {
            return;
        }

        let start = start.min(len - 1);
        for step in 0..8 {
            let idx = if direction < 0 {
                start.saturating_sub(step)
            } else {
                (start + step).min(len - 1)
            };
            let falloff = 1.0 - step as f32 / 10.0;
            self.raise_band(idx, EventKind::Wheel, amount * falloff);
        }

        self.record_event(Instant::now());
        self.event_count += 1;
        self.last_event = Some(EventKind::Wheel);
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

pub fn mouse_to_band(column: u16, width: u16, band_count: usize) -> usize {
    if band_count == 0 || width == 0 {
        return 0;
    }
    let normalized = column.min(width.saturating_sub(1)) as usize;
    (normalized * band_count / width as usize).min(band_count - 1)
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
    fn mouse_mapping_stays_in_range() {
        for width in [1, 2, 80, 120] {
            for column in [0, 1, 30, 79, 300] {
                let band = mouse_to_band(column, width, 64);
                assert!(band < 64);
            }
        }
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
        assert_eq!(app.input_rate(), 1);
    }
}
