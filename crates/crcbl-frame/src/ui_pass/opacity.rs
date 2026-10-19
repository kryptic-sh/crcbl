//! What a [`DrawList::push_opacity`](crcbl_ui::DrawList::push_opacity) group
//! needs from the UI pass: a layer to draw into, and the order of passes that
//! draws it there and blends it back.
//!
//! # A group is a layer, drawn and then blended
//!
//! A group's triangles are drawn into a layer of their own, cleared to
//! transparent, with the pass's usual blend — which, onto a transparent
//! target, leaves premultiplied colour behind. The layer is then drawn back
//! over the group's parent as one quad
//! ([`OpacityRun::layer_quad`](crcbl_ui::draw_list::OpacityRun::layer_quad))
//! whose colour is the opacity in all four lanes, through the pipeline that
//! blends premultiplied colour, `src + dst * (1 - src.a)`: the same
//! `ui.slang`, its texture primitive sampling the layer at set 1 exactly as a
//! [`UiTexture`](super::UiTexture) is sampled. No shader of its own.
//!
//! A render pass cannot be nested inside another, so a parent with a group in
//! it is cut into passes: what comes before the group, then the group's own
//! passes into its layer, then a pass that begins with the group's quad and
//! goes on with what follows it. [`plan`] works that order out — the one place
//! it is, so [`UiRenderer::counters`](super::UiRenderer::counters) and
//! [`UiRenderer::add_passes`](super::UiRenderer::add_passes) cannot disagree.
//! A frame with no group in it plans exactly the passes and draws it always
//! had.
//!
//! # The layer is the target's size, and pooled by the graph
//!
//! Each group's layer is a transient image of the graph, the size of the
//! target. **Not the group's bounds**: `ui.slang` places a vertex by dividing
//! its screen position by one viewport, and a layer at an offset would need an
//! offset it has no constant for — a new shader, and a new artifact on every
//! target, to save a clear. The group's bounds still decide what is
//! composited: the quad covers them and nothing else, so a status bar's box
//! blends its own few hundred pixels back, not the window.
//!
//! The graph aliases transients of one description whose lifetimes do not
//! overlap, so sibling groups share one physical layer and only nesting adds
//! another: in practice one image per depth. Its pool keeps them from frame to
//! frame and retires them when frames stop asking — nothing here allocates per
//! frame or owns an image to destroy.

use core::ops::Range;

use crcbl_hal::Format;
use crcbl_ui::draw_list::{OpacityRun, QUAD_INDICES, TextureRun};
use crcbl_ui::image::TextureId;

use super::textures::draws;

/// The layers' format: linear half floats.
///
/// The pass writes **linear** colour, and the target is usually an sRGB one
/// that blends in linear light and encodes on store. A layer has to hold what
/// the target would have blended without encoding it: an `Rgba8Unorm` layer
/// would quantise linear values to eight bits — the dark end of every
/// gradient banding — and an `Rgba8UnormSrgb` one would blend premultiplied
/// colour through a second encode. `Rgba16Float` is renderable, blendable and
/// filterable on every backend this pass targets, WebGPU's core included.
pub(super) const LAYER_FORMAT: Format = Format::Rgba16Float;

/// Indices per layer quad, which [`super::UiRenderer::begin_frame`] appends
/// after the list's own for every group.
pub(super) const QUAD_INDEX_COUNT: u32 = QUAD_INDICES.len() as u32;

/// Vertices per layer quad.
pub(super) const QUAD_VERTEX_COUNT: u32 = 4;

/// Where a planned pass draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    /// The image [`UiRenderer::add_passes`](super::UiRenderer::add_passes)
    /// was handed.
    Frame,
    /// The layer of the group at this index of the frame's runs.
    Layer(usize),
}

/// What one draw samples at set 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// The list's own triangles: the texture a run of them names, or the
    /// transparent placeholder.
    Indices(Option<TextureId>),
    /// The layer of the group at this index, blended back by its quad.
    Layer(usize),
}

