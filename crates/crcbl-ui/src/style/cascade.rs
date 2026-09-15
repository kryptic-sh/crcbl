//! The cascade: which rules a node gathers, in what order they apply, and the
//! cache of what they merge into.
//!
//! # The rule index
//!
//! [`RuleIndex`] holds every selector of every sheet as its own rule, sorted
//! into cascade order once — origin (the engine's `default.css` below every
//! app sheet), then [`Specificity`](super::Specificity) tier, then source order — so a node's
//! matched rules apply in ascending index and the last one to set a property
//! wins. Each rule is filed in one bucket by its rightmost compound: its id,
//! else its first class, else its type, else the universal bucket. A node's
//! candidates are the union of its id's, its classes' and its type's buckets
//! and the universal one; only those are matched, right to left.
//!
//! # Pseudo-class dependencies
//!
//! [`Candidates`] carries the union of what its rules test: the pseudo-classes
//! the node itself is tested for, those an ancestor is, and whether any rule
//! reads above the node at all. The tree folds only those bits of a node's
//! state into the key that decides whether to re-resolve it, so a node whose
//! candidates never mention `:hover` does not re-resolve when the pointer
//! moves.
//!
//! # The definition cache
//!
//! What a set of matched rules merges into is cached, keyed by the matched set,
//! the node's pseudo-state bits those rules depend on and its parent's
//! [`InheritedId`] — the interned identity of what a child can inherit:
//! `color`, `font-size` and the custom properties. Keying on that rather than
//! on the parent's whole resolved style is what lets a parent's `:hover`
//! background change without re-resolving its children.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::selector::{Bucket, Element, PseudoClasses, Selector};
use super::sheet::{Decl, Stylesheet, WideKeyword};
use super::var::{CustomProperties, resolve_custom, substitute};
use crate::tree::NodeStyle;

/// Where a sheet's rules sit in the cascade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    /// The engine's `default.css`.
    Engine,
    /// A sheet the app added.
    App,
}

/// One selector of one rule, filed in the index.
#[derive(Debug)]
struct IndexedRule {
    selector: Selector,
    declarations: Arc<[Decl]>,
}

/// Every rule of a set of sheets, in cascade order and bucketed; see the module
/// docs.
#[derive(Debug, Default)]
pub(crate) struct RuleIndex {
    rules: Vec<IndexedRule>,
    by_id: HashMap<Box<str>, Vec<u32>>,
    by_class: HashMap<Box<str>, Vec<u32>>,
    by_type: HashMap<Box<str>, Vec<u32>>,
    universal: Vec<u32>,
}

/// The rules a node could match, and what matching them reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Candidates {
    /// Indices into the index, ascending — cascade order.
    pub rules: Vec<u32>,
    /// What the rules test the node itself for.
    pub subject_pseudo: PseudoClasses,
    /// What the rules test an ancestor for.
    pub ancestor_pseudo: PseudoClasses,
    /// Whether any rule reads above the node.
    pub reads_ancestors: bool,
}

