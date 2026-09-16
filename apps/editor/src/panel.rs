//! The editor's panels: a docked outliner and inspector beside the viewport,
//! and the two-way join between what they show and what the [`Document`] holds.
//!
//! **Nothing here touches a device or a window**, which is
//! [`crate::document`]'s rule extended to the panels: a frame of this is a
//! [`Ui`] tree laid out against a [`FontAtlas`] and emitted into a
//! [`DrawList`], all of it arithmetic. [`crate::app`] opens the window, feeds
//! this the pointer and the keyboard, and hands the draw list to the renderer.
//! The tests below drive the real widgets — real clicks, at real rectangles —
//! with no GPU at all.
//!
//! # The three panes
//!
//! [`crate::layout`] names them and holds the [`DockLayout`] they are arranged
//! by. Two are built here; the third, the viewport, is **built empty on
//! purpose**. `docs/plan/08-editor.md` leaves the choice open between "a
//! secondary view rendered to a texture a UI rect samples" and "the scene drawn
//! full-window with UI panes around a scissored region", and the tree today can
//! draw neither: a `DrawCommand::Image` names no texture, `crcbl-render`'s
//! graph sets every pass's scissor from the whole attachment, and a secondary
//! view renders to its own offscreen image with nothing that composites it.
//! So the viewport pane is a **hole**: the scene is drawn over the whole
//! window, the panels are composited on top of it, and the pane's rectangle is
//! what a click has to land in to be a pick. That is why [`PanelFrame`] carries
//! that rectangle and why the ray is still cast through the whole window — the
//! picture under the cursor is the full-window one, so the unprojection must be
//! too.
//!
//! # Selection, in both directions
//!
//! One witness carries it: the selection the outliner was last told about. At
//! the top of a frame a document selection that is not that one came from
//! somewhere else — a ray pick in the viewport — so it is pushed into the
//! outliner, its system expanded and its row scrolled into view. At the bottom,
//! whatever the outliner's rows now say is pushed back into the document. So
//! neither side owns it and neither can drift.
//!
//! **Clicking a row also takes the keyboard**, because the row is a focusable
//! node: [`crate::app`] pushes the reserved `ui` context while
//! [`Panels::holds_keyboard`], so the arrows then walk the outliner instead of
//! nudging the selection, and a click in the viewport calls
//! [`Panels::release_keyboard`] to give them back. That is click-to-focus, and
//! it is what makes the arrows one thing at a time rather than two.
//!
//! # An inspector edit is a command
//!
//! [`Ui::inspector_with`] edits the component and reports each write as a
//! `FieldEdit`. Each becomes an [`EditCommand`](crate::command::EditCommand)
//! through [`Document::record_edit`], which rewinds the panel's own write first
//! so the inverse the log records is exact — that method's docs say why. Undo,
//! redo, the dirty marker and the collider all follow, because they follow
//! every command.
//!
//! # What the outliner costs
//!
//! The rows are virtualized: a frame builds the rows its window shows and no
//! more, whatever the scene holds. What is *not* free is the flatten, and the
//! outline it is flattened from — so the outline is read once and re-read only
//! when [`Document::entity_count`] moves, which in this slice is never, and the
//! flatten runs only when the widget says its model is stale.

use crcbl::math::Vec2;
use crcbl::scene::scn::SceneEntityId;
use crcbl::ui::style::Declaration;
use crcbl::ui::tree::{
    AvailableSpace, ClipboardRequest, DockLayout, FieldEdit, InspectorOptions, LengthAuto,
    NavInput, NodeKey, OUTLINER_ROW_HEIGHT, OutlinerId, OutlinerOptions, OutlinerState, Overrides,
    SelectMode, TextInput, Ui,
};
use crcbl::ui::{DrawList, FontAtlas, PointerInput};

use crate::document::Document;
use crate::layout::{self, PANE_MIN};

/// The editor's own stylesheet, over `crcbl-ui`'s `default.css`.
///
/// Only what the widget set cannot know: that a panel fills its pane, that the
/// two scrolling widgets may shrink below their content, and the title strip
/// over each. The widgets' own look is the engine's, which is the point of
/// having one.
///
/// **`min-width: 0` and `min-height: 0` are load-bearing, and they are set on
/// the dock's own blocks as well as this module's.** Flexbox gives an item
/// `min-width: auto`, so every block from the split down to the panel refuses
/// to shrink below the widest thing inside it — an inspector row, an outliner
/// label — and a pane whose `overflow: hidden` then clips what it cannot fit.
/// That is not only a look. A rectangle is a *hit test*: a panel wider than its
/// pane reaches across the divider into the viewport's rectangle, and a click
/// meant for a field is read as a click in the scene. `default.css` sets none
/// of these, which is worth an upstream look — a dock whose panes grow past
/// their dividers is a dock whose dividers lie.
const EDITOR_CSS: &str = "
#editor { flex-direction: row; }

