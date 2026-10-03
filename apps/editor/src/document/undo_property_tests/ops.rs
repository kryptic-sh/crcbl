//! The steps the undo property test generates — `play` is where each one
//! is played.
//!
//! A step holds draws — [`Index`]es and numbers — not ids: which entities and
//! systems a document holds depends on the steps before, so each draw is
//! resolved against the document as it stands when the step runs. A draw
//! shrinks toward the first candidate.

use proptest::prelude::*;
use proptest::sample::Index;

use crcbl::reflect::{Value, ValueKind};

use super::super::mesh_tests::{GONE, TRIANGLE};

/// The text a text leaf is written and the key a drop places: asset keys a
/// mesh admits — one the asset root holds and one it does not — none at all,
/// a label, and keys [`crcbl::scene_mesh::check_asset`] refuses.
pub(super) const TEXTS: [&str; 6] = [
    TRIANGLE,
    GONE,
    "",
    "Gate",
    "../outside.glb",
    "props/no-extension",
];

/// The width of the square about the origin a drop lands in.
const DROP_SPREAD: f64 = 16.0;

/// One step of a generated history.
#[derive(Clone, Debug)]
pub(super) enum Op {
    /// One leaf of one of `target`'s components written `value`, `through`
    /// one of the paths a single write takes.
    Write {
        target: Index,
        system: Index,
        leaf: Index,
        value: Draw,
        through: Through,
    },
    /// One leaf dragged in the inspector over several frames, one
    /// `Document::record_edits` a frame under one gesture.
    FieldDrag {
        target: Index,
        system: Index,
        leaf: Index,
        frames: Vec<Draw>,
    },
    /// An arrow key: every selected entity moved `delta` along `axis` as one
    /// entry, as `App::nudge` builds it.
    Nudge {
        selection: Pair,
        axis: Index,
        delta: f64,
    },
    /// The translate gizmo dragged over several frames: each frame every
    /// selected entity set to where it started plus that frame's offset, one
    /// `Document::apply_in` a frame under one gesture, as
    /// `App::move_handle` writes it — and, with `switch`, a second axis
    /// written from a frame part-way, so the frames report different leaves.
    Drag {
        selection: Pair,
        axis: Index,
        offsets: Vec<f64>,
        switch: Option<Switch>,
    },
    /// The rotate gizmo: the placing component's four rotation leaves as one
    /// batch, set to `quaternion` normalised.
    Turn { target: Index, quaternion: [f64; 4] },
    /// The scale gizmo: the placing component's three half extents as one
    /// batch, each multiplied by `factor`.
    Scale { target: Index, factor: f64 },
    /// An enum's variant picked: one of the enums the held entities'
    /// components hold, `target` resolved over all of them, switched to one
    /// of its variants or — for a draw past them — to a name it does not
    /// have; reported to `Document::record_edits` as the inspector's strip
    /// does while `inspector`, and as an `EditCommand::SetVariant` handed to
    /// `Document::apply` otherwise.
    Switch {
        target: Index,
        variant: Index,
        inspector: bool,
    },
    /// F2: `Document::rename` with one of `play::name_text`'s texts.
    Rename { target: Index, name: Index },
    /// Delete on a selection.
    Delete { selection: Pair },
    /// Ctrl+D on a selection.
    Duplicate { selection: Pair },
    /// Ctrl+C on a selection and Ctrl+V — or, `garbled`, a paste of text
    /// that is no clipping.
    Paste { selection: Pair, garbled: bool },
    /// A drop from the asset browser: `Document::spawn_mesh`.
    Drop { asset: Index, x: f64, z: f64 },
    /// The inspector's add button: `Document::attach` of any registered
    /// system.
    Attach { target: Index, system: Index },
    /// A section's remove button: `Document::detach` of one of the systems
    /// holding `target`, or of one that does not.
    Detach { target: Index, system: Index },
    /// `EditCommand::ListSystem` of any registered system at any place up
    /// to one past the manifest's end.
    List { system: Index, at: Index },
    /// `EditCommand::UnlistSystem` — while `empty`, of one of the
    /// manifest's systems holding no entity, which the document accepts;
    /// otherwise of any listed system, or of one the manifest does not list.
    ///
    /// `empty` because a listed system holding nothing is rare in a random
    /// history — every listing but [`Op::List`]'s fills the system it lists —
    /// so a draw over every system would almost never unlist one.
    Unlist { system: Index, empty: bool },
    /// Ctrl+Z.
    Undo,
    /// Ctrl+Y.
    Redo,
}

