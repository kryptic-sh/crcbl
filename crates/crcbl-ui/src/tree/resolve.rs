//! Style resolution for the tree: each node's style from the stylesheets, at
//! the moment it is built, with the work skipped when nothing it depends on
//! moved.
//!
//! # Two caches
//!
//! **Per node**, the store keeps the style a node resolved to and a key of
//! everything that decided it: the stylesheet generation and cache epoch, the
//! bits of the node's pseudo-state its candidate rules test, its parent's
//! [`InheritedId`], and — only when a candidate rule reads above the node —
//! each ancestor's selector and the pseudo-state bits the rules test ancestors
//! for. With the same selector and the same inline declarations as well, the
//! node takes its stored style without matching anything; that is what makes a
//! pointer move re-resolve only nodes whose rules mention `:hover`.
//!
//! **Across nodes**, a node that does match looks its merged definition up in
//! the [`crate::style`] cascade's cache, keyed by the rules it matched, so a
//! hundred rows of one class merge their rules once.
//!
//! Resolution runs before layout and emission see the node: a node's
//! [`NodeStyle`] is the resolved one, and [`NodeStyle::layout_hash`] of it is
//! what decides whether layout caches clear — so a rule that changes only a
//! colour on hover moves no layout.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::Duration;

use super::store::StoredCandidates;
use super::{Content, FrameNode, NodeStyle, Ui};
use crate::style::{
    Declaration, Diagnostic, Element, InheritedId, NodeSelector, PseudoClasses, SheetId, StyleStats,
};

/// What resolving one node produced.
pub(super) struct Resolved {
    pub style: NodeStyle,
    pub pseudo: PseudoClasses,
    pub inherited: InheritedId,
}

/// A node as matching sees it: the one being built, or one already built this
/// frame.
#[derive(Clone, Copy)]
struct FrameElement<'a> {
    nodes: &'a [FrameNode],
    selectors: &'a str,
    node: Node<'a>,
}

#[derive(Clone, Copy)]
enum Node<'a> {
    Building {
        selector: NodeSelector<'a>,
        span: bool,
        pseudo: PseudoClasses,
        parent: Option<usize>,
    },
    Built(usize),
}

impl<'a> FrameElement<'a> {
    fn selector(&self) -> NodeSelector<'a> {
        match self.node {
            Node::Building { selector, .. } => selector,
            Node::Built(at) => {
                let (start, end) = self.nodes[at].selector;
                // Only a selector that parsed is stored.
                NodeSelector::parse(&self.selectors[start..end]).unwrap_or(NodeSelector::EMPTY)
            }
        }
    }
}

const fn default_type(span: bool) -> &'static str {
    if span { "span" } else { "block" }
}

impl Element for FrameElement<'_> {
    fn type_name(&self) -> &str {
        let span = match self.node {
            Node::Building { span, .. } => span,
            Node::Built(at) => !matches!(self.nodes[at].content, Content::Block),
        };
        self.selector().type_name.unwrap_or(default_type(span))
    }

    fn id(&self) -> Option<&str> {
        self.selector().id
    }

    fn has_class(&self, class: &str) -> bool {
        self.selector().classes().any(|own| own == class)
    }

    fn pseudo(&self) -> PseudoClasses {
        match self.node {
            Node::Building { pseudo, .. } => pseudo,
            Node::Built(at) => self.nodes[at].pseudo,
        }
    }

    fn parent(&self) -> Option<Self> {
        let parent = match self.node {
            Node::Building { parent, .. } => parent,
            Node::Built(at) => self.nodes[at].parent,
        };
        parent.map(|at| Self {
            node: Node::Built(at),
            ..*self
        })
    }
}

