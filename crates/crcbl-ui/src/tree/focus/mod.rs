//! Focus, keyboard and gamepad navigation, and the engaged state:
//! `docs/plan/07-ui-debug.md` rung 6.
//!
//! # One focused node per tree
//!
//! A [`Ui`] is one UI context and holds at most one focused node. A node takes
//! part through the [`Behavior`] its builder declares
//! ([`Ui::block_with`]): a [`Role::Button`] or [`Role::Engage`] node is
//! focusable unless it opts out, a plain block is not unless it opts in, and a
//! `disabled` node never is. Like hover, focus is resolved when a frame begins,
//! against the tree the previous frame built — so a node built for the first
//! time is focusable from the frame after.
//!
//! # The frame's navigation input
//!
//! [`Ui::begin_frame_with`] takes a [`NavInput`]: one directional step, the
//! tree-order steps, accept, back, and which kind of device spoke last. The
//! tree reads no device; the caller maps its own input to these, which is what
//! lets this rung land before `crcbl-input` has the context stack the plan's
//! reserved `ui_*` actions need.
//!
//! **The mixed-input rule**: in [`InputMode::Pointer`] the pointer's hover sets
//! `:hover` and focus sets nothing; in [`InputMode::Navigation`] focus sets
//! `:focus` and hover sets nothing — so a stylesheet's focus ring shows only
//! while the pad or the keyboard is driving. Focus itself is kept in both: a
//! click focuses what it clicked, so the pad continues from there. When the
//! pad speaks and nothing is focused, focus lands — on the most recently
//! focused node still in the tree, else the node under the pointer, else the
//! first focusable node in tree order, all inside an open modal and preferring
//! the modal's remembered node to the pointer's — and the press that landed it
//! moves nothing further.
//!
//! # Moves
//!
//! A direction goes, in order of precedence, where the focused node's
//! `nav-up`/`nav-right`/`nav-down`/`nav-left` names (`none` stays put), else
//! where the beam-first spatial search in `spatial.rs` lands. That search is
//! **clamped**: it looks first inside the innermost scope root, `overflow:
//! scroll` container or `nav-wrap` container around the focused node, and only
//! when nothing lies that way inside does it widen to the next one out — or,
//! for a `nav-wrap` container, wrap round to its far side on that axis. A
//! [`Scope::Modal`] is never widened past. [`NavInput::next`] and
//! [`NavInput::prev`] walk every focusable node in tree order, wrapping at the
//! ends, inside the modal when there is one: the always-works fallback.
//!
//! # Scopes
//!
//! **Each scope root remembers the last node focused inside it**, and a
//! directional move that enters a scope from outside resumes at that node
//! rather than at the geometric winner — provided the remembered node lies in
//! the direction pressed as well, so memory never sends focus backwards. The topmost [`Scope::Modal`] in paint
//! order traps focus: focus outside it is pulled in, a click outside it
//! focuses nothing, and no move leaves it. When the modal goes, the landing
//! rule's history returns focus to what was focused before it opened.
//!
//! **An `overflow: scroll` block scrolls the focused node into view**: its
//! offset moves the least that puts the node's border box inside the block's
//! content box — the block's padding is the margin a focus ring is drawn in —
//! clamped to how far its content reaches.
//!
//! # Focused and engaged (LOCKED)
//!
//! Focus never captures navigation. A [`Role::Engage`] node starts consuming it
//! only once engaged, by a click or by accept while focused. While it is
//! engaged every directional and tree-order step is handed to it
//! ([`Response::captured`]) instead of moving focus; accept, or a click
//! anywhere outside it, **commits**; back **cancels**. One node per context is
//! engaged, and engaging another commits the first in the same frame. The
//! engaged node has `:engaged`. [`Ui::snapshot`] is the widget's half of the
//! contract: called every frame with the value the widget edits, it keeps a
//! copy when engagement begins and writes it back on a cancel. A
//! [`Role::Button`] has no engaged state: accept fires it through the same
//! [`Response::clicked`] a click does.

mod debug;
pub mod spatial;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::collections::HashSet;

use glam::Vec2;

use super::store::NodeKey;
use super::style::{NavTarget, Overflow};
use super::{Response, Ui};

