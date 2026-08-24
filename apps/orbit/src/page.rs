//! The flight instruments and the map: the whole of what this sample draws.
//!
//! ```text
//!  ┌ flight ───────┐                 ·  ·  ·
//!  │ PHASE  FLYING │            ·                ·
//!  │ ALT    82 km  │         ·        ▟▛▜▙        ·
//!  │ VEL   2246 m/s│        ·        ▟█████▙       ·
//!  │ APO   101 km  │        ·        ▜█████▛      ·
//!  │ PERI   98 km  │         ·        ▝▜▛▘       ·
//!  │ FUEL  ▰▰▱▱▱▱  │            ·                ·
//!  └───────────────┘                 ·  ·  ·
//!                        W/S throttle   A/D turn   . , warp
//! ```
//!
//! # Why the orbit is a row of dots
//!
//! [`DrawList`] draws axis-aligned rectangles and text, and the engine has no
//! line primitive anywhere — so a curve is drawn as the points it was sampled
//! at. That suits this one: the samples *are* the propagator's answers, spread
//! evenly in **time** rather than in angle, so they bunch up at apoapsis and
//! spread out at periapsis and the picture shows where the ship spends its
//! orbit. `docs/backlog.md` carries the line primitive the 3D milestone wants.
//!
//! The planet and its atmosphere are filled discs, drawn as a stack of
//! horizontal rectangles — the one shape a rectangle primitive can fill exactly
//! at the row boundaries.
//!
//! # Laid out against the surface
//!
//! Every position is derived from the extent the swapchain was actually
//! acquired at, so the page is correct in a resized window and in the headless
//! offscreen ring at whatever `--size` asked for.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::game::{Phase, RenderState};

// ---- palette -----------------------------------------------------------------

/// What the frame is cleared to behind the page: space.
pub const BACKDROP: [f32; 4] = [0.02, 0.02, 0.05, 1.0];

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.88];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
const TRACK: [f32; 4] = [0.14, 0.15, 0.20, 1.0];
const FUEL_FILL: [f32; 4] = [0.92, 0.72, 0.24, 1.0];
const THROTTLE_FILL: [f32; 4] = [0.36, 0.80, 0.52, 1.0];
const GROUND: [f32; 4] = [0.22, 0.34, 0.26, 1.0];
const SKY: [f32; 4] = [0.16, 0.28, 0.46, 0.55];
const PATH: [f32; 4] = [0.44, 0.62, 0.90, 0.85];
const SHIP: [f32; 4] = [1.0, 0.94, 0.72, 1.0];
const FLAME: [f32; 4] = [1.0, 0.56, 0.20, 1.0];
const WARNING: [f32; 4] = [0.95, 0.42, 0.36, 1.0];

// ---- layout ------------------------------------------------------------------

/// How many horizontal rows a filled disc is drawn from.
///
/// Enough that the edge reads as a curve at any window this sample opens at,
/// and few enough that two discs are a hundred and some rectangles rather than
/// a mesh.
const DISC_ROWS: usize = 72;

/// The side of the square the ship and each path sample are drawn as, in
/// pixels.
const SHIP_SIZE: f32 = 5.0;
/// See [`SHIP_SIZE`].
const PATH_SIZE: f32 = 2.0;

/// How much of the smaller screen dimension the whole map fits inside.
const MAP_FILL: f32 = 0.82;

/// The instrument panel's inset from the top-left, and the gap between rows.
const PANEL_INSET: f32 = 18.0;
/// See [`PANEL_INSET`].
const ROW_HEIGHT: f32 = 20.0;
/// See [`PANEL_INSET`].
const PANEL_PAD: f32 = 12.0;
/// See [`PANEL_INSET`].
const PANEL_WIDTH: f32 = 208.0;

/// How wide the label column is inside the panel, in pixels.
const LABEL_WIDTH: f32 = 74.0;

/// The rows the instrument panel prints, top to bottom.
const ROWS: usize = 9;

/// What the page drew, for the loop's own tests and its summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// Draws the whole page into `list`, laid out against a surface of `extent`.
///
/// `atlas` is only measured against — the glyphs themselves are the UI pass's
/// business — and it is what right-aligns the readouts against a proportional
/// font rather than against a guess.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    state: &RenderState,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    draw_map(list, width, height, state);
    draw_panel(list, atlas, state);
    draw_hint(list, atlas, width, height, state);

    PageStats {
        commands: list.len(),
    }
}

// ---- the map -----------------------------------------------------------------

/// Metres to pixels, and where the body's centre sits on screen.
struct Map {
    centre: Vec2,
    scale: f32,
}

