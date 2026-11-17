//! The hours-per-tag bar chart.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use timew_core::interval::is_uncounted_tag;

use crate::app::App;
use crate::format::{decimal_hours, decimal_hours_f, parse_hms};
use crate::ui::{ACCENT, PAD};

/// Colors of the chart bars as (strong, light) pairs from the 256-color
/// palette: strong for a tag's own share of the time, light for the time it
/// shares with other tags. The bars take them in order, top to bottom, so
/// the order is chosen such that every color clearly differs from the ones
/// before and after it (also from the last back to the first, for charts
/// with more bars than colors).
pub(crate) const TAG_COLORS: [(Color, Color); 12] = [
    (Color::Indexed(31), Color::Indexed(117)),  // blue
    (Color::Indexed(166), Color::Indexed(216)), // orange
    (Color::Indexed(28), Color::Indexed(120)),  // green
    (Color::Indexed(127), Color::Indexed(219)), // magenta
    (Color::Indexed(37), Color::Indexed(159)),  // cyan
    (Color::Indexed(124), Color::Indexed(210)), // red
    (Color::Indexed(64), Color::Indexed(193)),  // lime
    (Color::Indexed(55), Color::Indexed(147)),  // purple
    (Color::Indexed(136), Color::Indexed(229)), // olive
    (Color::Indexed(24), Color::Indexed(153)),  // steel blue
    (Color::Indexed(162), Color::Indexed(218)), // pink
    (Color::Indexed(94), Color::Indexed(180)),  // brown
];

/// The chart's name for time without tags.
pub(crate) const NO_TAG: &str = "(no tag)";

/// Gray (strong, light) pair, for "(no tag)" and the legend.
pub(crate) const GRAY_COLORS: (Color, Color) = (Color::Indexed(244), Color::Indexed(250));

/// The (strong, light) colors of each bar of the chart: the palette in
/// order, so that neighboring bars always differ; "(no tag)" is gray (and
/// doesn't use up a palette color).
pub(crate) fn bar_colors(totals: &[TagTotal]) -> Vec<(Color, Color)> {
    let mut next = 0;
    totals
        .iter()
        .map(|t| {
            if t.tag == NO_TAG {
                GRAY_COLORS
            } else {
                next += 1;
                TAG_COLORS[(next - 1) % TAG_COLORS.len()]
            }
        })
        .collect()
}

/// One bar of the hours-per-tag chart (times in seconds).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TagTotal {
    pub(crate) tag: String,
    /// The tag's even share: an entry with n tags gives 1/n of its time to
    /// each of them. The shares of all tags add up to the period's total.
    pub(crate) share: f64,
    /// The rest of the time of entries that have this tag together with
    /// others (their time minus this tag's share of it). `share + shared` is
    /// the full time of all entries that have the tag.
    pub(crate) shared: f64,
}

/// Hours per tag over the rows currently in the list, splitting each entry's
/// time evenly among its tags (see `TagTotal`). Entries without tags count
/// for "(no tag)". An entry with an `_tag` (not counted in the totals)
/// counts only for its `_tags`, so the other bars add up to the total. Each
/// row's own (per-day, range-clipped) time is used, so the numbers match
/// the table. Sorted by share, then shared time, then name.
pub(crate) fn tag_totals(app: &App) -> Vec<TagTotal> {
    let mut totals = std::collections::BTreeMap::<String, (f64, f64)>::new();
    for row in &app.summary.rows {
        let secs = parse_hms(&row.time).unwrap_or(0) as f64;
        let all = &app.tracked[row.source].interval.tags;
        let uncounted: Vec<String> = all.iter().filter(|t| is_uncounted_tag(t)).cloned().collect();
        let tags = if row.counted { all } else { &uncounted };
        if tags.is_empty() {
            totals.entry(NO_TAG.to_string()).or_default().0 += secs;
        }
        let share = secs / tags.len().max(1) as f64;
        for tag in tags {
            let t = totals.entry(tag.clone()).or_default();
            t.0 += share;
            t.1 += secs - share;
        }
    }
    let mut v: Vec<TagTotal> =
        totals.into_iter().map(|(tag, (share, shared))| TagTotal { tag, share, shared }).collect();
    v.sort_by(|a, b| b.share.total_cmp(&a.share).then(b.shared.total_cmp(&a.shared)).then_with(|| a.tag.cmp(&b.tag)));
    v
}

/// Most bars shown in the bottom (narrow-layout) chart panel.
const CHART_MAX_BARS: usize = 10;

/// Height of the bottom chart panel for `totals`.
pub(crate) fn chart_height(totals: &[TagTotal]) -> u16 {
    let bars = totals.len().clamp(1, CHART_MAX_BARS) as u16;
    // bars + an "… more" line if cut off + blank + total line + borders + padding
    let more = u16::from(totals.len() > CHART_MAX_BARS);
    bars + more + 2 + 2 + PAD.top + PAD.bottom
}