pub use debug::{NAV_DEBUG_BEAM, NAV_DEBUG_CHOSEN, NAV_DEBUG_OUTSIDE, NAV_DEBUG_PATH};
pub use spatial::Score;

/// How many recently focused nodes a tree keeps, for landing focus again when
/// the focused node goes and for the debug overlay's history.
pub const FOCUS_HISTORY: usize = 16;

/// A direction a move goes in, on screen: Y down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Toward the top of the screen.
    Up,
    /// Toward the right.
    Right,
    /// Toward the bottom.
    Down,
    /// Toward the left.
    Left,
}

impl Direction {
    /// All four, clockwise from up.
    pub const ALL: [Self; 4] = [Self::Up, Self::Right, Self::Down, Self::Left];

    /// Whether the move runs along the horizontal axis.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Which kind of device spoke last: what decides whether hover or focus is
/// shown. See the module docs' mixed-input rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InputMode {
    /// A mouse or a finger: `:hover` is set, `:focus` is not.
    #[default]
    Pointer,
    /// A keyboard or a pad: `:focus` is set, `:hover` is not.
    Navigation,
}

/// One frame's navigation input, in the vocabulary of the plan's reserved UI
/// actions. Every field is this frame's edge — a held key repeats by the caller
/// sending it again — so the default is no input at all, from a pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavInput {
    /// Which kind of device spoke last.
    pub mode: InputMode,
    /// `ui_move`: one step in a direction.
    pub direction: Option<Direction>,
    /// `ui_next`: forward in tree order.
    pub next: bool,
    /// `ui_prev`: backward in tree order.
    pub prev: bool,
    /// `ui_accept`: fire a button, engage a widget, or commit the engaged one.
    pub accept: bool,
    /// `ui_back`: cancel the engaged widget, or else [`Ui::back_requested`].
    pub back: bool,
}

impl NavInput {
    /// A step in `direction`, from the pad or the keyboard.
    #[must_use]
    pub const fn toward(direction: Direction) -> Self {
        Self {
            direction: Some(direction),
            ..Self::NAVIGATION
        }
    }

    /// No press, with the pad or the keyboard the last to speak.
    pub const NAVIGATION: Self = Self {
        mode: InputMode::Navigation,
        direction: None,
        next: false,
        prev: false,
        accept: false,
        back: false,
    };

    /// `ui_next`, from the pad or the keyboard.
    pub const NEXT: Self = Self {
        next: true,
        ..Self::NAVIGATION
    };

    /// `ui_prev`, from the pad or the keyboard.
    pub const PREV: Self = Self {
        prev: true,
        ..Self::NAVIGATION
    };

    /// `ui_accept`, from the pad or the keyboard.
    pub const ACCEPT: Self = Self {
        accept: true,
        ..Self::NAVIGATION
    };

    /// `ui_back`, from the pad or the keyboard.
    pub const BACK: Self = Self {
        back: true,
        ..Self::NAVIGATION
    };

    /// Whether anything but `back` was pressed.
    const fn steers(self) -> bool {
        self.direction.is_some() || self.next || self.prev || self.accept
    }

    /// The step an engaged node would take from this input.
    const fn step(self) -> Option<NavStep> {
        match (self.direction, self.next, self.prev) {
            (Some(direction), _, _) => Some(NavStep::Move(direction)),
            (None, true, _) => Some(NavStep::Next),
            (None, false, true) => Some(NavStep::Prev),
            (None, false, false) => None,
        }
    }
}

/// A navigation step an engaged node took instead of focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavStep {
    /// A direction.
    Move(Direction),
    /// Forward in tree order.
    Next,
    /// Backward in tree order.
    Prev,
}

/// What a node is, to focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
    /// A plain block: not focusable unless it opts in, and accept fires it as
    /// a click when it does.
    #[default]
    None,
    /// Instant activation: focusable, and accept fires it as a click.
    Button,
    /// A widget that edits a value: focusable, and engaged by a click or by
    /// accept before it takes navigation.
    Engage,
}

/// Whether a node is a focus scope root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Not a scope.
    #[default]
    None,
    /// A pane or window: moves search inside it first, and it remembers the
    /// node last focused inside it.
    Root,
    /// As `Root`, and it traps focus while it is in the tree.
    Modal,
}

