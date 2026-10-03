//! The editor's panels: a docked outliner, asset browser and inspector beside
//! the viewport, and the two-way join between what they show and what the
//! [`Document`] holds.
//!
//! **Nothing here touches a device or a window**, which is
//! [`crate::document`]'s rule extended to the panels: a frame of this is a
//! [`Ui`] tree laid out against a [`FontAtlas`] and emitted into a
//! [`DrawList`], all of it arithmetic. [`crate::app`] opens the window, feeds
//! this the pointer and the keyboard, and hands the draw list to the renderer.
//! The tests below drive the real widgets — real clicks, at real rectangles —
//! with no GPU at all.
//!
//! # The four panes
//!
//! [`crate::layout`] names them and holds the [`DockLayout`] they are arranged
//! by. Three are built here — the outliner, the inspector and the asset browser
//! (`assets`' module docs) — and the fourth, the viewport, is a block with
//! nothing in it that the frame fills with **the scene's own picture**: a
//! [`DrawList::texture`] rectangle over the pane's laid-out rectangle, naming
//! [`VIEWPORT_TEXTURE`], which [`crate::app`] pairs with the target it drew the
//! scene into — sized to [`Panels::viewport_pixels`] — when the frame's passes
//! are added. That is `docs/plan/08-editor.md`'s decision of 2026-09-30: a view
//! rendered to a texture a UI rect samples, rather than the scene drawn
//! full-window under a hole in the panels.
//!
//! The rectangle is pushed **first**, under every panel, so a menu or a
//! dragged divider drawn over the pane covers the picture rather than the other
//! way round. It is also what a click has to land in to be a pick, which is
//! why [`PanelFrame`] carries it — and the ray is cast through the pane's own
//! camera from the click's position inside it, because that is the picture
//! under the cursor.
//!
//! # The toolbar
//!
//! A strip over the panes, outside the dock, with play mode's two buttons and
//! where play stands — so it costs the saved layout nothing: the dock's panes
//! are the ones [`crate::layout::load`] checks a settings file against, and a
//! pane a layout does not hold has to be migrated into it, as the asset
//! browser is (`crate::layout`'s module docs). A click is not acted
//! on here: [`PanelFrame::toolbar`] hands the [`Action`] its key would have
//! asked for back to [`crate::app`], which carries both out the same way.
//!
//! While a scene plays and its game offers play controls, a second strip
//! under the toolbar lists them and the run's numbers (`play`'s module docs):
//! a click there is a command the game's module is sent, not an edit.
//!
//! # Selection, in both directions
//!
//! One witness carries it: the selection the outliner was last told about. At
//! the top of a frame a document selection that is not that one came from
//! somewhere else — a ray pick in the viewport — so it is pushed into the
//! outliner, each entity's system expanded and the primary's row scrolled into
//! view. At the bottom, whatever the outliner's rows now say is pushed back
//! into the document. So neither side owns it and neither can drift.
//!
//! The document's selection is ordered and the outliner's is a set, so the
//! push back keeps the document's order for the entities still selected,
//! appends the ones the click added in row order, and makes the row the click
//! acted on the primary: a plain or Ctrl click's row, which the outliner keeps
//! as its anchor, and a Shift click's — the end of the run away from the
//! anchor. Every selected row is `:checked`, and the primary's label reads in
//! the amber its box is drawn in.
//!
//! **Clicking a row also takes the keyboard**, because the row is a focusable
//! node: [`crate::app`] pushes the reserved `ui` context while
//! [`Panels::holds_keyboard`], so the arrows then walk the outliner instead of
//! nudging the selection, and a click in the viewport calls
//! [`Panels::release_keyboard`] to give them back. That is click-to-focus, and
//! it is what makes the arrows one thing at a time rather than two.
//!
//! # Names, and renaming a row in place
//!
//! A named entity's row reads its name and then its id; an unnamed one reads
//! its id, as before names existed. F2 on the selection, or a double-click on
//! a row, puts a text input in that row ([`Panels::begin_rename`]), engaged so
//! the next key typed is the name's. Accept or a click elsewhere commits it
//! through [`Document::rename`] — one undoable command, refused in play mode —
//! and back cancels it. Clearing the text takes the name away.
//!
//! # Which field a clipboard key means
//!
//! [`Panels::field_target`] is the inspector leaf the keyboard means: the one
//! whose widget holds focus, or else the one under the pointer. Copy and paste
//! act on that field when there is one, and on the selected entity when there
//! is not — so a click into the viewport or onto an outliner row, which takes
//! focus away from the inspector, puts the keys back on the entity. A text
//! field being typed into answers both itself, before either is asked.
//!
//! # An inspector edit is a command
//!
//! The inspector draws a section per system holding the selected entity
//! (`inspector`'s module docs), each through [`Ui::inspector_with`], which
//! edits that component and reports each write as a `FieldEdit`. Each becomes
//! an [`EditCommand`](crate::command::EditCommand) on that section's system
//! through [`Document::record_edits`], which rewinds the panel's own write first
//! so the inverse the log records is exact — that method's docs say why. Undo,
//! redo, the dirty marker and the collider all follow, because they follow
//! every command. A section's remove button and the add buttons under the
//! sections are [`Document::detach`] and [`Document::attach`].
//!
//! # What the outliner costs
//!
//! The rows are virtualized: a frame builds the rows its window shows and no
//! more, whatever the scene holds. What is *not* free is the flatten, and the
//! outline it is flattened from — so the outline is read once and re-read only
//! when [`Document::membership`] moves, which is an entity entering or leaving,
//! and the flatten runs only when the widget says its model is stale.

