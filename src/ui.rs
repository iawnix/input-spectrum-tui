use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, EventKind, Mode, Theme};

pub fn draw(frame: &mut Frame<'_>, app: &AppState) {
    draw_spectrum(frame, frame.area(), app);
}

fn draw_spectrum(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(theme_border(app.theme)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 || app.bands.is_empty() {
        return;
    }

    let visible_bars = inner.width as usize;
    let band_count = app.bands.len();
    let height = inner.height as usize;
    let mut lines = Vec::with_capacity(height);

    for row in 0..height {
        let threshold = (height - row) as f32 / height as f32;
        let mut spans = Vec::with_capacity(visible_bars);

        for column in 0..visible_bars {
            let band_index = column * band_count / visible_bars;
            let band = &app.bands[band_index];
            let level = level_for_mode(app.mode, band.energy, band.peak, column, visible_bars);
            let selected = app.selected_band == Some(band_index);
            let active = level >= threshold;
            let peak = band.peak >= threshold && band.peak < threshold + (1.0 / height as f32);

            let (glyph, style) = if selected && active {
                ("█", Style::default().fg(Color::White).bg(theme_accent(app.theme)))
            } else if active {
                ("█", Style::default().fg(event_color(app.theme, band.last_event)))
            } else if peak {
                ("▀", Style::default().fg(theme_peak(app.theme)))
            } else if app.mode == Mode::Wave && near_wave(level, threshold, height) {
                ("·", Style::default().fg(theme_dim(app.theme)))
            } else {
                (" ", Style::default())
            };

            spans.push(Span::styled(glyph, style));
        }

        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn level_for_mode(mode: Mode, energy: f32, peak: f32, column: usize, width: usize) -> f32 {
    match mode {
        Mode::Bars => energy,
        Mode::Peaks => energy.max(peak * 0.72),
        Mode::Wave => {
            let phase = column as f32 / width.max(1) as f32;
            let ripple = (phase * std::f32::consts::TAU).sin().abs() * 0.12;
            (energy * 0.88 + ripple).clamp(0.0, 1.0)
        }
    }
}

fn near_wave(level: f32, threshold: f32, height: usize) -> bool {
    (level - threshold).abs() <= (1.0 / height.max(1) as f32) * 0.65
}

fn theme_accent(theme: Theme) -> Color {
    match theme {
        Theme::Cyber => Color::Cyan,
        Theme::Mono => Color::White,
        Theme::Amber => Color::Yellow,
    }
}

fn theme_border(theme: Theme) -> Color {
    match theme {
        Theme::Cyber => Color::Blue,
        Theme::Mono => Color::DarkGray,
        Theme::Amber => Color::Rgb(180, 104, 32),
    }
}

fn theme_peak(theme: Theme) -> Color {
    match theme {
        Theme::Cyber => Color::Magenta,
        Theme::Mono => Color::Gray,
        Theme::Amber => Color::LightRed,
    }
}

fn theme_dim(theme: Theme) -> Color {
    match theme {
        Theme::Cyber => Color::DarkGray,
        Theme::Mono => Color::DarkGray,
        Theme::Amber => Color::Rgb(96, 64, 32),
    }
}

fn event_color(theme: Theme, event: Option<EventKind>) -> Color {
    match theme {
        Theme::Cyber => match event {
            Some(EventKind::Key) => Color::Cyan,
            Some(EventKind::SpecialKey) => Color::LightBlue,
            Some(EventKind::Click) => Color::Magenta,
            Some(EventKind::Drag) => Color::LightMagenta,
            Some(EventKind::Move) => Color::Green,
            Some(EventKind::Wheel) => Color::Yellow,
            None => Color::Blue,
        },
        Theme::Mono => match event {
            Some(EventKind::Click | EventKind::Wheel) => Color::White,
            Some(EventKind::Move | EventKind::Drag) => Color::Gray,
            _ => Color::White,
        },
        Theme::Amber => match event {
            Some(EventKind::Key) => Color::Yellow,
            Some(EventKind::SpecialKey) => Color::LightYellow,
            Some(EventKind::Click) => Color::LightRed,
            Some(EventKind::Drag) => Color::Red,
            Some(EventKind::Move) => Color::Rgb(210, 140, 60),
            Some(EventKind::Wheel) => Color::White,
            None => Color::Rgb(180, 104, 32),
        },
    }
}
