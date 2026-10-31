//! Frame rendering (PLAN-30 §C.2): two panes + status bar + key hint bar.
//!
//! Everything here is a pure function of [`App`] state onto a ratatui
//! frame, so the whole UI is golden-testable against a `TestBackend`
//! (C.5) — no terminal, no daemon, no clock.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use pixcore::ResolvedSettings;
use pixcore::preview::preview_canvas;

use super::App;

/// The static key hint bar (C.2). `rm` is the two-key remove binding;
/// S7 decides its exact wiring (see mod.rs key handling).
pub const HINTS: &str = "a add · s set · c cycle · p pixel · l place · b bg · \
                        g gutter · t palette · d dither · r refresh · x export · \
                        rm remove · q";

/// One library row: `▸ id title pixel placement` — `▸` marks
/// `library.current` (the selection is drawn as a highlight, not a
/// marker). Title absorbs the slack; pixel/placement stay whole.
pub fn list_row(
    id: &str,
    title: &str,
    current: bool,
    pixel: &str,
    placement: &str,
    inner_w: u16,
) -> String {
    let mark = if current { "▸" } else { " " };
    // mark + 4 separators + id(8) + pixel + placement are fixed; the
    // title absorbs the rest (the row must fit inner_w exactly).
    let fixed = 1 + 4 + 8 + pixel.chars().count() + placement.chars().count();
    let title_w = (inner_w as usize).saturating_sub(fixed).max(1);
    let t: String = title.chars().take(title_w).collect();
    format!("{mark} {id:<8.8} {t:<title_w$} {pixel} {placement}")
}

/// The always-visible caption block (C.2): output size, pixel, placement,
/// palette, dither, bg, gutter — the exact `ResolvedSettings` the daemon
/// would use, at the preview canvas size.
pub fn caption_lines(r: &ResolvedSettings, canvas: (u32, u32), enabled: bool) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{}×{} · pixel {} · {}",
            canvas.0, canvas.1, r.pixel, r.placement
        ),
        format!("palette {} · dither {}", r.palette, r.dither),
        format!("bg {} · gutter {}px", r.bg_color, r.gutter_px),
    ];
    if !enabled {
        lines.push("pixelation OFF — originals (placement kept)".into());
    }
    lines
}

/// The honest no-image line (C.3): points at the export that *would*
/// show the same pixels. No fake art.
pub fn fallback_line(id: &str, canvas: (u32, u32)) -> String {
    format!(
        "image preview unsupported here — pixpapers export {id} {}x{} -o /tmp/prev.png",
        canvas.0, canvas.1
    )
}

/// Status bar segments (C.2): daemon probe, image-mode result (C3 adds
/// the `img:` segment), last action result — one line, most recent wins.
pub fn status_spans(app: &App) -> Vec<Span<'static>> {
    let daemon = if app.daemon_up {
        "daemon:up"
    } else {
        "daemon:down"
    };
    vec![
        Span::from(daemon.to_string()),
        Span::from(" · "),
        Span::from(app.last.clone()),
    ]
}

pub fn frame(f: &mut Frame, app: &App) {
    let area = f.area();
    let [body, status, hints] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

    // ── status bar + hint bar ──────────────────────────────────────────
    f.render_widget(Paragraph::new(Line::from(status_spans(app))), status);
    let hint: String = HINTS.chars().take(area.width as usize).collect();
    f.render_widget(
        Paragraph::new(hint).style(Style::default().add_modifier(Modifier::DIM)),
        hints,
    );

    // ── the two panes (right ≈ 60%) ────────────────────────────────────
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .areas(body);

    render_library(f, app, left);
    render_preview(f, app, right);
}

fn render_library(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" library ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.lib.entries.is_empty() {
        f.render_widget(
            Paragraph::new("library empty — add one with: pixpapers add <image>")
                .style(Style::default().add_modifier(Modifier::DIM)),
            inner,
        );
        return;
    }

    let mut lines = Vec::with_capacity(app.lib.entries.len());
    for (i, e) in app.lib.entries.iter().enumerate() {
        let current = app.lib.current.as_deref() == Some(e.id.as_str());
        let r = ResolvedSettings::resolve(&e.settings, &app.cfg);
        let text = list_row(
            &e.id,
            &e.title,
            current,
            &r.pixel.to_string(),
            &r.placement.to_string(),
            inner.width,
        );
        let mut style = Style::default();
        if !app.cfg.enabled {
            // R6 off: every entry renders as the original — dim the list
            style = style.add_modifier(Modifier::DIM);
        }
        if i == app.sel {
            style = style
                .bg(ratatui::style::Color::Gray)
                .fg(ratatui::style::Color::Black);
        }
        lines.push(Line::styled(text, style));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_preview(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" preview ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(entry) = app.selected() else {
        f.render_widget(
            Paragraph::new("no entry selected").style(Style::default().add_modifier(Modifier::DIM)),
            inner,
        );
        return;
    };

    let canvas = preview_canvas(inner.width as u32, inner.height as u32);
    let r = crate::commands::resolve_effective(entry, &app.cfg);
    let mut lines: Vec<Line> = caption_lines(&r, canvas, app.cfg.enabled)
        .into_iter()
        .map(Line::from)
        .collect();

    // Text fallback (C.3): caption + one honest line pointing at export.
    // (The image-capable branch arrives with detection.) The line wraps —
    // in fallback mode nothing else competes for the pane.
    lines.push(Line::from(""));
    lines.push(
        Line::from(fallback_line(&entry.id, canvas))
            .style(Style::default().add_modifier(Modifier::DIM)),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_row_marks_current_and_fits() {
        let row = list_row("4a1c9f01", "nebula", true, "16", "fit", 38);
        assert!(row.starts_with("▸ 4a1c9f01 nebula"), "{row}");
        assert!(row.ends_with("16 fit"));
        assert_eq!(row.chars().count(), 38);
        // non-current keeps the column grid
        let row2 = list_row("7c2e10ab", "dunes", false, "32", "cover-crop", 38);
        assert!(row2.starts_with("  7c2e10ab dunes"), "{row2}");
        // long titles truncate, never overflow
        let row3 = list_row(
            "aabbccdd",
            "an extremely long wallpaper title",
            true,
            "normal",
            "cover-gutter",
            38,
        );
        assert_eq!(row3.chars().count(), 38, "{row3}");
    }

    #[test]
    fn caption_names_every_knob() {
        let r = ResolvedSettings::resolve(&Default::default(), &Default::default());
        let lines = caption_lines(&r, (116, 28), true);
        assert_eq!(lines[0], "116×28 · pixel 16 · fit");
        assert_eq!(lines[1], "palette auto · dither none");
        assert_eq!(lines[2], "bg #000000 · gutter 24px");
        assert_eq!(lines.len(), 3);
        // the R6-off note rides along when pixelation is disabled
        assert_eq!(caption_lines(&r, (116, 28), false).len(), 4);
    }

    #[test]
    fn fallback_line_points_at_export() {
        let l = fallback_line("4a1c9f01", (116, 28));
        assert_eq!(
            l,
            "image preview unsupported here — pixpapers export 4a1c9f01 116x28 -o /tmp/prev.png"
        );
    }
}
