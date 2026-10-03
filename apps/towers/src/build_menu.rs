//! The build menu: a click or a tap on a plot opens a menu there, offering
//! what can be built or bought on it.
//!
//! ```text
//!   pointer ──▶ ray through the pixel ──▶ the plot it hits ──▶ hover outline
//!                                              │ press
//!                                              ▼
//!          ┌ EAST ───────────┐   at the pad's pixel, on the pop-up layer
//!          │ [bolt]   BOLT   40g │
//!          │ [splash] SPLASH 70g │   greyed when the purse cannot reach it
//!          │ [slow]   SLOW   50g │
//!          └─────────────────────┘
//!                │ click
//!                ▼
//!   Pick ──▶ crate::app: cursor to the plot, the kind picked, and the build
//!            key's latch — the very command `B` sends
//! ```
//!
//! # A pick is a key press, and the server still decides
//!
//! The menu never builds anything. A [`Pick`] moves the build cursor to its
//! plot, picks its kind and latches the build — or the upgrade — for the next
//! tick, which is exactly where the arrow keys, a digit and `B` leave the
//! client. So a pick is sealed into the same four bytes and refused by the same
//! server, a refusal shows on the page as a key's does, and nothing here can
//! disagree with the keyboard about what a build is. The prices and the greying
//! are the client agreeing with the server, as `crate::page`'s lists already
//! do, not replacing it.
//!
//! # On the pop-up layer, not a menu of the loop's
//!
//! The menu is a `crcbl::ui::tree` pop-up opened at a point
//! ([`Placement::At`]), the layer a context menu is built on, and its items are
//! styled with the context menu's own `default.css` classes. That layer brings
//! what a world-anchored menu needs and none of it is written here: it flips
//! and shifts to stay inside the window, it is hit-tested before the field
//! under it, a disabled item takes no click, and **a press outside it closes it
//! and is spent** — so a click meant to dismiss the menu never opens another
//! one on the plot it landed on. The loop's own `MenuSet` was the other
//! candidate and does not fit: its panels are centred and pause-shaped, and a
//! menu up there is one the loop claims every key for.
//!
//! The pop-up hangs from a block covering the whole window, which is what the
//! field is to the tree, and opens at the plot's pad as the camera sees it
//! when it opens — it does not follow a moving dev camera afterwards.
//!
//! # A plot is picked with a ray
//!
//! [`plot_under`] shoots [`Camera::ray_through`] the pointer's pixel and asks
//! `crcbl::phys`'s ray-box query for the nearest plot it hits, each plot a box
//! from its pad up to an upgraded tower's top — so a tower is clicked on its
//! post as well as on its pad. The boxes are the map's numbers rather than the
//! stage's physics world: a joiner has no stage, and which plot a pointer is
//! over is the client's question.
//!
//! # A tap is a click
//!
//! A browser hands the engine a finger's primary contact as the pointer
//! (`web/engine/shell.js`), so a tap reaches [`BuildMenu::pointer`] as a press
//! and a release. **Often in one frame's batch**, and the element tree reads a
//! press off the button being held, so the menu holds a release that came in
//! with its press over to the next frame rather than lose the tap.
//!
//! # Not the stage's
//!
//! Nothing here reaches the simulation but a [`Pick`], and opening and closing
//! the menu change nothing a tick reads —
//! `opening_and_closing_the_build_menu_leaves_the_stage_hash_alone` holds that.

use crcbl::engine::PointerUpdate;
use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::phys::query::ray_vs_aabb;
use crcbl::phys::{Aabb, Ray};
use crcbl::render::Camera;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::image::{AtlasError, AtlasImage, ImageAtlas};
use crcbl::ui::style::Declaration;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::tree::{AvailableSpace, Behavior, LengthAuto, NodeKey, Placement, Ui};
use crcbl::ui::widget::PointerInput;

use crate::art::{ICON_PX, Icons};
use crate::game::RenderState;
use crate::map::{PAD_EDGE, PAD_HEIGHT, TOWER_HEIGHT, UPGRADED_SCALE};
use crate::scene::Plot;
use crate::tower::{self, Tier};

/// What a plot under the pointer is outlined in: the page's picked green.
pub const HOVER: [f32; 4] = [0.55, 0.92, 0.62, 1.0];

/// How thick the hover outline is, in pixels.
pub const HOVER_PX: f32 = 2.0;

/// What a built tower that has nothing left to buy offers instead of an
/// upgrade.
pub const MAX_TIER: &str = "MAX TIER";

/// What an upgrade is offered as.
pub const UPGRADE: &str = "UPGRADE";

