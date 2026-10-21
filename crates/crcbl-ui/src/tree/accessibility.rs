//! The tree as a screen reader reads it: an AccessKit [`TreeUpdate`] built from
//! the frame as laid out, and the actions a reader asks for acted on. Behind
//! the `accesskit` feature; `crcbl-shell`'s feature of the same name carries
//! the update to the platform and brings the requests back.
//!
//! # The tree
//!
//! A node's [`NodeKey`] is stable across frames, so it is the node's
//! [`NodeId`]. Above the frame's roots — the tree's and every pop-up's and the
//! tooltip's, in build order — sits a synthetic [`Role::Window`] keyed by the
//! seed every root's key is derived from, which no node's key equals.
//!
//! **Every update carries the whole tree**, with [`TreeInfo`], rather than the
//! nodes that changed: simple, always valid however many frames the reader
//! missed, and what AccessKit asks for when a reader first connects. A frame
//! is rebuilt from nothing anyway, so a diff would cost a copy of last frame's
//! tree to compare against.
//!
//! A node's role comes from what its builder recorded it as
//! ([`WidgetKind`]), never from its selector, whose type a caller may have
//! replaced. A node no builder marked is a [`Role::Label`] holding its text
//! when it is a text span, a [`Role::Image`] when it is a picture, and a
//! [`Role::GenericContainer`] otherwise. A widget whose parts are all its
//! builder's own — a button, a checkbox, a tab, a select's option — is one
//! node: its spans' text becomes its label, or its value where the text is
//! what it holds (a drag-value's number, a select's choice), and its parts are
//! left out. Decorations — a checkbox's box, a toggle's glyph — are left out
//! with everything inside them.
//!
//! Text is as it was built, not as `text-overflow: ellipsis` cut it: a reader
//! should hear what a label says, not what fitted.
//!
//! # What it does not say
//!
//! Only what the tree holds. A slider's and a drag-value's numbers, their
//! range and step are the caller's and never stored, so a slider has no value
//! and a drag-value only the text it shows; a block a caller built to look
//! like a checkbox, without [`Ui::checkbox`], has no toggled state. A widget's
//! name drawn beside it rather than inside it (a text input's label, a select's
//! caption) is not tied to it.
//!
//! # Names a caller gives
//!
//! [`Ui::set_accessible_label`] names a node for this frame. Its own text spans
//! are then not read on their own — they are the pieces the name stands for, as
//! when a label is drawn as runs to accent part of it — and a block left with
//! nothing else to hold is read as one [`Role::Label`].
//!
//! # Actions
//!
//! A focusable node offers [`Action::Focus`], the focused node
//! [`Action::Blur`], and a node with a focus role that is not disabled
//! [`Action::Click`]; [`Ui::accessibility_action`] acts on those three.

use accesskit::{
    Action, ActionRequest, Node, NodeId, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate,
};

use super::focus::{self, Scope};
use super::widgets::{MASK, WidgetKind, WidgetState};
use super::{Content, NodeKey, ROOT_KEY, Ui};
use crate::style::PseudoClasses;

