//! The listing panel: what the document holds, and what did not survive coming
//! in.
//!
//! ```text
//!   I  ──▶ Listing::toggle
//!            Viewer::draw ──▶ Listing::render ──▶ DrawList ──▶ UiRenderer
//! ```
//!
//! `docs/plan/sample/05-viewer.md`'s milestone 2 opens with a
//! "mesh/material/texture listing". This is that listing, and it is the
//! viewer's **own**: rule 4's debug panel is the engine's, reached with `F3`,
//! and it answers a different question. `F3` says how the *frame* is going;
//! this says what the *file* is.
//!
//! # The lines, and why these ones
//!
//! A panel that lists everything is as useless as one that lists nothing, so
//! every line here is one that changes what a person does next.
//!
//! * **The document's name**, as a heading. It is the key every message about
//!   this file already carries — [`crate::model`]'s errors, the conversion's
//!   warnings, the skip report printed at start-up — so it is what ties the
//!   numbers below to the lines in the terminal. A viewer opens one file at a
//!   time and a user opens several viewers.
//! * **`meshes`, `vertices`, `indices`.** The question a listing exists for:
//!   *is this the whole model?* A mesh count on its own cannot tell an intact
//!   import from one that dropped a primitive at a time, and the vertex and
//!   index totals are the two figures a DCC tool's own statistics panel
//!   reports — so they can be read straight against the tool that exported the
//!   file. Indices rather than triangles because indices are what was made
//!   resident; a reader who wants triangles divides by three.
//! * **`materials`.** The most common loss after geometry, and the one that
//!   looks like a renderer bug rather than an import one: a model that arrives
//!   with one material where the file had twelve is uniformly grey. It counts
//!   the document's own rows, not the description's — the conversion always
//!   reserves one for glTF's default material, and a panel that counted it
//!   would report a material nobody authored.
//! * **`textures`, with the page extent.** Every base-colour image a document
//!   actually uses is decoded and resampled onto **one** square array page —
//!   see [`crcbl::scene::gltf_render`] — so neither number is a property of the
//!   file. The count is how many images survived, and the extent is what they
//!   were resampled *to*: a 4096-texel texture arriving at 2048 is a visible
//!   loss that nothing else in this application reports. Layer 0 of that page
//!   is the renderer's own opaque white, which no document supplied, so it is
//!   not counted.
//! * **`instances`.** How many objects were placed. [`crate::Summary`] reports
//!   it once the run is over; a person looking at the window needs it while
//!   they are looking, because "one instance" beside a mesh count of forty is
//!   exactly the shape of a scene graph that did not survive.
//! * **`size` and `centre`.** The box
//!   [`OrbitCamera::frame`](crcbl::render::OrbitCamera::frame) was pointed at, in
//!   metres — which is glTF's unit, and therefore how a file exported in
//!   centimetres announces itself: as a hundred-metre chair. The centre is what
//!   explains a model that turns around a point somewhere off to the side.
//! * **`exposure`.** The one line here that is not a fact about the file: it is
//!   what the two keys in [`crate::app`] change, and a control with no readout
//!   is a control a user cannot tell has reached the end of its range — which
//!   [`ForwardRenderer::set_exposure`](crcbl::render::ForwardRenderer::set_exposure)
//!   clamps to. It is on *this* panel rather than on `F3` because it is the
//!   viewer's own control and not a fact about how the frame is going, and
//!   because a person who has just pressed a key is looking at the panel they
//!   opened. Written as the multiplier rather than in stops, because the
//!   multiplier is what the shader applies.
//! * **Every skip, spelled the way stderr spells it.** `crate::app` prints them
//!   once at start-up and the conversion logs them; a user who has scrolled
//!   past that, or who started the viewer from a file manager and has no
//!   terminal at all, has no other way back to them. This is the line
//!   `docs/plan/sample/05-viewer.md`'s exit criterion is about, so it is the
//!   one thing here that is never reduced to a count — a document with nothing
//!   skipped says so instead, because "nothing was lost" is an answer and a
//!   missing section is not.
//!
//! **And deliberately not:** frame time, fps, draw and triangle counts, pool
//! occupancy. All of those are on `F3` already, and a second copy is two panels
//! that can disagree. Nor a row per mesh or per material — a list as long as the
//! document needs scrolling, and there is no input for a scrolling widget here;
//! nor the node hierarchy, which is a tree rather than a line.
//!
//! # The box is measured, never assumed
//!
//! Nothing below carries a pixel size for the panel. Its width is the widest
//! line [`FontAtlas::text_width`] measures, its height is a line height per line
//! drawn, and both are then clamped against the framebuffer the frame is
//! actually being drawn at — which is not the window that was asked for and
//! changes under a resize. Past that clamp:
//!
//! * a line too wide is cut and ends in [`ELLIPSIS`], rather than running out
//!   of the panel and off the screen;
//! * lines that do not fit vertically are dropped, and the last drawn line says
//!   how many went — the skips are last, so what a small window loses is the
//!   tail of the skip list and never the counts.
//!
//! The colours and spacing are [`DebugStyle::default`]'s, on purpose: the two
//! panels share a screen and a font, and a second palette written out here would
//! be a second thing to keep in step. The one colour it has no field for is
//! [`SKIPPED`], because nothing on the engine's panel is a warning.