/// The menu's own rules, over `default.css`'s `.context-menu` and
/// `.context-item` — which already give the items their hover, their
/// disabled grey and the menu its frame. What is here is the title, the price
/// column, and the icon drawn in its own colours unless its item is disabled,
/// when it greys with the label.
const STYLE: &str = "
.build-menu-title { padding: 1px 6px; color: #f5c400; }
.build-price { color: #e6e9ef; }
.context-item:disabled > .build-price { color: #7a8190; }
.build-icon { color: #ffffff; }
.context-item:disabled > .build-icon { color: #7a8190; }
";

/// Where the tree is told the pointer is before the loop has said where it
/// is: up and left of the window's corner, where nothing is laid out.
const NOWHERE: Vec2 = Vec2::splat(-1.0);

/// What one item of the menu asks for when picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Build `kind` on `plot` — the arrows to the plot, the kind's digit, `B`.
    Build {
        /// The plot.
        plot: u8,
        /// The kind.
        kind: tower::Kind,
    },
    /// Step the tower on `plot` up a tier — the arrows to the plot, `U`.
    Upgrade {
        /// The plot.
        plot: u8,
    },
}

/// One item on the menu, as it is offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What it reads.
    pub label: String,
    /// What it costs, or `None` for an item with nothing to buy.
    pub price: Option<u32>,
    /// Whether it can be picked: whether the purse reaches the price.
    pub enabled: bool,
    /// What picking it asks for.
    pub pick: Pick,
    /// The picture beside the label.
    pub icon: AtlasImage,
}

/// What the menu offers on `plot`, read off `state`: a build of every kind on
/// an empty plot, an upgrade on a base tower, and nothing to buy on an
/// upgraded one. An item the purse cannot reach is offered disabled.
#[must_use]
pub fn entries(state: &RenderState, plot: u8, icons: &Icons) -> Vec<Entry> {
    match state.towers[usize::from(plot)] {
        None => tower::ALL
            .iter()
            .map(|&kind| {
                let cost = kind.spec(Tier::Base).cost;
                Entry {
                    label: kind.label().to_uppercase(),
                    price: Some(cost),
                    enabled: state.gold >= cost,
                    pick: Pick::Build { plot, kind },
                    icon: icons.kind(kind),
                }
            })
            .collect(),
        Some(standing) => vec![match standing.tier {
            Tier::Base => {
                let cost = standing.kind.spec(Tier::Upgraded).cost;
                Entry {
                    label: UPGRADE.to_string(),
                    price: Some(cost),
                    enabled: state.gold >= cost,
                    pick: Pick::Upgrade { plot },
                    icon: icons.upgrade(),
                }
            }
            Tier::Upgraded => Entry {
                label: MAX_TIER.to_string(),
                price: None,
                enabled: false,
                pick: Pick::Upgrade { plot },
                icon: icons.upgrade(),
            },
        }],
    }
}

/// The box a plot is picked by: its pad, and up to the top of an upgraded
/// tower standing on it.
#[must_use]
pub fn pick_box(plot: &Plot) -> Aabb {
    let feet = DVec3::from(plot.position);
    let half = 0.5 * PAD_EDGE;
    Aabb {
        min: feet - DVec3::new(half, 0.0, half),
        max: feet + DVec3::new(half, TOWER_HEIGHT * f64::from(UPGRADED_SCALE), half),
    }
}

/// The plot whose [`pick_box`] the ray through the pixel at `at` meets first,
/// in a frame `extent` pixels across seen through `camera` — or `None` for a
/// pixel over no plot.
///
/// `at` is a position in the frame's pixels, as the shell reports one; the ray
/// goes through the centre of the pixel it falls in, which is what
/// [`Camera::ray_through`] asks of a picking caller.
#[must_use]
pub fn plot_under(plots: &[Plot], camera: &Camera, extent: (u32, u32), at: Vec2) -> Option<u8> {
    if extent.0 == 0 || extent.1 == 0 {
        return None;
    }
    let view = camera.ray_through(at.floor() + Vec2::splat(0.5), extent);
    let ray = Ray::new(view.origin.as_dvec3(), view.direction.as_dvec3());
    plots
        .iter()
        .enumerate()
        .filter_map(|(index, plot)| Some((ray_vs_aabb(&ray, &pick_box(plot))?.t, index)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .and_then(|(_, index)| u8::try_from(index).ok())
}

/// The point a plot's menu opens at and its outline is drawn round: the
/// middle of its pad's top.
#[must_use]
pub fn pad_top(plot: &Plot) -> Vec3 {
    (DVec3::from(plot.position) + DVec3::Y * PAD_HEIGHT).as_vec3()
}

/// The pointer as the menu has been told of it, folded across the frames the
/// loop delivered it on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pointer {
    /// Where it last was, in the loop's −1…1 surface coordinates.
    at: Option<Vec2>,
    /// The button is held, as the menu's last frame saw it.
    down: bool,
    /// A press not yet handed to a frame.
    pressed: bool,
    /// A release not yet handed to a frame.
    released: bool,
}

impl Pointer {
    /// Takes in one update from the loop.
    fn fold(&mut self, update: PointerUpdate) {
        // `None` is a frame it did not move — or left the window, which is not
        // a place to forget the last one over.
        if update.at.is_some() {
            self.at = update.at;
        }
        self.pressed |= update.pressed;
        self.released |= update.released;
    }

    /// What one frame of the element tree is handed, at a frame `extent`
    /// pixels across, and whether a press began on it.
    ///
    /// **A press is handed over on a frame of its own.** The tree reads a press
    /// off the button being held, so a release that arrived in the same batch
    /// as its press — a tap, on a phone — is kept for the next frame; handed
    /// over together, the button would never be seen down at all.
    fn take(&mut self, extent: (u32, u32)) -> (PointerInput, bool) {
        let pos = self
            .at
            .and_then(|at| {
                PointerUpdate {
                    at: Some(at),
                    ..PointerUpdate::default()
                }
                .pixels(extent)
            })
            .unwrap_or(NOWHERE);
        let pressed = core::mem::take(&mut self.pressed);
        let released = !pressed && core::mem::take(&mut self.released);
        if pressed {
            self.down = true;
        } else if released {
            self.down = false;
        }
        (
            PointerInput {
                pos,
                down: self.down,
                released,
                secondary_pressed: false,
            },
            pressed,
        )
    }

    /// Where it is in a frame `extent` pixels across, if it has been anywhere.
    fn pixels(&self, extent: (u32, u32)) -> Option<Vec2> {
        PointerUpdate {
            at: self.at,
            ..PointerUpdate::default()
        }
        .pixels(extent)
    }
}

/// The menu: its element tree, its icons, which plot it is open on, and the
/// pointer it is played with.
#[derive(Debug)]
pub struct BuildMenu {
    ui: Ui,
    icons: Icons,
    pointer: Pointer,
    /// The plot the menu is open on.
    open: Option<u8>,
    /// What it offered on its last frame, for this crate's tests.
    offered: Vec<Entry>,
    /// The key each of those was built under, which
    /// [`BuildMenu::item_rect`] finds its rectangle by.
    items: Vec<NodeKey>,
}

/// Everything one frame of the menu reads.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// The camera the field is drawn from this frame.
    pub camera: &'a Camera,
    /// The frame's size in pixels.
    pub extent: (u32, u32),
    /// The field as the frame draws it.
    pub state: &'a RenderState,
    /// The map's plots.
    pub plots: &'a [Plot],
    /// Whether the menu is being played at all: not under the pause panel,
    /// the lobby or a join's panel, where it closes and takes no pointer.
    pub live: bool,
}

impl BuildMenu {
    /// A closed menu, its icons registered into `images`.
    ///
    /// # Errors
    ///
    /// [`AtlasError`] when `images` has no room for the icons.
    pub fn new(images: &mut ImageAtlas) -> Result<Self, AtlasError> {
        let mut ui = Ui::new();
        ui.add_stylesheet("towers/build_menu.css", STYLE);
        Ok(Self {
            ui,
            icons: Icons::register(images)?,
            pointer: Pointer::default(),
            open: None,
            offered: Vec::new(),
            items: Vec::new(),
        })
    }

    /// Takes in the pointer, as the loop delivers it — see
    /// [`crcbl::engine::HostedGame::pointer_event`].
    pub fn pointer(&mut self, update: PointerUpdate) {
        self.pointer.fold(update);
    }

    /// Closes the menu, if it is open — a run being replaced under it.
    pub fn close(&mut self) {
        self.open = None;
        self.offered.clear();
        self.items.clear();
    }

    /// The plot the menu is open on.
    #[must_use]
    pub const fn open_on(&self) -> Option<u8> {
        self.open
    }

    /// What the menu offered on its last frame — nothing while it is closed.
    #[must_use]
    pub fn offered(&self) -> &[Entry] {
        &self.offered
    }

    /// Where item `index` of [`BuildMenu::offered`] was last laid out, top-left
    /// then bottom-right, in the frame's pixels.
    #[must_use]
    pub fn item_rect(&self, index: usize) -> Option<(Vec2, Vec2)> {
        self.ui.rect(*self.items.get(index)?)
    }

    /// The icons the menu draws.
    #[must_use]
    pub const fn icons(&self) -> &Icons {
        &self.icons
    }

    /// The plot to outline this frame: the one the menu is open on, or else
    /// the one under the pointer.
    #[must_use]
    pub fn highlighted(&self, frame: &Frame<'_>) -> Option<u8> {
        if !frame.live {
            return None;
        }
        self.open.or_else(|| {
            let at = self.pointer.pixels(frame.extent)?;
            plot_under(frame.plots, frame.camera, frame.extent, at)
        })
    }

    /// Outlines [`BuildMenu::highlighted`]'s pad into `list`: under the page,
    /// so a panel is never drawn over by it.
    pub fn draw_highlight(&self, list: &mut DrawList, frame: &Frame<'_>) {
        let Some(plot) = self.highlighted(frame) else {
            return;
        };
        let centre = pad_top(&frame.plots[usize::from(plot)]);
        let half = 0.5 * PAD_EDGE as f32;
        let corners: Option<Vec<Vec2>> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .into_iter()
            .map(|(x, z)| {
                frame
                    .camera
                    .pixel_of(centre + Vec3::new(x * half, 0.0, z * half), frame.extent)
            })
            .collect();
        if let Some(corners) = corners {
            list.polyline(corners, HOVER_PX, true, HOVER);
        }
    }

    /// Runs the menu for one frame and draws it into `list`, over everything
    /// drawn before it: opens it on the plot a press landed on, closes it on a
    /// press outside it, and answers the item a click picked.
    ///
    /// `atlas` is the one the UI pass renders text with, for the reason
    /// [`crate::page::draw`] gives.
    pub fn frame(
        &mut self,
        list: &mut DrawList,
        atlas: &FontAtlas,
        frame: &Frame<'_>,
    ) -> Option<Pick> {
        let (input, pressed) = self.pointer.take(frame.extent);
        let (input, pressed) = if frame.live {
            (input, pressed)
        } else {
            self.close();
            (PointerInput::hovering(input.pos), false)
        };
        let was_open = self.open.is_some();
        self.ui.begin_frame(input);

        let screen = Vec2::new(frame.extent.0 as f32, frame.extent.1 as f32);
        let field = self
            .ui
            .block(
                "#field",
                &[
                    Declaration::Width(LengthAuto::Px(screen.x)),
                    Declaration::Height(LengthAuto::Px(screen.y)),
                ],
                |_| {},
            )
            .key;

        if was_open && !self.ui.is_popup_open(field) {
            // The pop-up layer closed it: a press outside, which it spends.
            self.close();
        } else if !was_open && pressed {
            let opened = self
                .pointer
                .pixels(frame.extent)
                .and_then(|at| plot_under(frame.plots, frame.camera, frame.extent, at));
            if let Some(plot) = opened {
                let at = frame
                    .camera
                    .pixel_of(pad_top(&frame.plots[usize::from(plot)]), frame.extent)
                    .unwrap_or(input.pos);
                self.open = Some(plot);
                self.ui.open_popup_at(field, Placement::At(at));
            }
        }
        if self.open.is_none() && self.ui.is_popup_open(field) {
            self.ui.close_popup(field);
        }

        let picked = self.open.and_then(|plot| self.build(field, frame, plot));
        if picked.is_some() {
            self.ui.close_popup(field);
            self.close();
        }

        self.ui
            .layout(Vec2::ZERO, AvailableSpace::definite(screen), atlas);
        self.ui.emit(list);
        picked
    }

    /// Builds the open menu on `plot` and answers the item clicked this frame.
    fn build(&mut self, field: NodeKey, frame: &Frame<'_>, plot: u8) -> Option<Pick> {
        let offered = entries(frame.state, plot, &self.icons);
        let title = match frame.state.towers[usize::from(plot)] {
            Some(standing) => format!(
                "{} - {}",
                frame.plots[usize::from(plot)].label.to_uppercase(),
                standing.kind.label().to_uppercase()
            ),
            None => frame.plots[usize::from(plot)].label.to_uppercase(),
        };
        let icon_size = [
            Declaration::Width(LengthAuto::Px(ICON_PX)),
            Declaration::Height(LengthAuto::Px(ICON_PX)),
        ];
        let mut picked = None;
        let mut items = Vec::with_capacity(offered.len());
        self.ui.popup(field, ".context-menu.build-menu", &[], |ui| {
            ui.span(".build-menu-title", title.as_str(), &[]);
            for (index, entry) in offered.iter().enumerate() {
                let behavior = Behavior {
                    disabled: !entry.enabled,
                    ..Behavior::BUTTON
                };
                let price = entry.price.map(|cost| format!("{cost}g"));
                let response = ui.block_keyed_with(index, ".context-item", &[], behavior, |ui| {
                    ui.span(".build-icon", entry.icon, &icon_size);
                    ui.span(".context-item-label", entry.label.as_str(), &[]);
                    if let Some(price) = &price {
                        ui.span(".build-price", price.as_str(), &[]);
                    }
                });
                // A disabled item is never reported clicked: that is
                // the tree's rule, and `behavior` is what invokes it.
                if response.clicked {
                    picked = Some(entry.pick);
                }
                items.push(response.key);
            }
        });
        self.offered = offered;
        self.items = items;
        picked
    }
}

#[cfg(test)]
mod tests;