/// How a [`Op::Write`] reaches the document.
#[derive(Clone, Copy, Debug)]
pub(super) enum Through {
    /// An `EditCommand::SetProperty` handed to `Document::apply`.
    Command,
    /// The inspector: the leaf written in place and the edit reported to
    /// `Document::record_edits`, which rewinds it and applies the command.
    Inspector,
    /// A field paste: the value's ron text to `Document::paste_field`.
    FieldPaste,
}

/// Two draws of a selection: one entity, or two when the second draw names
/// another.
#[derive(Clone, Debug)]
pub(super) struct Pair {
    pub(super) first: Index,
    pub(super) second: Option<Index>,
}

/// Where a [`Op::Drag`] starts writing another leaf: from the frame `at`
/// names — never the first — the second `axis` too, or instead of the first
/// unless `keep_first`.
#[derive(Clone, Debug)]
pub(super) struct Switch {
    pub(super) at: Index,
    pub(super) axis: Index,
    pub(super) keep_first: bool,
}

/// The draws a leaf's new value is made from.
#[derive(Clone, Debug)]
pub(super) struct Draw {
    /// A kind other than the leaf's own, sometimes — which the leaf refuses
    /// unless the draw lands on its own kind.
    kind: Option<Index>,
    float: f64,
    whole: i64,
    flag: bool,
    text: Index,
}

/// A history's steps, weighted toward edits.
pub(super) fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<Index>(), any::<Index>(), any::<Index>(), draw(), through())
            .prop_map(|(target, system, leaf, value, through)| Op::Write {
                target,
                system,
                leaf,
                value,
                through,
            })
            .boxed(),
        1 => (
            any::<Index>(),
            any::<Index>(),
            any::<Index>(),
            proptest::collection::vec(draw(), 2..=4),
        )
            .prop_map(|(target, system, leaf, frames)| Op::FieldDrag {
                target,
                system,
                leaf,
                frames,
            })
            .boxed(),
        2 => (pair(), any::<Index>(), -4.0..4.0f64)
            .prop_map(|(selection, axis, delta)| Op::Nudge {
                selection,
                axis,
                delta,
            })
            .boxed(),
        2 => (
            pair(),
            any::<Index>(),
            proptest::collection::vec(offset(), 2..=4),
            proptest::option::of(
                (any::<Index>(), any::<Index>(), any::<bool>())
                    .prop_map(|(at, axis, keep_first)| Switch { at, axis, keep_first }),
            ),
        )
            .prop_map(|(selection, axis, offsets, switch)| Op::Drag {
                selection,
                axis,
                offsets,
                switch,
            })
            .boxed(),
        1 => (any::<Index>(), proptest::array::uniform4(-1.0..1.0f64))
            .prop_map(|(target, quaternion)| Op::Turn { target, quaternion })
            .boxed(),
        1 => (any::<Index>(), -0.5..3.0f64)
            .prop_map(|(target, factor)| Op::Scale { target, factor })
            .boxed(),
        2 => (any::<Index>(), any::<Index>(), any::<bool>())
            .prop_map(|(target, variant, inspector)| Op::Switch {
                target,
                variant,
                inspector,
            })
            .boxed(),
        2 => (any::<Index>(), any::<Index>())
            .prop_map(|(target, name)| Op::Rename { target, name })
            .boxed(),
        1 => pair().prop_map(|selection| Op::Delete { selection }).boxed(),
        1 => pair().prop_map(|selection| Op::Duplicate { selection }).boxed(),
        1 => (pair(), proptest::bool::weighted(0.2))
            .prop_map(|(selection, garbled)| Op::Paste { selection, garbled })
            .boxed(),
        1 => (
            any::<Index>(),
            -DROP_SPREAD / 2.0..DROP_SPREAD / 2.0,
            -DROP_SPREAD / 2.0..DROP_SPREAD / 2.0,
        )
            .prop_map(|(asset, x, z)| Op::Drop { asset, x, z })
            .boxed(),
        2 => (any::<Index>(), any::<Index>())
            .prop_map(|(target, system)| Op::Attach { target, system })
            .boxed(),
        2 => (any::<Index>(), any::<Index>())
            .prop_map(|(target, system)| Op::Detach { target, system })
            .boxed(),
        2 => (any::<Index>(), any::<Index>())
            .prop_map(|(system, at)| Op::List { system, at })
            .boxed(),
        1 => (any::<Index>(), proptest::bool::weighted(0.8))
            .prop_map(|(system, empty)| Op::Unlist { system, empty })
            .boxed(),
        3 => Just(Op::Undo).boxed(),
        2 => Just(Op::Redo).boxed(),
    ]
}

