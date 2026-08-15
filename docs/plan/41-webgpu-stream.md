# Stage 41 — The WebGPU command stream

The encoding `crcbl-webgpu` speaks. Wasm serialises HAL calls into a buffer it
owns; JS decodes that buffer and replays it against WebGPU, and answers back
through a second buffer wasm also owns. This document fixes the conventions and
settles the cases that are easy to get wrong. It is slice 2 of the WebGPU track
in `ROADMAP.md`, and it exists because the encoding is the one part of that
track with no external specification — every bug in it is ours alone, and no
tool anywhere can see it.

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
  JS-side canvas table — chosen so no string crosses the boundary. A pointer
  must never be transmitted as a `u64`, and the encoding is what enforces that:
  `StreamWriter::create_surface` takes the `u32` rather than a `SurfaceTarget`,
  so a pointer variant has nothing to be encoded into. **The refusal therefore
  belongs to the `Instance` impl that unwraps the target**, not to the encoder —
  an earlier draft of this document put it on the encoder, which by then had no
  `SurfaceTarget` to refuse.

  `Offscreen` is the second reachable variant and it is deliberately **not**
  `CreateSurface`: it has no canvas key, and a reserved id standing in for one
  would be a magic number both decoders had to agree on. It gets its own command
  when the parity gate needs frames read back, because the replayer's two jobs
  differ — one resolves a canvas out of the shim's registry and takes its
  `webgpu` context, the other has no canvas to resolve and must allocate a ring
  of textures nothing presents.

  Neither of them **configures** anything. `GPUCanvasContext.configure` takes a
  `GPUDevice`, and `create_surface` is an `Instance` method the seam lets a
  caller make before any device exists — the surface is what
  `DeviceDesc::compatible_surface` then names. The configure call belongs to
  swapchain creation.

- **Callbacks.** There are none. No method on `Instance`, `Device`,
  `PendingDevice` or `CommandEncoder` takes a closure, function pointer or trait
  object as a parameter, and the HAL has an object-safety regression test that
  would fail if one were added.

## The two channels

**This section said three, and it is two.** The third — out-parameter buffers
for `poll_readback` and `query_results` — was a separate mechanism only for as
long as nobody wrote it down as bytes. Both methods answer a call that has
already returned, so both need the reply channel's sequence number anyway; once
the reply carries a length-prefixed payload the out-parameter is that payload,
and a second buffer with a second lifetime buys nothing. What is built:

1. **The command stream**, wasm → JS. One buffer, appended to during a frame,
   replayed when the frame ends. `crcbl-webgpu`'s `writer`/`reader`, and
   `web/engine/gpu-stream.js` on the far side.
2. **The reply stream**, JS → wasm. The same format read the other way, for the
   methods that return something the caller cannot be handed synchronously —
   adapter enumeration, the device-request poll, `poll_readback`,
   `query_results`. `crcbl-webgpu`'s `reply`, and `web/engine/gpu-reply.js` on
   the far side.

   **The transport is what defers an answer, not always the browser.**
   `surface_caps` is the case that shows the difference: WebGPU has no
   asynchronous capability query and the replayer answers it inside the call it
   was replayed by, and it is still a reply — the frame boundary sits between
   the two halves of every call on this seam whatever the browser can do.

They share their byte primitives — one bounds-checked reader, one writer, one
error type, in `crcbl-webgpu`'s `bytes` module — because they are one format,
and two near-identical readers are two places for a bound to be wrong.

### What the reply stream carries, and how a reply names its command

A header of magic and version, then replies back to back, each one a tag byte, a
`u64` sequence, and a body. Three things differ from the command stream and each
is forced:

- **A different magic.** `CRCBLRPL` against the stream's `CRCBLGPU`. The two
  buffers travel opposite ways through the same shim and their tag spaces
  deliberately reuse numbers, so a channel wired backwards has to fail on the
  first eight bytes rather than on whichever tag happens to be unclaimed in the
  other table.