use std::collections::BTreeMap;

use crcbl::math::Vec2;
use crcbl::scene::scn::SceneEntityId;
use crcbl::ui::style::Declaration;
use crcbl::ui::tree::{
    AvailableSpace, ClipboardRequest, DockLayout, Engagement, FieldEdit, LengthAuto, NavInput,
    NodeKey, OUTLINER_ROW_HEIGHT, OutlinerId, OutlinerOptions, OutlinerState, Overrides,
    SelectMode, TextInput, TextInputOptions, Ui,
};
use crcbl::ui::{DrawList, FontAtlas, PointerInput, TextureId};

use crate::command::Gesture;
use crate::document::{Document, EditError, PlayState};
use crate::keys::Action;
use crate::layout::{self, PANE_MIN};

mod assets;
mod inspector;
mod play;

/// The name the viewport pane's picture goes by in the panels' draw list —
/// see the module docs. The only texture the editor draws, so the first
/// number.
pub const VIEWPORT_TEXTURE: TextureId = TextureId::new(0);

/// The tint the viewport's picture is drawn with: none, so the pane shows the
/// scene's pixels as the renderer wrote them.
const UNTINTED: [f32; 4] = [1.0; 4];

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
#editor { flex-direction: column; }

#panes { flex-grow: 1; min-width: 0; min-height: 0; }

#status {
  flex-shrink: 0;
  padding: 2px 6px;
  background: #1b1f27;
  color: #9aa3b2;
}

#status.warning { color: #e0b050; }

#toolbar {
  flex-shrink: 0;
  align-items: center;
  padding: 2px 4px;
  background: #1b1f27;
}

#toolbar button { margin-right: 4px; }

#play-state { padding: 0 6px; color: #9aa3b2; }

#play-controls {
  flex-shrink: 0;
  flex-wrap: wrap;
  align-items: center;
  padding: 2px 4px;
  background: #1b1f27;
}

.play-game { flex-direction: row; align-items: center; }

.play-game button { margin-right: 4px; }

.play-status { padding: 0 6px; color: #9aa3b2; }

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

.outliner-rename { flex-grow: 1; min-width: 0; }

.outliner-label.primary { color: #ffb833; }

#props {
  flex-direction: column;
  flex-grow: 1;
  min-width: 0;
  min-height: 0;
  overflow: scroll;
}

.inspector-section, .inspector-add { flex-direction: column; flex-shrink: 0; }

.section-head {
  align-items: center;
  padding: 2px 4px;
  background: #1b1f27;
  color: #9aa3b2;
}

.section-title { flex-grow: 1; min-width: 0; }

.inspector-add .section-title { padding: 2px 4px; color: #9aa3b2; }

.add-group { flex-direction: row; flex-wrap: wrap; align-items: center; min-width: 0; }

.add-group-label { padding: 2px 4px; color: #7a8190; }
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
    /// This frame's navigation edges, from [`crcbl::ui_nav::nav_input`].
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

/// The inspector leaf a clipboard key means: see the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldTarget {
    /// Whose component the inspector was drawn for.
    pub entity: SceneEntityId,
    /// Which of its components: the system of the section the leaf is in.
    pub system: String,
    /// The leaf's path inside it.
    pub path: String,
}

/// An outliner row being renamed in place: see the module docs.
#[derive(Debug)]
struct Renaming {
    /// Whose row.
    id: SceneEntityId,
    /// What the row's text input holds.
    text: String,
    /// Whether the input has been engaged — asked for once it was first laid
    /// out, because the tree engages only a node it has laid out.
    engaged: bool,
}

/// What one frame of the panels did.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelFrame {
    /// The viewport pane's rectangle in framebuffer pixels, as this frame laid
    /// it out: `(top-left, bottom-right)`. A pointer outside it is a panel's.
    pub viewport: (Vec2, Vec2),
    /// How many commands this frame's inspector edits became.
    pub commands: usize,
    /// What a click on the toolbar asked for this frame — the action the
    /// button's key asks for — for the caller to carry out.
    pub toolbar: Option<Action>,
    /// The mesh asset accept was pressed on in the asset browser this frame,
    /// for the caller to place at the view's centre.
    pub spawn: Option<String>,
}

/// The inspector's sections, add buttons and add-list headings, as a frame
/// built them.
#[derive(Debug, Default)]
struct Inspected {
    sections: Vec<inspector::Section>,
    /// Each add button and the system it attaches, in the order drawn.
    adds: Vec<(String, NodeKey)>,
    /// Each add-list heading and the label it reads, in the order drawn.
    headings: Vec<(String, NodeKey)>,
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
    /// [`Document::outline`], read again only when the document's membership
    /// moves — see the module docs.
    outline: Vec<(String, Vec<SceneEntityId>)>,
    /// The [`Document::membership`] the outline was read at.
    counted: u64,
    /// Every named entity's name, read again when the membership or
    /// [`Document::naming`] moves.
    names: BTreeMap<SceneEntityId, String>,
    /// The [`Document::naming`] the names were read at.
    named: u64,
    /// The row being renamed, if one is.
    renaming: Option<Renaming>,
    /// The inspector leaf a clipboard key means, as the last frame found it.
    field: Option<FieldTarget>,
    /// The selection the outliner was last told about: the witness that says
    /// which side changed it. See the module docs.
    shown: Vec<SceneEntityId>,
    /// A row to put in view once this frame has been laid out.
    reveal: Option<SceneEntityId>,
    /// The viewport pane's rectangle, as the last frame laid it out.
    viewport: (Vec2, Vec2),
    /// The outliner block, as the last frame laid it out.
    outliner_key: Option<NodeKey>,
    /// The inspector block, as the last frame laid it out.
    props_key: Option<NodeKey>,
    /// The inspector, as the last frame built it.
    inspector: Inspected,
    /// The field a held pointer is dragging — its system and the paths it
    /// writes, comma-joined — and the gesture its edits share; see
    /// [`Panels::apply_edits`].
    field_gesture: Option<(SceneEntityId, String, String, Gesture)>,
    /// The line under the panes: what the editor last had to say, and whether
    /// it is a warning — see [`Panels::set_status`].
    status: (String, Tone),
    /// The status line, as the last frame laid it out.
    status_key: Option<NodeKey>,
    /// The toolbar, as the last frame laid it out.
    toolbar_key: Option<NodeKey>,
    /// The asset browser's listing and rows — see `assets`.
    browser: assets::Browser,
    /// The play strip's choices and what it last drew — see `play`.
    strip: play::Strip,
}

/// How the status line reads a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// What happened, as it was asked for.
    Info,
    /// Something a person should look at: a refusal, or a save its game would
    /// not load.
    Warning,
}

