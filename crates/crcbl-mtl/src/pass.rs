//! The bookkeeping a *recorded* render pass needs, and none of the Metal.
//!
//! `crcbl_mtl::command` no longer drives an `MTLRenderCommandEncoder` as each
//! command arrives: a render pass is recorded into a list and encoded in one
//! go at [`end_render_pass`](crcbl_hal::CommandEncoder::end_render_pass). This
//! module holds the two decisions that shape has to make and that are not
//! themselves Metal calls — where the seam's render area lands as a scissor
//! rectangle, and which of Metal's two debug-group stacks a label was pushed
//! onto.
//!
//! # Not macOS-only, and that is the point
//!
//! This module holds no Objective-C type — it is integer arithmetic and a
//! stack of two-field records — so off macOS it exists in the test build alone
//! and `cargo test` on any host checks it, exactly as [`crate::argument`],
//! [`crate::present`], [`crate::query`] and [`crate::quirk`] are compiled for.
//!
//! That matters more here than the module's size suggests, because **neither
//! decision fails loudly when it is wrong**. A scissor rectangle that leaves
//! the render target makes `setScissorRect:` raise, which aborts the process
//! rather than returning an error; a debug group popped on the wrong stack
//! folds every later command into the wrong region of a capture tree, which
//! nothing reports at all. Both are reachable only from a machine with Metal
//! on it, and one of them is only visible inside a GPU capture tool.

use crcbl_hal::Rect2d;

/// A scissor rectangle, in the units Metal's own `MTLScissorRect` carries.
///
/// `u64` rather than `NSUInteger` because that type does not exist off macOS,
/// and the widening is the caller's — `crcbl_mtl::device`'s `to_ns` — for the
/// same reason every other bound in this backend is computed in `u64` first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scissor {
    pub(crate) x: u64,
    pub(crate) y: u64,
    pub(crate) width: u64,
    pub(crate) height: u64,
}

/// The scissor rectangle a render pass's area becomes, or `None` when Metal
/// should not be told one at all.
///
/// # Why a render area is a scissor here
///
/// Metal's `MTLRenderPassDescriptor` has **no render-area rectangle**.
/// `renderTargetWidth`/`renderTargetHeight` are an origin-anchored *size limit*
/// on the whole pass rather than Vulkan's `renderArea`, and they cannot express
/// an offset at all. So [`RenderPassDesc::render_area`](crcbl_hal::RenderPassDesc::render_area)
/// is applied as the render encoder's scissor rectangle, which bounds draws
/// exactly as Vulkan's render area does.
///
/// # The two answers of `None`
///
/// * **The whole attachment**, which is what the seam documents as the usual
///   case and what the render graph always passes. Metal's default scissor is
///   already the whole render target, so the common case makes no call.
/// * **A degenerate rectangle**, which Metal rejects. It can only arise from an
///   area that starts at or past the far edge, and refusing to send it is what
///   keeps `setScissorRect:` from raising.
///
/// `width` and `height` are the attachment's own, off the first attachment's
/// texture; the rectangle is clamped to them because `setScissorRect:` raises
/// on one that leaves the render target.
pub(crate) fn plan_scissor(area: Rect2d, width: u64, height: u64) -> Option<Scissor> {
    // A negative origin has no Metal spelling and clamps to the near edge, the
    // same direction the far edge is clamped in below.
    let x = u64::try_from(area.x).unwrap_or(0).min(width);
    let y = u64::try_from(area.y).unwrap_or(0).min(height);
    let scissor = Scissor {
        x,
        y,
        width: u64::from(area.width).min(width - x),
        height: u64::from(area.height).min(height - y),
    };
    let whole =
        scissor.x == 0 && scissor.y == 0 && scissor.width >= width && scissor.height >= height;
    if whole || scissor.width == 0 || scissor.height == 0 {
        return None;
    }
    Some(scissor)
}

/// Which of Metal's two debug-group stacks a push went on.
///
/// Metal has two rather than one: `MTLCommandBuffer::pushDebugGroup:` for the
/// space *between* encoders, and `MTLCommandEncoder::pushDebugGroup:` for the
/// space inside one. They are not interchangeable — a group pushed on an
/// encoder ends when that encoder does, whatever the seam's caller intended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// The command buffer's stack, which outlives every encoder.
    CommandBuffer,
    /// An encoder's stack, which `endEncoding` closes.
    Encoder,
}

/// One open debug group.
#[derive(Clone, Copy, Debug)]
struct Group {
    placement: Placement,
    /// Which encoder it went on, as [`Labels::push`] was told. Only meaningful
    /// for [`Placement::Encoder`].
    epoch: u64,
}

/// The debug groups a command buffer has open, innermost last.
///
/// # The epoch, and what it prevents
///
/// A group pushed on a Metal encoder is closed by that encoder's `endEncoding`,
/// with no `popDebugGroup` and no way to ask whether it is still open. So a
/// [`Placement::Encoder`] group whose encoder has since closed must be
/// **dropped** rather than popped off whichever encoder is open by then —
/// popping it there would close a group that encoder's own push opened, folding
/// every later command into the wrong region of the capture tree. That is the
/// same failure `crcbl-vk`'s `render_pass_label` exists to avoid.
///
/// The caller bumps a counter every time it opens an encoder and hands the
/// value in; comparing it at the pop is what tells the two cases apart.
#[derive(Debug, Default)]
pub(crate) struct Labels {
    open: Vec<Group>,
}

impl Labels {
    /// Records a group that has just been pushed, on `placement`'s stack, while
    /// encoder `epoch` was open.
    pub(crate) fn push(&mut self, placement: Placement, epoch: u64) {
        self.open.push(Group { placement, epoch });
    }