impl RuleIndex {
    /// Indexes `sheets`, each with its origin, in the order given: a later
    /// sheet's rule beats an earlier one's of the same origin and tier.
    pub fn build<'a>(sheets: impl IntoIterator<Item = (Origin, &'a Stylesheet)>) -> Self {
        let mut ordered = Vec::new();
        for (origin, sheet) in sheets {
            for rule in &sheet.rules {
                for selector in &rule.selectors {
                    let sequence = ordered.len();
                    ordered.push((
                        (origin, selector.specificity(), sequence),
                        IndexedRule {
                            selector: selector.clone(),
                            declarations: rule.declarations.clone(),
                        },
                    ));
                }
            }
        }
        ordered.sort_by_key(|(key, _)| *key);

        let mut index = Self::default();
        for (at, (_, rule)) in ordered.into_iter().enumerate() {
            let at = u32::try_from(at).expect("fewer than four billion rules");
            match rule.selector.bucket() {
                Bucket::Id(id) => index.by_id.entry(id.into()).or_default().push(at),
                Bucket::Class(class) => index.by_class.entry(class.into()).or_default().push(at),
                Bucket::Type(name) => index.by_type.entry(name.into()).or_default().push(at),
                Bucket::Universal => index.universal.push(at),
            }
            index.rules.push(rule);
        }
        index
    }

    /// The candidates for a node of `type_name` with `id` and `classes`.
    pub fn candidates<'a>(
        &self,
        type_name: &str,
        id: Option<&str>,
        classes: impl Iterator<Item = &'a str>,
    ) -> Candidates {
        let mut rules = self.universal.clone();
        if let Some(bucket) = id.and_then(|id| self.by_id.get(id)) {
            rules.extend(bucket);
        }
        for class in classes {
            if let Some(bucket) = self.by_class.get(class) {
                rules.extend(bucket);
            }
        }
        // Type selectors are lowercased as they parse; a node's type is written
        // in code, so it is looked up as written and, if that misses, lowered.
        let bucket = self.by_type.get(type_name).or_else(|| {
            type_name
                .bytes()
                .any(|byte| byte.is_ascii_uppercase())
                .then(|| self.by_type.get(type_name.to_ascii_lowercase().as_str()))
                .flatten()
        });
        if let Some(bucket) = bucket {
            rules.extend(bucket);
        }
        rules.sort_unstable();
        rules.dedup();

        let mut candidates = Candidates {
            rules,
            ..Candidates::default()
        };
        for &at in &candidates.rules {
            let selector = &self.rules[at as usize].selector;
            candidates.subject_pseudo = candidates.subject_pseudo | selector.subject_pseudo();
            candidates.ancestor_pseudo = candidates.ancestor_pseudo | selector.ancestor_pseudo();
            candidates.reads_ancestors |= selector.has_ancestors();
        }
        candidates
    }

    /// The candidates `element` matches, in cascade order.
    pub fn matching(&self, candidates: &Candidates, element: &impl Element) -> Vec<u32> {
        candidates
            .rules
            .iter()
            .copied()
            .filter(|&at| self.rules[at as usize].selector.matches(element))
            .collect()
    }
}

/// What a child inherits from its parent, interned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InheritedId(u32);