#panes split,
#panes .split-pane,
#panes .dock-pane {
  min-width: 0;
  min-height: 0;
}

.editor-panel {
  flex-direction: column;
  flex-grow: 1;
  min-width: 0;
  min-height: 0;
  background: #14171d;
}

.editor-title {
  padding: 2px 4px;
  background: #232833;
  color: #9aa3b2;
  flex-shrink: 0;
}

.editor-note { padding: 4px; color: #7a8190; }

#outline { flex-grow: 1; min-width: 0; min-height: 0; border-width: 0; }

#props { flex-grow: 1; min-width: 0; min-height: 0; overflow: scroll; }
";

/// Where a system's row sits in an [`OutlinerId`], above every entity's.
///
/// A [`SceneEntityId`] is a `u32` and an [`OutlinerId`] is a `u64`, so the
/// halves cannot meet: an id below this is an entity's and one above it is the
/// index of a system in [`Document::outline`].
const SYSTEM_ROW: u64 = 1 << 32;

/// A row of slack kept at each edge when a row is scrolled into view.
///
/// [`Ui::rect`] answers the **border** box and an outliner scrolls inside its
/// content box, so a row put exactly at the edge of the border box can sit a
/// border and a padding under the one it is meant to be beside. A row of slack
/// is cheaper than reaching for a measurement the tree does not expose, and it
/// is what a person wants anyway: a revealed row with its neighbour in sight.
const REVEAL_MARGIN: f32 = OUTLINER_ROW_HEIGHT;

/// What one frame of the panels is driven by.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelInput {
    /// Where the pointer is and what its primary button is doing.
    pub pointer: PointerInput,
    /// This frame's navigation edges, from [`crcbl::nav::nav_input`].
    pub nav: NavInput,
    /// This frame's typing, from [`crcbl::text_input::TextPump::frame`].
    pub text: TextInput,
    /// The framebuffer the panels are laid out over.
    pub extent: (u32, u32),
    /// How a click on an outliner row changes the selection this frame: the
    /// caller's own modifier keys, mapped as the widget's docs ask.
    pub select: SelectMode,
    /// How far the wheel asked the panel under the pointer to scroll, in
    /// pixels, positive downwards — the caller's own detents-to-pixels policy,
    /// which `crcbl_core::input::ScrollDelta` says is the application's.
    ///
    /// **The tree takes no scroll input of its own**: a scroll offset is
    /// something a caller writes, which is why this is here rather than in
    /// [`PointerInput`].
    pub scroll: f32,
}

/// What one frame of the panels did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelFrame {
    /// The viewport pane's rectangle in framebuffer pixels, as this frame laid
    /// it out: `(top-left, bottom-right)`. A pointer outside it is a panel's.
    pub viewport: (Vec2, Vec2),
    /// How many commands this frame's inspector edits became.
    pub commands: usize,
}

/// The editor's panels, and everything they keep between frames.
#[derive(Debug)]
pub struct Panels {
    ui: Ui,
    atlas: FontAtlas,
    list: DrawList,
    layout: DockLayout,
    outliner: OutlinerState,
    overrides: Overrides,
    /// [`Document::outline`], read again only when the entity count moves —
    /// see the module docs.
    outline: Vec<(String, Vec<SceneEntityId>)>,
    /// The entity count the outline was read at.
    counted: usize,
    /// The selection the outliner was last told about: the witness that says
    /// which side changed it. See the module docs.
    shown: Option<SceneEntityId>,
    /// A row to put in view once this frame has been laid out.
    reveal: Option<SceneEntityId>,
    /// The viewport pane's rectangle, as the last frame laid it out.
    viewport: (Vec2, Vec2),
    /// The outliner block, as the last frame laid it out.
    outliner_key: Option<NodeKey>,
    /// The inspector block, as the last frame laid it out.
    props_key: Option<NodeKey>,
}

impl Panels {
    /// The panels for `document`, arranged by `layout`, with every system
    /// expanded so the scene's entities are showing at start-up.
    #[must_use]
    pub fn new(document: &mut Document, layout: DockLayout, extent: (u32, u32)) -> Self {
        let mut ui = Ui::new();
        ui.add_stylesheet("editor.css", EDITOR_CSS);
        let outline = document.outline();
        let mut outliner = OutlinerState::new();
        for index in 0..outline.len() {
            outliner.set_expanded(system_row(index), true);
        }
        let mut panels = Self {
            ui,
            atlas: FontAtlas::built_in(),
            list: DrawList::new(),
            layout,
            outliner,
            overrides: Overrides::vectors(),
            counted: document.entity_count(),
            outline,
            // Deliberately not the document's selection: leaving the witness
            // empty makes the idle frame below *push* whatever is selected into
            // the outliner, so a document handed over with a selection keeps it
            // rather than having it read back off an outliner that has none.
            shown: None,
            reveal: None,
            viewport: (Vec2::ZERO, Vec2::ZERO),
            outliner_key: None,
            props_key: None,
        };
        // One idle frame, so the first real one has rectangles to hit-test
        // against: the tree resolves a click against the *previous* layout, and
        // a viewport whose rectangle is still zero would claim no click at all.
        panels.frame(
            document,
            PanelInput {
                pointer: PointerInput::hovering(Vec2::splat(f32::NEG_INFINITY)),
                nav: NavInput::default(),
                text: TextInput::default(),
                extent,
                select: SelectMode::Replace,
                scroll: 0.0,
            },
        );
        panels
    }