use crcbl::math::Vec2;
use crcbl::render::{Geometry, MeshDesc};
use crcbl::shaders::mesh::VERTEX_STRIDE;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::{DebugStyle, LINE_HEIGHT, NATURAL_FONT_SIZE};

use crate::model::Model;

/// How far the panel sits in from the top-left corner of the framebuffer.
///
/// Top-left because [`crcbl::ui::DebugPanel`] pins itself top-right and the
/// menu is centred, so the three can be on screen together without overlapping.
pub const MARGIN: f32 = 8.0;

/// What a line that had to be cut ends in.
///
/// Three periods rather than U+2026: the built-in atlas holds printable ASCII
/// and draws everything else as `.notdef`, so the single-glyph spelling would
/// arrive as a box.
pub const ELLIPSIS: &str = "...";

/// The colour a skip is drawn in, and the colour of the line that says how many
/// lines did not fit.
///
/// Amber rather than [`DebugStyle::value`]'s white: every other line here is a
/// fact about the document, and these are the ones that say something was lost.
pub const SKIPPED: [f32; 4] = [1.0, 0.72, 0.3, 1.0];

/// One line of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    /// The document's name at the top, and the heading over the skip list.
    Heading(String),
    /// A count and what it counts, laid out in two columns.
    Row {
        /// The left column.
        label: String,
        /// The right column.
        value: String,
    },
    /// One skip, spelled as [`crcbl::scene::gltf_render::Skip`] spells itself —
    /// the same sentence `crate::app` prints to stderr at start-up.
    Skip(String),
}

/// What the loaded document holds, ready to draw.
///
/// Built once, from the [`Model`], because nothing in this milestone can change
/// the document under it: there is no hot reload yet and no way to open a second
/// file. When milestone 3's reload lands, a reloaded model is a new [`Listing`].
///
/// The one line that is not the document's is `exposure`, and it is the one line
/// that is rewritten after construction — see [`Listing::set_exposure`].
#[derive(Clone, Debug)]
pub struct Listing {
    lines: Vec<Line>,
    /// Where the `exposure` row sits in [`Listing::lines`], so
    /// [`Listing::set_exposure`] rewrites a row rather than searching for one by
    /// its label.
    exposure_row: usize,
    visible: bool,
}