- **The sequence is a field.** The command stream's numbers are positional — the
  nth command is `base + n` — and that is exactly what a reply cannot do: JS
  answers when the browser has an answer, so replies arrive out of order, spread
  over frames, or never. The number is `u64` for the reason the counter is, and
  the JS half carries it as a `BigInt` rather than a number.
- **No base sequence in the header**, because there is nothing positional left
  for it to anchor.

**A reply for a sequence nothing is waiting on is an error, not a dropped
record.** Wasm keeps the set of sequences it expects — `expect_reply`, bounded,
because a reply that never arrives would otherwise leave its sequence registered
for ever — and `drain_replies` refuses the whole buffer with
`UnexpectedSequence` if any reply names a number outside it, including a second
reply to a sequence already answered. A replayer answering the wrong command is
precisely the bug this channel could otherwise hide, and it looks exactly like
an answer.

The reply set built so far is **partial and deliberately so**: one reply per
encoding shape — a handle alone, a handle plus an unbounded payload, a scalar
plus a string, a counted array of fixed-size elements. The device-request poll,
the rest of `AdapterInfo` and `DeviceCaps` are not encoded yet; each is one of
those four shapes or a composition of them.

**Surface capabilities was not**, and that sentence used to claim it was.
`SurfaceCaps` needed two shapes the list above does not hold: a counted array of
**enum codes**, where — unlike a fixed-size scalar — every element has values no
variant claims and folding one into its neighbour is a silent wrong answer
rather than a refusal; and an **optional field that is neither a handle nor a
string**, which `current_extent: Option<(u32, u32)>` is the first of. Both now
exist.

The optional one is worth stating as a rule, because the niche argument does not
reach it. `Option<Handle>` rides the zero-generation niche and `Option<&str>`
takes a presence byte because `Some("")` and `None` must differ; a fixed-width
pair has no niche and the tempting mistake is neither of those, it is a
**sentinel**. There is no value to spare: `(0, 0)` is what an unconfigured or
minimised window reports, and `0xFFFF_FFFF` is the Vulkan "no opinion" value
that `SurfaceCaps::current_extent`'s own docs oblige a backend to turn into
`None` before it reaches the seam. So a presence byte, and a third value of that
byte is an error.

### The buffer JS writes into

Wasm owns it, as it owns everything on this seam. The export that sizes it —
`__crcbl_web_gpu_reply_buffer` — is the one that allocates and therefore **the
one that can grow wasm memory**, which is `crcbl-store`'s fetch ABI exactly:
build the `Uint8Array` after that call, from the pointer it returned, and never
store it. Nothing else in the module can move memory.

A committed buffer the engine has not drained is not overwritten: the next
`reply_buffer` answers `0` and the shim keeps its replies for the next frame. A
dropped reply is a command that waits for ever, so "not now" has to be
expressible and has to be the refusal's meaning.

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
- **Tags grouped into contiguous ranges by family**, as `codec.rs` groups
  messages by direction — creation, destruction, encoder state, draws, dispatch,
  copies, queries, presentation — so a corrupt tag usually lands outside a
  family rather than inside a neighbouring command.

  **A nibble per family does not fit, and this document said it did.** `Device`
  declares seventeen `create_*` methods and sixteen `destroy_*`, and
  `CommandEncoder`'s state commands come to sixteen as well: creation was over a
  nibble's capacity before a single command existed. The ranges are sized to
  what each family must eventually hold, the table lives in `crcbl-webgpu`'s
  `tag` module, and a test walks it to catch a range that overlaps its neighbour
  or is too small for the HAL methods it has to carry.

- **A presence byte for every optional field that is not a handle**, with any
  value other than the two canonical ones refused rather than treated as truthy
  — the shape `crcbl-net`'s `Hello` uses for its optional resume token. Only
  `Option<Handle>` avoids it, via the niche below. A bare length prefix will not
  do for `Option<&str>`, because `Some("")` and `None` must stay distinct.