    /// The layout as it stands, dividers included: what a save writes.
    #[must_use]
    pub const fn layout(&self) -> &DockLayout {
        &self.layout
    }

    /// The viewport pane's rectangle, as the last frame laid it out.
    #[must_use]
    pub const fn viewport(&self) -> (Vec2, Vec2) {
        self.viewport
    }

    /// Whether `at` is inside the viewport pane — whether, that is, a click
    /// there is the scene's rather than a panel's.
    #[must_use]
    pub fn in_viewport(&self, at: Vec2) -> bool {
        let (min, max) = self.viewport;
        at.x >= min.x && at.x < max.x && at.y >= min.y && at.y < max.y
    }

    /// Whether a panel holds the keyboard: something in the tree has focus, or
    /// is engaged.
    ///
    /// What the reserved `ui` context is pushed on — see [`crate::keys`].
    #[must_use]
    pub fn holds_keyboard(&self) -> bool {
        self.ui.focused().is_some() || self.ui.engaged().is_some()
    }

    /// Whether a text field is being typed into: what the reserved `text`
    /// context is pushed on.
    #[must_use]
    pub fn text_editing(&self) -> bool {
        self.ui.text_editing()
    }

    /// Takes the keyboard back from the panels, committing whatever was being
    /// edited — what a click in the viewport means.
    pub fn release_keyboard(&mut self) {
        self.ui.clear_focus();
    }

    /// What the last frame drew.
    #[must_use]
    pub const fn draw_list(&self) -> &DrawList {
        &self.list
    }

    /// The font the panels were measured against.
    ///
    /// **The same one the compositor must draw with**: a page measured with one
    /// atlas and drawn with another is a page whose text does not fit the boxes
    /// laid out for it.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The tree itself, for the crate's own tests: a widget's rectangle is what
    /// a scripted click needs, and [`Ui::rect`] and [`Ui::child_keys`] are how
    /// one is found.
    #[cfg(test)]
    pub(crate) const fn ui(&self) -> &Ui {
        &self.ui
    }

    /// The outliner block, as the last frame laid it out.
    #[cfg(test)]
    pub(crate) const fn outliner_key(&self) -> Option<NodeKey> {
        self.outliner_key
    }

    /// The inspector block, as the last frame laid it out; [`None`] on a frame
    /// with nothing selected, which builds none.
    #[cfg(test)]
    pub(crate) const fn props_key(&self) -> Option<NodeKey> {
        self.props_key
    }

    /// Which rows the outliner shows as selected.
    #[cfg(test)]
    pub(crate) fn selected_rows(&self) -> Vec<OutlinerId> {
        self.outliner.selected().collect()
    }

    /// The outliner's rows, in the order the last frame built them: a system's
    /// header and then the entities under it.
    #[cfg(test)]
    pub(crate) fn row_keys(&self) -> Vec<NodeKey> {
        let Some(outliner) = self.outliner_key else {
            return Vec::new();
        };
        self.ui
            .child_keys(outliner)
            .first()
            .map_or_else(Vec::new, |&content| self.ui.child_keys(content))
    }

