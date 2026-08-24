//! What bracket draws: a ladder, a queue, and the trade-off between them.
//!
//! ```text
//!  BRACKET   tick 1240   4113 matches
//!  ┌ LADDER ────────────┐  ┌ CONVERGENCE ──────────────┐
//!  │  1  P31  ███████ 2015│  │ ＼                        │
//!  │  2  P29  ██████  1962│  │   ＼___                   │
//!  │  ...                 │  │       ‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾  │
//!  └──────────────────────┘  └───────────────────────────┘
//!  ┌ QUEUE ─────────────┐  ┌ RECENT ───────────────────┐
//!  │ P07  waited 3       │  │ P12 beat P44   gap 31  +9 │
//!  └──────────────────────┘  └───────────────────────────┘
//! ```
//!
//! # The curve is the claim
//!
//! Every panel here is a readout except one. The convergence plot is the
//! sample's actual argument — a rating system nobody can falsify is a number
//! generator, so the distance between what the ladder believes and what the
//! players are really worth is drawn, falling, on screen. It is one
//! [`DrawList::polyline`] through [`Sim::history`].
//!
//! # Laid out against the surface
//!
//! Every position comes from the extent the swapchain was actually acquired at,
//! so the page is right in a resized window and in the headless offscreen ring
//! at whatever `--size` asked for.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::queue::PlayerId;
use crate::sim::Sim;

// ---- palette -----------------------------------------------------------------

/// The page behind everything.
const BACKDROP: [f32; 4] = [0.09, 0.10, 0.16, 1.0];
/// A panel's fill.
const PANEL: [f32; 4] = [0.13, 0.15, 0.23, 1.0];
/// A panel's border, and the plot's axes.
const EDGE: [f32; 4] = [0.30, 0.34, 0.48, 1.0];
/// Ordinary text.
const INK: [f32; 4] = [0.86, 0.89, 0.96, 1.0];
/// Text that labels rather than reports.
const FAINT: [f32; 4] = [0.52, 0.57, 0.70, 1.0];
/// A rating bar, and the convergence curve.
const ACCENT: [f32; 4] = [0.45, 0.78, 0.94, 1.0];
/// How far a rating is from the truth — the thing being driven down.
const ERROR: [f32; 4] = [0.95, 0.55, 0.42, 1.0];
/// A player currently waiting.
const WAITING: [f32; 4] = [0.96, 0.80, 0.36, 1.0];

// ---- layout ------------------------------------------------------------------

/// The margin around the page and the gap between panels.
const GAP: f32 = 14.0;

/// A panel's title sits this far into its top edge.
const TITLE_INSET: f32 = 8.0;

/// How thick a panel border is.
const BORDER: f32 = 1.0;

/// How wide the convergence curve and the plot's axes are stroked.
const STROKE: f32 = 1.5;

/// How many ladder places the left panel shows before it runs out of room.
const LADDER_ROWS: usize = 14;

/// How many waiting players the queue panel lists.
const QUEUE_ROWS: usize = 5;

/// The rating range the ladder bars are drawn across.
///
/// Wider than the true skill range the population is drawn from, so a ladder
/// that has stretched past it still has somewhere to go rather than clipping
/// silently against the panel edge.
const BAR_FLOOR: f64 = 800.0;
/// See [`BAR_FLOOR`].
const BAR_CEILING: f64 = 2400.0;

/// Draw the whole page. Returns how many commands it emitted, which the debug
/// panel reports and the tests assert is not zero.
pub fn draw(list: &mut DrawList, screen: Vec2, atlas: &FontAtlas, sim: &Sim) -> usize {
    let before = list.len();
    let font = NATURAL_FONT_SIZE;
    let line = font + 5.0;

    list.rect(Vec2::ZERO, screen, BACKDROP);

    // The header, and the two columns under it.
    let header = GAP + font;
    list.text(Vec2::new(GAP, GAP), "BRACKET", INK, font);
    let heading = format!(
        "tick {}   {} matches   {} players",
        sim.tick_count(),
        sim.matches_played(),
        sim.players().len()
    );
    list.text(
        Vec2::new(GAP + atlas.text_width("BRACKET   ", 1.0), GAP),
        heading,
        FAINT,
        font,
    );

    let top = header + GAP;
    let column = ((screen.x - GAP * 3.0) * 0.42).max(1.0);
    let right_x = GAP * 2.0 + column;
    let right_w = (screen.x - GAP - right_x).max(1.0);
    let bottom = (screen.y - GAP).max(top + 1.0);

    // Left: the ladder, full height.
    ladder(
        list,
        atlas,
        sim,
        Vec2::new(GAP, top),
        Vec2::new(column, bottom - top),
        line,
        font,
    );

    // Right: the curve on top, then the readouts, then the queue and results.
    let plot_h = ((bottom - top) * 0.38).max(1.0);
    convergence(
        list,
        atlas,
        sim,
        Vec2::new(right_x, top),
        Vec2::new(right_w, plot_h),
        font,
    );

    let trade_y = top + plot_h + GAP;
    let trade_h = line * 4.0 + GAP;
    trade_off(
        list,
        atlas,
        sim,
        Vec2::new(right_x, trade_y),
        Vec2::new(right_w, trade_h),
        line,
        font,
    );

    let queue_y = trade_y + trade_h + GAP;
    let queue_h = (bottom - queue_y).max(1.0);
    activity(
        list,
        atlas,
        sim,
        Vec2::new(right_x, queue_y),
        Vec2::new(right_w, queue_h),
        line,
        font,
    );

    list.len() - before
}

