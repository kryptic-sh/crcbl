//! The WebGPU command stream: the encoding wasm and JS agree on.
//!
//! Wasm serialises HAL calls into a buffer it owns; JS decodes that buffer and
//! replays it against WebGPU, and answers back through a second buffer wasm also
//! owns. `docs/plan/41-webgpu-stream.md` is the specification, and this crate is
//! its first three slices — **the encoding, the transport that carries it, and
//! the reply channel that carries answers home**. There is no `Instance`, no
//! `Device` and no `CommandEncoder` implementation here yet; those are later
//! slices, and they encode through [`StreamWriter`] and read
//! [`Reply`]s when they arrive.
//!
//! Nothing but integers and buffers wasm owns crosses the boundary, which is
//! the convention `crcbl-store`'s fetch ABI and the OPFS entry points already
//! use. The stream carries no pointers: the trait objects on the HAL seam are
//! returned and never taken, so each is an id and nothing more. [`web`] is that
//! boundary — where each buffer is, how long it is, and who is finished with it.
//!
//! # Two directions, one format
//!
//! * **Commands, wasm → JS.** [`StreamWriter`] appends them during a frame;
//!   [`decode_stream`] (and, in production, `web/engine/gpu-stream.js`) reads
//!   them back. Sequence numbers are positional and never on the wire.
//! * **Replies, JS → wasm.** [`ReplyWriter`] — and, in production,
//!   `web/engine/gpu-reply.js` — encodes the answers to the calls that cannot be
//!   answered synchronously; [`decode_replies`] reads them. **Each reply carries
//!   the sequence of the command it answers**, because replies need not arrive in
//!   order or at all. See [`reply`].
//!
//! Both are the same format with the same caps and the same bounds-checked
//! cursor: one magic per direction, a version word, a tag byte per record, `u32`
//! length prefixes. Only the header and the tag table differ.
//!
//! # What is encoded, and what is not
//!
//! **The command set here is a deliberate subset**, one command per *encoding
//! shape*, so that every shape the seam has is exercised by a round-trip test
//! before the other eighty-odd commands are written against it:
//!
//! | Shape | Command |
//! | --- | --- |
//! | fixed-size scalars only | [`StreamWriter::draw`] |
//! | a handle by value | [`StreamWriter::bind_graphics_pipeline`] |
//! | a slice of POD elements | [`StreamWriter::bind_group`] |
//! | an unbounded byte payload | [`StreamWriter::push_constants`] |
//! | a borrowed `&str` | [`StreamWriter::begin_debug_label`] |
//! | an optional string, and structs with optional handles | [`StreamWriter::begin_render_pass`] |
//! | creation with a caller-allocated handle | [`StreamWriter::create_buffer`] |
//! | a destroy op | [`StreamWriter::destroy_buffer`] |
//!
//! The rest of the seam is made of these same shapes, so adding a command is
//! adding a tag in [`tag`] and a method beside its neighbours — not a new
//! convention. The reply set is a subset for the same reason and on the same
//! terms; [`reply`] lists what it covers and what it does not.
//!
//! # The wire
//!
//! Inherited wholesale from `crcbl-net`'s `codec`, which is the house style and
//! has already made these choices for a format that ships. Little-endian
//! throughout; a tag byte first so a decoder dispatches rather than
//! trial-decodes; a `u32` length prefix before every variable-length field, then
//! the raw bytes back to back with no padding; a cap on every length; and a
//! bounds-checked reader rather than hand-rolled offset arithmetic.
//!
//! A buffer is a header — [`tag::STREAM_MAGIC`], [`tag::STREAM_VERSION`] and the
//! sequence number of its first command — followed by commands back to back.
//!
//! ```
//! use crcbl_hal::ShaderStages;
//! use crcbl_webgpu::{Command, StreamWriter, decode_stream};
//! # use crcbl_core::Handle;
//! # let layout = Handle::from_bits(1 << 32).unwrap();
//!
//! let mut stream = StreamWriter::new();
//! stream.push_constants(ShaderStages::VERTEX, 0, &[1, 2, 3, 4], layout);
//! stream.draw(0..3, 0..1);
//!
//! let commands = decode_stream(stream.bytes())?;
//! assert_eq!(commands[1], Command::Draw { vertices: 0..3, instances: 0..1 });
//! # Ok::<(), crcbl_webgpu::DecodeError>(())
//! ```
//!
//! ## Handles
//!
//! [`Handle::to_bits`](crcbl_core::Handle::to_bits) is the sanctioned wire form
//! and is documented as never zero, so **an `Option<Handle>` is a bare `u64`
//! with zero for `None`** and carries no presence byte. Everything else optional
//! does carry one, because nothing else has the niche.
//!
//! A handle carries no kind: every HAL handle is the same eight bytes and the
//! type distinction is compile-time only, so **the opcode says which table an id
//! indexes**. A replayer with one flat table per resource kind is correct; a
//! single table keyed on handle bits is not.
//!
//! ## Creation, without a synchronous answer
//!
//! A stream cannot answer during the call, so **wasm allocates the handle itself
//! and writes it into the stream alongside the descriptor** — see
//! [`StreamWriter::create_buffer`]. Failure arrives out of band through
//! [`Device::take_error`](crcbl_hal::Device::take_error), which is what WebGPU
//! does regardless of transport: a pipeline whose shader will not compile is
//! handed back as a valid object and the reason arrives later.
//!
//! ## Destroying what was never created
//!
//! `crcbl-render` holds a creation `Result` unfrozen, destroys the object, and
//! *then* applies `?`. So a destroy naming an id whose creation turned out to
//! have failed is a legal stream op, and **the replayer must treat a destroy of
//! an empty slot as a no-op rather than as stream corruption**. The decoder here
//! consults no table at all, which is the same rule seen from this side: it
//! decodes a destroy for an id nothing in the buffer ever created.
//!
//! # Sequence numbers
//!
//! Every command has one, because a WebGPU validation error names the replayer —
//! a JS function decoding opcodes — and not the Rust that encoded the command.
//! See [`StreamWriter`] for where the number lives and why it is not a
//! per-command field.

mod bytes;
pub mod command;
pub mod reader;
pub mod reply;
pub mod tag;
pub mod web;
pub mod writer;

pub use bytes::DecodeError;
pub use command::Command;
pub use reader::{StreamReader, decode_stream};
pub use reply::{Reply, ReplyReader, ReplyWriter, decode_replies};
pub use writer::StreamWriter;