/// How a node takes part in focus; see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Behavior {
    /// What it is.
    pub role: Role,
    /// `Some` to opt in or out of focus; `None` for its role's default.
    pub focusable: Option<bool>,
    /// Never focusable, never fired or engaged, and `:disabled`.
    pub disabled: bool,
    /// Whether it is a scope root.
    pub scope: Scope,
}

impl Behavior {
    /// A plain block.
    pub const NONE: Self = Self {
        role: Role::None,
        focusable: None,
        disabled: false,
        scope: Scope::None,
    };

    /// A button.
    pub const BUTTON: Self = Self {
        role: Role::Button,
        ..Self::NONE
    };

    /// A widget with an engaged state.
    pub const ENGAGE: Self = Self {
        role: Role::Engage,
        ..Self::NONE
    };

    /// A pane: a scope root.
    pub const SCOPE: Self = Self {
        scope: Scope::Root,
        ..Self::NONE
    };

    /// A modal: a scope root that traps focus.
    pub const MODAL: Self = Self {
        scope: Scope::Modal,
        ..Self::NONE
    };

    /// Whether focus can rest on the node.
    #[must_use]
    pub const fn is_focusable(self) -> bool {
        let wanted = match self.focusable {
            Some(wanted) => wanted,
            None => !matches!(self.role, Role::None),
        };
        wanted && !self.disabled
    }
}

/// Where a node's engagement is, this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Engagement {
    /// Not engaged.
    #[default]
    Idle,
    /// Engaged from this frame: the moment to snapshot.
    Began,
    /// Engaged since an earlier frame.
    Engaged,
    /// Engagement ended this frame, keeping the value.
    Committed,
    /// Engagement ended this frame, and the value goes back to the snapshot.
    Cancelled,
}

impl Engagement {
    /// Whether the node is engaged this frame: `:engaged` is set.
    #[must_use]
    pub const fn is_engaged(self) -> bool {
        matches!(self, Self::Began | Self::Engaged)
    }
}

/// One candidate a directional move scored, for the debug overlay and for a
/// test asking why focus went where it did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavScore {
    /// The candidate.
    pub key: NodeKey,
    /// Its score.
    pub score: Score,
    /// Whether the move landed on it.
    pub chosen: bool,
}

/// The value an engaged widget copied when it engaged.
struct Snapshot {
    key: NodeKey,
    value: Box<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// A tree's focus: what is focused and engaged, and what the last frame's
/// navigation did.
#[derive(Debug, Default)]
pub(crate) struct FocusState {
    focused: Option<NodeKey>,
    engaged: Option<NodeKey>,
    snapshot: Option<Snapshot>,
    pub mode: InputMode,
    back: bool,
    /// Newest last, at most [`FOCUS_HISTORY`] long, no key twice in a row.
    history: Vec<NodeKey>,
    scores: Vec<NavScore>,
    pub debug: bool,
    /// What [`Ui::set_focus`] asked for, applied when the next frame begins.
    requested: Option<NodeKey>,
    /// Nodes whose `nav-*` id was already reported as matching nothing.
    warned: HashSet<NodeKey>,
}

/// What one frame's resolution did to engagement, for writing into the nodes.
#[derive(Default)]
struct Events {
    ended: Option<(NodeKey, Engagement)>,
    began: Option<NodeKey>,
    captured: Option<NavStep>,
    activated: Option<NodeKey>,
}

impl Ui {
    /// Whether `key` is in last frame's tree, focusable, and drawn.
    fn can_focus(&self, key: NodeKey) -> bool {
        self.store
            .by_key(key)
            .is_some_and(|node| node.hittable && node.behavior.is_focusable())
    }

    /// Whether `key` is `region` or inside it; everything is inside no region.
    fn inside(&self, key: NodeKey, region: Option<NodeKey>) -> bool {
        region.is_none_or(|region| self.store.is_within(key, region))
    }

    /// The topmost modal scope in last frame's tree.
    fn active_modal(&self) -> Option<NodeKey> {
        self.store
            .iter()
            .filter(|node| node.hittable && node.behavior.scope == Scope::Modal)
            .max_by_key(|node| node.paint_order)
            .map(|node| node.key)
    }

