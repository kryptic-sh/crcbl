# Stage 41 — The WebGPU command stream

The encoding `crcbl-webgpu` speaks. Wasm serialises HAL calls into a buffer it
owns; JS decodes that buffer and replays it against WebGPU. This document fixes
the conventions and settles the cases that are easy to get wrong. It is slice 2
of the WebGPU track in `ROADMAP.md`, and it exists because the encoding is the
one part of that track with no external specification — every bug in it is ours
alone, and no tool anywhere can see it.

The decision that produced it is recorded in `ROADMAP.md`: a pure command
stream, with no graphics imports, so that `check-exports.mjs`'s allowed-import
set can go empty.

## What crosses the boundary

Nothing but integers, and one buffer wasm owns.

This is not a new rule. `crcbl-store`'s fetch ABI, the OPFS entry points and
`crcbl-shell`'s key scratch all work this way already: every export is
`(i32, …) -> i32`, wasm owns the memory, JS reads and writes in place, and JS
never passes a pointer in. The stream follows it rather than inventing a second
convention.

Three things on the HAL seam look like they must cross and do not:

- **The trait objects.** `Box<dyn PendingDevice>`, `Box<dyn Device>` and
  `Box<dyn CommandEncoder>` are returned, never taken. Each is state that lives
  on the far side, so each is an id and nothing more. The `GPUDevice` stays in
  JS for its whole life.
- **`SurfaceTarget`.** Four of its six variants carry `NonNull` pointers to
  platform objects. In a browser only `Web { canvas_id: u32 }` and `Offscreen`
  are reachable, and `canvas_id` is already a registry key into the shell's
  JS-side canvas table — chosen so no string crosses the boundary. **The encoder
  rejects the four pointer variants with a hard error.** A pointer must never be
  transmitted as a `u64`.
- **Callbacks.** There are none. No method on `Instance`, `Device`,
  `PendingDevice` or `CommandEncoder` takes a closure, function pointer or trait
  object as a parameter, and the HAL has an object-safety regression test that
  would fail if one were added.

## The three channels

1. **The command stream**, wasm → JS. One buffer, appended to during a frame,
   replayed when the frame ends.
2. **Reply slots**, JS → wasm. For the methods that return something the caller
   cannot be handed synchronously.
3. **Out-parameter buffers**, JS → wasm. `poll_readback` and `query_results`
   take `&mut [T]` and are the only two methods that do.

### When the stream is replayed

**Once per frame, at the `requestAnimationFrame` boundary.** The browser loop
already calls one export per frame; everything encoded during that call is
replayed when it returns, and anything that comes back is read on the next
frame.

This is why the polled shapes already in the HAL matter so much: they are the
ones that survive a boundary that only opens once a frame.
`Instance::request_device` already yields a `PendingDevice` with its own `poll`,
and buffer mapping is already `request_readback` plus `poll_readback`.

## Wire conventions

Inherited wholesale from `crcbl-net`'s `codec.rs`, which is the house style and
has already made these choices for a wire format that ships. There is no
`serde`, `bincode`, `postcard` or `rkyv` anywhere in the workspace, so there is
no framework to adopt and no reason to depart from what is here.

- **Little-endian throughout**, via `to_le_bytes` / `from_le_bytes`.
- **A tag byte first**, so a decoder dispatches rather than trial-decodes.
  `codec.rs` states the reason and it holds here: a trial decode makes "unknown
  command" indistinguishable from "malformed known command".
- **Tags grouped by high nibble**, as `codec.rs` groups messages by direction.
  Command families — creation, destruction, encoder state, draws, dispatch,
  copies, queries, presentation — get a nibble each, so a corrupt tag usually
  lands outside a family rather than inside a neighbouring command.
- **A `u32` little-endian length prefix before every variable-length field**,
  then the raw bytes, written back to back with no padding.
- **A bounds-checked reader, never hand-rolled offset arithmetic.** `codec.rs`'s
  `ByteReader` exists because "a hand-rolled `if cursor + N > payload.len()` at
  each field is one more chance to get a bound wrong, and there are dozens of
  fields". This seam has more fields than that one.
- **A cap per length prefix.** The buffer is process-internal so the numbers
  differ from the network's, but an unchecked length out of a corrupt stream is
  the same defect either way.