/// One draw of a planned pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Draw {
    pub(super) indices: Range<u32>,
    pub(super) source: Source,
}

/// One render pass of a half, in the order the graph runs them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pass {
    pub(super) target: Target,
    /// Whether it clears its target to transparent first: the first pass into
    /// a layer does, and nothing that draws into the frame ever does.
    pub(super) clear: bool,
    pub(super) draws: Vec<Draw>,
}

/// The passes that draw `segment` — one half's indices — onto the frame, with
/// the texture `runs` split into draws as [`draws`] splits them and each of
/// `groups` drawn into its layer and blended back.
///
/// `quads` is the index the layer quads start at: group `n`'s is the
/// [`QUAD_INDEX_COUNT`] indices from `quads + n * QUAD_INDEX_COUNT`. A
/// group outside `segment` is the other half's and plays no part.
pub(super) fn plan(
    segment: Range<u32>,
    runs: &[TextureRun],
    groups: &[OpacityRun],
    quads: u32,
) -> Vec<Pass> {
    let mut planner = Planner {
        runs,
        groups,
        quads,
        passes: Vec::new(),
    };
    if !segment.is_empty() {
        let first = groups
            .iter()
            .position(|group| group.indices.start >= segment.start)
            .unwrap_or(groups.len());
        planner.target(Target::Frame, segment, first);
    }
    planner.passes
}

struct Planner<'a> {
    runs: &'a [TextureRun],
    groups: &'a [OpacityRun],
    quads: u32,
    passes: Vec<Pass>,
}

impl Planner<'_> {
    /// Plans `range` onto `target`, starting at group `next` — the first that
    /// can begin inside it — and returns the first group after the ones it
    /// drew.
    ///
    /// The runs are in push order, so a group's descendants follow it
    /// directly: the recursion into a group consumes them, and what it
    /// returns is the group's next sibling or later.
    fn target(&mut self, target: Target, range: Range<u32>, mut next: usize) -> usize {
        let mut clear = matches!(target, Target::Layer(_));
        let mut pending = Vec::new();
        let mut cursor = range.start;
        while let Some(group) = self.groups.get(next)
            && group.indices.start < range.end
        {
            let index = next;
            // Clamped rather than trusted, as `draws` clamps a texture run: a
            // group reaching past its parent would otherwise draw out of it.
            let inside = group.indices.start.max(cursor)..group.indices.end.min(range.end);
            self.content(cursor..inside.start, &mut pending);
            self.flush(target, &mut clear, &mut pending);
            next = self.target(Target::Layer(index), inside.clone(), index + 1);
            let quad = self.quads + index as u32 * QUAD_INDEX_COUNT;
            pending.push(Draw {
                indices: quad..quad + QUAD_INDEX_COUNT,
                source: Source::Layer(index),
            });
            cursor = inside.end;
        }
        self.content(cursor..range.end, &mut pending);
        // A layer that got no pass at all would be blended back uncleared;
        // the runs are never empty, so this is a guard, and the pass clears.
        if !pending.is_empty() || clear {
            self.passes.push(Pass {
                target,
                clear,
                draws: pending,
            });
        }
        next
    }

    /// The draws of the list's own triangles in `range`.
    fn content(&self, range: Range<u32>, pending: &mut Vec<Draw>) {
        pending.extend(draws(range, self.runs).into_iter().map(|draw| Draw {
            indices: draw.indices,
            source: Source::Indices(draw.texture),
        }));
    }

    /// Ends the pass `pending` holds, if it holds anything.
    fn flush(&mut self, target: Target, clear: &mut bool, pending: &mut Vec<Draw>) {
        if pending.is_empty() {
            return;
        }
        self.passes.push(Pass {
            target,
            clear: *clear,
            draws: core::mem::take(pending),
        });
        *clear = false;
    }
}