    /// Every focusable node inside `region`, in tree order.
    fn focusable_in(&self, region: Option<NodeKey>) -> Vec<NodeKey> {
        let mut nodes: Vec<_> = self
            .store
            .iter()
            .filter(|node| node.hittable && node.behavior.is_focusable())
            .filter(|node| self.inside(node.key, region))
            .map(|node| (node.paint_order, node.key))
            .collect();
        nodes.sort_unstable_by_key(|(order, _)| *order);
        nodes.into_iter().map(|(_, key)| key).collect()
    }

    /// Resolves this frame's focus and engagement from `nav` and the click the
    /// pointer made, then writes the result into every stored node. Runs after
    /// the pointer is resolved, before anything is built.
    pub(super) fn resolve_navigation(&mut self, nav: NavInput, clicked: Option<NodeKey>) {
        self.focus.mode = nav.mode;
        self.focus.back = false;
        self.focus.scores.clear();
        // A snapshot outlives its engagement by the one frame that reported the
        // end, for the widget to read it back in.
        if self
            .focus
            .snapshot
            .as_ref()
            .is_some_and(|held| Some(held.key) != self.focus.engaged)
        {
            self.focus.snapshot = None;
        }
        if self.focus.focused.is_some_and(|key| !self.can_focus(key)) {
            self.focus.focused = None;
        }
        if self.focus.engaged.is_some_and(|key| {
            !self.can_focus(key)
                || self.store.by_key(key).map(|node| node.behavior.role) != Some(Role::Engage)
        }) {
            self.focus.engaged = None;
        }

        let modal = self.active_modal();
        let mut events = Events::default();

        // A request stands in for a click on the node it names.
        if let Some(requested) = self.focus.requested.take()
            && self.can_focus(requested)
            && self.inside(requested, modal)
        {
            if let Some(engaged) = self.focus.engaged
                && engaged != requested
            {
                self.focus.engaged = None;
                events.ended = Some((engaged, Engagement::Committed));
            }
            self.move_focus(requested);
        }

        if let Some(clicked) = clicked {
            if let Some(engaged) = self.focus.engaged
                && !self.store.is_within(clicked, engaged)
            {
                self.focus.engaged = None;
                events.ended = Some((engaged, Engagement::Committed));
            }
            let target = self
                .store
                .ancestry(Some(clicked))
                .into_iter()
                .find(|&key| self.can_focus(key));
            if let Some(target) = target
                && self.inside(target, modal)
            {
                self.move_focus(target);
                let engages =
                    self.store.by_key(target).map(|node| node.behavior.role) == Some(Role::Engage);
                if engages && self.focus.engaged != Some(target) {
                    self.focus.engaged = Some(target);
                    events.began = Some(target);
                }
            }
        }

        let outside_modal = modal.is_some()
            && self
                .focus
                .focused
                .is_none_or(|focused| !self.inside(focused, modal));
        let landing = outside_modal
            || (self.focus.focused.is_none()
                && (nav.mode == InputMode::Navigation || nav.steers()));
        let spent = landing && self.land(modal) && nav.steers();

        if let Some(engaged) = self.focus.engaged {
            if nav.accept && events.began != Some(engaged) {
                self.focus.engaged = None;
                events.ended = Some((engaged, Engagement::Committed));
            } else if nav.back {
                self.focus.engaged = None;
                events.ended = Some((engaged, Engagement::Cancelled));
            } else {
                events.captured = nav.step();
            }
        } else {
            if nav.back {
                self.focus.back = true;
            }
            if !spent {
                if let Some(direction) = nav.direction {
                    self.move_spatially(direction, modal);
                } else if nav.next || nav.prev {
                    self.move_in_tree_order(nav.next, modal);
                }
                if nav.accept
                    && let Some(focused) = self.focus.focused
                {
                    let role = self.store.by_key(focused).map(|node| node.behavior.role);
                    if role == Some(Role::Engage) {
                        self.focus.engaged = Some(focused);
                        events.began = Some(focused);
                    } else {
                        events.activated = Some(focused);
                    }
                }
            }
        }

        let FocusState {
            focused, engaged, ..
        } = self.focus;
        for node in self.store.iter_mut() {
            let key = node.key;
            let interaction = &mut node.interaction;
            interaction.focused = focused == Some(key);
            interaction.engagement = match events.ended {
                Some((ended, how)) if ended == key => how,
                _ if events.began == Some(key) => Engagement::Began,
                _ if engaged == Some(key) => Engagement::Engaged,
                _ => Engagement::Idle,
            };
            interaction.captured = events.captured.filter(|_| engaged == Some(key));
            if events.activated == Some(key) {
                interaction.clicked = true;
            }
        }
    }