impl Ui {
    /// The whole tree as an AccessKit update, for `crcbl-shell`'s
    /// `Shell::update_accessibility`: valid once the frame is laid out, and
    /// sent whole every time — see the module docs for what it holds.
    ///
    /// Focus is the focused node when this frame built it, else the window.
    #[must_use]
    pub fn accessibility_update(&self) -> TreeUpdate {
        let root = NodeId(ROOT_KEY.0);
        let mut window = Node::new(Role::Window);
        let mut nodes = Vec::with_capacity(self.nodes.len() + 1);
        for index in 0..self.nodes.len() {
            if self.nodes[index].parent.is_none() {
                self.push_accessible(index, &mut window, &mut nodes);
            }
        }
        let focus = self
            .focused()
            .map(|key| NodeId(key.0))
            .filter(|focused| nodes.iter().any(|(id, _)| id == focused))
            .unwrap_or(root);
        nodes.push((root, window));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    /// Acts on what a screen reader asked of a node in the tree
    /// [`Ui::accessibility_update`] built: [`Action::Focus`] focuses it as
    /// [`Ui::set_focus`] does, [`Action::Blur`] takes focus away from the
    /// focused node as [`Ui::clear_focus`] does, and [`Action::Click`] makes
    /// it report [`Response::clicked`](super::Response::clicked) on the next
    /// frame, as a pointer's click would — focusing it, and engaging a widget
    /// that engages.
    ///
    /// Returns whether it was acted on: false for another tree, a node the
    /// store does not hold (the window among them), an action the node does
    /// not offer, and any other action.
    pub fn accessibility_action(&mut self, request: &ActionRequest) -> bool {
        if request.target_tree != TreeId::ROOT {
            return false;
        }
        let key = NodeKey(request.target_node.0);
        let Some(node) = self.store.by_key(key) else {
            return false;
        };
        match request.action {
            Action::Focus if node.behavior.is_focusable() => {
                self.set_focus(key);
                true
            }
            Action::Blur if self.focused() == Some(key) => {
                self.clear_focus();
                true
            }
            Action::Click if clickable(node.behavior) => {
                self.requested_click = Some(key);
                true
            }
            _ => false,
        }
    }

    /// Names the node `key` for screen readers this frame, as `label`: what a
    /// reader hears for it, in place of the text spans directly inside it. For
    /// a name drawn as pieces — a label split into runs so the characters a
    /// search matched can be accented reads as its runs, "ga" then "mma",
    /// unless the block holding them is named "gamma". Set it each frame the
    /// node is built; a key not built this frame is ignored.
    pub fn set_accessible_label(&mut self, key: NodeKey, label: &str) {
        self.accessible_labels.insert(key, label.to_owned());
    }

    /// The node a screen reader clicked since the last frame, now reporting
    /// clicked — unless it has gone, or been disabled, since.
    pub(super) fn resolve_requested_click(&mut self) -> Option<NodeKey> {
        let key = self.requested_click.take()?;
        let slot = self.store.find(key)?;
        let node = self.store.get_mut(slot);
        if !clickable(node.behavior) {
            return None;
        }
        node.interaction.clicked = true;
        Some(key)
    }

    /// Adds the frame node `index` under `parent`, and pushes it and what it
    /// holds onto `out`, unless it is a decoration.
    fn push_accessible(&self, index: usize, parent: &mut Node, out: &mut Vec<(NodeId, Node)>) {
        let frame = &self.nodes[index];
        let stored = self.store.get(frame.slot);
        let kind = stored.kind;
        if kind == WidgetKind::Decoration {
            return;
        }
        let id = NodeId(frame.key.0);
        parent.push_child(id);

        let mut node = Node::new(role_of(frame.content, kind));
        let (min, max) = stored.rect;
        node.set_bounds(Rect {
            x0: f64::from(min.x),
            y0: f64::from(min.y),
            x1: f64::from(max.x),
            y1: f64::from(max.y),
        });
        if !stored.hittable {
            node.set_hidden();
        }
        if stored.behavior.disabled {
            node.set_disabled();
        }
        if stored.behavior.scope == Scope::Modal {
            node.set_modal();
        }
        if stored.behavior.is_focusable() {
            node.add_action(Action::Focus);
        }
        if self.focused() == Some(frame.key) {
            node.add_action(Action::Blur);
        }
        if clickable(stored.behavior) {
            node.add_action(Action::Click);
        }
        if let Content::Text { start, end } | Content::Run { start, end } = frame.content {
            node.set_value(&self.text[start..end]);
        }

        let state = stored.state;
        match kind {
            WidgetKind::Checkbox => node.set_toggled(if state.contains(PseudoClasses::CHECKED) {
                Toggled::True
            } else {
                Toggled::False
            }),
            WidgetKind::Disclosure | WidgetKind::Select => {
                node.set_expanded(state.contains(PseudoClasses::OPEN));
            }
            WidgetKind::MenuItem if stored.widget == WidgetState::MenuBranch => {
                node.set_expanded(state.contains(PseudoClasses::OPEN));
            }
            WidgetKind::TreeRow | WidgetKind::OutlinerRow => {
                if let WidgetState::TreeItem {
                    open, branch: true, ..
                } = stored.widget
                {
                    node.set_expanded(open);
                }
            }
            _ => {}
        }
        if matches!(
            kind,
            WidgetKind::Tab | WidgetKind::SelectOption | WidgetKind::OutlinerRow
        ) {
            node.set_selected(state.contains(PseudoClasses::CHECKED));
        }
        // A list row is selected only in a list that has a selection,
        // [`Ui::selectable_list`]; a plain list's rows say nothing either way.
        if kind == WidgetKind::ListRow && state.contains(PseudoClasses::CHECKED) {
            node.set_selected(true);
        }

        let label = self.accessible_labels.get(&frame.key);
        if is_whole(kind) {
            self.name_from_contents(index, kind, &mut node);
        } else {
            let mut held = false;
            let mut next = frame.first_child;
            while let Some(child) = next {
                let is_text = matches!(
                    self.nodes[child].content,
                    Content::Text { .. } | Content::Run { .. }
                );
                if label.is_none() || !is_text {
                    self.push_accessible(child, &mut node, out);
                    held = true;
                }
                next = self.nodes[child].next_sibling;
            }
            if label.is_some()
                && !held
                && frame.content == Content::Block
                && kind == WidgetKind::None
            {
                node.set_role(Role::Label);
            }
        }
        if let Some(label) = label {
            node.set_label(label.as_str());
        }
        out.push((id, node));
    }

    /// Names the widget at frame node `index`, one node with no children in
    /// the tree, from the text its parts hold: its label, or for a widget that
    /// holds a value, its value.
    fn name_from_contents(&self, index: usize, kind: WidgetKind, node: &mut Node) {
        let mut text = String::new();
        self.gather_text(index, &mut text);
        let key = self.nodes[index].key;
        match kind {
            WidgetKind::TextInput { masked } => {
                let value = self
                    .edits
                    .get(&key)
                    .map(|state| state.text())
                    .unwrap_or_default();
                if masked {
                    node.set_value(String::from_iter(std::iter::repeat_n(
                        MASK,
                        value.chars().count(),
                    )));
                } else {
                    node.set_value(value);
                }
                // An empty input draws its placeholder in its value's place.
                if value.is_empty() && !text.is_empty() {
                    node.set_placeholder(text);
                }
            }
            WidgetKind::DragValue => {
                let typed = self
                    .store
                    .by_key(key)
                    .is_some_and(|stored| stored.widget.is_typed_number());
                match self.edits.get(&key).filter(|_| typed) {
                    Some(state) => node.set_value(state.text()),
                    None => node.set_value(text),
                }
            }
            WidgetKind::Select => node.set_value(text),
            _ if !text.is_empty() => node.set_label(text),
            _ => {}
        }
    }

    /// Appends the text of every span inside frame node `index`, outside
    /// decorations, to `out`, a space between each.
    fn gather_text(&self, index: usize, out: &mut String) {
        let mut next = self.nodes[index].first_child;
        while let Some(child) = next {
            let node = &self.nodes[child];
            if self.store.get(node.slot).kind != WidgetKind::Decoration {
                if let Content::Text { start, end } | Content::Run { start, end } = node.content {
                    let text = self.text[start..end].trim();
                    if !text.is_empty() {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(text);
                    }
                }
                self.gather_text(child, out);
            }
            next = node.next_sibling;
        }
    }
}

/// Whether a node that behaves as `behavior` takes a click a reader asks for:
/// one with a focus role, as a pointer's press goes to, and not disabled.
fn clickable(behavior: focus::Behavior) -> bool {
    behavior.role != focus::Role::None && !behavior.disabled
}

/// The role a node of `content`, which its builder recorded as `kind`, has.
const fn role_of(content: Content, kind: WidgetKind) -> Role {
    match kind {
        WidgetKind::None | WidgetKind::Decoration => match content {
            Content::Block => Role::GenericContainer,
            Content::Text { .. } | Content::Run { .. } => Role::Label,
            Content::Paragraph => Role::Paragraph,
            Content::Image(_) => Role::Image,
        },
        WidgetKind::Button | WidgetKind::Disclosure => Role::Button,
        WidgetKind::Checkbox => Role::CheckBox,
        WidgetKind::Slider => Role::Slider,
        WidgetKind::DragValue => Role::SpinButton,
        WidgetKind::TextInput { masked: false } => Role::TextInput,
        WidgetKind::TextInput { masked: true } => Role::PasswordInput,
        WidgetKind::Select => Role::ComboBox,
        WidgetKind::SelectList => Role::ListBox,
        WidgetKind::SelectOption => Role::ListBoxOption,
        WidgetKind::TreeRow | WidgetKind::OutlinerRow => Role::TreeItem,
        WidgetKind::Outliner => Role::Tree,
        WidgetKind::OutlinerToggle => Role::DisclosureTriangle,
        WidgetKind::List => Role::List,
        WidgetKind::ListRow => Role::ListItem,
        WidgetKind::TabStrip => Role::TabList,
        WidgetKind::Tab => Role::Tab,
        WidgetKind::TabPanel => Role::TabPanel,
        WidgetKind::Splitter => Role::Splitter,
        WidgetKind::Pane => Role::Pane,
        WidgetKind::ScrollBar => Role::ScrollBar,
        WidgetKind::Popup => Role::Dialog,
        WidgetKind::Menu => Role::Menu,
        WidgetKind::MenuItem => Role::MenuItem,
        WidgetKind::Tooltip => Role::Tooltip,
        WidgetKind::Inspector => Role::Group,
    }
}

/// Whether a widget recorded as `kind` is one node in the tree, named from
/// its parts' text: every part is its builder's, so none holds anything a
/// reader could reach on its own. A list's or an outliner's row holds what the
/// caller built in it, a button among them, so it keeps its children.
const fn is_whole(kind: WidgetKind) -> bool {
    matches!(
        kind,
        WidgetKind::Button
            | WidgetKind::Checkbox
            | WidgetKind::Slider
            | WidgetKind::DragValue
            | WidgetKind::TextInput { .. }
            | WidgetKind::Select
            | WidgetKind::SelectOption
            | WidgetKind::Disclosure
            | WidgetKind::TreeRow
            | WidgetKind::Tab
            | WidgetKind::MenuItem
            | WidgetKind::Splitter
            | WidgetKind::OutlinerToggle
    )
}
