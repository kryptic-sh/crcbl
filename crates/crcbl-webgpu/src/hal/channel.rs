//! The two things every HAL object in this crate shares: the one stream
//! [`StreamChannel`] they encode through, and the id source their handles come
//! from.

use crcbl_core::Handle;

use crate::web::StreamChannel;

// ── SharedChannel ──────────────────────────────────────────────────────────

/// The representation the shared channel wraps.
///
/// **The split wgpu's own web types already make.** On the web the backend runs
/// on one thread and a `GPUDevice` is `!Send`, so the channel is a plain
/// [`Rc`](std::rc::Rc) — which is also what [`install`](crate::web::install)
/// keeps a [`Weak`](std::rc::Weak) of, so the JS shim can find the buffer. Off
/// the web the HAL seam demands `Send + Sync` (the P8 job system moves a
/// [`Device`](crcbl_hal::Device) across threads), and a
/// [`RefCell`](core::cell::RefCell)-holding [`StreamChannel`] is not `Sync`, so
/// it goes behind an [`Arc`](std::sync::Arc)`<`[`Mutex`](std::sync::Mutex)`>`.
/// The wire and the encoding are identical either way; only the way the one
/// buffer is reached differs.
#[cfg(target_arch = "wasm32")]
type Repr = std::rc::Rc<StreamChannel>;

/// See the wasm [`Repr`].
#[cfg(not(target_arch = "wasm32"))]
type Repr = std::sync::Arc<std::sync::Mutex<StreamChannel>>;

/// A cloneable handle onto the one [`StreamChannel`] an
/// [`Instance`](crcbl_hal::Instance) and every object it makes encode through.
///
/// Cloning it is cloning the shared handle, not the buffer: an
/// [`Instance`](crate::hal::WebGpuInstance), the
/// [`Device`](crate::hal::WebGpuDevice) it opens and every
/// [`CommandEncoder`](crate::hal::WebGpuCommandEncoder) that device makes all
/// hold clones of the same channel, which is what puts their commands on one
/// ordered stream with one sequence counter.
#[derive(Clone, Debug)]
pub struct SharedChannel(Repr);

impl Default for SharedChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedChannel {
    /// A fresh channel holding one empty frame.
    #[must_use]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self(std::rc::Rc::new(StreamChannel::new()))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self(std::sync::Arc::new(std::sync::Mutex::new(
                StreamChannel::new(),
            )))
        }
    }

    /// Run `f` against the underlying [`StreamChannel`] and return what it
    /// returned.
    ///
    /// The one door onto the buffer, so every encode, drain and probe request
    /// comes through here. Off the web it takes the channel's lock for the call
    /// — do not nest one `with` inside another on the same channel, which a
    /// non-reentrant [`Mutex`](std::sync::Mutex) would deadlock on; nothing in
    /// this crate does.
    pub fn with<R>(&self, f: impl FnOnce(&StreamChannel) -> R) -> R {
        #[cfg(target_arch = "wasm32")]
        {
            f(&self.0)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            f(&self
                .0
                .lock()
                .expect("the WebGPU stream channel mutex was poisoned"))
        }
    }

    /// Make this channel the one the JS shim's entry points reach.
    ///
    /// Only meaningful on the web, where the shim reads the command buffer and
    /// writes replies through the thread-local [`install`](crate::web::install)
    /// keeps. Off the web there is no shim, so this touches the handle and
    /// answers `true`: nothing to install, and not a failure to report. The
    /// browser wiring proper is a later slice.
    pub fn install(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            crate::web::install(&self.0)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // No shim off the web; nothing to install. Touch the handle so this
            // reads as deliberate rather than an unused receiver.
            let _ = &self.0;
            true
        }
    }
}

// ── HandlePool ─────────────────────────────────────────────────────────────

/// The id source for the handles this crate's creation methods mint.
///
/// The stream cannot answer during a `create_*` call — the object is built when
/// the frame is replayed — so identity is positional: wasm allocates the id
/// here, writes it into the stream beside the descriptor, and returns it at
/// once. See [`StreamWriter::create_buffer`](crate::StreamWriter::create_buffer).
///
/// One monotonic counter serves every kind, which is stronger than the seam
/// requires: a [`Handle`] carries no kind and the opcode says which table an id
/// indexes, so two kinds sharing bits would be legal. Sharing one counter makes
/// them not share bits anyway, which keeps a dump unambiguous. The packed value
/// starts one generation in and only climbs, so its high half — the generation
/// [`Handle::from_bits`] requires to be non-zero — stays non-zero for the life
/// of the process, and the low half carries into it at 2³² without ever
/// repeating a value.
///
/// Cloneable and `Send + Sync`: an [`Instance`](crate::hal::WebGpuInstance)
/// shares its pool with the device it opens, so a surface and a buffer never
/// collide.
#[derive(Clone, Debug)]
pub struct HandlePool(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl Default for HandlePool {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlePool {
    /// A fresh pool, its first handle carrying generation 1 and index 0.
    #[must_use]
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
            1 << 32,
        )))
    }

    /// Mint the next handle of any kind.
    ///
    /// Unique across every kind drawn from this pool, and never zero — see the
    /// type docs.
    pub fn alloc<T>(&self) -> Handle<T> {
        let bits = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Handle::from_bits(bits).expect("the generation half is non-zero by construction")
    }
}
