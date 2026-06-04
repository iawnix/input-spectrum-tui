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
        let mut spans = Vec::with_capacity(visible_bars);

        for column in 0..visible_bars {
            let band = render_band_for_column(app, column, visible_bars);
            let base_style = Style::default().bg(background);
            let (glyph, style) =
                render_cell(app, band, row, column, height, visible_bars, base_style);

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

fn render_cell(
    app: &AppState,
    band: RenderBand,
    row: usize,
    column: usize,
    height: usize,
    width: usize,
    base_style: Style,
) -> (&'static str, Style) {
    match app.mode {
        Mode::Bars => render_bar_cell(app, band, row, column, height, base_style),
        Mode::Wave => render_wave_cell(app, band, row, column, height, width, base_style),
        Mode::Peaks => render_peak_cell(app, band, row, column, height, base_style),
    }
}

fn render_bar_cell(
    app: &AppState,
    band: RenderBand,
    row: usize,
    column: usize,
    height: usize,
    base_style: Style,
) -> (&'static str, Style) {
    let threshold = threshold_for_row(row, height);
    let peak = band.peak >= threshold && band.peak < threshold + row_step(height);

    if let Some(glyph) = active_glyph(band.energy, threshold, row, column, height) {
        (glyph, base_style.fg(event_color(app.theme, band.last_event, band.energy)))
    } else if peak {
        (peak_glyph(column), base_style.fg(theme_peak(app.theme)))
    } else {
        (" ", base_style)
    }
}

fn render_wave_cell(
    app: &AppState,
    band: RenderBand,
    row: usize,
    column: usize,
    height: usize,
    width: usize,
    base_style: Style,
) -> (&'static str, Style) {
    let threshold = threshold_for_row(row, height);
    let contour = wave_contour(band.energy, column, width);
    let width = row_step(height) * 0.85;
    let distance = (contour - threshold).abs();

    if distance <= width {
        let glyph = wave_glyph(row, column, distance / width.max(f32::EPSILON));
        (glyph, base_style.fg(event_color(app.theme, band.last_event, band.energy)))
    } else if band.peak > 0.08 && (band.peak - threshold).abs() <= width * 0.70 {
        (peak_glyph(column), base_style.fg(theme_dim(app.theme)))
    } else {
        (" ", base_style)
    }
}

fn render_peak_cell(
    app: &AppState,
    band: RenderBand,
    row: usize,
    column: usize,
    height: usize,
    base_style: Style,
) -> (&'static str, Style) {
    let threshold = threshold_for_row(row, height);
    let step = row_step(height);
    let peak_line = band.peak >= threshold && band.peak < threshold + step;
    let energy_line = band.energy >= threshold && band.energy < threshold + step * 1.8;
    let floor_trace = band.energy > 0.20 && threshold < (band.energy * 0.22).max(step);

    if peak_line {
        (peak_glyph(column), base_style.fg(theme_peak(app.theme)))
    } else if energy_line {
        ("╷", base_style.fg(event_color(app.theme, band.last_event, band.energy)))
    } else if floor_trace {
        ("·", base_style.fg(theme_dim(app.theme)))
    } else {
        (" ", base_style)
    }
}

fn threshold_for_row(row: usize, height: usize) -> f32 {
    (height - row) as f32 / height.max(1) as f32
}

fn row_step(height: usize) -> f32 {
    1.0 / height.max(1) as f32
}

fn wave_contour(energy: f32, column: usize, width: usize) -> f32 {
    let phase = column as f32 / width.max(1) as f32;
    let carrier = (phase * std::f32::consts::TAU * 2.0).sin() * 0.5 + 0.5;
    let harmonic = (phase * std::f32::consts::TAU * 5.0).sin() * 0.5 + 0.5;
    (energy.powf(0.72) * 0.84 + carrier * 0.10 + harmonic * 0.04).clamp(0.0, 1.0)
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

fn wave_glyph(row: usize, column: usize, distance_ratio: f32) -> &'static str {
    if distance_ratio < 0.34 {
        match (row + column) % 4 {
            0 => "╱",
            1 => "━",
            2 => "╲",
            _ => "─",
        }
    } else {
        match (row + column) % 4 {
            0 => "·",
            1 => "∙",
            2 => "⋅",
            _ => "˙",
        }
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