    /// Removes the innermost group and says where — if anywhere — it must be
    /// popped, given that encoder `epoch` is the one open now.
    ///
    /// `None` covers the two cases with nothing to do, and the group is removed
    /// either way: there was no open group, or it was pushed on an encoder that
    /// has since closed and took the group with it. Removing it regardless is
    /// what lets `finish` drain the stack by calling this until
    /// [`is_empty`](Self::is_empty).
    pub(crate) fn pop(&mut self, epoch: u64) -> Option<Placement> {
        let group = self.open.pop()?;
        match group.placement {
            Placement::CommandBuffer => Some(Placement::CommandBuffer),
            Placement::Encoder if group.epoch == epoch => Some(Placement::Encoder),
            Placement::Encoder => None,
        }
    }

    /// Whether any group is still open.
    pub(crate) fn is_empty(&self) -> bool {
        self.open.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam's own "usually the full attachment size", which Metal's default
    /// scissor already is.
    #[test]
    fn the_whole_attachment_is_not_a_scissor_call() {
        let area = Rect2d {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        };
        assert_eq!(plan_scissor(area, 640, 480), None);
    }

    /// An area larger than the attachment still covers it, so there is still
    /// nothing to say.
    #[test]
    fn an_area_larger_than_the_attachment_is_not_a_scissor_call() {
        let area = Rect2d {
            x: 0,
            y: 0,
            width: 4096,
            height: 4096,
        };
        assert_eq!(plan_scissor(area, 640, 480), None);
    }

    /// A genuine sub-rectangle survives unchanged.
    #[test]
    fn a_sub_rectangle_is_passed_through() {
        let area = Rect2d {
            x: 16,
            y: 32,
            width: 64,
            height: 128,
        };
        assert_eq!(
            plan_scissor(area, 640, 480),
            Some(Scissor {
                x: 16,
                y: 32,
                width: 64,
                height: 128,
            })
        );
    }

    /// An offset rectangle the size of the whole target is *not* the whole
    /// target, and Metal raises if it is sent unclamped.
    #[test]
    fn an_offset_area_is_clamped_to_the_far_edge() {
        let area = Rect2d {
            x: 16,
            y: 32,
            width: 640,
            height: 480,
        };
        assert_eq!(
            plan_scissor(area, 640, 480),
            Some(Scissor {
                x: 16,
                y: 32,
                width: 624,
                height: 448,
            })
        );
    }

    /// A negative origin clamps to zero rather than wrapping to a vast one.
    #[test]
    fn a_negative_origin_becomes_the_near_edge() {
        let area = Rect2d {
            x: -8,
            y: -8,
            width: 64,
            height: 64,
        };
        assert_eq!(
            plan_scissor(area, 640, 480),
            Some(Scissor {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            })
        );
    }

    /// An origin at the far edge leaves nothing, and Metal rejects an empty
    /// rectangle.
    #[test]
    fn a_degenerate_rectangle_is_never_sent() {
        let area = Rect2d {
            x: 640,
            y: 0,
            width: 64,
            height: 64,
        };
        assert_eq!(plan_scissor(area, 640, 480), None);
        let empty = Rect2d {
            x: 8,
            y: 8,
            width: 0,
            height: 64,
        };
        assert_eq!(plan_scissor(empty, 640, 480), None);
    }

    #[test]
    fn a_command_buffer_group_pops_on_the_command_buffer() {
        let mut labels = Labels::default();
        labels.push(Placement::CommandBuffer, 3);
        assert_eq!(labels.pop(7), Some(Placement::CommandBuffer));
        assert!(labels.is_empty());
    }

    #[test]
    fn an_encoder_group_pops_while_its_encoder_is_open() {
        let mut labels = Labels::default();
        labels.push(Placement::Encoder, 3);
        assert_eq!(labels.pop(3), Some(Placement::Encoder));
        assert!(labels.is_empty());
    }

    /// The rule this type exists for: `endEncoding` already ended the group, so
    /// popping it on the *next* encoder would close one of that encoder's own.
    #[test]
    fn an_encoder_group_whose_encoder_closed_is_dropped() {
        let mut labels = Labels::default();
        labels.push(Placement::Encoder, 3);
        assert_eq!(labels.pop(4), None);
        assert!(labels.is_empty());
    }

    /// Innermost first, and the two stacks interleave freely.
    #[test]
    fn groups_pop_innermost_first() {
        let mut labels = Labels::default();
        labels.push(Placement::CommandBuffer, 0);
        labels.push(Placement::Encoder, 1);
        labels.push(Placement::CommandBuffer, 1);
        assert_eq!(labels.pop(1), Some(Placement::CommandBuffer));
        assert_eq!(labels.pop(1), Some(Placement::Encoder));
        assert_eq!(labels.pop(1), Some(Placement::CommandBuffer));
        assert!(labels.is_empty());
    }

    /// `finish` drains by popping until the stack is empty, so a pop of nothing
    /// has to terminate rather than report anything.
    #[test]
    fn popping_an_empty_stack_answers_nothing() {
        let mut labels = Labels::default();
        assert!(labels.is_empty());
        assert_eq!(labels.pop(0), None);
    }

    /// Every pop removes a group, including the dropped kind — which is what
    /// makes `finish`'s drain terminate.
    #[test]
    fn every_pop_shortens_the_stack() {
        let mut labels = Labels::default();
        labels.push(Placement::Encoder, 1);
        labels.push(Placement::Encoder, 1);
        labels.push(Placement::CommandBuffer, 1);
        let mut popped = 0;
        while !labels.is_empty() {
            // The epoch has moved on, so both encoder groups are dropped.
            labels.pop(9);
            popped += 1;
        }
        assert_eq!(popped, 3);
    }
}