impl Map {
    /// Where a point in the frame's plane lands on screen.
    ///
    /// `y` is negated because the simulation's `+y` is away from the body and
    /// the surface's `+y` is down the screen.
    fn at(&self, point: [f64; 2]) -> Vec2 {
        Vec2::new(
            self.centre.x + point[0] as f32 * self.scale,
            self.centre.y - point[1] as f32 * self.scale,
        )
    }
}

/// Fits the body, the ship and the whole trajectory into the window.
fn fit(width: f32, height: f32, state: &RenderState) -> Map {
    let mut extent = state.body_radius * 1.25;
    let mut consider = |point: [f64; 2]| {
        let distance = (point[0] * point[0] + point[1] * point[1]).sqrt();
        if distance > extent && distance.is_finite() {
            extent = distance;
        }
    };
    consider(state.ship);
    for point in &state.path {
        consider(*point);
    }

    let span = width.min(height) * MAP_FILL;
    Map {
        centre: Vec2::new(width * 0.5, height * 0.5),
        // `extent` is a radius, so the fitted span is twice it.
        scale: span / (2.0 * extent as f32),
    }
}

fn draw_map(list: &mut DrawList, width: f32, height: f32, state: &RenderState) {
    let map = fit(width, height, state);

    // The atmosphere first, so the planet is drawn over it and the shell shows
    // only as a halo. The moon has none, and `body_radius` is what tells them
    // apart without this file knowing which body it is drawing.
    if state.body == "PLANET" {
        let shell = (state.body_radius + crate::game::AIR.ceiling) as f32 * map.scale;
        disc(list, map.centre, shell, SKY);
    }
    disc(
        list,
        map.centre,
        state.body_radius as f32 * map.scale,
        GROUND,
    );

    for point in &state.path {
        let at = map.at(*point);
        dot(list, at, PATH_SIZE, PATH);
    }

    // The engine's plume, drawn before the ship so the ship sits on top of it:
    // a short line of dots out the back, as long as the throttle is open.
    let ship = map.at(state.ship);
    if state.throttle > 0.0 && state.fuel > 0.0 {
        let back = Vec2::new(-state.attitude[0] as f32, state.attitude[1] as f32);
        let reach = SHIP_SIZE * 4.0 * state.throttle as f32;
        for step in 1..=4 {
            let along = reach * step as f32 / 4.0;
            dot(
                list,
                Vec2::new(ship.x + back.x * along, ship.y + back.y * along),
                PATH_SIZE,
                FLAME,
            );
        }
    }
    dot(list, ship, SHIP_SIZE, SHIP);
}

/// A filled disc of `radius` pixels, as a stack of horizontal rectangles.
///
/// Skipped entirely below a pixel: a disc smaller than that is a rounding
/// error's worth of rows, and the ship's own marker is already drawn.
fn disc(list: &mut DrawList, centre: Vec2, radius: f32, color: [f32; 4]) {
    // A NaN radius — from a body whose scale came out of a degenerate orbit —
    // lands here rather than in the loop, where it would reach the vertex
    // buffer and take the whole draw with it.
    if radius.is_nan() || radius < 1.0 {
        return;
    }
    let rows = DISC_ROWS as f32;
    for row in 0..DISC_ROWS {
        // The row's top and bottom as fractions of the diameter, so successive
        // rows share an edge exactly and the disc has no seams in it.
        let top = radius * (2.0 * row as f32 / rows - 1.0);
        let bottom = radius * (2.0 * (row + 1) as f32 / rows - 1.0);
        // Half-width at whichever edge is nearer the equator, so the rows
        // circumscribe the circle and the silhouette has no notches.
        let nearest = if top.abs() < bottom.abs() {
            top
        } else {
            bottom
        };
        let half = (radius * radius - nearest * nearest).max(0.0).sqrt();
        list.rect(
            Vec2::new(centre.x - half, centre.y + top),
            Vec2::new(centre.x + half, centre.y + bottom),
            color,
        );
    }
}

/// A square of `size` pixels centred on `at`.
fn dot(list: &mut DrawList, at: Vec2, size: f32, color: [f32; 4]) {
    let half = size * 0.5;
    list.rect(
        Vec2::new(at.x - half, at.y - half),
        Vec2::new(at.x + half, at.y + half),
        color,
    );
}

// ---- the instruments ---------------------------------------------------------