fn through() -> impl Strategy<Value = Through> {
    prop_oneof![
        Just(Through::Command),
        Just(Through::Inspector),
        Just(Through::FieldPaste),
    ]
}

/// A drag frame's offset from where the drag began: sometimes none at all,
/// so a drag can end where it started.
fn offset() -> impl Strategy<Value = f64> {
    prop_oneof![4 => -4.0..4.0f64, 1 => Just(0.0)]
}

fn pair() -> impl Strategy<Value = Pair> {
    (any::<Index>(), proptest::option::of(any::<Index>()))
        .prop_map(|(first, second)| Pair { first, second })
}

/// Mostly a modest float, sometimes zero, sometimes anything at all —
/// infinities, NaNs and magnitudes no `f32` holds included.
fn float() -> impl Strategy<Value = f64> {
    prop_oneof![
        8 => -50.0..50.0f64,
        1 => Just(0.0),
        1 => proptest::num::f64::ANY,
    ]
}

fn draw() -> impl Strategy<Value = Draw> {
    (
        proptest::option::weighted(0.1, any::<Index>()),
        float(),
        prop_oneof![0..100i64, any::<i64>()],
        any::<bool>(),
        any::<Index>(),
    )
        .prop_map(|(kind, float, whole, flag, text)| Draw {
            kind,
            float,
            whole,
            flag,
            text,
        })
}

impl Op {
    /// The step's name, as the test's tallies count it.
    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Write {
                through: Through::Command,
                ..
            } => "property set",
            Self::Write {
                through: Through::Inspector,
                ..
            } => "inspector edit",
            Self::Write {
                through: Through::FieldPaste,
                ..
            } => "field paste",
            Self::FieldDrag { .. } => "inspector drag",
            Self::Nudge { .. } => "nudge",
            Self::Drag { .. } => "translate drag",
            Self::Turn { .. } => "turn",
            Self::Scale { .. } => "scale",
            Self::Switch {
                inspector: true, ..
            } => "inspector switch",
            Self::Switch {
                inspector: false, ..
            } => "variant command",
            Self::Rename { .. } => "rename",
            Self::Delete { .. } => "delete",
            Self::Duplicate { .. } => "duplicate",
            Self::Paste { .. } => "paste",
            Self::Drop { .. } => "drop",
            Self::Attach { .. } => "attach",
            Self::Detach { .. } => "detach",
            Self::List { .. } => "list a system",
            Self::Unlist { .. } => "unlist a system",
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }
}

/// Every step's [`Op::name`] — what the test asserts some history accepted.
pub(super) const EVERY_OP: [&str; 21] = [
    "property set",
    "inspector edit",
    "field paste",
    "inspector drag",
    "nudge",
    "translate drag",
    "turn",
    "scale",
    "inspector switch",
    "variant command",
    "rename",
    "delete",
    "duplicate",
    "paste",
    "drop",
    "attach",
    "detach",
    "list a system",
    "unlist a system",
    "undo",
    "redo",
];

impl Draw {
    /// A value for a leaf of `kind`: of that kind, or of the other kind this
    /// draw names.
    pub(super) fn of(&self, kind: ValueKind) -> Value {
        const KINDS: [ValueKind; 5] = [
            ValueKind::Bool,
            ValueKind::Int,
            ValueKind::UInt,
            ValueKind::Float,
            ValueKind::Text,
        ];
        match self.kind.as_ref().map_or(kind, |other| *other.get(&KINDS)) {
            ValueKind::Bool => Value::Bool(self.flag),
            ValueKind::Int => Value::Int(self.whole),
            ValueKind::UInt => Value::UInt(self.whole.unsigned_abs()),
            ValueKind::Float => Value::Float(self.float),
            ValueKind::Text => Value::Text((*self.text.get(&TEXTS)).to_owned()),
        }
    }
}