- **A version word in the header.** The Rust and JS halves ship as separate
  artifacts and can be cached independently, so a mismatch is reachable in a
  browser in a way it is not in a single binary. `crcbl-store`'s replay and save
  files use an 8-byte ASCII magic plus a `u16` version; the stream wants at
  least the version.

### Enum tags are ours, not the compiler's

**Never write a HAL enum to the wire with `as u8`.** None of them carries
`#[repr(u8)]` or explicit discriminants, so their values are declaration order,
and `Format` is deliberately not `#[non_exhaustive]` — a variant may be inserted
in the middle. An `as u8` encoding silently renumbers every tag after the
insertion point, and the failure lands in a decoder on the other side of a
language boundary, where nothing connects it back to the edit that caused it.

The encoder keeps its own explicit tag table, in the same shape as `codec.rs`'s
named `*_TAG` constants. This is the single most likely way for this encoding to
acquire a bug that survives review.

## Handles

`crcbl_core::Handle<T>` is `{ index: u32, generation: NonZeroU32 }`, `repr(C)`,
eight bytes — not a plain integer newtype. It already has a sanctioned wire
form: `to_bits` packs generation and index into a `u64` that is documented as
"stable and never zero, which makes it safe to use as a wire/serialization
representation with `0` reserved for 'none'", and `from_bits` rejects a zero
generation.

Two consequences:

- **`Option<Handle>` encodes as a bare `u64`, zero meaning `None`.** No presence
  byte. This is what the niche was for, and optional handles are common —
  `ColorAttachment::resolve`, `DeviceDesc::compatible_surface`,
  `AcquiredFrame`'s semaphores.
- **The handle carries no kind.** Every HAL handle is the same eight bytes and
  the type distinction is compile-time only, so **the opcode is what says which
  table an id indexes.** A replayer with one flat table per resource kind is
  correct; a single table keyed on handle bits is not.

There is a trap the HAL already documents: a `Handle`'s bits cannot carry owner
identity, because its index and generation are fully spoken for and two pools
genuinely do issue identical bits. A backend is obliged to keep a side table
stamping an owner id and to answer `HalError::ForeignObject` on a mismatch. A JS
replayer inherits that obligation the moment a second device exists.

## Creation, without a synchronous answer

A stream cannot answer during the call, so creation cannot return a real handle.
Instead:

**Wasm allocates the handle itself, from its own pool, and writes the id into
the stream alongside the descriptor.** JS creates the object at replay time and
stores it in the table at that id. The call returns `Ok(handle)` immediately.

This works because the identity is positional — the id is decided by the caller,
not derived from anything the browser returns — and it is why the id table is
not new machinery: the HAL hands out opaque handles already.

Failure then arrives out of band, through `Device::take_error`, which exists for
exactly this and is drained at the top of every frame by `Gpu::acquire`. WebGPU
behaves this way regardless of the transport: a pipeline whose shader will not
compile is handed back as a valid object and the reason arrives later.

An audit of the workspace found that **no production code inspects which
`HalError` variant a creation call returned**, and that four callers branch on
creation failure at all — all in `crcbl-render`, all optional subsystems that
switch themselves off. Three re-express against capability checks or a frame of
`take_error` delay. The fourth, `cached_group` in `ssao.rs`, sits inside a graph
execute closure with nowhere to return an error to, and is slice 3's subject —
**it is not settled by this document.**

### The destroy op

`destroy_*` is an ordinary in-stream command with no reply, and it must be,
because of a pattern that recurs throughout `crcbl-render`: hold the creation
`Result` unfrozen, destroy the shader module, _then_ apply `?`. Several sites
also match `Err` only to destroy a sibling resource before returning the same
error.

Neither needs the error synchronously. Both mean **"destroy the thing I just
pre-allocated" has to be a valid stream op**, including for a handle whose
creation will turn out to have failed. The replayer must therefore tolerate a
destroy naming an id whose slot holds nothing, and treat it as a no-op rather
than as stream corruption.

## Error attribution

Under a stream, a WebGPU validation error names the replayer — a JS function
decoding opcodes — and not the Rust that encoded the command. Without something
carrying the correspondence, slices 5 and 6 are debugged blind. This has to be
built before them, not after.