impl InheritedId {
    /// A root's parent: the initial `color` and `font-size` and no custom
    /// properties.
    pub const ROOT: Self = Self(0);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Inherited {
    color: [u32; 4],
    font_size: u32,
    custom: Arc<CustomProperties>,
}

impl Inherited {
    fn new(style: &NodeStyle, custom: Arc<CustomProperties>) -> Self {
        Self {
            color: style.color.map(f32::to_bits),
            font_size: style.font_size.to_bits(),
            custom,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DefinitionKey {
    matched: Box<[u32]>,
    pseudo: PseudoClasses,
    parent: InheritedId,
}

/// What a matched set of rules merges into.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub style: NodeStyle,
    pub custom: Arc<CustomProperties>,
}

/// How many definitions and inherited identities a [`Resolver`] holds before
/// [`Resolver::trim`] drops them all for one full re-resolve.
///
/// Both grow with what a frame builds — every distinct inline colour on a
/// block is an inherited identity of its own — so an app animating one would
/// grow them forever without a bound.
pub(crate) const CACHE_CAPACITY: usize = 4096;

/// A tree's cascade state: the index it matches against, and the caches of
/// what matching produced.
#[derive(Debug)]
pub(crate) struct Resolver {
    pub index: Arc<RuleIndex>,
    definitions: HashMap<DefinitionKey, Arc<Definition>>,
    inherited: HashMap<Inherited, InheritedId>,
    inherited_values: Vec<Inherited>,
    /// Bumped whenever the caches are dropped, so a node's stored key from
    /// before can never match again.
    pub epoch: u64,
}

impl Resolver {
    pub fn new(index: Arc<RuleIndex>) -> Self {
        let root = Inherited::new(&NodeStyle::DEFAULT, Arc::default());
        Self {
            index,
            definitions: HashMap::new(),
            inherited: HashMap::from([(root.clone(), InheritedId::ROOT)]),
            inherited_values: vec![root],
            epoch: 0,
        }
    }

    /// Replaces the index, dropping everything cached against the old one.
    pub fn set_index(&mut self, index: Arc<RuleIndex>) {
        self.index = index;
        self.clear();
    }

    /// Drops the caches if either has outgrown [`CACHE_CAPACITY`]. Only
    /// between frames: a node resolved this frame holds an id into them.
    pub fn trim(&mut self) {
        if self.definitions.len() > CACHE_CAPACITY || self.inherited_values.len() > CACHE_CAPACITY {
            self.clear();
        }
    }

    fn clear(&mut self) {
        self.definitions.clear();
        self.inherited.retain(|_, id| *id == InheritedId::ROOT);
        self.inherited_values.truncate(1);
        self.epoch += 1;
    }

    /// The identity of what `style` and `custom` pass to a child.
    pub fn intern(&mut self, style: &NodeStyle, custom: Arc<CustomProperties>) -> InheritedId {
        let key = Inherited::new(style, custom);
        if let Some(id) = self.inherited.get(&key) {
            return *id;
        }
        let id =
            InheritedId(u32::try_from(self.inherited_values.len()).expect("trimmed long before"));
        self.inherited.insert(key.clone(), id);
        self.inherited_values.push(key);
        id
    }

    /// The definition `matched` merges into under `parent`, from the cache or
    /// computed into it; the flag says which.
    pub fn definition(
        &mut self,
        matched: Vec<u32>,
        pseudo: PseudoClasses,
        parent: InheritedId,
    ) -> (Arc<Definition>, bool) {
        let key = DefinitionKey {
            matched: matched.into_boxed_slice(),
            pseudo,
            parent,
        };
        if let Some(definition) = self.definitions.get(&key) {
            return (definition.clone(), false);
        }
        let inherited = &self.inherited_values[parent.0 as usize];
        let definition = Arc::new(compute(&self.index, &key.matched, inherited));
        self.definitions.insert(key, definition.clone());
        (definition, true)
    }
}

/// Merges the `matched` rules, in order, over what `parent` passes down.
fn compute(index: &RuleIndex, matched: &[u32], parent: &Inherited) -> Definition {
    let declarations = || {
        matched
            .iter()
            .flat_map(|&at| index.rules[at as usize].declarations.iter())
    };

    // The starting point: initial values, with the inherited ones taken from
    // the parent. It is also what `unset` goes back to.
    let base = NodeStyle {
        color: parent.color.map(f32::from_bits),
        font_size: f32::from_bits(parent.font_size),
        ..NodeStyle::DEFAULT
    };

    let own: BTreeMap<Arc<str>, Arc<str>> = declarations()
        .filter_map(|decl| match decl {
            Decl::Custom(name, value) => Some((name.clone(), value.clone())),
            _ => None,
        })
        .collect();
    let custom = if own.is_empty() {
        parent.custom.clone()
    } else {
        Arc::new(resolve_custom(&parent.custom, own))
    };

    let mut style = base;
    let mut parsed = Vec::new();
    for decl in declarations() {
        match decl {
            Decl::Set(declaration) => declaration.apply(&mut style),
            Decl::Keyword(property, WideKeyword::Initial) => {
                property.copy(&NodeStyle::DEFAULT, &mut style)
            }
            Decl::Keyword(property, WideKeyword::Unset) => property.copy(&base, &mut style),
            Decl::Custom(..) => {}
            Decl::Var {
                property,
                name,
                css,
                at,
            } => {
                parsed.clear();
                let valid =
                    substitute(css, &mut |var| custom.get(var).cloned()).is_some_and(|text| {
                        let mut input = cssparser::Parser::new(&text);
                        property.parse(&mut input, &mut parsed).is_ok()
                    });
                if valid {
                    for declaration in &parsed {
                        declaration.apply(&mut style);
                    }
                } else {
                    // Invalid at computed-value time: the property is unset,
                    // whatever an earlier rule gave it.
                    property.copy(&base, &mut style);
                    crcbl_core::warn!(
                        "{}:{}:{}: warning: `{name}: {css}` is not a valid value once its variables \
                         are substituted; `{name}` is unset",
                        at.file,
                        at.line,
                        at.column,
                    );
                }
            }
        }
    }
    Definition { style, custom }
}