impl Listing {
    /// The lines describing `model`, hidden.
    ///
    /// Hidden because a viewer's job is to show the user's asset unadorned —
    /// see [`crate`] — so the panel is something asked for rather than
    /// something to dismiss.
    #[must_use]
    pub fn of(model: &Model) -> Self {
        let scene = &model.render.scene;
        let mut lines = vec![Line::Heading(model.key.display().to_string())];

        lines.push(row("meshes", scene.meshes.len().to_string()));
        lines.push(row(
            "vertices",
            scene
                .meshes
                .iter()
                .map(vertex_count)
                .sum::<usize>()
                .to_string(),
        ));
        lines.push(row(
            "indices",
            scene
                .meshes
                .iter()
                .map(index_count)
                .sum::<usize>()
                .to_string(),
        ));
        // Row 0 is glTF's *default* material, which the conversion always
        // reserves for a primitive naming none — see
        // `crcbl::scene::gltf_render::GLTF_DEFAULT_MATERIAL`. No document
        // supplies it, so counting it would report one material for a file that
        // declares none. Same argument, same subtraction, as the page below.
        let materials = scene.materials.len().saturating_sub(1);
        lines.push(row("materials", materials.to_string()));

        // Layer 0 is the page's opaque-white layer, which belongs to the
        // renderer rather than to the document — see the module docs.
        let textures = scene.page.layers().len().saturating_sub(1);
        let extent = scene.page.extent();
        lines.push(row(
            "textures",
            if textures == 0 {
                "none".to_string()
            } else {
                format!("{textures} at {extent}x{extent}")
            },
        ));

        lines.push(row("instances", model.render.instances.len().to_string()));

        let size = model.bounds.half_extent() * 2.0;
        lines.push(row(
            "size",
            format!("{:.3} x {:.3} x {:.3} m", size.x, size.y, size.z),
        ));
        let centre = model.bounds.center();
        lines.push(row(
            "centre",
            format!("{:.3}, {:.3}, {:.3}", centre.x, centre.y, centre.z),
        ));

        // Seeded with the renderer's own default and overwritten by the first
        // `crate::app::Viewer::draw`, which is where the exposure actually in
        // force can be asked for. Written down here rather than left blank so
        // that a panel is never a row short of the one the layout measured.
        let exposure_row = lines.len();
        lines.push(row(
            "exposure",
            exposure_value(crcbl::shaders::tonemap::DEFAULT_EXPOSURE),
        ));

        let skipped = model.skipped();
        if skipped.is_empty() {
            lines.push(Line::Heading("nothing was skipped".to_string()));
        } else {
            lines.push(Line::Heading(format!("skipped {}", skipped.len())));
            lines.extend(skipped.iter().map(|skip| Line::Skip(skip.to_string())));
        }

        Self {
            lines,
            exposure_row,
            visible: false,
        }
    }

    /// Rewrites the `exposure` row.
    ///
    /// Called every frame from `crate::app::Viewer::draw` with what the renderer
    /// answered — not with what a key asked for — so the panel reports the value
    /// after the renderer's clamp and says plainly when the control has reached
    /// an end of its range.
    pub fn set_exposure(&mut self, exposure: f32) {
        let value = exposure_value(exposure);
        match &mut self.lines[self.exposure_row] {
            Line::Row { value: slot, .. } => *slot = value,
            other => unreachable!("the exposure row is a row, and is a {other:?}"),
        }
    }