    /// Lands focus inside `region`: on the newest node in the history still
    /// there, else the scope's remembered node, else the focusable node under
    /// the pointer, else the first in tree order. Returns whether focus moved.
    fn land(&mut self, region: Option<NodeKey>) -> bool {
        let usable = |ui: &Self, key: NodeKey| ui.can_focus(key) && ui.inside(key, region);
        let from_history = self
            .focus
            .history
            .iter()
            .rev()
            .copied()
            .find(|&key| usable(self, key));
        let remembered = region
            .and_then(|region| self.store.by_key(region))
            .and_then(|node| node.remembered)
            .filter(|&key| usable(self, key));
        let hovered = self
            .store
            .iter()
            .filter(|node| node.interaction.hovered && node.behavior.is_focusable())
            .max_by_key(|node| node.paint_order)
            .map(|node| node.key)
            .filter(|&key| usable(self, key));
        let target = from_history
            .or(remembered)
            .or(hovered)
            .or_else(|| self.focusable_in(region).first().copied());
        match target {
            Some(target) if self.focus.focused != Some(target) => {
                self.move_focus(target);
                true
            }
            _ => false,
        }
    }

    /// Focuses `key`: remembers it in every scope around it, records it in the
    /// history, and scrolls it into view.
    fn move_focus(&mut self, key: NodeKey) {
        if self.focus.focused == Some(key) {
            return;
        }
        self.focus.focused = Some(key);
        for scope in self.store.ancestry(Some(key)) {
            if let Some(slot) = self.store.find(scope) {
                let node = self.store.get_mut(slot);
                if node.behavior.scope != Scope::None {
                    node.remembered = Some(key);
                }
            }
        }
        let history = &mut self.focus.history;
        history.retain(|&held| held != key);
        history.push(key);
        if history.len() > FOCUS_HISTORY {
            history.remove(0);
        }
        self.scroll_into_view(key);
    }

    /// Moves every `overflow: scroll` block around `key` the least that puts
    /// its border box inside the block's content box, clamped to the block's
    /// reach.
    fn scroll_into_view(&mut self, key: NodeKey) {
        let Some(node) = self.store.by_key(key) else {
            return;
        };
        let (mut min, mut max) = node.rect;
        for ancestor in self.store.ancestry(Some(key)).into_iter().skip(1) {
            let Some(slot) = self.store.find(ancestor) else {
                continue;
            };
            let container = self.store.get_mut(slot);
            if container.resolved.overflow != Overflow::Scroll {
                continue;
            }
            // The content box: the border box less the border and the padding
            // the layout resolved.
            let (border, padding) = (container.unrounded.border, container.unrounded.padding);
            let view = (
                container.rect.0 + Vec2::new(border.left + padding.left, border.top + padding.top),
                container.rect.1
                    - Vec2::new(border.right + padding.right, border.bottom + padding.bottom),
            );
            let mut delta = Vec2::ZERO;
            for axis in 0..2 {
                if min[axis] < view.0[axis] {
                    delta[axis] = min[axis] - view.0[axis];
                } else if max[axis] > view.1[axis] {
                    // A node taller than the view keeps its start in sight.
                    delta[axis] = (max[axis] - view.1[axis]).min(min[axis] - view.0[axis]);
                }
            }
            let before = container.scroll_offset;
            let after = (before + delta).clamp(Vec2::ZERO, container.scroll_max.max(Vec2::ZERO));
            container.scroll_offset = after;
            let applied = after - before;
            min -= applied;
            max -= applied;
        }
    }