/// A titled panel. Returns the rectangle left inside it for content.
fn panel(
    list: &mut DrawList,
    atlas: &FontAtlas,
    at: Vec2,
    size: Vec2,
    title: &str,
    font: f32,
) -> (Vec2, Vec2) {
    let max = at + size;
    list.rect(at, max, PANEL);
    list.rect_outline(at, max, BORDER, EDGE);
    // The title sits on the border, so the fill behind it is punched out rather
    // than the text being drawn over a line.
    let width = atlas.text_width(title, 1.0);
    let label = Vec2::new(at.x + TITLE_INSET, at.y - font * 0.5);
    list.rect(
        Vec2::new(label.x - 4.0, label.y),
        Vec2::new(label.x + width + 4.0, label.y + font),
        PANEL,
    );
    list.text(label, title, FAINT, font);

    let inner = Vec2::new(at.x + TITLE_INSET, at.y + font);
    let inner_max = Vec2::new(max.x - TITLE_INSET, max.y - TITLE_INSET * 0.5);
    (inner, inner_max)
}

/// The ladder: who the system currently believes is best.
fn ladder(
    list: &mut DrawList,
    atlas: &FontAtlas,
    sim: &Sim,
    at: Vec2,
    size: Vec2,
    line: f32,
    font: f32,
) {
    let (inner, inner_max) = panel(list, atlas, at, size, "LADDER", font);
    let order = sim.ladder();
    let rows = LADDER_ROWS.min(order.len());
    let bar_left = inner.x + atlas.text_width("00  P000  ", 1.0);
    let bar_right = (inner_max.x - atlas.text_width(" 0000", 1.0)).max(bar_left + 1.0);

    for (place, id) in order.iter().take(rows).enumerate() {
        let y = inner.y + line * place as f32;
        if y + line > inner_max.y {
            break;
        }
        let player = sim.player(*id);
        list.text(
            Vec2::new(inner.x, y),
            format!("{:>2}  {}", place + 1, name(*id)),
            INK,
            font,
        );

        // The bar is the rating; the notch on it is the truth. A ladder that
        // has drifted shows as every notch sitting off the end of its bar.
        let span = (bar_right - bar_left).max(1.0);
        let across = |points: f64| {
            let unit = ((points - BAR_FLOOR) / (BAR_CEILING - BAR_FLOOR)).clamp(0.0, 1.0);
            bar_left + span * unit as f32
        };
        let bar_y = y + font * 0.2;
        let bar_bottom = y + font * 0.85;
        list.rect(
            Vec2::new(bar_left, bar_y),
            Vec2::new(across(player.rating.points()), bar_bottom),
            ACCENT,
        );
        let truth = across(player.skill);
        list.rect(
            Vec2::new(truth - 1.0, y),
            Vec2::new(truth + 1.0, y + font),
            ERROR,
        );

        list.text(
            Vec2::new(bar_right + 4.0, y),
            format!("{:>4.0}", player.rating.points()),
            INK,
            font,
        );
    }

    let legend = inner_max.y - font;
    if legend > inner.y + line * rows as f32 {
        list.text(
            Vec2::new(inner.x, legend),
            "bar = rating    mark = true skill",
            FAINT,
            font,
        );
    }
}