    /// Whether the panel draws anything.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the panel.
    pub const fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Flips [`Listing::is_visible`]. What the toggle key calls.
    pub const fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Draws the panel into `dl`, for a framebuffer of `screen` pixels.
    ///
    /// Emits nothing at all — not even a backing rect — while the panel is
    /// hidden, or into a framebuffer with no room for one line of text.
    pub fn render(&self, dl: &mut DrawList, screen: Vec2, atlas: &FontAtlas) {
        let Some(layout) = self.layout(screen, atlas) else {
            return;
        };
        let style = DebugStyle::default();
        let scale = style.font_size / NATURAL_FONT_SIZE;

        let origin = Vec2::splat(MARGIN);
        dl.rect(origin, origin + layout.size, style.bg);

        let mut cursor = origin + Vec2::splat(style.padding);
        let write = |dl: &mut DrawList, at: Vec2, text: &str, room: f32, color| {
            let fitted = fit(atlas, text, room, scale);
            if !fitted.is_empty() {
                dl.text(at, fitted, color, style.font_size);
            }
        };
        for line in &self.lines[..layout.shown] {
            match line {
                Line::Heading(text) => {
                    write(dl, cursor, text, layout.content_width, style.title);
                }
                Line::Skip(text) => write(dl, cursor, text, layout.content_width, SKIPPED),
                Line::Row { label, value } => {
                    write(dl, cursor, label, layout.content_width, style.label);
                    write(
                        dl,
                        Vec2::new(cursor.x + layout.value_column, cursor.y),
                        value,
                        layout.content_width - layout.value_column,
                        style.value,
                    );
                }
            }
            cursor.y += layout.line_height;
        }
        if layout.dropped > 0 {
            write(
                dl,
                cursor,
                &dropped_line(layout.dropped),
                layout.content_width,
                SKIPPED,
            );
        }
    }

    /// Measures the panel against `screen`, or `None` if it would draw nothing.
    ///
    /// There is no `if !self.visible` anywhere else: this is the one place that
    /// reads it, so a hidden panel cannot half-draw.
    fn layout(&self, screen: Vec2, atlas: &FontAtlas) -> Option<Layout> {
        if !self.visible || self.lines.is_empty() {
            return None;
        }
        let style = DebugStyle::default();
        let scale = style.font_size / NATURAL_FONT_SIZE;
        let line_height = LINE_HEIGHT * scale;

        // What the framebuffer leaves for text once the inset from the corner
        // and the panel's own padding are taken off both sides.
        let inner = screen - Vec2::splat(2.0 * (MARGIN + style.padding));
        if inner.x <= 0.0 || inner.y < line_height || line_height <= 0.0 {
            return None;
        }

        // The tail line takes a line of its own, so a panel that has to drop
        // anything drops one more than it is short by.
        let fits = (inner.y / line_height).floor() as usize;
        let (shown, dropped) = if self.lines.len() > fits {
            (fits - 1, self.lines.len() - (fits - 1))
        } else {
            (self.lines.len(), 0)
        };

        let mut label_width = 0.0f32;
        let mut value_width = 0.0f32;
        let mut span_width = 0.0f32;
        for line in &self.lines[..shown] {
            match line {
                Line::Row { label, value } => {
                    label_width = label_width.max(atlas.text_width(label, scale));
                    value_width = value_width.max(atlas.text_width(value, scale));
                }
                Line::Heading(text) | Line::Skip(text) => {
                    span_width = span_width.max(atlas.text_width(text, scale));
                }
            }
        }
        if dropped > 0 {
            span_width = span_width.max(atlas.text_width(&dropped_line(dropped), scale));
        }

        let value_column = label_width + style.column_gap;
        let content_width = (value_column + value_width).max(span_width).min(inner.x);
        let drawn = shown + usize::from(dropped > 0);
        let content_height = drawn as f32 * line_height;
        Some(Layout {
            size: Vec2::new(content_width, content_height) + Vec2::splat(style.padding * 2.0),
            content_width,
            value_column,
            line_height,
            shown,
            dropped,
        })
    }
}

/// What [`Listing::layout`] worked out, shared by measuring and drawing.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// The backing rect's extent, padding included.
    size: Vec2,
    /// The widest a line may be drawn, padding excluded.
    content_width: f32,
    /// Where the value column starts, relative to the label column.
    value_column: f32,
    line_height: f32,
    /// How many of the listing's lines are drawn as themselves.
    shown: usize,
    /// How many did not fit, and are therefore summarised by one line instead.
    dropped: usize,
}