    /// A directional move from the focused node; see the module docs.
    fn move_spatially(&mut self, direction: Direction, modal: Option<NodeKey>) {
        let Some(from) = self.focus.focused else {
            return;
        };
        let Some(current) = self.store.by_key(from) else {
            return;
        };
        match current.resolved.nav(direction) {
            NavTarget::Auto => {}
            NavTarget::None => return,
            NavTarget::Id(id) => {
                let target = self
                    .focusable_in(modal)
                    .into_iter()
                    .find(|&key| self.store.by_key(key).and_then(|node| node.id) == Some(id));
                if let Some(target) = target {
                    self.move_focus(target);
                    return;
                }
                if self.focus.warned.insert(from) {
                    let selector = current
                        .candidates
                        .as_ref()
                        .map_or("", |held| held.selector.as_str());
                    crcbl_core::warn!(
                        "ui tree: `{selector}`'s nav-{} names an id no focusable node has; the \
                         move searches instead",
                        match direction {
                            Direction::Up => "up",
                            Direction::Right => "right",
                            Direction::Down => "down",
                            Direction::Left => "left",
                        }
                    );
                }
            }
        }

        let origin = current.rect;
        let candidates: Vec<(NodeKey, (Vec2, Vec2))> = self
            .focusable_in(modal)
            .into_iter()
            .filter_map(|key| self.store.by_key(key).map(|node| (key, node.rect)))
            .collect();
        let mut scored: Vec<NavScore> = Vec::new();
        let mut record = |found: &[(NodeKey, Score)]| {
            for &(key, score) in found {
                if !scored.iter().any(|held| held.key == key) {
                    scored.push(NavScore {
                        key,
                        score,
                        chosen: false,
                    });
                }
            }
        };

        // Innermost first: every scope root, scroll container and wrapping
        // container around the focused node, then the whole tree.
        let mut target = None;
        let mut regions: Vec<Option<NodeKey>> = self
            .store
            .ancestry(Some(from))
            .into_iter()
            .skip(1)
            .filter(|&key| {
                self.store.by_key(key).is_some_and(|node| {
                    node.behavior.scope != Scope::None
                        || node.resolved.overflow == Overflow::Scroll
                        || node.resolved.nav_wrap.wraps(direction.is_horizontal())
                })
            })
            .map(Some)
            .collect();
        regions.push(None);
        for region in regions {
            let inside: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|&(key, _)| key != from && self.inside(key, region))
                .collect();
            let (found, scores) = spatial::pick(origin, inside, direction);
            record(&scores);
            target = found;
            let node = region.and_then(|region| self.store.by_key(region));
            if target.is_none()
                && let Some(node) = node
                && node.resolved.nav_wrap.wraps(direction.is_horizontal())
            {
                let shifted = wrapped(origin, node.rect, direction);
                let around: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|&(key, _)| self.inside(key, region))
                    .collect();
                let (found, scores) = spatial::pick(shifted, around, direction);
                record(&scores);
                target = found.filter(|&key| key != from);
            }
            if target.is_some() || node.is_some_and(|node| node.behavior.scope == Scope::Modal) {
                break;
            }
        }