/// A stacked bar of `width` cells using eighth blocks: the first `strong_frac`
/// (0..=1) in `strong`, then up to `strong_frac + light_frac` in `light`.
/// The cell where the strong part ends shows it on a light background.
/// Returns styled spans that are exactly `width` cells wide.
pub(crate) fn stacked_bar(strong_frac: f64, light_frac: f64, width: usize, strong: Color, light: Color) -> Vec<Span<'static>> {
    const PARTS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = |f: f64| (f.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let s = eighths(strong_frac);
    let t = eighths(strong_frac + light_frac).max(s);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for cell in 0..width {
        let fill_s = s.saturating_sub(cell * 8).min(8);
        let fill_t = t.saturating_sub(cell * 8).min(8);
        let (text, style) = if fill_s == 8 {
            ("█", Style::new().fg(strong))
        } else if fill_s > 0 && fill_t > fill_s {
            (PARTS[fill_s], Style::new().fg(strong).bg(light))
        } else if fill_s > 0 {
            (PARTS[fill_s], Style::new().fg(strong))
        } else if fill_t == 8 {
            ("█", Style::new().fg(light))
        } else if fill_t > 0 {
            (PARTS[fill_t], Style::new().fg(light))
        } else {
            (" ", Style::new())
        };
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => spans.push(Span::styled(text, style)),
        }
    }
    spans
}

pub(crate) fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let totals = tag_totals(app);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(if app.wide { Line::raw("") } else { Line::from(" b/Esc close ").right_aligned().dim() })
        .padding(PAD);
    let inner = block.inner(area);

    // Fit the bars into the panel: all bars plus the total line if possible,
    // else without the total, else as many bars as fit plus a "… more" line.
    let avail = inner.height as usize;
    let limit = if app.wide { usize::MAX } else { CHART_MAX_BARS };
    let n = totals.len();
    let fits = |bars: usize, more: bool, total: bool| bars + usize::from(more) + 2 * usize::from(total) <= avail;
    let (shown, show_total) = if n <= limit && fits(n, false, true) {
        (n, true)
    } else if n > limit && fits(limit, true, true) {
        (limit, true)
    } else if n <= limit && fits(n, false, false) {
        (n, false)
    } else {
        (avail.saturating_sub(1).min(limit), false)
    };
    // the number of tags only matters when not all of them are shown
    let block = if shown < n {
        block.title(format!(" Hours per tag · {n} "))
    } else {
        block.title(" Hours per tag ")
    };
    // bars are scaled to the largest full time (share + shared)
    let max = totals.iter().map(|t| t.share + t.shared).fold(0.0, f64::max).max(1.0);
    let sum = app.summary.grand_total as f64;
    let any_shared = totals.iter().any(|t| t.shared > 0.0);
    let name_w = totals.iter().take(shown).map(|t| t.tag.chars().count()).max().unwrap_or(0);
    let value_w = totals.iter().take(shown).map(|t| decimal_hours_f(t.share).len()).max().unwrap_or(4);
    let shared_w =
        totals.iter().take(shown).filter(|t| t.shared > 0.0).map(|t| decimal_hours_f(t.shared).len()).max();
    // "name  ███▒▒  12.50 h  +3.25 h   42 %":
    // name + 2, bar, 2 + value + " h", [2 + "+" + shared + " h"], 2 + pct(3) + " %"
    let shared_cols = shared_w.map_or(0, |w| 2 + 1 + w + 2);
    let fixed = name_w + 2 + (2 + value_w + 2) + shared_cols + (2 + 3 + 2);
    let bar_w = (inner.width as usize).saturating_sub(fixed).max(1);

    let colors = bar_colors(&totals);
    let mut lines: Vec<Line> = totals
        .iter()
        .zip(colors)
        .take(shown)
        .map(|(t, (strong, light))| {
            let pct = if sum > 0.0 { t.share * 100.0 / sum } else { 0.0 };
            let mut spans = vec![Span::styled(format!("{:<name_w$}  ", t.tag), Style::new().fg(strong))];
            spans.extend(stacked_bar(t.share / max, t.shared / max, bar_w, strong, light));
            // an `_tag` isn't part of the total: its hours in light gray
            let value_style =
                if is_uncounted_tag(&t.tag) { Style::new().fg(Color::Gray) } else { Style::new().fg(Color::Yellow).bold() };
            spans.push(Span::styled(format!("  {:>value_w$} h", decimal_hours_f(t.share)), value_style));
            if let Some(w) = shared_w {
                let text = if t.shared > 0.0 {
                    format!("  +{:>w$} h", decimal_hours_f(t.shared))
                } else {
                    " ".repeat(2 + 1 + w + 2)
                };
                spans.push(Span::styled(text, Style::new().fg(Color::Gray)));
            }
            // an `_tag` is not part of the total, so it has no percentage
            let pct = if is_uncounted_tag(&t.tag) { "     ".to_string() } else { format!("{pct:>3.0} %") };
            spans.push(Span::styled(format!("  {pct}"), Style::new().dim()));
            Line::from(spans)
        })
        .collect();
    if shown < totals.len() {
        let rest: f64 = totals[shown..].iter().map(|t| t.share).sum();
        lines.push(Line::styled(
            format!("… {} more tags, {} h", totals.len() - shown, decimal_hours_f(rest)),
            Style::new().dim(),
        ));
    }
    if totals.is_empty() {
        lines.push(Line::styled("No entries.", Style::new().dim()));
    }
    if show_total {
        lines.push(Line::raw(""));
        let mut total = vec![
            Span::styled("Total ", Style::new().dim()),
            Span::styled(format!("{} h", decimal_hours(app.summary.grand_total)), Style::new().fg(Color::Yellow).bold()),
        ];
        if any_shared {
            // legend for the two shades
            total.extend([
                Span::raw("    "),
                Span::styled("█", Style::new().fg(GRAY_COLORS.0)),
                Span::styled(" own share  ", Style::new().dim()),
                Span::styled("█", Style::new().fg(GRAY_COLORS.1)),
                Span::styled(" shared (+)", Style::new().dim()),
            ]);
        }
        lines.push(Line::from(total));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