    /// The clipboard requests this frame's text fields made, for the caller to
    /// carry to the shell.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.ui.take_clipboard_requests()
    }

    /// Builds, lays out and emits one frame of the panels, and carries what
    /// they changed into `document`.
    ///
    /// **Once a frame.** The tree resolves this frame's click when the frame
    /// begins, so a second build would latch it twice.
    pub fn frame(&mut self, document: &mut Document, input: PanelInput) -> PanelFrame {
        self.refresh(document);
        self.follow_document(document);

        let extent = Vec2::new(input.extent.0 as f32, input.extent.1 as f32);
        let selected = document.selected();
        let mut edits: Vec<FieldEdit> = Vec::new();
        let mut viewport = None;
        let mut outliner_key = None;
        let mut props = None;

        // Last frame's scrolling blocks, read before the fields are borrowed
        // below — and last frame's is the right answer anyway, because that is
        // the layout the pointer is over.
        let scrollers: Vec<NodeKey> = [self.outliner_key, self.props_key]
            .into_iter()
            .flatten()
            .collect();
        let Self {
            ui,
            atlas,
            list,
            layout,
            outliner,
            overrides,
            outline,
            ..
        } = self;
        let options = OutlinerOptions {
            select: input.select,
            ..OutlinerOptions::default()
        };

        ui.begin_frame_with(input.pointer, input.nav);
        ui.set_text_input(input.text);
        scroll(ui, &scrollers, input.pointer.pos, input.scroll);
        ui.block(
            "#editor",
            &[
                Declaration::Width(LengthAuto::Px(extent.x)),
                Declaration::Height(LengthAuto::Px(extent.y)),
            ],
            |ui| {
                ui.dock("#panes", layout, PANE_MIN, |ui, pane| match pane {
                    // Built empty: the scene is drawn under it. See the module
                    // docs.
                    layout::VIEWPORT => viewport = ui.current_key(),
                    layout::OUTLINER => {
                        outliner_key = Some(build_outliner(ui, outliner, &options, outline));
                    }
                    layout::INSPECTOR => {
                        props = Some(build_inspector(
                            ui, document, selected, overrides, &mut edits,
                        ));
                    }
                    other => {
                        // Unreachable while `crate::layout::load` refuses a
                        // layout whose panes are not this build's, and said out
                        // loud rather than left blank so that a fourth pane
                        // added without a builder is visible rather than empty.
                        let note = format!("no pane called {other}");
                        ui.span(".editor-note", note.as_str(), &[]);
                    }
                });
            },
        );
        ui.layout(Vec2::ZERO, AvailableSpace::definite(extent), atlas);
        list.clear();
        ui.emit(list);

        if let Some(rect) = viewport.and_then(|key| self.ui.rect(key)) {
            self.viewport = rect;
        }
        self.outliner_key = outliner_key;
        self.props_key = props.flatten();
        if let (Some(key), Some(id)) = (outliner_key, self.reveal.take()) {
            self.reveal_row(key, id);
        }

        let commands = self.apply_edits(document, selected, &edits);
        self.follow_outliner(document);
        PanelFrame {
            viewport: self.viewport,
            commands,
        }
    }

    /// Reads [`Document::outline`] again when the document has gained or lost
    /// an entity.
    ///
    /// The count rather than a revision, because the count is what changes:
    /// every command this slice issues is a property set, which moves no row in
    /// or out — so this is the trigger for the day one does, and costs an
    /// integer comparison until then.
    fn refresh(&mut self, document: &mut Document) {
        if document.entity_count() == self.counted {
            return;
        }
        self.counted = document.entity_count();
        self.outline = document.outline();
        self.outliner.invalidate();
    }

    /// Pushes a selection the document gained from somewhere else — a ray pick
    /// in the viewport — into the outliner, and asks for its row.
    fn follow_document(&mut self, document: &Document) {
        let selected = document.selected();
        if selected == self.shown {
            return;
        }
        self.shown = selected;
        self.outliner.clear_selection();
        let Some(id) = selected else {
            return;
        };
        // A row inside a collapsed system is not in the flattened model, so
        // nothing would scroll to it and nothing would show it selected.
        if let Some(index) = self.outline.iter().position(|(_, ids)| ids.contains(&id)) {
            self.outliner.set_expanded(system_row(index), true);
        }
        self.outliner.select(entity_row(id), SelectMode::Replace);
        self.reveal = Some(id);
    }

    /// Pushes what the outliner's rows now say back into the document.
    ///
    /// The first selected row that stands for an entity, in row order — a
    /// system's own row selects nothing, which is what clicking a header
    /// means.
    fn follow_outliner(&mut self, document: &mut Document) {
        let picked = self.outliner.selected().find_map(entity_of);
        if picked != document.selected() {
            document.select(picked);
        }
        self.shown = document.selected();
    }

    /// Turns this frame's inspector edits into commands on `document`.
    ///
    /// `selected` is the entity the inspector was **drawn** for, which is the
    /// one the paths are relative to — reading the document's selection again
    /// here would attach an edit to whatever a click in the outliner selected
    /// in the same frame.
    fn apply_edits(
        &self,
        document: &mut Document,
        selected: Option<SceneEntityId>,
        edits: &[FieldEdit],
    ) -> usize {
        let Some(id) = selected else {
            return 0;
        };
        let mut applied = 0;
        for edit in edits {
            match document.record_edit(id, &edit.path, &edit.before, &edit.after) {
                Ok(()) => applied += 1,
                Err(error) => crcbl::log::warn!("editor: {error}"),
            }
        }
        applied
    }

    /// Moves the outliner's scroll the least that puts `id`'s row in view, with
    /// [`REVEAL_MARGIN`] of slack at each edge.
    fn reveal_row(&mut self, key: NodeKey, id: SceneEntityId) {
        let row = entity_row(id);
        let Some(index) = self.outliner.rows().iter().position(|each| each.id == row) else {
            return;
        };
        let Some((min, max)) = self.ui.rect(key) else {
            return;
        };
        let view = max.y - min.y;
        let top = index as f32 * OUTLINER_ROW_HEIGHT;
        let offset = self.ui.scroll_offset_of(key).y;
        let wanted = if top < offset + REVEAL_MARGIN {
            top - REVEAL_MARGIN
        } else if top + OUTLINER_ROW_HEIGHT + REVEAL_MARGIN > offset + view {
            top + OUTLINER_ROW_HEIGHT + REVEAL_MARGIN - view
        } else {
            return;
        };
        self.ui
            .set_scroll_offset_of(key, Vec2::new(0.0, wanted.max(0.0)));
    }
}

