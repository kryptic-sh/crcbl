//! The flight instruments and the map: the whole of what this sample draws.
//!
//! ```text
//!  ┌ flight ───────┐                 ·  ·  ▽ PE
//!  │ PHASE  FLYING │            ·                ·
//!  │ ALT    82 km  │         ·        ▟▛▜▙        ·
//!  │ VEL   2246 m/s│        ◇        ▟█████▙       ·
//!  │ APO   101 km  │        ·        ▜█████▛      ·
//!  │ PERI   98 km  │         ·        ▝▜▛▘       ·
//!  └───────────────┘            ·                ·
//!   ▰▰▰▰▱▱▱▱  fuel                   △ AP ·  ·
//!   ▰▰▱▱▱▱▱▱  throttle
//!      ╭───────╮
//!      │ ⊕   ▾ │   the navball-lite
//!      ╰───────╯
//!                        W/S throttle   A/D turn   . , warp
//! ```
//!
//! # The chrome and the markers are `.crpix` art
//!
//! Sample rule 11: the panel's window and its gauges are nine-slices, and the
//! navball-lite, its prograde, retrograde and heading markers, the map's apsis
//! glyphs and the ship are sprites — every one a frame [`crate::art`]
//! registered into the UI pass's image atlas. What stays geometry is what is
//! *data* rather than chrome — the bodies and the trajectory, whose shape is
//! the simulation's answer — and the readouts stay text, because they are
//! numbers.
//!
//! Every sprite is drawn at a whole number of pixels per texel and put on a
//! whole pixel, so the sharp-bilinear sampling keeps its texels square.
//!
//! # The navball-lite is a dial, not a ball
//!
//! The flight is in one plane, so the navball is flat and holds still while
//! its markers go round the rim. **Its top is straight up from the surface
//! under the ship**, not the top of the screen: a marker's angle on the dial is
//! the angle its direction makes with the local vertical, so the prograde
//! marker swinging from the top down to the horizon *is* the gravity turn, and
//! sitting on the horizon is an orbit.
//!
//! # The orbit is a stroked curve
//!
//! The trajectory is one [`DrawList::polyline`] through the propagator's own
//! samples, closed when the orbit comes back round. The samples are spread
//! evenly in **time** rather than in angle, so the vertices bunch at apoapsis
//! and spread at periapsis — which costs nothing here, because a stroked run
//! is smoothest exactly where its vertices are densest.
//!
//! A sample that came back non-finite is passed through rather than dropped:
//! `polyline` breaks its run at one, so a propagator that diverged draws the
//! gap instead of a curve that quietly skips it.
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
use crcbl::ui::image::AtlasImage;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::art::{Art, CHROME_SCALE, TEXEL_PX};
use crate::game::{Phase, RenderState};

// ---- palette -----------------------------------------------------------------

/// What the frame is cleared to behind the page: space.
pub const BACKDROP: [f32; 4] = [0.02, 0.02, 0.05, 1.0];

/// What a sprite is drawn with when its own colours are the ones wanted.
const UNTINTED: [f32; 4] = [1.0; 4];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
const FUEL_FILL: [f32; 4] = [0.92, 0.72, 0.24, 1.0];
const THROTTLE_FILL: [f32; 4] = [0.36, 0.80, 0.52, 1.0];
const GROUND: [f32; 4] = [0.22, 0.34, 0.26, 1.0];
const SKY: [f32; 4] = [0.16, 0.28, 0.46, 0.55];
const PATH: [f32; 4] = [0.44, 0.62, 0.90, 0.85];
const FLAME: [f32; 4] = [1.0, 0.56, 0.20, 1.0];
const WARNING: [f32; 4] = [0.95, 0.42, 0.36, 1.0];

// ---- layout ------------------------------------------------------------------

/// How many horizontal rows a filled disc is drawn from.
///
/// Enough that the edge reads as a curve at any window this sample opens at,
/// and few enough that two discs are a hundred and some rectangles rather than
/// a mesh.
const DISC_ROWS: usize = 72;