        if let Some(found) = target {
            let target = self.remembered_on_entry(from, found, origin, direction);
            for score in &mut scored {
                score.chosen = score.key == target;
            }
            self.focus.scores = scored;
            self.move_focus(target);
        } else {
            self.focus.scores = scored;
        }
    }

    /// `target`, or — when it lies in scopes `from` is not in — the node the
    /// outermost of those scopes remembers, if it is still focusable there and
    /// lies in `direction` from `origin` too, so a remembered node never pulls
    /// focus against the press.
    fn remembered_on_entry(
        &self,
        from: NodeKey,
        target: NodeKey,
        origin: (Vec2, Vec2),
        direction: Direction,
    ) -> NodeKey {
        let entered = self
            .store
            .ancestry(Some(target))
            .into_iter()
            .skip(1)
            .rfind(|&scope| {
                self.store
                    .by_key(scope)
                    .is_some_and(|node| node.behavior.scope != Scope::None)
                    && !self.store.is_within(from, scope)
            });
        entered
            .and_then(|scope| {
                let remembered = self.store.by_key(scope)?.remembered?;
                let ahead = self
                    .store
                    .by_key(remembered)
                    .is_some_and(|node| spatial::score(origin, node.rect, direction).is_some());
                (ahead && self.can_focus(remembered) && self.store.is_within(remembered, scope))
                    .then_some(remembered)
            })
            .unwrap_or(target)
    }

    /// A tree-order step: forward when `forward`, wrapping at the ends.
    fn move_in_tree_order(&mut self, forward: bool, modal: Option<NodeKey>) {
        let order = self.focusable_in(modal);
        if order.is_empty() {
            return;
        }
        let at = self
            .focus
            .focused
            .and_then(|focused| order.iter().position(|&key| key == focused));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % order.len(),
            (Some(at), false) => (at + order.len() - 1) % order.len(),
            (None, true) => 0,
            (None, false) => order.len() - 1,
        };
        self.move_focus(order[next]);
    }

    // -- the public surface ---------------------------------------------------

    /// The focused node, in either input mode.
    #[must_use]
    pub fn focused(&self) -> Option<NodeKey> {
        self.focus.focused
    }

    /// Focuses `key` when the next frame begins, committing the engaged node
    /// there if it is another — unless `key` is not a focusable node of the
    /// tree that frame is resolved against, or lies outside an open modal,
    /// which leaves focus where it is.
    pub fn set_focus(&mut self, key: NodeKey) {
        self.focus.requested = Some(key);
    }

    /// The engaged node.
    #[must_use]
    pub fn engaged(&self) -> Option<NodeKey> {
        self.focus.engaged
    }

    /// Which kind of device this frame's input says spoke last.
    #[must_use]
    pub fn input_mode(&self) -> InputMode {
        self.focus.mode
    }

    /// Whether back was pressed this frame with nothing engaged to cancel: the
    /// caller's to close a modal or pop a screen with.
    #[must_use]
    pub fn back_requested(&self) -> bool {
        self.focus.back
    }

    /// The nodes focused most recently, oldest first; see [`FOCUS_HISTORY`].
    #[must_use]
    pub fn focus_history(&self) -> &[NodeKey] {
        &self.focus.history
    }

    /// Every candidate the frame's directional move scored, in tree order, the
    /// one it landed on marked. Empty on a frame with no directional move.
    #[must_use]
    pub fn nav_scores(&self) -> &[NavScore] {
        &self.focus.scores
    }

    /// The widget's half of the engaged contract, called every frame with the
    /// node's `response` and the value it edits: on [`Engagement::Began`] a copy
    /// of `value` is kept, on [`Engagement::Cancelled`] the copy is written back
    /// into `value`, and on [`Engagement::Committed`] it is dropped.
    ///
    /// A cancel that finds a copy of another type — the widget passed a
    /// different `T` from the one it engaged with — leaves `value` alone and
    /// logs a warning.
    pub fn snapshot<T: Clone + Send + Sync + 'static>(
        &mut self,
        response: &Response,
        value: &mut T,
    ) {
        match response.engagement {
            Engagement::Began => {
                self.focus.snapshot = Some(Snapshot {
                    key: response.key,
                    value: Box::new(value.clone()),
                });
            }
            Engagement::Cancelled | Engagement::Committed => {
                let Some(held) = self.focus.snapshot.take_if(|held| held.key == response.key)
                else {
                    return;
                };
                if response.engagement == Engagement::Committed {
                    return;
                }
                match held.value.downcast::<T>() {
                    Ok(copy) => *value = *copy,
                    Err(_) => crcbl_core::warn!(
                        "ui tree: an engaged widget cancelled with a value of another type than \
                         it engaged with; its value is left as it is"
                    ),
                }
            }
            Engagement::Idle | Engagement::Engaged => {}
        }
    }
}

/// `origin` moved to just before `container`'s start along a move in
/// `direction`, so that a search from it finds what lies first inside the
/// container that way.
fn wrapped(origin: (Vec2, Vec2), container: (Vec2, Vec2), direction: Direction) -> (Vec2, Vec2) {
    let (min, max) = origin;
    let shift = match direction {
        Direction::Right => Vec2::new(container.0.x - max.x, 0.0),
        Direction::Left => Vec2::new(container.1.x - min.x, 0.0),
        Direction::Down => Vec2::new(0.0, container.0.y - max.y),
        Direction::Up => Vec2::new(0.0, container.1.y - min.y),
    };
    (min + shift, max + shift)
}
