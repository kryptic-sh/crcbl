//! A context menu: a list of items in a pop-up, opened on a widget by a
//! secondary press over it or by `ui_menu` while it holds focus.
//!
//! # Asking for one
//!
//! A widget's builder passes the key of the node the menu belongs to — a
//! button's [`Response::key`](crate::tree::Response::key), an outliner row's
//! [`Ui::current_key`] — to [`Ui::context_menu`] with the items, every frame,
//! after building the node, as it passes one to [`Ui::tooltip`]. The call
//! marks the node and builds nothing while the menu is closed, so it can sit
//! after a widget unconditionally.
//!
//! **The subject** is decided when a frame begins, from last frame's marks: for
//! a secondary press, the innermost node under the pointer that asked for a
//! menu; for `ui_menu` ([`NavInput::menu`]), the focused node or the innermost
//! node around it that asked. So a menu on a panel and another on a row inside
//! it each open where they should. A disabled node's menu never opens.
//!
//! # Where it opens
//!
//! A secondary press opens the menu with its top-left corner at the pointer
//! ([`Placement::At`]); `ui_menu` opens it below the widget
//! ([`Placement::Below`]), where the keyboard's attention is. A submenu opens
//! beside its item ([`Placement::Beside`]). Each flips at the viewport's edges
//! and is shifted inside it, as `popup.rs` says.
//!
//! **A secondary press closes every pop-up above the layer it lands in**, as a
//! primary press does, but it is not spent: it goes on to open the menu of
//! whatever it landed on, so a right-click on another row moves the menu there
//! and a right-click on the open menu's own widget reopens it at the new
//! point, as desktop menus do. Nothing captures it, so it presses, hovers and
//! clicks nothing.
//!
//! # Items
//!
//! Each item is a [`Behavior::BUTTON`], so a click and accept pick it alike,
//! and focus moves into the menu as into every pop-up — by the landing rule,
//! onto its first enabled item. **Picking an action item reports its value
//! once and closes the whole chain**, submenus and all, giving focus back to
//! the widget. A disabled item is `:disabled`, so it is never focused or
//! picked, and a separator is a rule between items that takes no part in
//! focus. An item with a submenu opens it on a click or accept and on the right
//! arrow, and left inside a submenu closes it with focus back on its item.
//! Back closes the topmost menu, a level at a time, and a press outside every
//! menu closes them all and is spent — the pop-up stack's rules.
//!
//! Items are keyed by their label, as a drop-down's options are, so two items
//! of one label in one menu are a duplicate key.

use super::{Ui, WidgetState};
use crate::style::PseudoClasses;
use crate::tree::{
    Behavior, Direction, KeySource, NavInput, NodeKey, Placement, hash_of, popup::OpenPopup,
};
use crate::widget::PointerInput;

/// What an item with a submenu shows after its label: a letter rather than an
/// arrow, because the built-in bitmap font covers printable ASCII and nothing
/// else.
pub const CONTEXT_ARROW: &str = ">";

/// One entry of a context menu; `T` is what picking an action reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextItem<'a, T> {
    /// An item that reports `value` when picked.
    Action {
        /// What it reads.
        label: &'a str,
        /// What picking it reports.
        value: T,
        /// Whether it can be picked; disabled, it is `:disabled`.
        enabled: bool,
    },
    /// An item that opens `items` beside it.
    Submenu {
        /// What it reads.
        label: &'a str,
        /// What its submenu holds.
        items: &'a [ContextItem<'a, T>],
        /// Whether it can be opened; disabled, it is `:disabled`.
        enabled: bool,
    },
    /// A rule between items.
    Separator,
}

impl<'a, T> ContextItem<'a, T> {
    /// An enabled item reading `label` that reports `value` when picked.
    pub const fn action(label: &'a str, value: T) -> Self {
        Self::Action {
            label,
            value,
            enabled: true,
        }
    }