/// How far the engine's plume reaches behind the ship at full throttle, in
/// pixels.
const PLUME_REACH: f32 = 20.0;
/// How wide the trajectory and the engine plume are stroked, in pixels.
const PATH_WIDTH: f32 = 2.0;

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

/// How tall a gauge is, in pixels: the chrome's `track` frame at
/// [`CHROME_SCALE`], so the nine-slice draws it at its own size on that axis.
const GAUGE_HEIGHT: f32 = 8.0;
/// The gap above each gauge and above the status line under them, in pixels.
const GAUGE_GAP: f32 = 6.0;

/// The gap between the status line under the gauges and the navball, in
/// pixels.
const NAVBALL_GAP: f32 = 12.0;

/// How many texels of the navball's rim are bezel rather than sky — the `k`
/// and `r` rings of `assets/navball.crpix` — which the markers sit just inside.
const NAVBALL_BEZEL_TEXELS: f32 = 2.0;

/// Below this speed, in m/s, the velocity has no direction worth drawing, and
/// the prograde and retrograde markers are left off the navball rather than
/// spun by rounding — a ship on the pad is the case.
const MIN_MARKER_SPEED: f64 = 0.5;

/// The gap between an apsis glyph and its label on the map, in pixels.
const APSIS_LABEL_GAP: f32 = 2.0;

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
/// font rather than against a guess. `art` is where [`crate::art`] registered
/// the sprites, in the image atlas of the UI pass `list` is handed to.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    art: &Art,
    extent: (u32, u32),
    state: &RenderState,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    draw_map(list, art, width, height, state);
    let bottom = draw_panel(list, atlas, art, state);
    draw_navball(list, art, bottom + NAVBALL_GAP, state);
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

fn draw_map(list: &mut DrawList, art: &Art, width: f32, height: f32, state: &RenderState) {
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

    // `map.at` is total, so a non-finite sample arrives here still non-finite
    // and `polyline` splits the run on it rather than bridging the gap.
    let path: Vec<Vec2> = state.path.iter().map(|point| map.at(*point)).collect();
    list.polyline(path, PATH_WIDTH, state.path_closed, PATH);

    // The engine's plume, drawn before the ship so the ship sits on top of it:
    // a stroke out the back, as long as the throttle is open.
    let ship = map.at(state.ship);
    if state.throttle > 0.0 && state.fuel > 0.0 {
        let back = Vec2::new(-state.attitude[0] as f32, state.attitude[1] as f32);
        let reach = PLUME_REACH * state.throttle as f32;
        list.line(
            ship,
            Vec2::new(ship.x + back.x * reach, ship.y + back.y * reach),
            PATH_WIDTH,
            FLAME,
        );
    }

    // The apsides before the ship, so a ship passing through one is on top.
    for (at, glyph, label) in [
        (state.apoapsis_at, &art.apoapsis, "AP"),
        (state.periapsis_at, &art.periapsis, "PE"),
    ] {
        let Some(at) = at else { continue };
        let centre = map.at(at);
        if !centre.is_finite() {
            continue;
        }
        let half = sprite(list, glyph, centre);
        list.text(
            Vec2::new(
                (centre.x + half + APSIS_LABEL_GAP).round(),
                (centre.y - NATURAL_FONT_SIZE * 0.5).round(),
            ),
            label,
            LABEL,
            NATURAL_FONT_SIZE,
        );
    }
    sprite(list, &art.ship, ship);
}

