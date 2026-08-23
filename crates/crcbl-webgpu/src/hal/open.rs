//! Opening an instance: adapter enumeration as a `Future`.
//!
//! Every other backend's `Instance` exists the moment its constructor returns —
//! `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` all enumerate synchronously. This
//! one cannot: it learns its adapters over the reply channel a frame or more
//! after it asks, so the constructor hands back a `Future` whose
//! `poll` drains the channel and absorbs into an [`AdapterProbe`] each frame
//! until the browser answers.
//!
//! **Two questions are asked on that one trip, not one.** The canvas
//! capabilities are the other thing only the browser knows and the instance
//! cannot go without: [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps)
//! is synchronous and `crcbl::engine`'s open calls it on the line after
//! `create_surface`, so an instance that had not already fetched them would have
//! to guess — and guessing the format a canvas is configured with costs a
//! full-canvas copy on every present when the guess is wrong. So
//! [`SurfaceCapsProbe`] is asked alongside the enumeration, on the same frame
//! and through the same drain, and the instance is assembled when both have
//! answered.
//!
//! This is the shape the backend registry's `open: fn() -> InstanceFuture` will
//! call in a later slice. For now [`WebGpuInstanceOpen::start`] is the
//! constructor and the future is driven directly.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use crcbl_hal::HalError;

use crate::instance::{AdapterProbe, SurfaceCapsProbe};

use super::channel::{HandlePool, SharedChannel};
use super::instance::WebGpuInstance;

/// The instance-open future.
///
/// Resolves to a [`WebGpuInstance`] once the browser grants an adapter and says
/// what a canvas accepts, or to a [`HalError`] when it grants no adapter. A
/// drain per poll, dispatched into both probes, exactly as the browser gate
/// polls its own: the engine's `PendingInstance` re-polls it each frame, which
/// is the executor this future is written for.
#[derive(Debug)]
pub struct WebGpuInstanceOpen {
    channel: SharedChannel,
    pool: HandlePool,
    probe: AdapterProbe,
    canvas: SurfaceCapsProbe,
}

impl WebGpuInstanceOpen {
    /// Start opening an instance: install a fresh channel, ask it to enumerate
    /// adapters, and ask what a canvas will accept.
    ///
    /// [`AdapterProbe::request`](crate::instance::AdapterProbe::request) encodes
    /// `enumerate_adapters` and registers the wait in one step. A channel that
    /// would not take it leaves the probe [`Unasked`](crate::instance::AdapterProbe::Unasked),
    /// which [`poll`](Future::poll) reports as a backend error rather than a
    /// request that waits for ever.
    /// [`SurfaceCapsProbe::request`](crate::instance::SurfaceCapsProbe::request)
    /// is the same call for the canvas query, and its refusal is reported the
    /// same way.
    ///
    /// **The enumeration is encoded first**, so it holds sequence `0` on a fresh
    /// channel and the canvas query holds `1`. Nothing on the wire depends on
    /// that — every reply names its own sequence — but the fixtures a test feeds
    /// by hand do, and the order is the one they were written against.
    #[must_use]
    pub fn start() -> Self {
        let channel = SharedChannel::new();
        channel.install();
        let probe = channel.with(AdapterProbe::request).unwrap_or_default();
        let canvas = channel.with(SurfaceCapsProbe::request).unwrap_or_default();
        Self {
            channel,
            pool: HandlePool::new(),
            probe,
            canvas,
        }
    }

    /// A clone of the channel this future drives, for feeding it the adapter
    /// reply in a test with no browser in the loop.
    #[must_use]
    pub fn channel(&self) -> SharedChannel {
        self.channel.clone()
    }
}

impl Future for WebGpuInstanceOpen {
    type Output = Result<WebGpuInstance, HalError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        // One drain per poll, dispatched into both probes — the buffer holds
        // whichever answers the browser had ready, in either order and not
        // necessarily together. A frame with no answer for a probe's sequence
        // leaves that probe waiting, which is `Pending`.
        if let Some(Ok(replies)) = this.channel.with(crate::web::StreamChannel::drain_replies) {
            this.probe.absorb(&replies);
            this.canvas.absorb(&replies);
        }
        // A refused canvas query is *not* an arm here: `surface_caps` has an
        // `Err` of its own and the offscreen surfaces do not depend on the
        // answer, so the reason is carried into the instance and returned by the
        // call it belongs to. An adapter is what an instance cannot be built
        // without.
        let (info, canvas) = match (&this.probe, &this.canvas) {
            (AdapterProbe::Refused { reason }, _) => {
                return Poll::Ready(Err(HalError::Backend(format!(
                    "the browser granted no WebGPU adapter: {reason}"
                ))));
            }
            (AdapterProbe::Unasked, _) => {
                return Poll::Ready(Err(HalError::Backend(
                    "the WebGPU stream channel would not accept the adapter enumeration"
                        .to_string(),
                )));
            }
            (_, SurfaceCapsProbe::Unasked) => {
                return Poll::Ready(Err(HalError::Backend(
                    "the WebGPU stream channel would not accept the canvas capability query"
                        .to_string(),
                )));
            }
            (AdapterProbe::Granted { info }, SurfaceCapsProbe::Answered { caps }) => {
                (info, Ok(caps.clone()))
            }
            (AdapterProbe::Granted { info }, SurfaceCapsProbe::Refused { reason, .. }) => {
                (info, Err(reason.clone()))
            }
            _ => {
                // The rAF loop re-polls next frame; wake so a generic executor
                // does too, rather than parking on a future the browser's event
                // loop — not a registered waker — is what advances.
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };
        Poll::Ready(Ok(WebGpuInstance::new(
            this.channel.clone(),
            vec![info.clone()],
            this.pool.clone(),
            canvas,
        )))
    }
}