impl Ui {
    /// Resolves the style of the node about to be pushed into `slot` under
    /// `parent`. See the module docs.
    pub(super) fn resolve_style(
        &mut self,
        slot: usize,
        fresh: bool,
        parent: Option<usize>,
        selector: NodeSelector<'_>,
        span: bool,
        inline: &[Declaration],
    ) -> Resolved {
        self.styles.stats.nodes += 1;
        let generation = self.styles.generation;
        let stored = self.store.get(slot);
        let interaction = stored.interaction;
        let mut pseudo = PseudoClasses::NONE;
        if interaction.hovered {
            pseudo = pseudo | PseudoClasses::HOVER;
        }
        if interaction.pressed {
            pseudo = pseudo | PseudoClasses::ACTIVE;
        }

        let reused = stored.candidates.as_ref().is_some_and(|held| {
            held.generation == generation && held.span == span && held.selector == selector.text()
        });
        if !reused {
            let candidates = self.styles.resolver.index.candidates(
                selector.type_name.unwrap_or(default_type(span)),
                selector.id,
                selector.classes(),
            );
            self.store.get_mut(slot).candidates = Some(StoredCandidates {
                selector: selector.text().to_owned(),
                span,
                generation,
                candidates,
            });
        }
        let stored = self.store.get(slot);
        let candidates = &stored
            .candidates
            .as_ref()
            .expect("gathered above")
            .candidates;

        let parent_inherited = parent.map_or(InheritedId::ROOT, |at| self.nodes[at].inherited);
        let own_pseudo = pseudo & candidates.subject_pseudo;
        let mut key = DefaultHasher::new();
        (
            generation,
            self.styles.resolver.epoch,
            own_pseudo,
            parent_inherited,
        )
            .hash(&mut key);
        if candidates.reads_ancestors {
            let mut next = parent;
            while let Some(at) = next {
                let node = &self.nodes[at];
                let (start, end) = node.selector;
                (
                    &self.selectors[start..end],
                    matches!(node.content, Content::Block),
                    node.pseudo & candidates.ancestor_pseudo,
                )
                    .hash(&mut key);
                next = node.parent;
            }
        }
        let key = key.finish();

        if !fresh && reused && stored.style_key == key && stored.inline == inline {
            return Resolved {
                style: stored.resolved,
                pseudo,
                inherited: stored.inherited,
            };
        }

        self.styles.stats.resolves += 1;
        let element = FrameElement {
            nodes: &self.nodes,
            selectors: &self.selectors,
            node: Node::Building {
                selector,
                span,
                pseudo,
                parent,
            },
        };
        let matched = self.styles.resolver.index.matching(candidates, &element);
        let (definition, computed) =
            self.styles
                .resolver
                .definition(matched, own_pseudo, parent_inherited);
        if computed {
            self.styles.stats.definitions += 1;
        }
        let mut style = definition.style;
        for declaration in inline {
            declaration.apply(&mut style);
        }
        // A span has no children to pass anything to.
        let inherited = if span {
            InheritedId::ROOT
        } else {
            self.styles
                .resolver
                .intern(&style, definition.custom.clone())
        };

        let stored = self.store.get_mut(slot);
        stored.style_key = key;
        stored.inline.clear();
        stored.inline.extend_from_slice(inline);
        stored.resolved = style;
        stored.inherited = inherited;
        Resolved {
            style,
            pseudo,
            inherited,
        }
    }

    /// Adds a stylesheet parsed from `css`, above `default.css` and every sheet
    /// added before it. Its diagnostics go to the log as `name:line:column`;
    /// with no earlier sheet to keep, the rules that parsed are used even when
    /// some did not. Takes effect at the next [`Ui::begin_frame`].
    pub fn add_stylesheet(&mut self, name: &str, css: &str) -> SheetId {
        self.styles.add(name, css)
    }

    /// Adds the stylesheet at `path`, as [`Ui::add_stylesheet`], and watches
    /// it for [`Ui::poll_stylesheets`].
    ///
    /// # Errors
    ///
    /// When the file cannot be read.
    pub fn load_stylesheet(&mut self, path: impl AsRef<Path>) -> std::io::Result<SheetId> {
        self.styles.load(path.as_ref())
    }

    /// Replaces a stylesheet with one parsed from `css`, from the next
    /// [`Ui::begin_frame`] — **unless the parse has an error, which keeps the
    /// last good sheet** and returns every error. Warnings replace the sheet
    /// and are only logged.
    ///
    /// # Errors
    ///
    /// The parse's errors, when it had any.
    pub fn replace_stylesheet(&mut self, sheet: SheetId, css: &str) -> Result<(), Vec<Diagnostic>> {
        self.styles.replace(sheet, css)
    }

    /// Reads again every stylesheet loaded from a path whose modification time
    /// or length changed, as [`Ui::replace_stylesheet`] — at most once per
    /// [`crate::style::STYLESHEET_POLL_INTERVAL`] of `now`, the UI's own clock.
    /// Returns how many sheets were replaced.
    pub fn poll_stylesheets(&mut self, now: Duration) -> usize {
        self.styles.poll(now)
    }

    /// How many times a change to the stylesheets has taken effect. Each one
    /// re-resolves every node once.
    #[must_use]
    pub fn stylesheet_generation(&self) -> u64 {
        self.styles.generation
    }

    /// What style resolution did in the frame being built, or the last one.
    #[must_use]
    pub fn style_stats(&self) -> StyleStats {
        self.styles.stats
    }
}
