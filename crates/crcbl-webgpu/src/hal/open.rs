//! Opening an instance: adapter enumeration as a `Future`.
//!
//! Every other backend's `Instance` exists the moment its constructor returns —
//! `crcbl-wgpu` blocks on the enumeration future, `crcbl-vk` enumerates
//! synchronously. This one cannot: it learns its adapters over the reply channel
//! a frame or more after it asks, so the constructor hands back a `Future` whose
//! `poll` drains the channel and absorbs into an [`AdapterProbe`] each frame
//! until the browser answers.
//!
//! This is the shape the backend registry's `open: fn() -> InstanceFuture` will
//! call in a later slice. For now [`WebGpuInstanceOpen::start`] is the
//! constructor and the future is driven directly.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use crcbl_hal::HalError;

use crate::instance::AdapterProbe;

use super::channel::{HandlePool, SharedChannel};
use super::instance::WebGpuInstance;

/// The instance-open future.
///
/// Resolves to a [`WebGpuInstance`] once the browser grants an adapter, or to a
/// [`HalError`] when it grants none. A drain per poll, absorbed into the probe,
/// exactly as the browser gate polls its own probes: the engine's
/// `PendingInstance` re-polls it each frame, which is the executor this future
/// is written for.
#[derive(Debug)]
pub struct WebGpuInstanceOpen {
    channel: SharedChannel,
    pool: HandlePool,
    probe: AdapterProbe,
}

impl WebGpuInstanceOpen {
    /// Start opening an instance: install a fresh channel and ask it to
    /// enumerate adapters.
    ///
    /// [`AdapterProbe::request`](crate::instance::AdapterProbe::request) encodes
    /// `enumerate_adapters` and registers the wait in one step. A channel that
    /// would not take it leaves the probe [`Unasked`](crate::instance::AdapterProbe::Unasked),
    /// which [`poll`](Future::poll) reports as a backend error rather than a
    /// request that waits for ever.
    #[must_use]
    pub fn start() -> Self {
        let channel = SharedChannel::new();
        channel.install();
        let probe = channel.with(AdapterProbe::request).unwrap_or_default();
        Self {
            channel,
            pool: HandlePool::new(),
            probe,
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
        // One drain per poll, dispatched into the adapter probe. A frame with no
        // answer for this sequence leaves the probe waiting, which is `Pending`.
        if let Some(Ok(replies)) = this.channel.with(crate::web::StreamChannel::drain_replies) {
            this.probe.absorb(&replies);
        }
        match &this.probe {
            AdapterProbe::Granted { info } => Poll::Ready(Ok(WebGpuInstance::new(
                this.channel.clone(),
                vec![info.clone()],
                this.pool.clone(),
            ))),
            AdapterProbe::Refused { reason } => Poll::Ready(Err(HalError::Backend(format!(
                "the browser granted no WebGPU adapter: {reason}"
            )))),
            AdapterProbe::Unasked => Poll::Ready(Err(HalError::Backend(
                "the WebGPU stream channel would not accept the adapter enumeration".to_string(),
            ))),
            AdapterProbe::Waiting { .. } => {
                // The rAF loop re-polls next frame; wake so a generic executor
                // does too, rather than parking on a future the browser's event
                // loop — not a registered waker — is what advances.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}