/// What the status line says before anything has happened.
const READY: &str = "Ready";

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
            overrides: inspector::overrides(),
            counted: document.membership(),
            outline,
            names: names_of(document),
            named: document.naming(),
            renaming: None,
            field: None,
            // Deliberately not the document's selection: leaving the witness
            // empty makes the idle frame below *push* whatever is selected into
            // the outliner, so a document handed over with a selection keeps it
            // rather than having it read back off an outliner that has none.
            shown: Vec::new(),
            reveal: None,
            viewport: (Vec2::ZERO, Vec2::ZERO),
            outliner_key: None,
            props_key: None,
            inspector: Inspected::default(),
            field_gesture: None,
            status: (READY.to_owned(), Tone::Info),
            status_key: None,
            toolbar_key: None,
            browser: assets::Browser::new(document.assets()),
            strip: play::Strip::default(),
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

    /// The viewport pane's rectangle in **window** pixels: [`viewport`]'s,
    /// converted at the draw list's [`scale`](DrawList::scale).
    ///
    /// What the scene's target is sized to and what a click is measured
    /// against, so the picture is drawn one texel to one window pixel at any
    /// scale and a click lands on the texel under it. At a scale of one — the
    /// only one this editor lays out at today — it is [`viewport`] itself.
    ///
    /// [`viewport`]: Self::viewport
    #[must_use]
    pub fn viewport_pixels(&self) -> (Vec2, Vec2) {
        let scale = self.list.scale();
        (self.viewport.0 * scale, self.viewport.1 * scale)
    }

    /// The whole pixels [`viewport_pixels`](Self::viewport_pixels) covers,
    /// never less than one on a side: the extent the scene is drawn at and a
    /// click is unprojected against.
    #[must_use]
    pub fn viewport_extent(&self) -> (u32, u32) {
        let (min, max) = self.viewport_pixels();
        let size = (max - min).round().max(Vec2::ONE);
        (size.x as u32, size.y as u32)
    }

    /// Whether `at`, in window pixels, is inside the viewport pane — whether,
    /// that is, a click there is the scene's rather than a panel's.
    #[must_use]
    pub fn in_viewport(&self, at: Vec2) -> bool {
        let (min, max) = self.viewport_pixels();
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

    /// The inspector leaf a clipboard key means, as the last frame found it:
    /// the one whose widget holds focus, else the one under the pointer, else
    /// none. See the module docs.
    #[must_use]
    pub const fn field_target(&self) -> Option<&FieldTarget> {
        self.field.as_ref()
    }

    /// Puts a text input in `id`'s outliner row, holding its name, and engages
    /// it once it is laid out — the next frame — so what is typed after that
    /// renames it. See the module docs.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, as every edit is, and
    /// [`EditError::NoEntity`] for an id `document` does not hold. Nothing
    /// is put in the row in either case.
    pub fn begin_rename(
        &mut self,
        document: &Document,
        id: SceneEntityId,
    ) -> Result<(), EditError> {
        if document.play_state() != PlayState::Editing {
            return Err(EditError::Playing);
        }
        let Some(index) = self.outline.iter().position(|(_, ids)| ids.contains(&id)) else {
            return Err(EditError::NoEntity(id));
        };
        // The row has to be built for its input to be: open its system and
        // scroll it into view, as a pick does.
        self.outliner.set_expanded(system_row(index), true);
        self.reveal = Some(id);
        self.renaming = Some(Renaming {
            id,
            text: self.names.get(&id).cloned().unwrap_or_default(),
            engaged: false,
        });
        Ok(())
    }

    /// The entity whose row is being renamed, if one is.
    #[must_use]
    pub fn renaming(&self) -> Option<SceneEntityId> {
        self.renaming.as_ref().map(|renaming| renaming.id)
    }

    /// Puts `text` on the status line under the panes, read as `tone`, until
    /// the next message replaces it.
    pub fn set_status(&mut self, text: impl Into<String>, tone: Tone) {
        self.status = (text.into(), tone);
    }

    /// The status line's rectangle as the last frame laid it out.
    #[cfg(test)]
    fn status_rect(&self) -> Option<(Vec2, Vec2)> {
        self.ui.rect(self.status_key?)
    }

    /// What the status line says, and how.
    #[must_use]
    pub fn status(&self) -> (&str, Tone) {
        (&self.status.0, self.status.1)
    }

    /// What the last frame drew.
    #[must_use]
    pub const fn draw_list(&self) -> &DrawList {
        &self.list
    }

    /// Physical pixels per logical one: what the list's pushes are scaled by.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.list.scale()
    }

    /// Lets `draw` push onto this frame's list over the viewport pane, clipped
    /// to it — what the gizmo is drawn with, after the frame that laid the pane
    /// out.
    ///
    /// The list's coordinates are logical pixels, which
    /// [`DrawList::to_logical`] turns a window pixel into.
    pub fn overlay_viewport(&mut self, draw: impl FnOnce(&mut DrawList)) {
        let (min, max) = self.viewport;
        self.list.push_clip(min, max);
        draw(&mut self.list);
        self.list
            .pop_clip()
            .expect("the clip pushed just above is on the stack");
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

    /// The toolbar's play and pause buttons, as the last frame laid them out.
    #[cfg(test)]
    pub(crate) fn toolbar_buttons(&self) -> [NodeKey; 2] {
        let keys = self
            .ui
            .child_keys(self.toolbar_key.expect("the toolbar is laid out"));
        [keys[0], keys[1]]
    }

    /// The play strip, as the last frame laid it out — [`None`] on a frame
    /// that drew none.
    #[cfg(test)]
    pub(crate) const fn play_strip(&self) -> Option<NodeKey> {
        self.strip.key()
    }

    /// The play strip's buttons, as the last frame laid them out, each with
    /// the label it read: an action's name, or a choice's current pick.
    #[cfg(test)]
    pub(crate) fn play_buttons(&self) -> &[(String, NodeKey)] {
        self.strip.buttons()
    }

    /// The run's numbers the play strip last drew, each with its label.
    #[cfg(test)]
    pub(crate) fn play_readout(&self) -> &[(&'static str, String)] {
        self.strip.shown()
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

    /// The systems of the inspector's sections, in the order the last frame
    /// drew them: the selection's systems, in manifest order.
    #[must_use]
    pub fn section_systems(&self) -> Vec<String> {
        self.inspector
            .sections
            .iter()
            .map(|section| section.system.clone())
            .collect()
    }

    /// The rows of the inspector's `index`th section — its inspector block —
    /// as the last frame laid it out, or [`None`] past the last section.
    #[must_use]
    pub fn section_fields(&self, index: usize) -> Option<NodeKey> {
        self.inspector
            .sections
            .get(index)
            .map(|section| section.fields)
    }

    /// The remove button of the section the last frame drew for `system`, if
    /// it drew one — it draws none while the selection is in one system.
    #[must_use]
    pub fn remove_button(&self, system: &str) -> Option<NodeKey> {
        self.inspector
            .sections
            .iter()
            .find(|section| section.system == system)
            .and_then(|section| section.remove)
    }

    /// The add buttons under the sections, each with the system it attaches,
    /// as the last frame laid them out.
    #[must_use]
    pub fn add_buttons(&self) -> Vec<(String, NodeKey)> {
        self.inspector.adds.clone()
    }

    /// The add list's headings, each with the label it reads, as the last
    /// frame laid them out — one per
    /// [`Document::attachable_groups`](crate::document::Document::attachable_groups)
    /// group, each above its buttons.
    #[must_use]
    pub fn add_headings(&self) -> Vec<(String, NodeKey)> {
        self.inspector.headings.clone()
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

    /// The mesh asset whose browser row is under `at`, in window pixels, as
    /// the last frame laid it out — what a press there starts dragging into
    /// the viewport.
    #[must_use]
    pub fn asset_at(&self, at: Vec2) -> Option<String> {
        self.browser
            .asset_at(&self.ui, self.list.to_logical(at))
            .map(str::to_owned)
    }

    /// Lists `document`'s asset source again — what a caller does after
    /// handing the document another one ([`Document::set_assets`]), and what
    /// the browser's refresh button does.
    pub fn relist_assets(&mut self, document: &Document) {
        self.browser.relist(document.assets());
    }

    /// The mesh assets the browser lists, in tree order.
    #[must_use]
    pub fn listed_assets(&self) -> Vec<&str> {
        self.browser.assets()
    }

    /// The folders the browser lists, in tree order.
    #[must_use]
    pub fn listed_folders(&self) -> Vec<&str> {
        self.browser.folders()
    }

    /// What the browser says beside its tree, if anything: why nothing could
    /// be listed, that nothing was, or that the walk stopped short.
    #[must_use]
    pub fn assets_note(&self) -> Option<&str> {
        self.browser.note()
    }

    /// The browser's rows, as the last frame built them, each with the key of
    /// what it stands for.
    #[cfg(test)]
    pub(crate) fn asset_rows(&self) -> Vec<(NodeKey, &str)> {
        self.browser.rows()
    }

    /// The browser's refresh button, as the last frame laid it out.
    #[cfg(test)]
    pub(crate) fn refresh_button(&self) -> Option<NodeKey> {
        self.browser.refresh_button()
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
        let selected = document.primary();
        let play = document.play_state();
        let selected_label = selected.map_or_else(String::new, |id| {
            let label = label_of(&self.outline, &self.names, entity_row(id));
            match document.selection().len() {
                1 => label,
                count => format!("{label} (primary of {count} selected)"),
            }
        });
        let mut toolbar = None;
        let mut toolbar_key = None;
        let mut viewport = None;
        let mut status_key = None;
        let mut outliner_key = None;
        let mut built = inspector::Built::default();
        let mut rename_input = None;
        let mut double_clicked = None;
        let mut relist = false;
        let mut asked_play = None;
        // Owned, so the strip is built while the inspector borrows the
        // document: the controls are a few pointers and the numbers a few
        // strings.
        let controls: Vec<_> = document
            .play_controls()
            .into_iter()
            .map(|(system, controls)| (system.to_owned(), *controls))
            .collect();
        let play_status = document.play_status();

        // Last frame's scrolling blocks, read before the fields are borrowed
        // below — and last frame's is the right answer anyway, because that is
        // the layout the pointer is over.
        let scrollers: Vec<NodeKey> = [self.outliner_key, self.props_key, self.browser.key()]
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
            names,
            renaming,
            status,
            browser,
            strip,
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
                (toolbar_key, toolbar) = build_toolbar(ui, play);
                asked_play = strip.build(ui, &controls, play_status);
                ui.dock("#panes", layout, PANE_MIN, |ui, pane| match pane {
                    // Built empty: the scene's picture is pushed over its
                    // rectangle once it is laid out. See the module docs.
                    layout::VIEWPORT => viewport = ui.current_key(),
                    layout::OUTLINER => {
                        let rows = Rows {
                            outline,
                            names,
                            renaming: renaming.as_mut(),
                            primary: selected,
                        };
                        let built = build_outliner(ui, outliner, &options, rows);
                        outliner_key = Some(built.key);
                        rename_input = built.rename;
                        double_clicked = outliner.double_clicked().and_then(entity_of);
                    }
                    layout::INSPECTOR => {
                        built =
                            inspector::build(ui, document, selected, &selected_label, overrides);
                    }
                    layout::ASSETS => relist = browser.build(ui, &options),
                    other => {
                        // Unreachable while `crate::layout::load` refuses a
                        // layout whose panes are not this build's, and said out
                        // loud rather than left blank so that a fourth pane
                        // added without a builder is visible rather than empty.
                        let note = format!("no pane called {other}");
                        ui.span(".editor-note", note.as_str(), &[]);
                    }
                });
                // Always drawn, "Ready" before anything happens, so a message
                // arriving never changes the viewport's size.
                let (text, tone) = &*status;
                let class = match tone {
                    Tone::Info => "#status",
                    Tone::Warning => "#status.warning",
                };
                status_key = Some(ui.span(class, text.as_str(), &[]).key);
            },
        );
        ui.layout(Vec2::ZERO, AvailableSpace::definite(extent), atlas);
        list.clear();
        // The scene's picture, under every panel: see the module docs.
        if let Some((min, max)) = viewport.and_then(|key| ui.rect(key)) {
            list.texture(
                min,
                max,
                VIEWPORT_TEXTURE,
                (Vec2::ZERO, Vec2::ONE),
                UNTINTED,
            );
        }
        ui.emit(list);

        if let Some(rect) = viewport.and_then(|key| self.ui.rect(key)) {
            self.viewport = rect;
        }
        self.outliner_key = outliner_key;
        self.status_key = status_key;
        self.toolbar_key = toolbar_key;
        let inspector::Built {
            props,
            sections,
            adds,
            headings,
            edits,
            field,
            change,
        } = built;
        self.props_key = props;
        self.inspector = Inspected {
            sections,
            adds,
            headings,
        };
        self.field = selected
            .zip(field)
            .map(|(entity, (system, path))| FieldTarget {
                entity,
                system,
                path,
            });
        if let (Some(key), Some(id)) = (outliner_key, self.reveal.take()) {
            self.reveal_row(key, id);
        }

        let commands = self.apply_edits(document, selected, &edits, input.pointer);
        if let (Some(id), Some(change)) = (selected, change) {
            let outcome = match &change {
                inspector::Change::Attach(system) => document.attach(id, system),
                inspector::Change::Detach(system) => document.detach(id, system),
            };
            if let Err(error) = outcome {
                crcbl::log::warn!("editor: {error}");
                self.set_status(error.to_string(), Tone::Warning);
            }
        }
        // Read off the focus before a relist could take the row away.
        let spawn = if input.nav.accept {
            self.browser.focused_asset(&self.ui).map(str::to_owned)
        } else {
            None
        };
        if relist {
            self.relist_assets(document);
        }
        if let Some(asked) = asked_play {
            match self.strip.send(document, &controls, &asked) {
                Ok(sent) => self.set_status(sent, Tone::Info),
                Err(unsent) => {
                    crcbl::log::warn!("editor: {unsent}");
                    self.set_status(unsent, Tone::Warning);
                }
            }
        }
        self.follow_outliner(document, input.select);
        self.follow_rename(document, rename_input);
        if let Some(id) = double_clicked
            && let Err(error) = self.begin_rename(document, id)
        {
            self.set_status(error.to_string(), Tone::Warning);
        }
        PanelFrame {
            viewport: self.viewport,
            commands,
            toolbar,
            spawn,
        }
    }

    /// Reads [`Document::outline`] again when the document has gained or lost
    /// an entity, and the names when one was renamed as well.
    ///
    /// On [`Document::membership`], not the entity count: a delete and a
    /// duplicate leave the count where it was and the rows different.
    fn refresh(&mut self, document: &mut Document) {
        let joined = document.membership() != self.counted;
        if joined || document.naming() != self.named {
            self.named = document.naming();
            self.names = names_of(document);
        }
        if !joined {
            return;
        }
        self.counted = document.membership();
        self.outline = document.outline();
        self.outliner.invalidate();
    }

    /// Carries a rename forward a frame: engages its input the first frame it
    /// is laid out, commits it when the input commits, and drops it when the
    /// input cancels, is not built, or did not take the engagement.
    ///
    /// `input` is the rename's text input as this frame built it, and where
    /// its engagement stood.
    fn follow_rename(&mut self, document: &mut Document, input: Option<(NodeKey, Engagement)>) {
        let Some(renaming) = &mut self.renaming else {
            return;
        };
        let Some((key, engagement)) = input else {
            // Its row is gone — deleted, or scrolled out of the window.
            self.renaming = None;
            return;
        };
        if !renaming.engaged {
            self.ui.engage(key);
            renaming.engaged = true;
            return;
        }
        match engagement {
            Engagement::Began | Engagement::Engaged => {}
            Engagement::Committed => {
                let Some(Renaming { id, text, .. }) = self.renaming.take() else {
                    return;
                };
                if let Err(error) = document.rename(id, &text) {
                    crcbl::log::warn!("editor: {error}");
                    self.set_status(error.to_string(), Tone::Warning);
                }
            }
            Engagement::Cancelled | Engagement::Idle => self.renaming = None,
        }
    }

    /// Pushes a selection the document gained from somewhere else — a ray pick
    /// in the viewport, a delete taking an entity out of it — into the
    /// outliner, and asks for the primary's row.
    fn follow_document(&mut self, document: &Document) {
        let selection = document.selection();
        if selection == self.shown.as_slice() {
            return;
        }
        self.shown = selection.to_vec();
        self.outliner.clear_selection();
        for &id in selection {
            // A row inside a collapsed system is not in the flattened model,
            // so nothing would scroll to it and nothing would show it
            // selected.
            if let Some(index) = self.outline.iter().position(|(_, ids)| ids.contains(&id)) {
                self.outliner.set_expanded(system_row(index), true);
            }
            // Toggled on in the document's order, so the last — the primary —
            // is the anchor a Shift click runs from.
            self.outliner.select(entity_row(id), SelectMode::Toggle);
        }
        self.reveal = document.primary();
    }

    /// Pushes what the outliner's rows now say back into the document, after
    /// a click whose modifier said `mode` — see the module docs for the order
    /// and the primary.
    ///
    /// A system's own row selects nothing, which is what clicking a header
    /// means. An entity is kept or dropped by the outliner's whole selection,
    /// not by the rows it shows, so collapsing a system does not deselect what
    /// is in it.
    fn follow_outliner(&mut self, document: &mut Document, mode: SelectMode) {
        let shown: Vec<SceneEntityId> = self.outliner.selected().filter_map(entity_of).collect();
        let kept: Vec<SceneEntityId> = document
            .selection()
            .iter()
            .copied()
            .filter(|id| self.outliner.is_selected(entity_row(*id)))
            .collect();
        let added: Vec<SceneEntityId> = shown
            .into_iter()
            .filter(|id| !document.is_selected(*id))
            .collect();
        if added.is_empty() && kept.len() == document.selection().len() {
            self.shown = document.selection().to_vec();
            return;
        }
        let mut next = kept;
        next.extend(added);
        if let Some(acted) = self.acted_on(mode)
            && let Some(at) = next.iter().position(|id| *id == acted)
        {
            let id = next.remove(at);
            next.push(id);
        }
        document.set_selection(next);
        self.shown = document.selection().to_vec();
    }

    /// The entity whose row a click with `mode` acted on: the outliner's
    /// anchor, which a plain or a Ctrl click moves to its row, or for a Shift
    /// click the end of the selected run away from the anchor, which a range
    /// leaves where it was. [`None`] when that row is a system's.
    fn acted_on(&self, mode: SelectMode) -> Option<SceneEntityId> {
        let anchor = self.outliner.anchor()?;
        let row = match mode {
            SelectMode::Replace | SelectMode::Toggle => anchor,
            SelectMode::Range => {
                let mut run = self.outliner.selected();
                let first = run.next()?;
                if first == anchor {
                    run.last().unwrap_or(first)
                } else {
                    first
                }
            }
        };
        entity_of(row)
    }

    /// Turns this frame's inspector edits into commands on `document`.
    ///
    /// `selected` is the entity the inspector was **drawn** for, which is the
    /// one the paths are relative to — reading the document's selection again
    /// here would attach an edit to whatever a click in the outliner selected
    /// in the same frame.
    ///
    /// # One drag, one entry
    ///
    /// A dragged field reports an edit every frame it moves. Edits to one leaf
    /// while `pointer` holds the primary button — and on the frame it comes up
    /// — share a [`Gesture`], which the log folds into
    /// one entry; a frame with the button up ends it, so the next drag is an
    /// entry of its own. The tree has no pointer capture to ask instead, and a
    /// held button over one leaf is what a drag is. A typed edit, made with the
    /// button up, is an entry each.
    ///
    /// # One row, one command
    ///
    /// A frame's consecutive edits to one section are one command
    /// ([`Document::record_edits`]): a widget edits one leaf, so several in one
    /// section in one frame are a row that writes several leaves together —
    /// the rotation row writing a quaternion's four — and splitting them would
    /// let an undo take back one leaf of a value whose leaves only mean
    /// something together. Their gesture is keyed by every path they name.
    fn apply_edits(
        &mut self,
        document: &mut Document,
        selected: Option<SceneEntityId>,
        edits: &[(String, FieldEdit)],
        pointer: PointerInput,
    ) -> usize {
        let held = pointer.down || pointer.released;
        let mut applied = 0;
        if let Some(id) = selected {
            for group in edits.chunk_by(|(a, _), (b, _)| a == b) {
                let system = &group[0].0;
                let row: Vec<FieldEdit> = group.iter().map(|(_, edit)| edit.clone()).collect();
                let paths = row
                    .iter()
                    .map(|edit| edit.path.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let gesture = held.then(|| match &self.field_gesture {
                    Some((entity, held_system, held_paths, gesture))
                        if *entity == id && held_system == system && *held_paths == paths =>
                    {
                        *gesture
                    }
                    _ => {
                        let gesture = document.begin_gesture();
                        self.field_gesture = Some((id, system.clone(), paths.clone(), gesture));
                        gesture
                    }
                });
                match document.record_edits(id, system, &row, gesture) {
                    Ok(()) => applied += 1,
                    Err(error) => {
                        crcbl::log::warn!("editor: {error}");
                        self.set_status(error.to_string(), Tone::Warning);
                    }
                }
            }
        }
        if !pointer.down {
            self.field_gesture = None;
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

/// The toolbar over the panes: play or stop, pause or resume, and where play
/// stands. Returns its key, and what a click on it asked for.
///
/// Each button is labelled with what it does **now** and the key that does the
/// same, so the strip is also where a person learns F5 and F6.
fn build_toolbar(ui: &mut Ui, play: PlayState) -> (Option<NodeKey>, Option<Action>) {
    let (start, hold, state) = match play {
        PlayState::Editing => ("Play (F5)", "Pause (F6)", "Editing"),
        PlayState::Playing => ("Stop (F5)", "Pause (F6)", "Playing"),
        PlayState::Paused => ("Stop (F5)", "Resume (F6)", "Paused"),
    };
    let mut asked = None;
    let toolbar = ui.block("#toolbar", &[], |ui| {
        if ui.button("#play", start).clicked {
            asked = Some(Action::PlayStop);
        }
        if ui.button("#pause", hold).clicked {
            asked = Some(Action::Pause);
        }
        ui.span("#play-state", state, &[]);
    });
    (Some(toolbar.key), asked)
}

/// What the outliner's rows are built from.
struct Rows<'a> {
    outline: &'a [(String, Vec<SceneEntityId>)],
    names: &'a BTreeMap<SceneEntityId, String>,
    renaming: Option<&'a mut Renaming>,
    /// The primary selected entity, whose label is drawn in its own colour.
    primary: Option<SceneEntityId>,
}

/// What [`build_outliner`] built.
struct BuiltOutliner {
    /// The outliner block's key.
    key: NodeKey,
    /// The rename's text input and where its engagement stood, if a row is
    /// being renamed and was built this frame.
    rename: Option<(NodeKey, Engagement)>,
}

/// The placeholder a rename's input shows while it is empty: what committing
/// it empty does.
const UNNAMED: &str = "unnamed";

/// The outliner pane: a title and the scene's rows, one of them a text input
/// while it is renamed.
fn build_outliner(
    ui: &mut Ui,
    state: &mut OutlinerState,
    options: &OutlinerOptions,
    rows: Rows<'_>,
) -> BuiltOutliner {
    let Rows {
        outline,
        names,
        mut renaming,
        primary,
    } = rows;
    let mut key = None;
    let mut rename = None;
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
                |ui, row| match renaming.as_deref_mut() {
                    Some(edit) if entity_of(row.id) == Some(edit.id) => {
                        let options = TextInputOptions {
                            placeholder: UNNAMED,
                            masked: false,
                        };
                        let input = ui.text_input_with(".outliner-rename", &mut edit.text, options);
                        rename = Some((input.key, input.engagement));
                    }
                    _ => {
                        let label = label_of(outline, names, row.id);
                        let class = if primary.is_some() && entity_of(row.id) == primary {
                            ".outliner-label.primary"
                        } else {
                            ".outliner-label"
                        };
                        ui.span(class, label.as_str(), &[]);
                    }
                },
            )
            .key,
        );
    });
    BuiltOutliner {
        key: key.expect("the outliner is built inside its panel"),
        rename,
    }
}