/// Draws `image` at [`TEXEL_PX`] centred on `centre`, snapped to the pixel
/// grid, and returns half the side it was drawn at.
fn sprite(list: &mut DrawList, image: &AtlasImage, centre: Vec2) -> f32 {
    let size = image.size() * TEXEL_PX;
    let min = (centre - size * 0.5).round();
    list.image(min, min + size, image, UNTINTED);
    size.x * 0.5
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

// ---- the instruments ---------------------------------------------------------

/// Draws the panel, its gauges and the status line under them, and returns
/// where the status line ends.
fn draw_panel(list: &mut DrawList, atlas: &FontAtlas, art: &Art, state: &RenderState) -> f32 {
    let min = Vec2::new(PANEL_INSET, PANEL_INSET);
    let max = Vec2::new(
        PANEL_INSET + PANEL_WIDTH,
        PANEL_INSET + PANEL_PAD * 2.0 + ROW_HEIGHT * ROWS as f32,
    );
    list.nine_slice(min, max, &art.panel, CHROME_SCALE, UNTINTED);

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

    // The two gauges sit under the rows, inside the same panel width.
    let fuel_top = max.y + GAUGE_GAP;
    gauge(list, art, min.x, right, fuel_top, state.fuel, FUEL_FILL);
    let throttle_top = fuel_top + GAUGE_HEIGHT + GAUGE_GAP;
    gauge(
        list,
        art,
        min.x,
        right,
        throttle_top,
        state.throttle,
        THROTTLE_FILL,
    );
    let status_top = throttle_top + GAUGE_HEIGHT + GAUGE_GAP;
    list.text(
        Vec2::new(min.x, status_top),
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
    status_top + ROW_HEIGHT
}

/// A horizontal gauge filled to `fraction` of its width: the chrome's `track`
/// as the well, and its `fill` tinted `color` over as much of it as is full.
fn gauge(
    list: &mut DrawList,
    art: &Art,
    left: f32,
    right: f32,
    top: f32,
    fraction: f64,
    color: [f32; 4],
) {
    let bottom = top + GAUGE_HEIGHT;
    list.nine_slice(
        Vec2::new(left, top),
        Vec2::new(right, bottom),
        &art.track,
        CHROME_SCALE,
        UNTINTED,
    );
    let filled = fraction.clamp(0.0, 1.0) as f32;
    if filled > 0.0 {
        list.nine_slice(
            Vec2::new(left, top),
            Vec2::new(left + (right - left) * filled, bottom),
            &art.fill,
            CHROME_SCALE,
            color,
        );
    }
}

// ---- the navball-lite --------------------------------------------------------

/// Draws the navball with its top edge at `top`, centred under the panel, and
/// its markers round the rim.
///
/// The ball's frame is the body's kind of sky — `air` under the planet's
/// atmosphere, `vacuum` round the moon — told apart the way the map tells the
/// atmosphere's halo apart, by the body's name.
fn draw_navball(list: &mut DrawList, art: &Art, top: f32, state: &RenderState) {
    let ball = if state.body == "PLANET" {
        &art.navball_air
    } else {
        &art.navball_vacuum
    };
    let radius = ball.size().x * TEXEL_PX * 0.5;
    let centre = Vec2::new(PANEL_INSET + PANEL_WIDTH * 0.5, top + radius).round();
    sprite(list, ball, centre);

    // Every marker rides the same circle, just inside the bezel.
    let up = local_up(state.ship);
    let rim = |image: &AtlasImage, direction: [f64; 2]| {
        let reach = radius - NAVBALL_BEZEL_TEXELS * TEXEL_PX - image.size().x * TEXEL_PX * 0.5;
        centre + dial(up, direction) * reach
    };
    let [vx, vy] = state.velocity;
    if (vx * vx + vy * vy).sqrt() > MIN_MARKER_SPEED {
        sprite(list, &art.retrograde, rim(&art.retrograde, [-vx, -vy]));
        sprite(list, &art.prograde, rim(&art.prograde, state.velocity));
    }
    // The heading last, so it is never hidden under the marker it is chasing.
    sprite(list, &art.heading, rim(&art.heading, state.attitude));
}

/// The direction straight up from the surface under a ship at `ship`, in the
/// frame's plane — or the frame's own `+y` for a ship at the body's centre,
/// which has no surface under it and is only ever the default state.
fn local_up(ship: [f64; 2]) -> [f64; 2] {
    let length = (ship[0] * ship[0] + ship[1] * ship[1]).sqrt();
    if length > 0.0 && length.is_finite() {
        [ship[0] / length, ship[1] / length]
    } else {
        [0.0, 1.0]
    }
}

/// Where `direction` lands on a dial whose top is `up`, as a unit offset in
/// screen space from the dial's centre.
///
/// The angle is signed anticlockwise in the frame's plane, as the map draws
/// it; the screen's `y` runs down where the plane's runs up, which is the sign
/// on both components. A direction of zero length lands on the top.
fn dial(up: [f64; 2], direction: [f64; 2]) -> Vec2 {
    let cross = up[0] * direction[1] - up[1] * direction[0];
    let dot = up[0] * direction[0] + up[1] * direction[1];
    let angle = cross.atan2(dot);
    Vec2::new(-angle.sin() as f32, -angle.cos() as f32)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::draw_list::DrawCommand;
    use crcbl::ui::image::ImageAtlas;

    use crate::game::{PLANET_RADIUS, TARGET_APOAPSIS};

    /// The extent the page is drawn at: the window every sample opens.
    const EXTENT: (u32, u32) = (960, 720);

    /// A flight at the top of the planet, moving anticlockwise along the
    /// horizon with the engine pointing straight up — so the navball's answer
    /// is known without its arithmetic: prograde on the left horizon,
    /// retrograde on the right, the heading at the top.
    fn orbiting() -> RenderState {
        let radius = PLANET_RADIUS + TARGET_APOAPSIS;
        RenderState {
            phase: Phase::Flying,
            body: "PLANET",
            body_radius: PLANET_RADIUS,
            altitude: TARGET_APOAPSIS,
            fuel: 0.5,
            throttle: 0.5,
            warp: 1,
            ship: [0.0, radius],
            attitude: [0.0, 1.0],
            velocity: [-2_000.0, 0.0],
            periapsis_at: Some([0.0, -radius]),
            apoapsis_at: Some([0.0, radius]),
            path: vec![[radius, 0.0], [0.0, radius], [-radius, 0.0], [0.0, -radius]],
            path_closed: true,
            ..RenderState::default()
        }
    }

    fn drawn(state: &RenderState) -> (Art, DrawList) {
        let art = Art::register(&mut ImageAtlas::new()).expect("an empty page has room");
        let mut list = DrawList::new();
        draw(&mut list, &FontAtlas::built_in(), &art, EXTENT, state);
        (art, list)
    }

    /// Where every whole-image quad of `image` was drawn, by its centre.
    fn centres(list: &DrawList, image: &AtlasImage) -> Vec<Vec2> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Image {
                    min,
                    max,
                    uv_min,
                    uv_max,
                    ..
                } if *uv_min == image.uv_min() && *uv_max == image.uv_max() => {
                    assert_eq!(*max - *min, image.size() * TEXEL_PX, "drawn at TEXEL_PX");
                    Some((*min + *max) * 0.5)
                }
                _ => None,
            })
            .collect()
    }

    /// Every quad drawn from inside `image`'s rectangle of the atlas: what a
    /// nine-slice of it emits, one per band.
    fn slices(list: &DrawList, image: &AtlasImage) -> usize {
        let (low, high) = (image.uv_min(), image.uv_max());
        list.commands()
            .iter()
            .filter(|command| match command {
                DrawCommand::Image { uv_min, uv_max, .. } => {
                    uv_min.cmpge(low).all() && uv_max.cmple(high).all()
                }
                _ => false,
            })
            .count()
    }

    /// **Every marker, the navball and the chrome are textured quads from the
    /// sheets**, and the only untextured rectangles left are the two bodies'
    /// disc rows — so a marker or a panel that fell back to a hand-placed quad
    /// is both missing from the sprites and one rectangle too many.
    #[test]
    fn the_flight_ui_is_drawn_from_the_sheets() {
        let (art, list) = drawn(&orbiting());
        for (name, image) in [
            ("prograde", &art.prograde),
            ("retrograde", &art.retrograde),
            ("heading", &art.heading),
            ("apoapsis", &art.apoapsis),
            ("periapsis", &art.periapsis),
            ("ship", &art.ship),
            ("navball", &art.navball_air),
        ] {
            assert_eq!(centres(&list, image).len(), 1, "the {name} sprite, once");
        }
        assert!(centres(&list, &art.navball_vacuum).is_empty(), "under air");
        assert!(slices(&list, &art.panel.image) > 0, "the panel's window");
        // The fuel gauge and the throttle's, a track and a fill apiece.
        assert!(slices(&list, &art.track.image) > 0, "the gauges' wells");
        assert!(slices(&list, &art.fill.image) > 0, "the gauges' fills");
        assert_eq!(
            art.track.image.height as f32 * CHROME_SCALE,
            GAUGE_HEIGHT,
            "a gauge is its track frame's own height, so no band stretches across it",
        );

        let rects = list
            .commands()
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    DrawCommand::Rect { .. } | DrawCommand::RectOutline { .. }
                )
            })
            .count();
        assert_eq!(
            rects,
            2 * DISC_ROWS,
            "the atmosphere's and the planet's rows only"
        );
    }

    /// **The navball's top is the local vertical**: moving anticlockwise along
    /// the horizon puts prograde on the dial's left horizon and retrograde on
    /// its right, and an engine pointing straight up puts the heading at the
    /// top — each the same distance from the centre, inside the bezel.
    #[test]
    fn the_navball_markers_point_where_the_ship_does() {
        let (art, list) = drawn(&orbiting());
        let centre = centres(&list, &art.navball_air)[0];
        let at = |image: &AtlasImage| centres(&list, image)[0] - centre;
        let (prograde, retrograde, heading) =
            (at(&art.prograde), at(&art.retrograde), at(&art.heading));
        assert!(prograde.x < 0.0 && prograde.y.abs() < 1.0, "{prograde}");
        assert!(
            retrograde.x > 0.0 && retrograde.y.abs() < 1.0,
            "{retrograde}"
        );
        assert!(heading.y < 0.0 && heading.x.abs() < 1.0, "{heading}");
        let radius = art.navball_air.size().x * TEXEL_PX * 0.5;
        for offset in [prograde, retrograde, heading] {
            let reach = offset.length() + art.prograde.size().x * TEXEL_PX * 0.5;
            assert!(
                reach <= radius - NAVBALL_BEZEL_TEXELS * TEXEL_PX + 1.0,
                "{offset}"
            );
        }

        // The same flight round the other side of the planet: the dial turns
        // with the vertical, so the markers land where they did.
        let mut below = orbiting();
        below.ship = [0.0, -below.ship[1]];
        below.velocity = [2_000.0, 0.0];
        below.attitude = [0.0, -1.0];
        let (_, turned) = drawn(&below);
        let centre_below = centres(&turned, &art.navball_air)[0];
        assert_eq!(centres(&turned, &art.prograde)[0] - centre_below, prograde);
        assert_eq!(centres(&turned, &art.heading)[0] - centre_below, heading);
    }

    /// **A ship that is not moving has no prograde**, and round the moon the
    /// navball is the vacuum frame.
    #[test]
    fn the_pad_has_no_prograde_and_the_moon_has_no_air() {
        let mut pad = orbiting();
        pad.velocity = [0.0, 0.0];
        pad.periapsis_at = None;
        pad.apoapsis_at = None;
        let (art, list) = drawn(&pad);
        assert!(centres(&list, &art.prograde).is_empty());
        assert!(centres(&list, &art.retrograde).is_empty());
        assert!(centres(&list, &art.apoapsis).is_empty());
        assert_eq!(centres(&list, &art.heading).len(), 1, "the heading always");

        let mut moon = orbiting();
        moon.body = "MOON";
        let (art, list) = drawn(&moon);
        assert_eq!(centres(&list, &art.navball_vacuum).len(), 1);
        assert!(centres(&list, &art.navball_air).is_empty());
    }
}