    /// An enabled item reading `label` that opens `items` beside it.
    pub const fn submenu(label: &'a str, items: &'a [Self]) -> Self {
        Self::Submenu {
            label,
            items,
            enabled: true,
        }
    }

    /// This item, enabled or disabled as `enabled` says; a separator as it
    /// is.
    #[must_use]
    pub fn enabled(self, enabled: bool) -> Self {
        match self {
            Self::Action { label, value, .. } => Self::Action {
                label,
                value,
                enabled,
            },
            Self::Submenu { label, items, .. } => Self::Submenu {
                label,
                items,
                enabled,
            },
            Self::Separator => Self::Separator,
        }
    }
}

/// What a frame's [`Ui::context_menu`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMenuResponse<T> {
    /// The menu opened this frame, or opened again at a new point: when a
    /// caller makes its widget what the menu acts on, as an editor selects
    /// the row a right-click landed on.
    pub opened: bool,
    /// The value of the action item picked this frame, which closed the menu.
    pub picked: Option<T>,
}

impl Ui {
    /// The context menu of the node `anchor`, which this frame built: a
    /// `popup.context-menu` holding a `.context-item` block per item — a
    /// `.context-item-label` span, and for a submenu's item a
    /// `.context-item-arrow` span showing [`CONTEXT_ARROW`], `:open` while
    /// its submenu is — and a `.context-separator` block per separator. A
    /// submenu is a `popup.context-menu` of its own. See the module docs for
    /// when it opens and closes; call it every frame the node is built, after
    /// it.
    pub fn context_menu<T: Copy>(
        &mut self,
        anchor: NodeKey,
        items: &[ContextItem<'_, T>],
    ) -> ContextMenuResponse<T> {
        let mut response = ContextMenuResponse {
            opened: false,
            picked: None,
        };
        let Some(slot) = self.store.find(anchor) else {
            return response;
        };
        if self.store.get(slot).behavior.disabled {
            self.close_popup(anchor);
            return response;
        }
        self.store.get_mut(slot).context_menu = true;
        if let Some((key, placement)) = self.context_request
            && key == anchor
        {
            // A press that asked closed every pop-up above the layer it landed
            // in, this menu's among them; it is open already only when none of
            // its items could take focus, so `ui_menu` came again from the
            // anchor, and then it stays where it is.
            self.context_request = None;
            response.opened = !self.is_popup_open(anchor);
            self.open_popup_at(anchor, placement);
        } else if let Some(value) = self.picked_item(anchor, items) {
            self.close_popup(anchor);
            response.picked = Some(value);
        }
        self.build_context_menu(anchor, items);
        response
    }

    /// The node this frame's secondary press or `ui_menu` asks for a context
    /// menu on, from last frame's tree, and where it goes; see the module
    /// docs. Runs once pointer and focus are resolved.
    pub(in crate::tree) fn resolve_context_menu(&mut self, pointer: PointerInput, nav: NavInput) {
        let asked =
            |ui: &Self, key: NodeKey| ui.store.by_key(key).is_some_and(|node| node.context_menu);
        self.context_request = if pointer.secondary_pressed {
            let over = self.store.hit_chain(pointer.pos);
            self.close_above_press(&over);
            // Again: what the press closed is no longer under it.
            let over = self.store.hit_chain(pointer.pos);
            over.into_iter()
                .find(|&key| asked(self, key))
                .map(|key| (key, Placement::At(pointer.pos)))
        } else if nav.menu {
            self.store
                .ancestry(self.focused())
                .into_iter()
                .find(|&key| asked(self, key))
                .map(|key| (key, Placement::Below))
        } else {
            None
        };
    }

    /// The left and right a focused menu item takes: right on an item with a
    /// submenu opens it, and left inside a submenu closes it, focus going
    /// back to its item when the next frame begins. Returns false — changing
    /// nothing — anywhere else, so the step moves focus as the layout says.
    pub(in crate::tree) fn submenu_step(&mut self, direction: Direction) -> bool {
        let Some(focused) = self.focused() else {
            return false;
        };
        match direction {
            Direction::Right
                if self.widget_state(focused) == WidgetState::MenuBranch
                    && !self.is_popup_open(focused) =>
            {
                self.open_popup_at(focused, Placement::Beside);
                true
            }
            Direction::Left => {
                let Some(&OpenPopup { anchor, .. }) = self.popups.last() else {
                    return false;
                };
                let inside = self.store.is_within(focused, Self::popup_key(anchor));
                if !inside || self.widget_state(anchor) != WidgetState::MenuBranch {
                    return false;
                }
                self.close_popups_from(self.popups.len() - 1);
                true
            }
            Direction::Right | Direction::Up | Direction::Down => false,
        }
    }

    /// The key of the item reading `label` in the menu whose root is `menu`.
    fn context_item_key(menu: NodeKey, label: &str) -> NodeKey {
        Self::key_under(menu, KeySource::Keyed(hash_of(label)))
    }

    /// The action item this frame's click or accept picked in the open menu
    /// hanging from `menu`, or in a submenu open from it — opening the submenu
    /// of an item that was clicked instead. Resolved before anything is built,
    /// so a picked menu is not drawn in the frame it closes.
    fn picked_item<T: Copy>(&mut self, menu: NodeKey, items: &[ContextItem<'_, T>]) -> Option<T> {
        if !self.is_popup_open(menu) {
            return None;
        }
        let root = Self::popup_key(menu);
        for item in items {
            match *item {
                ContextItem::Action {
                    label,
                    value,
                    enabled,
                } => {
                    let key = Self::context_item_key(root, label);
                    if enabled && self.interaction_of(key).clicked {
                        return Some(value);
                    }
                }
                ContextItem::Submenu {
                    label,
                    items,
                    enabled,
                } => {
                    let key = Self::context_item_key(root, label);
                    if enabled && self.interaction_of(key).clicked {
                        self.open_popup_at(key, Placement::Beside);
                    }
                    if let Some(value) = self.picked_item(key, items) {
                        return Some(value);
                    }
                }
                ContextItem::Separator => {}
            }
        }
        None
    }

    /// The open menu hanging from `menu` and every submenu open from it; see
    /// [`Ui::context_menu`] for what each holds.
    fn build_context_menu<T: Copy>(&mut self, menu: NodeKey, items: &[ContextItem<'_, T>]) {
        let root = Self::popup_key(menu);
        self.popup(menu, ".context-menu", &[], |ui| {
            for (index, item) in items.iter().enumerate() {
                let (label, enabled, submenu) = match *item {
                    ContextItem::Action { label, enabled, .. } => (label, enabled, None),
                    ContextItem::Submenu {
                        label,
                        items,
                        enabled,
                    } => (label, enabled, Some(items)),
                    ContextItem::Separator => {
                        ui.block_keyed(index, ".context-separator", &[], |_| {});
                        continue;
                    }
                };
                let key = Self::context_item_key(root, label);
                let state = if submenu.is_some() && ui.is_popup_open(key) {
                    PseudoClasses::OPEN
                } else {
                    PseudoClasses::NONE
                };
                let behavior = Behavior {
                    disabled: !enabled,
                    ..Behavior::BUTTON
                };
                let parsed = ui.node_selector(".context-item");
                ui.open_block(key, parsed, &[], behavior, state, |ui| {
                    ui.span(".context-item-label", label, &[]);
                    if submenu.is_some() {
                        ui.span(".context-item-arrow", CONTEXT_ARROW, &[]);
                    }
                });
                // Kept either way: a label can name a submenu one frame and an
                // action the next.
                let branch = submenu.map_or(WidgetState::None, |_| WidgetState::MenuBranch);
                ui.set_widget_state(key, branch);
                if let Some(items) = submenu {
                    ui.build_context_menu(key, items);
                }
            }
        });
    }
}