/// Every named entity's name, as the outliner shows it.
fn names_of(document: &Document) -> BTreeMap<SceneEntityId, String> {
    document
        .entity_names()
        .iter()
        .map(|(id, name)| (*id, name.as_str().to_owned()))
        .collect()
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
/// entity's name and id — its id alone when it has no name.
fn label_of(
    outline: &[(String, Vec<SceneEntityId>)],
    names: &BTreeMap<SceneEntityId, String>,
    row: OutlinerId,
) -> String {
    match entity_of(row) {
        Some(id) => match names.get(&id) {
            Some(name) => format!("{name} #{id}"),
            None => format!("#{id}"),
        },
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

    mod assets;
    mod inspector;
    mod naming;
    mod rotation;
    mod selection;

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
            self.frame_with(pointer, scroll, SelectMode::Replace)
        }

        /// [`frame`](Self::frame), with a row click selecting as `select`
        /// says — the modifier a person holds.
        fn frame_with(
            &mut self,
            pointer: PointerInput,
            scroll: f32,
            select: SelectMode,
        ) -> PanelFrame {
            self.panels.frame(
                &mut self.document,
                PanelInput {
                    pointer,
                    nav: NavInput::default(),
                    text: TextInput::default(),
                    extent: EXTENT,
                    select,
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
            self.click_with(at, SelectMode::Replace);
        }

        /// [`click`](Self::click), with the modifier `select` stands for held
        /// throughout.
        fn click_with(&mut self, at: Vec2, select: SelectMode) {
            self.frame_with(
                PointerInput {
                    pos: at,
                    down: true,
                    released: false,
                },
                0.0,
                select,
            );
            self.frame_with(
                PointerInput {
                    pos: at,
                    down: false,
                    released: true,
                },
                0.0,
                select,
            );
            self.frame_with(PointerInput::hovering(Vec2::splat(-1.0)), 0.0, select);
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

        /// [`drag`](Self::drag), moving in `steps` frames rather than one — a
        /// drag as a hand makes it.
        fn drag_in_steps(&mut self, at: Vec2, by: Vec2, steps: u32) {
            let held = |pos| PointerInput {
                pos,
                down: true,
                released: false,
            };
            self.frame(held(at), 0.0);
            for step in 1..=steps {
                self.frame(held(at + by * (step as f32 / steps as f32)), 0.0);
            }
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

        /// The drag-value editing component `axis` of the `row`th row of the
        /// inspector's first section: a vector row is a label span and then
        /// one `.inspector-axis` block per axis, each a label span and then
        /// its widget.
        fn axis_field(&self, row: usize, axis: usize) -> NodeKey {
            let fields = self
                .panels
                .section_fields(0)
                .expect("the inspector drew a section");
            let ui = self.panels.ui();
            let rows = ui.child_keys(fields);
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
                page.document.primary(),
                Some(*id),
                "row {} named {id} and selected something else",
                index + 1,
            );
        }

        // And a header selects nothing, which is what clicking a system means.
        let header = page.centre(page.panels.row_keys()[0]);
        page.click(header);
        assert_eq!(page.document.primary(), None);
    }

    /// **The rows follow an entity leaving and another arriving**, even when the
    /// two leave the entity count where it was — a delete and then a duplicate.
    /// Each row is clicked and read back, as above.
    #[test]
    fn the_outliner_follows_a_delete_and_a_duplicate_that_keep_the_count() {
        let mut page = Page::built_in();
        page.idle();
        let count = page.document.entity_count();
        page.document
            .delete(&[SceneEntityId(2)])
            .expect("in the scene");
        let copy = page
            .document
            .duplicate(&[SceneEntityId(1)])
            .expect("in the scene")[0];
        assert_eq!(page.document.entity_count(), count);
        page.idle();

        let entities: Vec<SceneEntityId> = page
            .document
            .outline()
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .collect();
        assert!(entities.contains(&copy) && !entities.contains(&SceneEntityId(2)));
        for (index, id) in entities.iter().enumerate() {
            let at = page.centre(page.panels.row_keys()[index + 1]);
            page.click(at);
            assert_eq!(page.document.primary(), Some(*id), "row {}", index + 1);
        }
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
        assert_eq!(document.primary(), Some(id), "the panels cleared it");
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
        assert_eq!(page.document.primary(), Some(id));
    }

    /// **A field dragged over many frames is one undo**, back to the value
    /// from before the drag — and a second drag of the same field is an entry
    /// of its own.
    #[test]
    fn an_inspector_drag_over_many_frames_is_one_undo() {
        let mut page = Page::built_in();
        let id = SceneEntityId(3);
        page.document.select(Some(id));
        page.idle();
        let before = page.document.files().expect("ids");

        let field = page.axis_field(0, 0);
        let at = page.centre(field);
        page.drag_in_steps(at, Vec2::new(30.0, 0.0), 6);
        assert_ne!(
            page.document.files().expect("ids"),
            before,
            "the drag moved nothing"
        );
        assert_eq!(
            page.document.log().len(),
            1,
            "a drag of six frames took more than one undo",
        );

        assert!(page.document.undo().expect("one entry"));
        assert_eq!(page.document.files().expect("ids"), before);

        // A second drag of the same field is a second entry, not folded into
        // the first.
        page.idle();
        page.drag_in_steps(at, Vec2::new(10.0, 0.0), 3);
        page.drag_in_steps(at, Vec2::new(10.0, 0.0), 3);
        assert_eq!(page.document.log().len(), 2, "two drags, two entries");
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

    /// **An inspector edit in play mode is refused, on the status line**, and
    /// the panel's own write is taken back: the scene as play left it, and
    /// nothing in the log. Paused, so no tick moves the scene under the
    /// comparison.
    #[test]
    fn an_inspector_edit_in_play_mode_is_refused_on_the_status_line() {
        let mut page = Page::over(crate::document::play_tests::drifting_document());
        page.document.select(Some(SceneEntityId(3)));
        page.document.play().expect("the scene plays");
        assert!(page.document.pause());
        page.idle();
        let before = page.document.files().expect("ids");

        let field = page.axis_field(0, 0);
        let at = page.centre(field);
        page.drag(at, Vec2::new(30.0, 0.0));

        assert_eq!(
            page.document.files().expect("ids"),
            before,
            "the panel's write into the playing scene stood",
        );
        assert!(
            page.document.log().is_empty(),
            "a refused edit was recorded"
        );
        let (text, tone) = page.panels.status();
        assert_eq!(tone, Tone::Warning, "{text}");
        assert!(text.contains("play mode"), "{text}");
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
        let (status_min, status_max) = page.panels.status_rect().expect("laid out");
        assert!(
            min.x > 0.0 && max.x == EXTENT.0 as f32 && max.y == status_min.y,
            "the viewport is not the right-hand pane down to the status line: \
             {min:?}..{max:?} over {status_min:?}",
        );
        assert!(
            status_min.x == 0.0
                && status_max.x == EXTENT.0 as f32
                && status_max.y == EXTENT.1 as f32,
            "the status line is not the window's bottom edge: {status_min:?}..{status_max:?}",
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