/// Moves the scrolling widget under `at` by `delta` pixels.
///
/// The tree takes no scroll input of its own, so the wheel is routed here:
/// whichever of last frame's scrolling blocks the pointer is over takes it, and
/// the next layout clamps the offset to that block's own content as it clamps
/// every other. A pointer over neither scrolls nothing — the viewport's wheel
/// is the camera's zoom, and the caller has already decided which this is.
fn scroll(ui: &mut Ui, scrollers: &[NodeKey], at: Vec2, delta: f32) {
    if delta == 0.0 {
        return;
    }
    let over = scrollers.iter().copied().find(|&key| {
        ui.rect(key).is_some_and(|(min, max)| {
            at.x >= min.x && at.x < max.x && at.y >= min.y && at.y < max.y
        })
    });
    let Some(key) = over else {
        return;
    };
    let offset = ui.scroll_offset_of(key);
    ui.set_scroll_offset_of(key, Vec2::new(offset.x, (offset.y + delta).max(0.0)));
}

/// The outliner pane: a title and the scene's rows. Returns the outliner
/// block's key.
fn build_outliner(
    ui: &mut Ui,
    state: &mut OutlinerState,
    options: &OutlinerOptions,
    outline: &[(String, Vec<SceneEntityId>)],
) -> NodeKey {
    let mut key = None;
    ui.block(".editor-panel", &[], |ui| {
        ui.span(".editor-title", "Scene", &[]);
        key = Some(
            ui.outliner_with(
                "#outline",
                state,
                options,
                |out| {
                    for (index, (_, ids)) in outline.iter().enumerate() {
                        out.branch(system_row(index), |out| {
                            for id in ids {
                                out.leaf(entity_row(*id));
                            }
                        });
                    }
                },
                |ui, row| {
                    let label = label_of(outline, row.id);
                    ui.span(".outliner-label", label.as_str(), &[]);
                },
            )
            .key,
        );
    });
    key.expect("the outliner is built inside its panel")
}

/// The inspector pane: a title naming what is selected, and its component's
/// rows — or a note, when nothing is.
fn build_inspector(
    ui: &mut Ui,
    document: &mut Document,
    selected: Option<SceneEntityId>,
    overrides: &Overrides,
    edits: &mut Vec<FieldEdit>,
) -> Option<NodeKey> {
    let mut key = None;
    ui.block(".editor-panel", &[], |ui| {
        let Some(id) = selected else {
            ui.span(".editor-title", "Properties", &[]);
            ui.span(".editor-note", "Nothing is selected", &[]);
            return;
        };
        let Some(component) = document.component(id) else {
            ui.span(".editor-title", "Properties", &[]);
            let note = format!("#{id} has no editable component");
            ui.span(".editor-note", note.as_str(), &[]);
            return;
        };
        let title = format!("#{id} {}", component.type_name());
        ui.span(".editor-title", title.as_str(), &[]);
        let options = InspectorOptions {
            overrides: Some(overrides),
            ..InspectorOptions::default()
        };
        let inspection = ui.inspector_with("#props", component, &options);
        key = Some(inspection.response.key);
        edits.extend(inspection.edits);
    });
    key
}

/// The row a system at `index` in [`Document::outline`] is drawn as.
const fn system_row(index: usize) -> OutlinerId {
    OutlinerId(SYSTEM_ROW + index as u64)
}

/// The row entity `id` is drawn as.
const fn entity_row(id: SceneEntityId) -> OutlinerId {
    OutlinerId(id.0 as u64)
}

/// The entity a row stands for, or [`None`] for a system's own row.
#[allow(clippy::cast_possible_truncation)]
const fn entity_of(row: OutlinerId) -> Option<SceneEntityId> {
    if row.0 < SYSTEM_ROW {
        Some(SceneEntityId(row.0 as u32))
    } else {
        None
    }
}

