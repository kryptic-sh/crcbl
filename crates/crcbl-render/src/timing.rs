//! Per-pass GPU timestamps — `crcbl_frame::timing`, re-exported — and how many
//! passes this crate's renderers record.
//!
//! The timers are the frame core's; what is this crate's is the sum below,
//! because the renderers it adds up are.

pub use crcbl_frame::timing::*;

use crate::forward::ForwardRenderer;
use crate::skinning::Skinning;
use crate::sprite_pass::SpriteRenderer;
use crate::ui_pass::UiRenderer;

/// How many passes one frame of this crate's renderers records, at most.
///
/// **What a caller passes [`PassTimers::new`] instead of a number of its own.**
/// Every renderer here says how many passes it adds — [`ForwardRenderer`],
/// [`SpriteRenderer`] and [`UiRenderer`] each carry a
/// `MAX_PASSES`, and [`Skinning`] carries one for the dispatch
/// [`ForwardRenderer::add_skinned_passes`] puts in front of them — and this is
/// their sum, so a pass added anywhere below moves this number with it. A game is in no position to know the count: it is a fact
/// about the renderer, and every sample that guessed it guessed low. The
/// samples' `8` was picked when a frame had three passes and was still `8` when
/// the shadow cascades and the light grid had taken it to fourteen, at which
/// point the timers bracketed the first eight and dropped the rest.
///
/// A caller that declares passes of its own — a clear, a readback, a pass of its
/// own devising — records that many more than this. The samples' clears fit
/// because none of them also runs a [`ForwardRenderer`], and a caller that
/// outgrows the bound gets the one warning [`PassTimers`] logs rather than a
/// silent truncation.
pub const MAX_TIMED_PASSES: u32 = ForwardRenderer::MAX_PASSES
    + Skinning::MAX_PASSES
    + SpriteRenderer::MAX_PASSES
    + UiRenderer::MAX_PASSES;