/// The convergence plot: how far the ladder is from the truth, over time.
fn convergence(list: &mut DrawList, atlas: &FontAtlas, sim: &Sim, at: Vec2, size: Vec2, font: f32) {
    let (inner, inner_max) = panel(list, atlas, at, size, "CONVERGENCE", font);
    let history = sim.history();

    let plot_top = inner.y + font * 0.5;
    let plot_bottom = (inner_max.y - font).max(plot_top + 1.0);
    let plot_left = inner.x + atlas.text_width("000 ", 1.0);
    let plot_right = inner_max.x.max(plot_left + 1.0);

    // The axes, drawn as strokes rather than as one-pixel rectangles, so they
    // are the same primitive as the curve and cannot drift apart from it.
    list.line(
        Vec2::new(plot_left, plot_top),
        Vec2::new(plot_left, plot_bottom),
        STROKE,
        EDGE,
    );
    list.line(
        Vec2::new(plot_left, plot_bottom),
        Vec2::new(plot_right, plot_bottom),
        STROKE,
        EDGE,
    );

    // A fixed ceiling rather than one fitted to the data: a curve that rescales
    // itself every frame looks flat however badly it is doing.
    let ceiling = 300.0f32;
    list.text(
        Vec2::new(inner.x, plot_top - font * 0.5),
        format!("{ceiling:.0}"),
        FAINT,
        font,
    );
    list.text(
        Vec2::new(inner.x, plot_bottom - font * 0.5),
        "0",
        FAINT,
        font,
    );

    if history.len() >= 2 {
        let span_x = (plot_right - plot_left).max(1.0);
        let span_y = (plot_bottom - plot_top).max(1.0);
        let last = (history.len() - 1) as f32;
        let points: Vec<Vec2> = history
            .iter()
            .enumerate()
            .map(|(index, error)| {
                let across = index as f32 / last;
                let up = (error / ceiling).clamp(0.0, 1.0);
                Vec2::new(plot_left + span_x * across, plot_bottom - span_y * up)
            })
            .collect();
        list.polyline(points, STROKE, false, ACCENT);
    }

    list.text(
        Vec2::new(plot_left + 4.0, inner_max.y - font),
        format!("{:.0} points from true skill", sim.mean_rating_error()),
        ACCENT,
        font,
    );
}

/// The two numbers the matchmaker trades against each other.
fn trade_off(
    list: &mut DrawList,
    atlas: &FontAtlas,
    sim: &Sim,
    at: Vec2,
    size: Vec2,
    line: f32,
    font: f32,
) {
    let (inner, _) = panel(list, atlas, at, size, "THE TRADE", font);
    let rows = [
        (
            "match quality",
            format!("{:.1} points apart", sim.mean_gap()),
        ),
        ("wait", format!("{:.2} ticks", sim.mean_wait())),
        ("queued now", format!("{}", sim.queue().len())),
    ];
    for (index, (label, value)) in rows.iter().enumerate() {
        let y = inner.y + line * index as f32;
        list.text(Vec2::new(inner.x, y), *label, FAINT, font);
        list.text(
            Vec2::new(inner.x + atlas.text_width("match quality  ", 1.0), y),
            value.clone(),
            INK,
            font,
        );
    }
}

/// Who is waiting, and what just happened.
fn activity(
    list: &mut DrawList,
    atlas: &FontAtlas,
    sim: &Sim,
    at: Vec2,
    size: Vec2,
    line: f32,
    font: f32,
) {
    let (inner, inner_max) = panel(list, atlas, at, size, "QUEUE AND RESULTS", font);
    let mut y = inner.y;

    for entry in sim.queue().entries().iter().take(QUEUE_ROWS) {
        if y + line > inner_max.y {
            return;
        }
        list.text(
            Vec2::new(inner.x, y),
            format!(
                "{} waiting {:>2}   accepts +/-{:.0}",
                name(entry.player),
                entry.waited,
                entry.tolerance()
            ),
            WAITING,
            font,
        );
        y += line;
    }

    y += line * 0.5;
    for report in sim.recent() {
        if y + line > inner_max.y {
            return;
        }
        let (winner, loser) = match report.outcome {
            crate::rating::Outcome::Loss => (report.b, report.a),
            _ => (report.a, report.b),
        };
        list.text(
            Vec2::new(inner.x, y),
            format!(
                "{} beat {}   gap {:>3.0}   {:+.0}",
                name(winner),
                name(loser),
                report.gap,
                report.delta_a.abs()
            ),
            INK,
            font,
        );
        y += line;
    }
}

/// How a player is named on screen.
fn name(player: PlayerId) -> String {
    format!("P{:02}", player.0)
}
