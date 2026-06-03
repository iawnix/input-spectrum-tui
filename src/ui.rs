use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, EventKind, Mode, Theme};

const NORD0: Color = Color::Rgb(46, 52, 64);
const NORD1: Color = Color::Rgb(59, 66, 82);
const NORD2: Color = Color::Rgb(67, 76, 94);
const NORD3: Color = Color::Rgb(76, 86, 106);
const NORD4: Color = Color::Rgb(216, 222, 233);
const NORD6: Color = Color::Rgb(236, 239, 244);
const NORD7: Color = Color::Rgb(143, 188, 187);
const NORD8: Color = Color::Rgb(136, 192, 208);
const NORD9: Color = Color::Rgb(129, 161, 193);
const NORD10: Color = Color::Rgb(94, 129, 172);
const NORD13: Color = Color::Rgb(235, 203, 139);
const NORD14: Color = Color::Rgb(163, 190, 140);
const NORD15: Color = Color::Rgb(180, 142, 173);

#[derive(Debug, Clone, Copy)]
struct RenderBand {
    energy: f32,
    peak: f32,
    last_event: Option<EventKind>,
}

pub fn draw(frame: &mut Frame<'_>, app: &AppState) {
    draw_spectrum(frame, frame.area(), app);
}

fn draw_spectrum(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(theme_border(app.theme)))
        .style(Style::default().bg(theme_background(app.theme)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 || app.bands.is_empty() {
        return;
    }

    let visible_bars = inner.width as usize;
    let height = inner.height as usize;
    let background = theme_background(app.theme);
    let mut lines = Vec::with_capacity(height);

    for row in 0..height {
        let threshold = (height - row) as f32 / height as f32;
        let mut spans = Vec::with_capacity(visible_bars);

        for column in 0..visible_bars {
            let band = render_band_for_column(app, column, visible_bars);
            let level = level_for_mode(app.mode, band.energy, band.peak, column, visible_bars);
            let peak = band.peak >= threshold && band.peak < threshold + (1.0 / height as f32);
            let base_style = Style::default().bg(background);

            let (glyph, style) = if let Some(glyph) =
                active_glyph(level, threshold, row, column, height)
            {
                (glyph, base_style.fg(event_color(app.theme, band.last_event, level)))
            } else if peak {
                (peak_glyph(column), base_style.fg(theme_peak(app.theme)))
            } else if app.mode == Mode::Wave && near_wave(level, threshold, height) {
                (wave_glyph(row, column), base_style.fg(theme_dim(app.theme)))
            } else {
                (" ", base_style)
            };

            spans.push(Span::styled(glyph, style));
        }

        lines.push(Line::from(spans));
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme_background(app.theme))),
        inner,
    );
}

fn render_band_for_column(app: &AppState, column: usize, width: usize) -> RenderBand {
    let band_count = app.bands.len();
    let start = column * band_count / width.max(1);
    let mut end = ((column + 1) * band_count + width.saturating_sub(1)) / width.max(1);
    end = end.clamp(start + 1, band_count);

    let mut energy: f32 = 0.0;
    let mut peak: f32 = 0.0;
    let mut last_event = None;
    let mut event_energy: f32 = -1.0;

    for band in &app.bands[start..end] {
        energy = energy.max(band.energy);
        peak = peak.max(band.peak);
        if band.energy >= event_energy && band.last_event.is_some() {
            last_event = band.last_event;
            event_energy = band.energy;
        }
    }

    RenderBand {
        energy,
        peak,
        last_event,
    }
}

fn level_for_mode(mode: Mode, energy: f32, peak: f32, column: usize, width: usize) -> f32 {
    match mode {
        Mode::Bars => energy,
        Mode::Peaks => energy.max(peak * 0.72),
        Mode::Wave => {
            let phase = column as f32 / width.max(1) as f32;
            let ripple = ((phase * std::f32::consts::TAU * 2.0).sin() * 0.5 + 0.5) * 0.10;
            (energy.powf(0.82) * 0.90 + ripple).clamp(0.0, 1.0)
        }
    }
}

fn near_wave(level: f32, threshold: f32, height: usize) -> bool {
    (level - threshold).abs() <= (1.0 / height.max(1) as f32) * 0.65
}

fn active_glyph(
    level: f32,
    threshold: f32,
    row: usize,
    column: usize,
    height: usize,
) -> Option<&'static str> {
    if level < threshold {
        return None;
    }

    let row_step = 1.0 / height.max(1) as f32;
    let depth = ((level - threshold) / row_step).clamp(0.0, 5.0);
    let dither = ((row * 17 + column * 29) % 11) as f32 / 10.0;
    let bucket = ((depth + dither) * 1.45).floor() as usize;
    let glyph = match bucket.min(7) {
        0 => "·",
        1 => "▪",
        2 => "▖",
        3 => "▄",
        4 => "▙",
        5 => "▛",
        6 => "▓",
        _ => "█",
    };

    Some(glyph)
}

fn peak_glyph(column: usize) -> &'static str {
    match column % 4 {
        0 => "▀",
        1 => "▔",
        2 => "▝",
        _ => "▘",
    }
}

fn wave_glyph(row: usize, column: usize) -> &'static str {
    match (row + column) % 5 {
        0 => "·",
        1 => "∙",
        2 => "⋅",
        3 => "˙",
        _ => " ",
    }
}

fn theme_background(theme: Theme) -> Color {
    match theme {
        Theme::Nord => NORD0,
        Theme::Mono => Color::Black,
        Theme::Amber => Color::Rgb(32, 24, 16),
    }
}

fn theme_border(theme: Theme) -> Color {
    match theme {
        Theme::Nord => NORD1,
        Theme::Mono => NORD2,
        Theme::Amber => Color::Rgb(94, 61, 38),
    }
}

fn theme_peak(theme: Theme) -> Color {
    match theme {
        Theme::Nord => NORD15,
        Theme::Mono => NORD4,
        Theme::Amber => NORD13,
    }
}

fn theme_dim(theme: Theme) -> Color {
    match theme {
        Theme::Nord => NORD3,
        Theme::Mono => NORD2,
        Theme::Amber => Color::Rgb(111, 76, 47),
    }
}

fn event_color(theme: Theme, event: Option<EventKind>, level: f32) -> Color {
    match theme {
        Theme::Nord => match event {
            Some(EventKind::Key) if level > 0.86 => NORD6,
            Some(EventKind::Key) if level > 0.64 => NORD8,
            Some(EventKind::Key) if level > 0.42 => NORD7,
            Some(EventKind::Key) => NORD9,
            Some(EventKind::SpecialKey) if level > 0.70 => NORD14,
            Some(EventKind::SpecialKey) => NORD13,
            None if level > 0.58 => NORD8,
            None => NORD10,
        },
        Theme::Mono => match event {
            Some(EventKind::SpecialKey) => NORD4,
            _ => NORD6,
        },
        Theme::Amber => match event {
            Some(EventKind::Key) if level > 0.72 => NORD13,
            Some(EventKind::Key) => Color::Rgb(208, 135, 112),
            Some(EventKind::SpecialKey) => Color::Rgb(191, 97, 106),
            None => Color::Rgb(180, 104, 32),
        },
    }
}