/// What a row reads: a system's name and how many entities it holds, or an
/// entity's own id.
fn label_of(outline: &[(String, Vec<SceneEntityId>)], row: OutlinerId) -> String {
    match entity_of(row) {
        Some(id) => format!("#{id}"),
        None => {
            let index = (row.0 - SYSTEM_ROW) as usize;
            match outline.get(index) {
                Some((system, ids)) => format!("{system} ({})", ids.len()),
                None => String::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    use crcbl::assets::MemorySource;
    use crcbl::math::DVec3;
    use crcbl::phys::Ray;
    use crcbl::ui::tree::{INSPECTOR_STEP, NodeKey};

    use crate::layout::default_layout;

    /// The framebuffer every page here is laid out over: the size the editor's
    /// own window opens at.
    const EXTENT: (u32, u32) = (960, 720);

    /// The panels over a document, driven a frame at a time with no device.
    struct Page {
        panels: Panels,
        document: Document,
    }

    impl Page {
        /// The panels over the compiled-in greybox scene.
        fn built_in() -> Self {
            Self::over(Document::built_in().expect("the compiled-in scene is a scene"))
        }

        fn over(mut document: Document) -> Self {
            let panels = Panels::new(&mut document, default_layout(), EXTENT);
            Self { panels, document }
        }

        /// One frame with the pointer at `pointer` and `scroll` pixels of
        /// wheel.
        fn frame(&mut self, pointer: PointerInput, scroll: f32) -> PanelFrame {
            self.panels.frame(
                &mut self.document,
                PanelInput {
                    pointer,
                    nav: NavInput::default(),
                    text: TextInput::default(),
                    extent: EXTENT,
                    select: SelectMode::Replace,
                    scroll,
                },
            )
        }

        /// A still frame with the pointer nowhere.
        fn idle(&mut self) -> PanelFrame {
            self.frame(PointerInput::hovering(Vec2::splat(-1.0)), 0.0)
        }

        /// A press and a release at `at`, and the frame after them — the tree
        /// resolves a click when the *next* frame begins.
        fn click(&mut self, at: Vec2) {
            self.frame(
                PointerInput {
                    pos: at,
                    down: true,
                    released: false,
                },
                0.0,
            );
            self.frame(
                PointerInput {
                    pos: at,
                    down: false,
                    released: true,
                },
                0.0,
            );
            self.idle();
        }

        /// Drags a value widget from `at` by `by` pixels, in one move: a press,
        /// a move past the drag threshold, and a release.
        fn drag(&mut self, at: Vec2, by: Vec2) {
            let held = |pos| PointerInput {
                pos,
                down: true,
                released: false,
            };
            self.frame(held(at), 0.0);
            self.frame(held(at + by), 0.0);
            self.frame(
                PointerInput {
                    pos: at + by,
                    down: false,
                    released: true,
                },
                0.0,
            );
        }

        /// The middle of a node the last frame laid out.
        fn centre(&self, key: NodeKey) -> Vec2 {
            let (min, max) = self.panels.ui().rect(key).expect("laid out last frame");
            (min + max) * 0.5
        }

        /// The drag-value editing component `axis` of the inspector's `row`th
        /// row: a vector row is a label span and then one `.inspector-axis`
        /// block per axis, each a label span and then its widget.
        fn axis_field(&self, row: usize, axis: usize) -> NodeKey {
            let props = self.panels.props_key().expect("the inspector was built");
            let ui = self.panels.ui();
            let rows = ui.child_keys(props);
            let cells = ui.child_keys(rows[row]);
            assert_eq!(
                cells.len(),
                1 + AXES_PER_ROW,
                "row {row} is not a vector row",
            );
            let cell = ui.child_keys(cells[1 + axis]);
            assert_eq!(cell.len(), 2, "an axis cell is a label and its widget");
            cell[1]
        }
    }

    /// How many components a `Block`'s vector rows have.
    const AXES_PER_ROW: usize = 3;

    /// A document of `count` greybox blocks in a row, for the tests that need
    /// more rows than an outliner can show at once.
    fn many_blocks(count: u32) -> Document {
        let mut rows = String::new();
        for id in 0..count {
            rows.push_str(&format!(
                "        ({id}, Block(\n            position: ({}.0, 0.0, 0.0),\n            \
                 half_extents: (0.4, 0.4, 0.4),\n        )),\n",
                id * 2,
            ));
        }
        let chunk = format!("Chunk(\n    system: \"blocks\",\n    entities: [\n{rows}    ],\n)");
        let mut source = MemorySource::new();
        for (key, text) in [
            (
                "many.scn/scene.ron",
                "Scene(\n    format: 0,\n    name: \"many\",\n    systems: [\n        \"blocks\",\n    ],\n)"
                    .to_owned(),
            ),
            (
                "many.scn/env.ron",
                "Env(\n    camera: Camera(\n        position: (0.0, 6.0, 14.0),\n        \
                 look_at: (0.0, 1.0, 0.0),\n    ),\n    ambient: (0.05, 0.05, 0.06),\n)"
                    .to_owned(),
            ),
            ("many.scn/sys/blocks.ron", chunk),
        ] {
            source
                .insert(Path::new(key), text.into_bytes())
                .expect("a nested scene key is a legal asset key");
        }
        Document::open(&source, Path::new("many.scn"), crate::scene::vocabulary())
            .expect("a scene this module just wrote")
    }

    /// **The outliner's rows are the document's outline**: one header per
    /// system and one row per entity under it, in the order the chunk file
    /// spells them — checked by clicking each row and reading back which entity
    /// it named, so a row that listed the right *number* of entities in the
    /// wrong order is red here.
    #[test]
    fn the_outliners_rows_are_the_documents_outline() {
        let mut page = Page::built_in();
        page.idle();
        let outline = page.document.outline();
        let entities: Vec<SceneEntityId> =
            outline.iter().flat_map(|(_, ids)| ids.clone()).collect();
        assert_eq!(
            page.panels.row_keys().len(),
            outline.len() + entities.len(),
            "a header per system and a row per entity",
        );

        for (index, id) in entities.iter().enumerate() {
            // Row 0 is the one system's header; the entities follow it.
            let row = page.panels.row_keys()[index + 1];
            let at = page.centre(row);
            page.click(at);
            assert_eq!(
                page.document.selected(),
                Some(*id),
                "row {} named {id} and selected something else",
                index + 1,
            );
        }

        // And a header selects nothing, which is what clicking a system means.
        let header = page.centre(page.panels.row_keys()[0]);
        page.click(header);
        assert_eq!(page.document.selected(), None);
    }

    /// **A document handed over with something already selected keeps it**, and
    /// the outliner opens showing that row selected — rather than the panels
    /// reading their own empty selection back over it on the first frame.
    #[test]
    fn the_panels_open_on_the_selection_the_document_already_had() {
        let mut document = Document::built_in().expect("the compiled-in scene is a scene");
        let id = SceneEntityId(2);
        document.select(Some(id));
        let panels = Panels::new(&mut document, default_layout(), EXTENT);
        assert_eq!(document.selected(), Some(id), "the panels cleared it");
        assert_eq!(panels.selected_rows(), [OutlinerId(u64::from(id.0))]);
    }

    /// **A ray pick moves the outliner's selection and scrolls its row into
    /// view.** The document is selected from outside the panels, exactly as
    /// [`crate::app`] selects it from a click in the viewport, and the row is
    /// far enough down that the outliner's window has to move for it.
    #[test]
    fn a_ray_pick_moves_the_outliner_and_scrolls_its_row_into_view() {
        const BLOCKS: u32 = 400;
        const WANTED: u32 = 300;

        let mut page = Page::over(many_blocks(BLOCKS));
        page.idle();
        let outliner = page.panels.outliner_key().expect("the outliner was built");
        assert_eq!(
            page.panels.ui().scroll_offset_of(outliner),
            Vec2::ZERO,
            "the outliner starts scrolled somewhere",
        );
        assert!(page.panels.selected_rows().is_empty());

        // What a pick does: the document's selection moves, and nothing tells
        // the panels.
        let id = SceneEntityId(WANTED);
        page.document.select(Some(id));
        page.idle();

        assert_eq!(
            page.panels.selected_rows(),
            [OutlinerId(u64::from(WANTED))],
            "the outliner did not follow the document's selection",
        );
        // Row 0 is the system's header, so the entity's row is one below its
        // own index.
        let row = (WANTED + 1) as f32 * OUTLINER_ROW_HEIGHT;
        let (min, max) = page.panels.ui().rect(outliner).expect("laid out");
        let offset = page.panels.ui().scroll_offset_of(outliner).y;
        assert!(offset > 0.0, "the outliner was not scrolled at all");
        assert!(
            offset <= row && row + OUTLINER_ROW_HEIGHT <= offset + (max.y - min.y),
            "row {row} is outside the view at offset {offset}, {} tall",
            max.y - min.y,
        );

        // And the document keeps the selection the panels were handed: a frame
        // that pushed the outliner's own answer back would clear it.
        assert_eq!(page.document.selected(), Some(id));
    }

    /// **An inspector edit becomes a command that undo reverses, and the thing
    /// on screen moves with it.**
    ///
    /// The observable is not the field: it is the entity's **bounds**, which is
    /// what the selection box is drawn from, and a **pick**, which is what the
    /// collider answers. An edit that wrote the component and skipped the
    /// command would satisfy neither undo nor the collider.
    #[test]
    fn an_inspector_edit_becomes_a_command_that_undo_reverses() {
        /// How far the drag goes, in pixels — one step per pixel, and a
        /// `Block`'s position carries no `#[reflect(step)]`, so this is
        /// `INSPECTOR_STEP` metres each.
        const PIXELS: f32 = 30.0;

        let mut page = Page::built_in();
        // The third step: it stands clear of its neighbours, so a pick either
        // side of its edge is a real question.
        let id = SceneEntityId(3);
        page.document.select(Some(id));
        page.idle();

        let (min, max) = page.document.bounds(id).expect("a block in this document");
        let was = (min + max) * 0.5;
        let edge = f64::from(min.x);
        let ray = |x: f64| Ray::new(DVec3::new(x, f64::from(was.y), 20.0), DVec3::NEG_Z);
        assert_eq!(
            page.document.pick(&ray(edge + 0.1)),
            Some(id),
            "the block does not pick just inside its own edge",
        );
        assert_eq!(
            page.document.pick(&ray(edge - 0.1)),
            None,
            "something else is already just outside it, so the picks below prove nothing",
        );

        // Row 0 is `Position`; component 0 is its x.
        let field = page.axis_field(0, 0);
        let at = page.centre(field);
        assert!(page.document.log().is_empty());
        page.drag(at, Vec2::new(PIXELS, 0.0));

        assert_eq!(
            page.document.log().position(),
            1,
            "one drag did not become exactly one command",
        );
        assert!(page.document.is_dirty(), "the dirty marker did not follow");
        let moved = {
            let (min, max) = page.document.bounds(id).expect("still there");
            (min + max) * 0.5
        };
        let by = f64::from(moved.x - was.x);
        assert!(
            (by - f64::from(PIXELS) * INSPECTOR_STEP).abs() < 1e-3,
            "the block moved {by} m, and {PIXELS} pixels is {} steps of \\
             {INSPECTOR_STEP}",
            PIXELS,
        );
        // The drag moved the block to the right, so the pixel just inside its
        // old left edge is now outside it.
        assert_eq!(
            page.document.pick(&ray(edge + 0.1)),
            None,
            "the collider did not follow the field the panel edited",
        );

        assert!(page.document.undo().expect("one entry"));
        assert_eq!(
            page.document.bounds(id).expect("still there"),
            (min, max),
            "undo did not put the block back exactly",
        );
        assert!(!page.document.is_dirty());
        assert_eq!(
            page.document.pick(&ray(edge + 0.1)),
            Some(id),
            "the collider did not follow the undo",
        );
    }

    /// **The wheel scrolls the panel the pointer is over**, and only that one —
    /// the tree takes no scroll of its own, so this is the editor's routing and
    /// nothing else's.
    #[test]
    fn the_wheel_scrolls_the_panel_under_the_pointer() {
        const BY: f32 = 48.0;

        let mut page = Page::over(many_blocks(200));
        page.document.select(Some(SceneEntityId(0)));
        page.idle();
        let outliner = page.panels.outliner_key().expect("built");
        let props = page.panels.props_key().expect("built");
        let offsets = |page: &Page| {
            (
                page.panels.ui().scroll_offset_of(outliner).y,
                page.panels.ui().scroll_offset_of(props).y,
            )
        };
        assert_eq!(offsets(&page), (0.0, 0.0));

        let on_outliner = page.centre(outliner);
        page.frame(PointerInput::hovering(on_outliner), BY);
        assert_eq!(
            offsets(&page),
            (BY, 0.0),
            "the wheel over the outliner did not scroll it, or scrolled both",
        );

        let on_props = page.centre(props);
        page.frame(PointerInput::hovering(on_props), BY);
        assert_eq!(
            offsets(&page),
            (BY, 0.0),
            "the inspector scrolled although its rows fit",
        );

        // Over neither: the viewport's wheel is the camera's, and the caller
        // hands this none — but a pointer between the panes must not move one
        // either.
        page.frame(PointerInput::hovering(Vec2::new(900.0, 400.0)), BY);
        assert_eq!(
            offsets(&page),
            (BY, 0.0),
            "a wheel outside a panel scrolled one"
        );

        // And it clamps at the top rather than running negative.
        page.frame(PointerInput::hovering(on_outliner), -4.0 * BY);
        assert_eq!(offsets(&page), (0.0, 0.0), "the offset ran past the top");
    }

    /// **The viewport pane's rectangle is the hole in the panels**, and
    /// [`Panels::in_viewport`] answers for it: a point in a panel is not in it,
    /// a point in the pane is, and the two together cover the window.
    #[test]
    fn the_viewport_rectangle_is_what_the_panels_leave() {
        let mut page = Page::built_in();
        // With something selected, so the inspector holds its widest rows: a
        // panel that may grow past its pane grows *here*, and the column split
        // hands the extra width to the outliner beside it.
        page.document.select(Some(SceneEntityId(3)));
        let frame = page.idle();
        let (min, max) = frame.viewport;
        assert_eq!(page.panels.viewport(), (min, max));
        assert!(
            min.x > 0.0 && max.x == EXTENT.0 as f32 && max.y == EXTENT.1 as f32,
            "the viewport is not the right-hand pane: {min:?}..{max:?}",
        );

        for key in [
            page.panels.outliner_key().expect("built"),
            page.panels.props_key().expect("built"),
        ] {
            let (panel_min, panel_max) = page.panels.ui().rect(key).expect("laid out");
            assert!(
                panel_max.x <= min.x,
                "a panel reaches into the viewport: {panel_max:?} against {min:?}",
            );
            assert!(
                !page.panels.in_viewport(panel_min + Vec2::splat(1.0))
                    && !page.panels.in_viewport(panel_max - Vec2::splat(1.0)),
                "a panel's own corners are inside the viewport pane",
            );
        }
        assert!(page.panels.in_viewport((min + max) * 0.5));
        assert!(!page.panels.in_viewport(min - Vec2::splat(1.0)));
    }
}