**Every command carries a monotonically increasing sequence number.** The
replayer keeps the sequence of the command it is currently executing; when an
error surfaces it reports `(sequence, message)`. Wasm keeps a side map from
sequence to opcode and to the `label` the descriptor carried, and renders the
pair into the string `take_error` hands back.

That map is bounded: it only has to cover commands in flight since the last
successfully drained error, and it can be dropped entirely in a build that does
not want it, at the cost of the attribution.

The granularity of `pushErrorScope`/`popErrorScope` around replay is an
implementation choice this document deliberately leaves open — per flush is the
cheap default, per command is the precise one, and which is affordable is a
measurement nobody has taken.

## The cases that are easy to get wrong

- **`ShaderModuleDesc`'s absence conventions differ per field, and the
  difference is load-bearing.** An empty `spirv` slice means absent, but
  `Some("")` for `wgsl` or `msl` means _present and empty_. Conflating them
  turns a truncated file into "this backend does not get WGSL". The encoding
  must preserve `Some("")` ≠ `None`.
- **`dxil` is the worst-shaped field on the seam**: a slice whose element is a
  tuple of a `&str` and an unbounded byte slice — two variable-length leaves
  under one slice. It is meaningless to a WebGPU replayer, but it is in the
  descriptor and **must be skipped correctly**, which a fixed-stride array
  decoder will not do.
- **`spirv` is `&[u32]` and wants four-byte alignment**, against a house style
  that pads nothing. WGSL is what a browser consumes, so the practical answer is
  that this payload is absent in the browser — but the decoder still has to
  traverse it, and a deliberate alignment exception is better than a realignment
  copy hidden in the reader.
- **`BindGroupEntry` is fixed-stride.** `BindingResource` has three variants, no
  slices, no strings and a fixed maximum body, so entries encode as a flat
  array. This is the one place the encoding is _simpler_ than expected, and both
  `BindGroupDesc::entries` and `update_bind_group`'s bare slice benefit.
- **`BindGroupLayoutDesc::entries` is order-sensitive.** A variable-count entry
  must be both last in the slice and highest-numbered, and the HAL enforces it.
  The decoder must preserve slice order exactly rather than rebuilding the list
  from binding numbers.
- **The deepest descriptor chain is the depth-stencil one**, not the colour one:
  a pipeline's `Option<DepthStencilState>` holds an `Option<StencilState>` which
  holds two `StencilFaceState`s of leaf enums. A decoder written to a
  three-level assumption will be wrong here first.
- **`poll_readback`'s output length is a hard contract**: exactly
  `ReadbackDesc::size` bytes, and a wrong length is `InvalidDescriptor`. The
  reply buffer is sized from the descriptor, never from what JS thinks it wrote.
- **`WHOLE_BUFFER` is `u64::MAX` and passes through verbatim.** WebGPU's
  `size: undefined` means the same thing; resolving it in the encoder would
  discard that.
- **`Range<u32>` is passed by value in four encoder methods and is not `Copy`.**
  On the wire it is two `u32`s and nothing more, but it is not a type that can
  be cast wholesale.
- **`bind_group` and `push_constants` both take the pipeline layout as their
  last parameter.** It is the argument most easily dropped when writing an
  encoder by hand.

## Precedent to read before implementing

`crcbl-hal`'s null backend already contains `record::Command`, a one-to-one
owned mirror of `CommandEncoder` built for the same reason this encoding exists
— the recorded stream must outlive the borrowed descriptors that produced it. It
has already resolved several questions this document asks:

- `&str` becomes `String`, `&[T]` becomes `Vec<T>`, and the record deep-copies
  at capture time.
- `BeginRenderPass` flattens the descriptor into named fields rather than
  nesting a descriptor struct. Opcode bodies should do the same.
- Copy direction lives in the variant name, never in a field, so the two cannot
  disagree.
- `Command::name()` returns a stable variant name, which is the natural source
  for a debug decoder that prints a stream.

One thing in it must **not** be copied: `PushConstants` stores only the length
of its data, not the bytes. That is a testing decision. A replayer needs the
bytes.

## What this document does not settle

- **`cached_group`'s contract** — slice 3.
- **Error-scope granularity** — needs a measurement.
- **The opcode table itself.** The families and the conventions are fixed here;
  the numbers belong with the encoder, next to the tag table, so that adding a
  command touches one file.