- **Bitflags go over as `bits()`, and decode through `from_bits`, not
  `from_bits_truncate`.** They are exempt from the enum rule below for a reason
  that has to be stated rather than assumed: `BufferUsage`, `ShaderStages`,
  `ImageAspect` and `ColorWrites` are not enums, and each bit is an explicit
  `1 << n`, so the value is already chosen rather than positional. Truncating
  would silently drop a bit the other half meant; `from_bits` makes an unclaimed
  bit an error.
- **The writer asserts the same caps the reader enforces**, so nothing this
  crate encodes is something it would refuse to decode. The numbers live beside
  the constants in the `tag` module rather than here, where they would drift.
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

  **"Zero" here means a zero generation, not a zero `u64`**, and a decoder that
  tests the whole word against zero is subtly wrong on a corrupt stream:
  `from_bits` rejects any value whose generation half is zero, so bits with a
  non-zero index and a zero generation are absent too. The packing — generation
  high, index low — lives in `handle.rs`, and a second decoder must read it
  there rather than infer it from this sentence.

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
execute closure with nowhere to return an error to.

**Its contract does not change, and nothing has to be built for it.** Under the
stream it returns `Some` regardless, the pass records its draw, the invalid
group makes the submission invalid, and the failure arrives through `take_error`
— which `Gpu::acquire` turns into an error that stops the frame. That is louder
than skipping a pass and it is the right way round: a bind group this code built
wrongly is a bug, not a device that ran out of room. The existing `None` branch
stays for the backends that can still answer immediately, and its documented
per-frame retry is deliberate rather than a flood to fix.

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

That reasoning is drawn entirely from `Device::destroy_*` sites, and the rule it
produces is wider than the sites that motivated it. `Instance::destroy_surface`
was the first `destroy` to land that is not a `Device` method at all — its
object is created before any of the `crcbl-render` code above runs — and it
obeys the same rule for the same practical reason: a replayer that consults no
table cannot tell a stale id from an unlucky one, so tolerating both is the only
behaviour that needs no table.

## Error attribution

Under a stream, a WebGPU validation error names the replayer — a JS function
decoding opcodes — and not the Rust that encoded the command. Without something
carrying the correspondence, slices 5 and 6 are debugged blind. This has to be
built before them, not after.

**Every command has a monotonically increasing sequence number, but it is not a
field on the wire.** The buffer's header carries the sequence of its _first_
command and the rest are positional: the nth command decoded is `base + n`. The
replayer keeps the sequence of the command it is currently executing; when an
error surfaces it reports `(sequence, message)`. Wasm keeps a side map from
sequence to opcode and to the `label` the descriptor carried, and renders the
pair into the string `take_error` hands back.

Positional rather than per-command because a `u32` field would restate what
sequential decoding already implies, cost four bytes on every command of every
frame, and create a second source of truth that can disagree with position — and
because it would **wrap**. At a few thousand commands a frame a `u32` exhausts
within hours of play, while a counter that never goes on the wire is free to be
`u64`. The counter carries across a buffer reset, which is what "commands in
flight since the last drained error" requires.

The reply stream is the exception, and not a contradiction of this: **a reply
does carry the number, as a `u64` field**, because a reply's position implies
nothing. The counter's other property is what the reply direction leans on —
monotonic across frames — so a reply arriving three frames later still names a
number no other command has had.

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
  `ReadbackDesc::size` bytes, and a wrong length is `InvalidDescriptor`. On the
  reply stream the payload carries its own `u32` length like every other
  variable-length field, so **decoding cannot check this** — nothing in the
  buffer says what the descriptor asked for. The check belongs to the caller
  that kept the descriptor, and it is the caller that must make a short answer
  an `InvalidDescriptor` rather than a short copy.
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

- **Error-scope granularity** — needs a measurement.
- **The opcode table itself.** The families and the conventions are fixed here;
  the numbers belong with the encoder, next to the tag table, so that adding a
  command touches one file.