/// A two-column line.
fn row(label: &str, value: String) -> Line {
    Line::Row {
        label: label.to_string(),
        value,
    }
}

/// The `exposure` row's value column.
///
/// Two decimals and an `x`, because it is a multiplier: the range runs from a
/// thirty-second to thirty-two, and two decimals is what tells the bottom of
/// that range apart from the step below it.
///
/// Shared with [`crate::menu`]'s slider, which draws the same number beside its
/// groove: two formatters would drift, and a panel and a menu disagreeing about
/// the exposure by one decimal is exactly the kind of thing nobody would look
/// for.
pub(crate) fn exposure_value(exposure: f32) -> String {
    format!("{exposure:.2}x")
}

/// The line drawn in place of the lines that did not fit.
///
/// It names stderr because that is where every one of them already is —
/// `crate::app` prints the whole skip list at start-up — so a user with a window
/// too small to hold them has somewhere to go rather than a number.
fn dropped_line(dropped: usize) -> String {
    format!("+{dropped} more, all of them on stderr")
}

/// `text` if it fits in `max` pixels, otherwise as much of it as leaves room for
/// [`ELLIPSIS`].
///
/// Empty when not even the ellipsis fits, which the caller draws as nothing at
/// all rather than as a partial glyph run.
fn fit(atlas: &FontAtlas, text: &str, max: f32, scale: f32) -> String {
    if atlas.text_width(text, scale) <= max {
        return text.to_string();
    }
    let room = max - atlas.text_width(ELLIPSIS, scale);
    if room < 0.0 {
        return String::new();
    }
    let mut kept = String::new();
    let mut width = 0.0;
    for c in text.chars() {
        let advance = atlas.glyph(c).advance * scale;
        if width + advance > room {
            break;
        }
        width += advance;
        kept.push(c);
    }
    kept.push_str(ELLIPSIS);
    kept
}

/// How many vertices one resident mesh holds.
///
/// The bytes divided by the stride, because that is what the description
/// carries: [`Geometry`] is vertex *bytes*, and
/// [`VERTEX_STRIDE`](crcbl::shaders::mesh::VERTEX_STRIDE) is the one vertex
/// format the mesh pool has.
fn vertex_count(mesh: &MeshDesc<'_>) -> usize {
    match &mesh.geometry {
        Geometry::Flat { vertices, .. } => vertices.len() / VERTEX_STRIDE,
        Geometry::Dag { levels, .. } => {
            levels.iter().map(|level| level.len()).sum::<usize>() / VERTEX_STRIDE
        }
    }
}