fn draw_panel(list: &mut DrawList, atlas: &FontAtlas, state: &RenderState) {
    let min = Vec2::new(PANEL_INSET, PANEL_INSET);
    let max = Vec2::new(
        PANEL_INSET + PANEL_WIDTH,
        PANEL_INSET + PANEL_PAD * 2.0 + ROW_HEIGHT * ROWS as f32,
    );
    list.rect(min, max, PANEL_BG);
    list.rect_outline(min, max, 1.0, BORDER);

    let left = min.x + PANEL_PAD;
    let right = max.x - PANEL_PAD;
    let mut row = min.y + PANEL_PAD;
    let mut line = |list: &mut DrawList, label: &str, value: String, color: [f32; 4]| {
        list.text(
            Vec2::new(left, row),
            label.to_string(),
            LABEL,
            NATURAL_FONT_SIZE,
        );
        let width = atlas.text_width(&value, NATURAL_FONT_SIZE);
        list.text(
            Vec2::new((right - width).max(left + LABEL_WIDTH), row),
            value,
            color,
            NATURAL_FONT_SIZE,
        );
        row += ROW_HEIGHT;
    };

    let phase_colour = match state.phase {
        Phase::Crashed => WARNING,
        _ => VALUE,
    };
    line(list, "PHASE", state.phase.label().to_string(), phase_colour);
    line(list, "BODY", state.body.to_string(), VALUE);
    line(list, "ALT", distance(state.altitude), VALUE);
    line(list, "VEL", format!("{:.0} m/s", state.speed), VALUE);
    line(
        list,
        "V/S",
        format!("{:+.0} m/s", state.vertical_speed),
        VALUE,
    );
    line(
        list,
        "APO",
        state
            .apoapsis
            .map_or_else(|| "ESCAPE".to_string(), distance),
        VALUE,
    );
    line(
        list,
        "PERI",
        distance(state.periapsis),
        if state.periapsis < 0.0 {
            WARNING
        } else {
            VALUE
        },
    );
    line(
        list,
        "T",
        state.period.map_or_else(|| "-".to_string(), clock),
        VALUE,
    );
    line(
        list,
        "WARP",
        format!("x{}", state.warp),
        if state.warp > 1 { THROTTLE_FILL } else { LABEL },
    );

    // The two bars sit under the rows, inside the same panel width.
    let bar_top = max.y + 8.0;
    bar(list, min.x, right, bar_top, state.fuel, FUEL_FILL);
    bar(
        list,
        min.x,
        right,
        bar_top + 14.0,
        state.throttle,
        THROTTLE_FILL,
    );
    list.text(
        Vec2::new(min.x, bar_top + 30.0),
        format!(
            "FUEL {:.0}%   THROTTLE {:.0}%   {}",
            state.fuel * 100.0,
            state.throttle * 100.0,
            if state.autopilot {
                "AUTOPILOT"
            } else {
                "MANUAL"
            },
        ),
        LABEL,
        NATURAL_FONT_SIZE,
    );
}

/// A horizontal bar filled to `fraction` of its width.
fn bar(list: &mut DrawList, left: f32, right: f32, top: f32, fraction: f64, color: [f32; 4]) {
    const HEIGHT: f32 = 8.0;
    let bottom = top + HEIGHT;
    list.rect(Vec2::new(left, top), Vec2::new(right, bottom), TRACK);
    let filled = fraction.clamp(0.0, 1.0) as f32;
    if filled > 0.0 {
        list.rect(
            Vec2::new(left, top),
            Vec2::new(left + (right - left) * filled, bottom),
            color,
        );
    }
}

fn draw_hint(list: &mut DrawList, atlas: &FontAtlas, width: f32, height: f32, state: &RenderState) {
    let hint = if state.phase.is_finished() {
        "SPACE restart".to_string()
    } else {
        format!(
            "W/S throttle   A/D turn   ,/. warp{}   SPACE {}",
            if state.warp_allowed { "" } else { " (blocked)" },
            if state.phase == Phase::Prelaunch {
                "launch"
            } else {
                "restart"
            },
        )
    };
    let text_width = atlas.text_width(&hint, NATURAL_FONT_SIZE);
    list.text(
        Vec2::new(
            (width - text_width) * 0.5,
            height - PANEL_INSET - ROW_HEIGHT,
        ),
        hint,
        LABEL,
        NATURAL_FONT_SIZE,
    );
}

// ---- formatting --------------------------------------------------------------

/// A distance in metres, in whichever unit reads at that size.
fn distance(metres: f64) -> String {
    if !metres.is_finite() {
        return "-".to_string();
    }
    if metres.abs() >= 10_000.0 {
        format!("{:.1} km", metres / 1_000.0)
    } else {
        format!("{metres:.0} m")
    }
}

/// A duration in seconds as `m:ss`, or hours where it needs them.
fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "-".to_string();
    }
    let whole = seconds as u64;
    let (hours, minutes, secs) = (whole / 3_600, (whole / 60) % 60, whole % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}