/// How many indices one resident mesh holds, on [`vertex_count`]'s terms.
fn index_count(mesh: &MeshDesc<'_>) -> usize {
    match &mesh.geometry {
        Geometry::Flat { indices, .. } => indices.len(),
        // A glTF conversion builds no DAGs — `crcbl::scene::gltf_render` says so
        // — and a DAG level's triangles live in its clusters rather than in an
        // index array, so there is no number to add here.
        Geometry::Dag { .. } => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::Vec3;
    use crcbl::ui::draw_list::DrawCommand;

    use crate::fixture;
    use crate::model;

    /// A window big enough that nothing is cut, so a test about *content* is
    /// not accidentally a test about truncation.
    const ROOMY: Vec2 = Vec2::new(1280.0, 800.0);

    /// A document written to a temporary directory and loaded through the real
    /// seam, because the listing's numbers are the conversion's and a
    /// hand-built `Model` would assert on numbers this test wrote itself.
    fn loaded(name: &str, document: &[u8]) -> (tempfile::TempDir, Model) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(name);
        std::fs::write(&path, document).expect("the document");
        let model = model::load(&path).expect("the fixture loads");
        (dir, model)
    }

    /// A visible listing of `document`.
    fn shown(name: &str, document: &[u8]) -> (tempfile::TempDir, Listing) {
        let (dir, model) = loaded(name, document);
        let mut listing = Listing::of(&model);
        listing.set_visible(true);
        (dir, listing)
    }

    /// Every `Text` command the panel produced, in order.
    fn drawn(listing: &Listing, screen: Vec2) -> Vec<String> {
        let mut dl = DrawList::new();
        listing.render(&mut dl, screen, &FontAtlas::built_in());
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The value drawn immediately after the row labelled `label`.
    fn value_of(lines: &[String], label: &str) -> String {
        let at = lines
            .iter()
            .position(|text| text == label)
            .unwrap_or_else(|| panic!("no {label} row in {lines:#?}"));
        lines
            .get(at + 1)
            .unwrap_or_else(|| panic!("no value after {label} in {lines:#?}"))
            .clone()
    }

    /// **The panel says what the document actually holds**, number by number.
    ///
    /// Every one of these is asserted against a fixture whose contents are
    /// written out in `crate::fixture`: one quad, four vertices, six indices,
    /// one primitive on one node. A panel that drew a plausible-looking listing
    /// of the wrong document — the renderer's capacities instead of its
    /// contents, say, or the page's white layer counted as a texture — passes
    /// "the panel drew something" and fails here.
    #[test]
    fn the_panel_lists_the_documents_own_counts() {
        let (_dir, listing) = shown("panel.glb", &fixture::quad_glb(fixture::QUAD_CENTRE));
        let lines = drawn(&listing, ROOMY);

        assert_eq!(lines.first().map(String::as_str), Some("panel.glb"));
        assert_eq!(value_of(&lines, "meshes"), "1");
        assert_eq!(value_of(&lines, "vertices"), "4");
        assert_eq!(value_of(&lines, "indices"), "6");
        // One, though the description carries two rows: the conversion always
        // reserves row 0 for glTF's default material. A panel that counted rows
        // would say "2" for a file that declares one material, which is the
        // number a person checking their export against this panel would then
        // go looking for.
        assert_eq!(value_of(&lines, "materials"), "1");
        assert_eq!(value_of(&lines, "instances"), "1");
        assert_eq!(
            value_of(&lines, "textures"),
            "none",
            "the page's opaque-white layer is the renderer's, not the document's",
        );

        // A one-metre quad in the XY plane, on a node that translates it — so
        // both of these fail if the box were the local one or the renderer's.
        assert_eq!(value_of(&lines, "size"), "1.000 x 1.000 x 0.000 m");
        assert_eq!(value_of(&lines, "centre"), "2.000, 1.000, -3.000");

        assert!(
            lines.iter().any(|line| line == "nothing was skipped"),
            "a clean document has to say so: {lines:#?}",
        );
    }

    /// **The `exposure` row is rewritten, and it is the only row that is.**
    ///
    /// [`Listing::set_exposure`] writes by index rather than by searching for a
    /// label, so the failure it can have is writing over a neighbour — a panel
    /// reporting the model's centre as `2.00x`, which still draws. Asserting the
    /// rest of the panel is untouched is what catches that; asserting the row
    /// alone would not.
    #[test]
    fn setting_the_exposure_rewrites_that_row_and_no_other() {
        let (_dir, mut listing) = shown("panel.glb", &fixture::quad_glb(fixture::QUAD_CENTRE));
        let before = drawn(&listing, ROOMY);
        assert_eq!(
            value_of(&before, "exposure"),
            "1.00x",
            "the row starts at the renderer's default",
        );

        listing.set_exposure(0.5);
        let after = drawn(&listing, ROOMY);
        assert_eq!(value_of(&after, "exposure"), "0.50x");
        assert_eq!(
            after.len(),
            before.len(),
            "a rewrite is not a line added or removed",
        );
        for (was, now) in before.iter().zip(&after) {
            assert!(
                was == now || was == "1.00x",
                "{was:?} became {now:?}, and the exposure was the only row that may move",
            );
        }
    }

    /// **A skipped feature is named on the panel**, not counted on it.
    ///
    /// The line `docs/plan/sample/05-viewer.md`'s exit criterion is about: the
    /// file, the feature and the reason, in front of the person who opened it.
    /// The fixture's node scales its axes unequally, which the conversion
    /// reports and still draws — so this also holds the case where the model
    /// looks fine and something was lost anyway.
    #[test]
    fn a_skipped_feature_is_named_on_the_panel_rather_than_counted() {
        let (_dir, listing) = shown("skewed.glb", &fixture::skewed_glb());
        let lines = drawn(&listing, ROOMY);

        assert!(
            lines.iter().any(|line| line == "skipped 1"),
            "the heading counts them: {lines:#?}",
        );
        let named = lines
            .iter()
            .find(|line| line.contains("scale"))
            .unwrap_or_else(|| panic!("no line named the skipped feature: {lines:#?}"));
        assert!(
            named.contains("node 0") && named.contains("panel"),
            "a skip has to say where in the document it was: {named}",
        );
        assert!(
            named.contains("unequally"),
            "and why it was skipped: {named}",
        );
        // The geometry still arrived, so this is not a document that failed to
        // convert wearing a skip.
        assert_eq!(value_of(&lines, "instances"), "1");
    }

    /// **Toggling changes what the draw list holds**, in both directions.
    ///
    /// The hidden panel emits *nothing* — no backing rect either, which is the
    /// half a text-only assertion would miss: a rect behind no text is a grey
    /// box over the model the user came to look at.
    #[test]
    fn toggling_the_panel_changes_what_the_frame_draws() {
        let (_dir, model) = loaded("panel.glb", &fixture::quad_glb(Vec3::ZERO));
        let mut listing = Listing::of(&model);
        let atlas = FontAtlas::built_in();
        assert!(!listing.is_visible(), "the panel starts hidden");

        let mut dl = DrawList::new();
        listing.render(&mut dl, ROOMY, &atlas);
        assert!(dl.is_empty(), "a hidden panel draws nothing at all");

        listing.toggle();
        assert!(listing.is_visible());
        listing.render(&mut dl, ROOMY, &atlas);
        assert!(
            drawn(&listing, ROOMY)
                .iter()
                .any(|line| line == "panel.glb"),
            "the shown panel names the document",
        );
        let shown_commands = dl.len();
        assert!(shown_commands > 0);

        listing.toggle();
        listing.render(&mut dl, ROOMY, &atlas);
        assert_eq!(dl.len(), shown_commands, "hiding it again appended nothing",);
    }

    /// **The box is the text's, not the window's.**
    ///
    /// The same listing in two very different framebuffers draws the same
    /// panel, because its width comes from the longest line and its height from
    /// the number of lines. A panel sized as a fraction of the screen would
    /// differ here, and would be a box of empty space at 1920 and a clipped one
    /// at 640.
    #[test]
    fn the_panel_is_the_size_of_its_text_wherever_there_is_room() {
        let (_dir, listing) = shown("panel.glb", &fixture::quad_glb(Vec3::ZERO));
        let small = panel_rect(&listing, Vec2::new(640.0, 480.0));
        let large = panel_rect(&listing, Vec2::new(1920.0, 1080.0));
        assert_eq!(small, large, "the panel resized itself with the window");

        let (min, max) = small.expect("a panel fits in a 640x480 window");
        assert_eq!(min, Vec2::splat(MARGIN), "it is pinned to the top left");
        assert!(
            max.x < 640.0 && max.y < 480.0,
            "the panel runs out of the smaller framebuffer: {max:?}",
        );
    }

    /// **A framebuffer too short for every line says how many it dropped**, and
    /// keeps the counts rather than the tail of the skip list.
    ///
    /// The alternative — drawing the whole list anyway — puts skips underneath
    /// the bottom of the window, where they are indistinguishable from skips
    /// that were never reported.
    ///
    /// The numbers are exact rather than "fewer than before", which is what
    /// makes the tail's arithmetic checkable: the skewed fixture's listing is a
    /// heading, nine rows, the skip heading and one skip; a window holding ten
    /// lines spends one of them on the tail, so nine are drawn and three go.
    #[test]
    fn a_short_window_drops_the_tail_and_says_how_much_it_dropped() {
        let (_dir, listing) = shown("skewed.glb", &fixture::skewed_glb());
        assert_eq!(listing.lines.len(), 12, "{:#?}", listing.lines);

        let padding = DebugStyle::default().padding;
        let short = Vec2::new(1280.0, 2.0 * (MARGIN + padding) + 10.0 * LINE_HEIGHT);
        let cut = drawn(&listing, short);

        assert_eq!(
            cut.first().map(String::as_str),
            Some("skewed.glb"),
            "the counts are kept and the tail is what goes: {cut:#?}",
        );
        assert_eq!(
            cut.last().map(String::as_str),
            Some("+3 more, all of them on stderr"),
            "the last line has to account for exactly what is missing: {cut:#?}",
        );
        assert!(
            !cut.iter().any(|line| line == "skipped 1"),
            "the dropped lines are the exposure row and the skip section, which are last: {cut:#?}",
        );
        assert_eq!(
            value_of(&cut, "centre"),
            "0.000, 0.000, 0.000",
            "the ninth line is still drawn in full: {cut:#?}",
        );

        let (_min, max) = panel_rect(&listing, short).expect("a panel still fits");
        assert!(
            max.y <= short.y,
            "the panel is taller than the window: {max:?}",
        );
    }

    /// **A line too wide is cut, not run off the edge.**
    ///
    /// A skip's reason is a whole sentence and the window is whatever the user
    /// dragged it to, so this is the ordinary case rather than an edge one.
    #[test]
    fn a_narrow_window_cuts_its_lines_instead_of_overflowing() {
        let (_dir, listing) = shown("skewed.glb", &fixture::skewed_glb());
        let narrow = Vec2::new(320.0, 800.0);
        let lines = drawn(&listing, narrow);

        assert!(
            lines.iter().any(|line| line.ends_with(ELLIPSIS)),
            "nothing was cut in a 320-pixel window: {lines:#?}",
        );
        let atlas = FontAtlas::built_in();
        let (min, max) = panel_rect(&listing, narrow).expect("a panel fits");
        for line in &lines {
            assert!(
                atlas.text_width(line, 1.0) <= max.x - min.x,
                "{line:?} is wider than the panel it is in",
            );
        }
        assert!(max.x <= narrow.x, "the panel is wider than the window");
    }

    /// A framebuffer with no room for even one line draws nothing, rather than
    /// a backing rect larger than the window it is in.
    ///
    /// Minimised windows report a zero extent, so this is a real frame and not
    /// a hypothetical one.
    #[test]
    fn a_window_with_no_room_for_a_line_draws_no_panel_at_all() {
        let (_dir, listing) = shown("panel.glb", &fixture::quad_glb(Vec3::ZERO));
        for screen in [Vec2::ZERO, Vec2::new(4.0, 4.0), Vec2::new(1280.0, 20.0)] {
            let mut dl = DrawList::new();
            listing.render(&mut dl, screen, &FontAtlas::built_in());
            assert!(
                dl.is_empty(),
                "a {screen:?} framebuffer drew {} commands",
                dl.len()
            );
        }
    }

    /// The backing rect the panel drew, or `None` if it drew none.
    fn panel_rect(listing: &Listing, screen: Vec2) -> Option<(Vec2, Vec2)> {
        let mut dl = DrawList::new();
        listing.render(&mut dl, screen, &FontAtlas::built_in());
        dl.commands().iter().find_map(|command| match command {
            DrawCommand::Rect { min, max, .. } => Some((*min, *max)),
            _ => None,
        })
    }
}
