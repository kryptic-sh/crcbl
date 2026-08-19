# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

### `render_e2e` never runs against dx12's own e2e job, and that hid a step

Found while trying to reproduce the WARP device removal. The
`dx12 e2e (software adapter)` job runs
`crates/crcbl-dx12/tests/run-dx12-e2e.sh`, which is
`cargo nextest run --package crcbl-dx12` — so the crate's own suite — and then a
_separate later step_, "Draw a frame through ForwardRenderer on WARP", runs
`run-render-e2e.sh` with `CRCBL_GPU=dx12`. The `render_e2e` tests are
`#[ignore]`d and appear as SKIP in every other job, including
`build + test (windows-latest)`, so a reader scanning that job's 3993 passing
tests would conclude the renderer is covered on Windows. It is not; only that
one step covers it.

The consequence to remember: **a failure in the crate suite stops the job before
the renderer step runs at all.** The first diagnostic attempt hit exactly that
and produced no evidence. Any future "did the renderer break on WARP?" question
has to check that the step actually executed rather than that the job went red.

Not a defect to fix so much as a shape to know. If it is ever worth changing,
the options are to run the render step first, or to give it its own job so the
two failures cannot mask each other.

### The GPU-encoded ICB probe ran, and every answer was yes

`crcbl_mtl::device`'s
`a_compute_kernel_encodes_the_draw_an_indirect_command_buffer_executes` passed
on CI's `Apple Paravirtual device`:

```text
icb-kernel: device="Apple Paravirtual device" argumentBuffersSupport=MTLArgumentBuffersTier(1)
            supportsFamily Metal3=false Apple3=true Mac2=true
icb-kernel: gpuResourceID=MTLResourceID { _impl: 90194771968 } size=1
icb-kernel: centre=[40, 80, C0, FF]  corner=[11, 22, 33, FF]
```

`centre` is the ink and `corner` the untouched prime, so the four open questions
are all answered:

1. **Metal's front end accepts hand-written `command_buffer`/`render_command`
   MSL** on that runner's MSL version.
2. **A Metal 3 argument buffer does read a `command_buffer` member as the
   `MTLResourceID` `gpuResourceID` hands out** — the `MTLArgumentEncoder`
   fallback was not needed.
3. **The device is `Tier1`** and encodes anyway, which is also what proved the
   bindless gate's tier half wrong.
4. **`useResource:usage:` covered the blit-reset-to-kernel-write hazard**, and
   `optimizeIndirectCommandBuffer:withRange:` was not needed to make it work.

So GPU-side ICB encoding is a proven mechanism on the only Mac this project has.
What it does **not** prove is a whole frame's worth: the slice built on it hung
the GPU, and the diagnosis is in its own entry below.

### A capability gate is a proxy until you check it — four in one day

`Capability::BindlessDescriptorArray` on Metal is closed **and driven**: the
seam reports `BindlessDescriptorArray supported`, 20 of 26 driven, and
`every seam obligation held on mtl`. Getting there took four wrong questions,
each of which looked reasonable and each of which the seam caught with the same
line — "this device withheld …, so metal was never asked":

1. **`supportsFamily(MTLGPUFamily::Metal3)`** — a family _feature set_ standing
   in for an _API's availability_. CI's `Apple Paravirtual device` answers
   `Metal3 = false` and returns usable `gpuAddress` values anyway.
2. **`argumentBuffersSupport() == Tier2`** — the tier governs argument buffers
   in the _resource-binding_ sense (`[[id(n)]]` slots, resource arrays, heaps).
   That device is `Tier1`, and both a bindless pointer table and an ICB handle
   in an argument buffer work on it under Metal API **and** GPU validation.
3. **`respondsToSelector(sel!(gpuAddress))` sent to the `MTLDevice`** —
   `gpuAddress` belongs to `MTLBuffer`, so a device never responds and the gate
   was false everywhere. The right question is the system's: an `NSProcessInfo`
   macOS-13 check.
4. **`Features::DESCRIPTOR_INDEXING` standing in for `UPDATE_AFTER_BIND`** — in
   the seam itself, not the backend. Independent `BindingFlags`, and the
   heuristic held only while no backend had one without the other. Metal gaining
   bindless broke it and scored a capability Metal genuinely supports as
   declared-and-refused. The exercise now asks by building a throwaway layout.

**The transferable rule: a capability gate must ask the question the code
depends on.** Every one of these substituted a nearby, easier question. Three of
them produced a row reported closed while running on no machine anywhere, which
is green and worthless — and the only reason any of it was caught is that the
seam prints what it did _not_ exercise. That print is load-bearing; do not let
it become a summary of counts.

### `Features::BUFFER_DEVICE_ADDRESS` on Metal rides a query that is wrong

`crcbl_mtl::adapter::features_of` reports it from
`supportsFamily(MTLGPUFamily::Metal3)`, on the reasoning that `gpuAddress` is a
Metal 3 property. That reasoning is **measurably false**: CI's
`Apple Paravirtual device` answers `supportsFamily(Metal3) = false` and returns
usable `gpuAddress` values anyway — `crcbl_mtl::binding`'s bindless probe read
four non-zero addresses on it and a kernel dereferenced every one. The family
query describes a feature set; the selector's availability is a macOS version
question, and they are not the same.

`Features::DESCRIPTOR_INDEXING` had the same gate and was fixed, because leaving
it would have reported the bindless capability closed while switching it off on
the one device that had proven it. **`BUFFER_DEVICE_ADDRESS` was deliberately
left alone**: nothing on this backend exercises `BufferUsage::DEVICE_ADDRESS`,
so correcting the gate would turn on a path no test covers — adding unproven
surface rather than fixing a measured defect. The honest fix, when something
needs it, is `respondsToSelector:` on `gpuAddress`, which is what
`DESCRIPTOR_INDEXING` uses now.

Worth knowing generally: **`supportsFamily:` is not an availability check.** Any
other capability here gated on a family as a proxy for "this selector exists" is
wrong the same way.

**Swept, and this is the only one.** Every other `supportsFamily:` in
`crcbl-mtl` is a probe printing what a device answers, not a gate deciding a
capability. Metal's mesh rows in particular do **not** ride a family query —
they are `Support::No` with "this backend builds no
`MTLMeshRenderPipelineDescriptor`", which is honestly unwritten work. The
runner's `Metal3 = false` is why writing them could not be _verified_ here, not
why they are reported unsupported today, and those are different claims.

### What `apps/viewer` still owes sample 05

`apps/viewer` is `docs/plan/sample/05-viewer.md`'s **milestone 1 minus the
grid**: a path argument, the load through `crcbl::assets::DirSource`, the
conversion, frame-on-load, orbit/pan/zoom/`F`, and one directional light. Every
module's docs name its own omission; this is the list in one place.

- **The grid floor is drawn, and its scale is fixed.** `crcbl_render::grid` is
  wired into `ForwardRenderer` behind `set_ground_grid`, off by default, drawn
  **after the tonemap** so it is not exposed and tonemapped like scene content —
  a reference grid has to look the same at any exposure — and depth-tested
  against the scene depth the forward pass already stores for SSR. `apps/viewer`
  turns it on, which completes milestone 1.

  **The scale is derived from the document.** `GridStyle::for_extent` picks the
  power of ten nearest `extent / 10` for the cell and fades at twenty times the
  extent, and the viewer feeds it the model's largest axis. A power of ten
  because a cell of `0.1`, `1` or `10` is a number a person can count in, which
  is what a grid is for — `3.7` gives the same line density and says nothing.

  **Verified on Vulkan only.** The wiring's pixel evidence is one RADV run; no
  golden anywhere turns the grid on, so `run-render-e2e.sh`'s Metal, DX12 and
  wgpu arms say nothing about it. The shader itself was checked on hardware in
  the previous slice, but `SV_Depth` on Metal and WebGPU is still compiled-and-
  read rather than executed.

  `Grid` bakes `target_format` into its pipeline at first use, so a caller
  drawing into a different format needs a second one. That matches the tonemap
  pipeline's existing assumption rather than adding a new limit.

- **Milestone 2 has landed in full** — the listing panel (`I`), the wireframe
  (`W`), runtime exposure on `-`/`=`, the normals view (`N`) and now the
  exposure slider on the `ESC` panel. What follows is what each of them left
  behind.

  The slider leaves two. **It has no keyboard of its own**: the engine owns the
  menu keys and arbitrates up, down and Enter, so left/right on a highlighted
  row would be a fourth reserved binding in `crcbl::engine` and a change to
  `MenuPump`. `-` and `=` reach the exposure whether or not the panel is up and
  the handle mirrors them, so the row is reachable without a pointer — it is the
  _handle_ that is not. Worth doing when a second sample wants a slider; not
  worth a reserved key for one. And **the groove is drawn with the UI pass's own
  rectangles**, not nine-sliced art, so it does not swap under
  `ButtonState::Pressed` the way the row behind it does; a skin would need three
  more sprites on the shipped sheet.

  Three things the wireframe left behind. Its **pixel proof runs only with a
  backend pinned** and was measured here on RADV through the _mesh-shader_ path;
  CI's lavapipe takes the indirect path, which is the arm nobody has measured —
  which is why its overlap bar is a simple majority rather than "nearly all".
  The **wireframe pipeline is never released until `destroy`**, so a session
  that pressed `W` once carries a pipeline it may never use again. And
  **wgpu/web is untested end to end**: the designed behaviour is
  `supports_wireframe` false, one line at start-up, and a warning per press,
  whose renderer half is tested but which no browser run confirms. `crate::gpu`
  has a `UiRenderer` pass now, so the blocker is no longer the pass: it is that
  none of those listings has been written.

  The normals view leaves two of its own. Its **pixel proof
  (`gpu::tests::the_normals_view_paints_each_face_the_encoding_of_its_world_normal`)
  also needs a pinned backend** and was measured here on RADV only; CI's
  lavapipe runs it, but Metal, DX12 and wgpu have executed the branch nowhere —
  the generated MSL and WGSL were read, not run. And
  **`RenderEffects::REFLECTIONS` contaminates it at the silhouette**: the
  fragment stage writes a zero `F0` in this view so `ssr.slang` marches nothing,
  but Schlick's grazing tail is `(1 - F0) * (1 - N·V)^5`, which is not zero, so
  a scene with an irradiance grid adds a faint environment along the edges. No
  caller hits it — `apps/viewer` requests no effects — and fixing it properly
  means teaching `ssr.slang` about the debug view, which is a second uniform and
  a second shader for a defect nobody can see.

- **The browser drop target**, the other half of V-F5. It needs an `AssetSource`
  over a file a browser handed the page, which stage 10 owns.

- **Hot reload has landed and leaves four.** `crate::watch` plus `Gpu::reload`,
  driven from `Viewer::tick`.

  **It stops while the `ESC` panel is up.** `FrameInfo::ticks` is zero while the
  loop calls itself paused, so no tick runs and nothing is polled — an artist
  who re-exports with the panel open sees the new file when they close it. The
  fix is a clock the frame carries rather than the tick: `FrameInfo` has
  `tick_dt` and `ticks` and no wall-clock delta at all, so a viewer polling in
  `draw` has nothing to advance a timer by. Adding one is an engine change every
  sample would see.

  **Verified against written files, not against Blender.** The tests write a
  `.glb` with `std::fs::write`, which is one `open`/`write`/`close`. A real
  exporter's write pattern — a temporary file and a rename, or a long
  progressive write — is what the settle is _for_, and no test reproduces it.
  `watch::tests::a_file_still_growing_is_never_offered` is the closest and is
  still a synthetic sequence.

  **A rename onto the path is a stamp change and would be picked up**, which is
  the common exporter pattern and is fine, but nothing here proves it: the
  `stat` follows the path, so the new inode's stamp is what the next poll sees.
  Untested.

  **Neither half of the stamp is individually pinned by the suite.** A stamp is
  a modification time and a length, and each covers the other's blind spot — but
  breaking `watch::stamp` to report a constant length, and again to report no
  modification time, leaves every `watch::tests` case green on Linux. The length
  half is what a coarse clock needs, and CI proved that the hard way: a test
  that leaned on the modification time moving between two writes failed on
  Windows, where the clock a write is stamped from advances on the ~15.6 ms
  timer tick and all three writes shared a timestamp. The modification-time half
  is what a same-length edit needs, and **nothing tests it** — `std` has no way
  to set a file's modification time, so a test cannot hold one still. Closing it
  means a `filetime` dev-dependency, which is a decision rather than a chore.

  **`load_and_report`'s `drew_nothing` branch is unreachable.** `model::load`
  computes the bounds from the primitives the conversion would draw, so a
  document with nothing to draw is already a `LoadError::NoGeometry` from
  `load`. The branch after it can therefore never run, and its
  `LoadError::NoGeometry(options.model.clone())` is dead. Found while writing
  the reload path — `Viewer::tick` has the same shape and says in a comment why
  it passes the flag on rather than hard-coding it. Not removed: it is start-up
  behaviour this slice had no reason to touch, and the belt-and-braces reading
  is defensible if `world_bounds` ever counts something the conversion skips.

### The hosted seam carries no modifiers

`HostedGame::key_event`, `button_event` and `wheel_event` all hand over the key
or button and an edge, and no `Modifiers`. `ShellEvent::Wheel`'s own docs call
its modifiers load-bearing — "Ctrl+wheel is zoom nearly everywhere" — and
`ShellEvent::Key` and `ShellEvent::Button` carry them too, so the fold in
`Pending::observe` is where they are dropped.

Nothing in `apps/` binds a modifier today, which is why the field was not added
to one hook on its own: a `wheel_event` that carried them beside a `key_event`
that did not would be an inconsistency with nothing exercising it. The change to
make when a caller arrives is all three at once, and the open question is
whether it belongs on the hooks or on a `Loop::modifiers()` the game reads —
held state is what a binding actually wants, and an edge-carried copy of it goes
stale between events.

### A press and the motion in the same batch cannot be ordered

`Loop::frame_body` collapses a pump to one `PointerUpdate` and dispatches
`button_event` and `wheel_event` before it, so a press and the movement that
follows it inside one frame are applied in that order. The reverse — moving,
then pressing, inside the same batch — is applied as though the press came
first, which credits up to one frame of hover to the drag.

Sub-frame, and inherent to collapsing the pointer at all: the same trade
`pointer_pressed` already made before this. Fixing it means a per-event pointer
stream beside the collapsed one, which is a second seam for a defect nobody has
reported. Noted because `apps/viewer` is the first caller where it is observable
at all — a game's paddle does not care, a turntable in principle could.

### The viewer frames the document's geometry, not the geometry it draws

`apps/viewer`'s `model::world_bounds` unions one `Aabb` per glTF primitive,
pushed through its instance's composed transform. Those are the primitives the
**document** declares, and `build_render_scene` may skip some of them — so a
document with a skipped primitive is framed a little wide.

It errs in the safe direction (wider, never tighter) and every skip is printed,
so this is a refinement rather than a defect. Fixing it needs `RenderScene` to
say which `(mesh, primitive)` each of its `instances` came from, or to carry
per-instance bounds; neither exists, and adding one to `crcbl-scene` for a
cosmetic framing difference was not worth it here.

### The one device-loss ordering that does not hold

`gpu-replay.js` now watches `GPUDevice.lost` and files the loss once, first,
with its reason, and both comments in `#requestReadback` and `#loseDevice` point
here for the case it cannot make clean.

**The ordering that does hold.** The specification's "lose the device" resolves
`lost` **before** completing the steps waiting on a loss, so a map rejection
caused by a genuine loss arrives after `#loseDevice` has already filed the
readback with the loss text, and the rejection handler leaves it alone. That is
the path a real device failure takes.

**The one that does not.** `GPUDevice.destroy()` does not follow that route: it
cancels an outstanding map through the **buffer**, and Chromium was watched
rejecting one with "Buffer was unmapped before mapping was resolved" a whole
task _ahead_ of `lost`. `#loseDevice` re-files such an entry so the readback
ends up carrying the loss either way — but the rejection was pushed to the
**error queue** when it landed, and a queued error cannot be taken back. So on
that single path a reader sees the browser's sentence before the loss.

Closing it means either not filing a map rejection until a turn has passed, so a
loss can still claim it — which delays every honest rejection to tidy one — or
making the error queue support retraction, which is a wire-format change to
`Reply::DeviceErrors` for a cosmetic ordering. Neither is obviously worth it,
and the entry that matters (the readback's own failure reason) is already
correct in both orderings. Recorded so the next reader does not mistake the
ordering for an oversight.

### A lost device is reported as whatever call noticed it first

**Reported from a real machine, 2026-08-18.** An NVIDIA GeForce MX550 laptop
(hybrid with Intel UHD, nvidia 610.57.04, i915, Linux 7.1.6) failed two ways on
consecutive loads:

```
gpu error: the device reported vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY
  - While calling [Device "horde"].CreateBuffer([BufferDescriptor "sprite instances"])
gpu error: readback 386.1 (command 989) could not be mapped: AbortError: Failed to
  execute 'mapAsync' on 'GPUBuffer': [Device "lumen"] is lost.
```

**Neither is a memory problem and the first is not a horde bug.** Measured on
the source: `INITIAL_RING_BYTES` is 1024, `grown` doubles by
`next_power_of_two`, `INSTANCE_STRIDE` is 64, `DEFAULT_MAX_ENEMIES` is 1500, the
four baked `.crpix` sheets total about 45 KB, `Scene::build` calls
`self.stack.clear()` every frame so instances cannot accumulate, and
`view_half_width` scales with **aspect only**, so a larger monitor does not
enlarge the tile lattice. `MemoryLocation::HostUpload` becomes plain `COPY_DST`
with writes through `queue.writeBuffer` (`MEMORY_LOCATION_USAGE` in
`gpu-replay.js`), so there is no host-visible BAR allocation and no staging
double. The reporting machine had **81 MB of 2.0 GB** in use and failed
immediately on first load. A kilobyte-scale allocation cannot exhaust that.

Both messages are the same event — the device failing — surfacing through
whichever call happened to touch it. The second one says so outright.

**What is actually missing:** `web/engine/gpu-replay.js` deliberately does not
watch `GPUDevice.lost`, and says why at `#requestDevice` — it "means the device
is gone rather than that a call failed, so what it wants is the seam's
device-lost path, and there is none on this channel yet". That reasoning is
sound and the channel has since grown one: `Reply::ReadbackFailed` becomes
`HalError::DeviceLost`, and its doc already lists "a device lost mid-map" among
what reaches it. So the plumbing exists; what is missing is the **cause**.
`GPUDevice.lost` resolves with a `reason` and a `message` that name why the
device died, and nothing reads them, so every loss is diagnosed from the tail of
an unrelated call's error.

**The work:** watch `GPUDevice.lost`, record the reason, and make the replayer
fail fast afterwards — every subsequent command answering "the device was lost:
&lt;reason&gt;" rather than each producing its own downstream error. That is the
distinction `#requestDevice`'s comment asks for, not a contradiction of it: not
"one more error to log and carry on from", but a terminal state that names
itself.

**Not diagnosed:** why that machine's device dies. It is a hybrid laptop and the
demos run on the discrete GPU. Whether Chrome on the Intel UHD survives is the
cheapest next measurement and needs that machine; nothing in the tree reproduces
it, and every gate here runs on an RX 7900 XTX or on CI's software adapters.
Running Chrome with `DRI_PRIME=0` and no `__NV_PRIME_RENDER_OFFLOAD` is the way
to take it.

**Recovery is the follow-up, and it is what the platform expects.** Reporting a
loss well still leaves the demo dead. The WebGPU specification treats device
loss as recoverable and expects an application to respond by requesting a new
adapter and rebuilding its resources — it is the reason `GPUDevice.lost` is a
promise carrying a reason rather than a fatal error. So the sequence is: report
the loss with its cause (in flight), then rebuild on it.

**Note what the engine does not currently choose.** `gpu-replay.js` calls
`this.#gpu.requestAdapter()` with **no options at all** — no `powerPreference` —
so the browser picks, and on a hybrid laptop that is typically the discrete GPU.
This is deliberate as a default and should stay: forcing `'low-power'` would
move every desktop with a healthy discrete GPU onto its integrated one, which
trades a real regression everywhere for a workaround on one machine. But it
means a recovery path has a genuine decision in it — whether a **second**
`requestAdapter` after a loss should ask for something different from the one
that just died, or whether retrying the same choice is honest and a repeated
loss should surface as a permanent failure. Retrying identically risks an
immediate second loss; asking differently silently changes which GPU renders.
Worth deciding when recovery is built, not before.

### The canvas encode is gated for three demos, not six

Group G in `web/tools/browser-e2e.mjs` closed the gap the sRGB bug reached a
user through: it reads a demo's own on-screen canvas through `toDataURL` and
holds the demo's flat clear colour against the byte an sRGB target holds. It is
the only check anywhere that reads engine output off the element a visitor looks
at — the probe gate's groups I and X cover both halves of the mechanism, but
every byte either compares is copied out on the GPU into wasm on a page with no
engine running.

**What remains is that only `breakout`, `flappy` and `hud` can make the claim.**
The other three carry no `backdrop` row and the driver says why per demo:
`asteroids` ends under its start menu's full-screen scrim, `horde` tiles grass
sprites over every pixel of `GROUND`, and `lumen` has no `clear_color` pass at
all. So a regression that spared those three demos' clears and broke only
`lumen`'s tone mapping would still pass. Closing it needs a different observable
than a flat fill — the flat fill is what makes the current check exact on
SwiftShader and paravirtual Metal without a tolerance, and that property is
worth keeping rather than trading for coverage.

**But it is not a gap in coverage of the sRGB bug, and that distinction decides
whether it is worth a slice.** The canvas encode is one shared mechanism — the
configuration `web/engine/gpu-replay.js` applies for every demo — so a
regression in it cannot spare three demos and break the other three; whichever
demos are covered would catch it. The escape this entry names is _lumen's tone
mapping_, which is a per-demo rendering regression and a different class of
thing. So three of six is enough for the bug this group was built for, and the
open half is end-to-end output coverage in general.

That would be falsified by a demo choosing its own canvas format rather than
taking the shared one; nothing does today, and if one ever does, this reasoning
stops holding and the group needs all six.

**What closing it properly would take**, recorded so the option is not
re-derived: comparing each demo's canvas against the engine's own framebuffer
through the sRGB transfer function, which is content-independent and needs no
flat region and no golden. It is not cheap — `crcbl-web` exports no readback to
JS (`__crcbl_web_*` covers input, audio, fetch and OPFS, and nothing reads
pixels), so it means new engine surface existing only for a test. That is why it
has not been done, rather than nobody having thought of it.

Measured, not assumed: with `surfaceCapsFor` offering only linear formats, `hud`
scores **33 of 34** with group G the only failure, while group D's "the canvas
is not one flat colour" passes on `rgb(8,8,16)` at 89.2%. The same broken build
fails the probe gate too, so neither gate is load-bearing alone.

### TOP PRIORITY — backend feature parity, enforced so it cannot rot

**The goal: every render feature works on every backend — vk, dx12, Metal,
WebGPU — with divergence made impossible to introduce silently. `crcbl-wgpu` is
dropped once that holds.** It is kept until then, but not as a conformance
oracle: on Linux it runs Vulkan underneath, so it is a second abstraction over
the same driver rather than an independent implementation, and agreement between
them proves less than it appears. What it caught (`fill_buffer` non-zero) came
from its own refusal policy, not from a different GPU stack — and **Metal
refuses the same call for a different reason**, since `fillBuffer:range:value:`
takes a byte, not a word. Two of four backends cannot honour a contract the seam
documents.

**The structural problem.** Nothing connects "what the seam promises" to "what a
backend does". A feature added to `crcbl-vk` alone breaks nothing: the other
backends simply never implement it, their `Unsupported` is invisible until
something runs, and the agnostic suites only cover behaviours somebody
remembered to put there. Parity is currently maintained by attention, which is
why it has drifted.

**The numbers live in the code, not here.** This entry used to restate the row
counts and the per-kind breakdown, and they went stale inside a day — twice.
`cargo test -p crcbl-hal` computes them: `parity_blockers()` answers what is
left, and `the_parity_blockers_are_exactly_the_reviewed_list` fails when that
set changes, so `REVIEWED_BLOCKERS` in `crates/crcbl-hal/src/capability.rs` is
always the current answer. Read it there. (The blocker total is quoted below
because the decision turns on it, and it is checked whenever it is touched.)

**Every row carries a kind.** `Divergence` has a `DivergenceKind`: `ApiAbsence`
(the API cannot express it — its reason must carry the evidence), `Unwritten`,
`Declined`, and `Unclassified` for rows that cannot be settled without hardware
nobody here has.

`parity_blockers()` is the query — a row on vk, dx12, Metal or WebGPU whose kind
is anything but `ApiAbsence` — and `crcbl-wgpu` is excluded **by construction**
rather than by a reader remembering. A snapshot test fails when that set
changes, including when a kind is widened to `ApiAbsence` to make a row vanish,
which its failure message names.

**Nine blockers: dx12 4, Metal 5, WebGPU 0.** That is what stands between here
and the deletion, and it can now be asked rather than re-derived.

**Five contradictions were settled against the installed interfaces**, not
recall — and two of them had been recorded in this file the wrong way round:
**wgpu 30 does have mesh shaders** (`EXPERIMENTAL_MESH_SHADER`,
`create_mesh_pipeline`, `draw_mesh_tasks`), so the rows saying it exposes none
were false; and **Metal can express an indirect count** through
`executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:`, which reads its
execution range from GPU memory — it has no `countBuffer:` draw, but the count
is expressible and only the encoding is missing, so that row is `Unwritten` and
the backend's "(the Metal ICB slice)" was right where this list was wrong.

**The two `Unclassified` rows are Metal's counter-sampled queries.** Whether a
device samples at all depends on which `MTLCounterSamplingPoint` values it
reports, and whether `MTLCommonCounterSetStatistic` exists depends on that
device's `counterSets`. (The seam's half of this is settled: it asks for a
sample only where an encoder opens and closes, which is what
`sampleBufferAttachments` express.) Both need a Mac. They block parity
deliberately: a guess would have read exactly like a checked classification, and
"nobody has looked" is not "done".

**Why `Features` cannot be the mechanism on its own.** It is `bitflags`, and a
bitflag has no exhaustiveness: a backend that never sets a new bit compiles
silently, which is precisely the rot. The anti-rot property needs an `enum` and
a `match` — adding a variant then fails to build everywhere it is not answered.
`Features` stays what it is (optional capabilities a _caller requests_ at device
creation); the new enum is a different thing (seam behaviours a _backend must
answer for_), and the two should not be merged.

**The mechanism that fixes it — make omission a compile error, then a test
failure.**

1. **An exhaustive capability enum in `crcbl-hal`.** Every seam behaviour that a
   backend could plausibly not have becomes a variant — not just the big
   divisions `Features` already covers (descriptor indexing, mesh shaders,
   timestamps) but the small ones that have bitten us: non-zero `fill_buffer`,
   byte-versus-word fill, a backwards timeline signal, a second acquire without
   a present, stencil reference, indirect count.
2. **Each backend answers for every variant, through an exhaustive `match`.**
   Adding a variant then **fails to compile on every backend that has not
   declared its answer** — supported, or unsupported with a reason. That is the
   anti-rot property, and Rust gives it for free; nothing has to remember.
3. **The agnostic e2e suite drives the enum, both directions.** For each
   capability: declared supported must actually work, and declared unsupported
   must refuse with the documented error. A backend that claims a capability it
   cannot serve fails; one that quietly refuses something it claims fails too.
   This is what turns "vk-only feature" into a failing test on the other three
   rather than a silence.
4. **A parity report test.** Any capability supported by some backends and not
   others must appear on an explicit, reviewed exception list with its reason.
   Divergence stays possible — Metal genuinely has no GPU-side draw count — but
   it becomes deliberate and visible instead of accidental.

**Order of work:**

1. Finish migrating the white-box tests to the agnostic suites, so all four
   backends actually run them (`lights`, `shadow`, `depth_probe` are mechanical;
   `mesh` and the sprite cluster need the golden-blessing decision first).
2. Build the capability enum and the exhaustive per-backend answer.
3. Drive the suites from it, both directions, plus the parity report.
4. Close the divergences it surfaces — starting with the ones already known:
   `fill_buffer` (narrow it to a zero fill, which is what its own doc says it is
   for), the timeline-signal and double-acquire disagreements, and the WebGPU
   commands still refused.
5. **Then delete `crcbl-wgpu` entirely** — crate, `wgpu-e2e` suite, CI job,
   registry entry, `CRCBL_GPU=wgpu`, and `wgpu` from the dependency graph.

   **Inventory, so the deletion is mechanical rather than exploratory:** only
   `crates/crcbl/Cargo.toml` actually depends on it — the `crcbl-mtl` and
   `crcbl-shaders` manifests merely _mention_ it in comments, as do
   `crcbl-golden`, `crcbl-core`'s `surface.rs` and three files in
   `crcbl-webgpu`. Its own runner is `crates/crcbl-wgpu/tests/run-wgpu-e2e.sh`,
   and all three workflows (`ci.yml`, `pages.yml`, `cron.yml`) name it. The
   workspace root's `Cargo.toml` holds the `path` entry the member manifest
   resolves through, so that is a second manifest edit and not just a comment.

   **A second CI job goes with it, and the inventory above did not name it:**
   `cross-backend image compare (vk vs wgpu)`, which runs
   `crates/crcbl/tests/run-cross-backend-e2e.sh`. Its own header calls it
   `docs/plan/12-testing.md`'s "the tier system's regression net" and the P5
   gate, P5 being "when a _second_ backend implements the seam". After the
   deletion **no two backends share a runner** — vk on Linux, dx12 on Windows,
   Metal on macOS, WebGPU in a browser — so there is no pair left to compare on
   one machine.

   **The loss is smaller than that framing suggests, and the difference is
   measured rather than argued.** Goldens are shared, not backend-keyed: the 27
   images under `crates/crcbl/tests/golden/` carry no backend in their names
   (the four matching "mesh" are geometry scenes), so every backend already
   compares against the same references on its own runner and cross-backend
   agreement survives transitively. What actually goes is the **direct** pixel
   comparison, whose bound is tighter than golden tolerance — the runner's own
   worked example is two backends differing in 84.23% of pixels at a max channel
   delta of 1, none over tolerance — together with its refusal to pass two blank
   frames, which is the check that both backends rendered at all.

   So the options are: drop the job with the crate and rely on shared goldens;
   or re-point it across runners, comparing vk's Linux frames against dx12's
   Windows frames through uploaded artifacts, which is a bigger job than the one
   being deleted and buys a comparison across two software rasterisers that
   already disagree in the last bit. Worth deciding deliberately rather than
   discovering when the job's name stops resolving.

   **The lockfile was checked and is not a hazard, which is worth recording
   because the manifests imply it might be.** `crcbl-mtl`'s comment justifies
   `objc2-quartz-core` and `objc2-core-foundation` as "already in this
   workspace's lockfile — `wgpu-hal` resolves them for `crcbl-wgpu`", which
   stops being the reason once `wgpu-hal` goes. Both are single-version
   (`0.3.2`) with three dependents — `crcbl-mtl`, `raw-window-metal` and
   `wgpu-hal` — so `raw-window-metal` keeps them resolved and no version
   unification changes. What the deletion actually owes here is a reworded
   comment, the same debt as `crcbl-shaders`' `naga` pin below, not a dependency
   fix. Note `cargo deny` runs `--all-features` in CI, so any duplicate a
   narrower local check would miss shows up there.

   **The deletion was going to cost four GPU exercises that exist nowhere else**
   — not a `DIVERGENCES` row, so nothing else would have flagged it. **Two are
   now closed.** The seam suite's raster fixture drives a padded indirect stride
   positively (the first in the tree; every other site uses the tight stride),
   and an MSAA resolve now runs on vk, wgpu, Metal and dx12 there plus on WebGPU
   through browser probe group AD — where `crcbl-wgpu`'s suite had been the only
   thing in the workspace attaching a resolve view to a real device.

   **Three are now closed.** The push-constant range came with the seam suite's
   `exercise_push_constants`, which builds a layout with a real
   `PushConstantRange` and drives it on every backend — the same piece of work
   that unblocked dx12's and Metal's rows, which is why it was worth doing
   properly rather than three times.

   **The fourth is closed, and it took a shader.**
   `crates/crcbl-shaders/shaders/bindless_probe.slang` is the first committed
   artifact in the tree to declare an array of descriptors — an unbounded
   `StructuredBuffer<uint> sources[]` at the last and highest-numbered binding —
   and `exercise_bindless_descriptor_array` in
   `crates/crcbl/tests/hal_seam_e2e.rs` binds four distinct buffers into it,
   dispatches one workgroup per descriptor and asserts each descriptor's own
   words came back in that descriptor's own block.
   `Capability:: BindlessDescriptorArray` is driven on every backend the suite
   runs, so the per-backend refusal checks that remain — `crcbl-vk`'s
   `a_bindless_capable_layout_is_accepted_or_refused_according_to_the_tier` and
   `crcbl-wgpu`'s pair — are a duplicate of a contract the agnostic suite now
   drives, not the only thing holding it.

   **`crcbl-wgpu`'s dishonest `Support::No` is fixed, by refusing rather than by
   implementing.** Its reason used to say wgpu "offer[s] no partial binding"
   while `hal_features_for` in `crcbl-wgpu/src/instance.rs` _requires_
   `PARTIALLY_BOUND_BINDING_ARRAY` before granting `DESCRIPTOR_INDEXING` — and
   `create_bind_group_layout` then built a real fixed-size array from a
   `VARIABLE_COUNT` entry, which would have scored `Worked` against a declared
   `No`. It now refuses a `VARIABLE_COUNT` layout with `HalError::Unsupported`
   and refuses `BindGroupDesc::variable_count` at `create_bind_group` with the
   same variant, which is the shape `crcbl-mtl` already used for the pair. The
   reason and the `DIVERGENCES` row say what is actually missing: a wgpu binding
   array's length is the layout's count and the length of the slice a group is
   created with, and there is no `update_bind_group` to fill a slot later. This
   was deliberately not an implementation — the crate is scheduled for deletion.
   Fallout, all of it followed through: `BindGroupLayoutSlot::variable_binding`
   is gone, `crcbl_wgpu::binding::check_variable_count` collapsed to the
   refusal, and
   `a_wgpu_variable_count_the_entries_contradict_is_refused_not_dropped` is now
   `a_wgpu_variable_count_is_refused_not_dropped`.

   **`naga` does NOT leave with it, and notes here said otherwise twice.**
   `crcbl-shaders` takes `naga` as its own **dev-dependency** (`version = "30"`,
   `wgsl-in`) to validate the generated WGSL — "a module naga rejects is a
   pipeline that fails to create". That validation matters _more_ once WebGPU is
   the only browser backend, since the WGSL it checks is what actually ships. It
   stays, and its manifest comment needs rewording: it justifies the pin as "the
   version `wgpu` already resolves", which stops being true.

**The coverage model afterwards:** the agnostic e2e suites own every behaviour
all backends owe, and bespoke per-backend tests keep only what cannot be
expressed there — validation and debug-layer wiring, API-quirk refusals, adapter
and limit enumeration. A behaviour sitting in a bespoke test that other backends
also owe is a bug in the split.

### After the WebGPU migration: features, then the sample that proves them

The standing pattern from the roadmap — build the feature, then ship the sample
that consumes it — resumes once the migration and its test coverage are done.
Built so far: breakout, asteroids, flappy, horde, hud, lumen. **The next unbuilt
sample in sequence is `docs/plan/sample/05-viewer.md`**, a glTF model viewer:
open a file, orbit it, inspect it. It is the asset pipeline's acceptance test
and the editor viewport's warm-up act.

**What it needs**, sliced so each lands on its own — one of these turned out to
be built already, which is why the list says what was checked and when:

- **V-F1 — glTF reaches the renderer. DONE, and this entry described it as
  missing.** `crcbl_scene::gltf_render` exists and does the whole conversion:
  `build_render_scene` runs `pack_page` (glTF images → `PageDesc` layers),
  `material_rows` (rows carrying those layers), `resident_meshes` and
  `place_instances`, handing back a `SceneDesc` plus its `InstanceDesc`s and a
  list of what it skipped. Nineteen unit tests, and
  `crates/crcbl/tests/gltf_e2e.rs` builds a `.glb` through the real `DirSource`,
  converts it, hands it to `ForwardRenderer::with_scene` and draws — verified on
  an RX 7900 XTX on 2026-08-19: _"an imported glTF drew its own texture on vk"_,
  via `crates/crcbl/tests/run-gltf-e2e.sh`.

  What the sample still needs from this area is narrower than a slice: the
  converter has no _app_ consumer, since `gltf_e2e` is a test. That is V-F5's
  job (opening a file) rather than a missing bridge.

  Checked at the same time, and also still open then: **V-F2** — since built, as
  `crcbl_render::orbit` — and **V-F4**, where nothing in `crates/` mentions hot
  reload.

- **V-F3 — UI for tools, not just a debug overlay.** Node/mesh tree, material
  and texture listing with sizes and triangle counts, a stats panel. Audit what
  `crcbl-ui` already offers before adding widgets.
- **V-F4 — asset hot reload. The mechanism is built; the demo is not recorded.**
  `crate::watch` plus `Gpu::reload` pick a re-export up — verified by hand on
  2026-08-19 against a real Vulkan device, swapping Khronos `Avocado.glb` for
  `Box.glb` under a running viewer, and the reload landed one poll later. What
  the milestone actually asks for is a **recording** of the Blender loop, and
  nobody has made one. It also remains the app's own poll rather than P9's
  reload: `crcbl-assets` has no reload of its own.
- **V-F5 — opening a file.** Path argument natively, drop target in the browser.

Then **V-S**, the sample itself, against the plan's own exit criteria: ≥90% of
the Khronos glTF-Sample-Models suite loads without crashing, unsupported
features log actionable skip messages, and the Blender re-export loop updates
live.

**None of those three has been measured.** The skip messages are asserted by
`app::tests`' `skip_report` cases, which is the message's _shape_ and not a
claim about any real document; the ≥90% figure needs the suite fetched and run
in bulk, which nothing does; and the Blender loop has been exercised with
`std::fs::write` and one manual `cp`, never with Blender. The suite is ~2 GB, so
the shape this wants is a script that fetches it, runs
`viewer --headless --frames 1` over every `.glb`, and reports the pass rate — a
slice of its own.

Two things the plan settles that are easy to get wrong: the viewer is the one
sanctioned exception to the server-authoritative rule (it is a tool and
simulates nothing), and it is exempt from the `.crpix` art rule, because the
whole point is that it shows _the user's_ asset unadorned.

### What the orbit camera left out

`crcbl_render::orbit` covers orbit, pan, zoom and frame-selected, and stops
there. What it does not do, and why:

- **No orthographic mode.** `OrbitCamera::new` panics on
  `Projection::Orthographic` rather than accepting one, because zoom under an
  orthographic projection is a change of `half_height` and not of distance: a
  controller that took one would move the eye and change nothing on screen,
  which is "not supported" arriving as "worked". An editor with a front/side/top
  view needs this, and the shape is a zoom that scales `half_height` while
  `frame` fits `radius * FRAME_MARGIN` into it with no distance term at all.
  Deferred because nothing has an orthographic viewport yet.
- **No app consumes it.** By design — V-F5 (opening a file) is where the viewer
  gets both its file and its camera, and wiring a controller to a window with
  nothing to look at would have been a second slice's decisions taken early.
  Nothing outside the crate has therefore exercised it against a real pointer.
- **No sensitivity, damping or inertia.** It takes raw deltas and applies them
  immediately. Smoothing is a frame-rate-dependent filter over the same deltas
  and belongs to whatever owns the frame clock, not to the arithmetic;
  `apps/lumen`'s `Flyer` keeps its own `TURN` constant for the same reason.
  Considered and declined at this slice.
- **Pan is in fractions of the viewport height, not metres or pixels.** Chosen
  so a drag tracks the pointer at any zoom and on any window size — the
  conversion needs the field of view and the distance, which is exactly the
  arithmetic that would otherwise be copied into every app. The cost is that a
  caller holding a world-space delta has to divide by the visible height itself;
  no caller does yet.
- **Framing fits the bounding _sphere_, not the box.** A box fit would frame an
  object at one orbit angle and clip it at the next, since the box's silhouette
  turns with the camera and the sphere's does not. The price is visible air
  around a flat object viewed face-on: framing a 10 × 1 × 1 m box in a 16:9
  window leaves its corners at 0.48 of the frame rather than filling it. A
  silhouette fit that re-ran on every orbit step would be tighter and is not
  written.

### Three copies of the Chromium launch and CDP plumbing

`web/tools/browser-e2e.mjs`, `web/tools/render-harness-e2e.mjs` and now
`web/tools/probe-e2e.mjs` each carry a near-identical copy of the same code:
finding a browser, the flag list, launching it, waiting on `DevToolsActivePort`,
a minimal CDP client, and `Runtime.evaluate` wrapping. The third arrived with
the standalone probe page, which followed the existing per-driver-copy pattern
rather than extracting a shared module — consolidating would have meant editing
the parity gate, which that slice had no mandate to touch.

Three copies is where drift starts, and the flag list is the part that matters:
it encodes which adapter Chromium picks and whether WebGPU is exposed at all, so
two drivers disagreeing about it means two gates testing different things while
both look green. A shared `web/tools/chromium.mjs` is the obvious shape.

**Do this together with the cross-platform work below**, not before it: that
work has to touch browser discovery and the Xvfb branch in every driver anyway,
and doing both at once means changing the plumbing in one place instead of
three, and then again three more times per platform.

### The browser lost its WebGL2 fallback with wgpu

Un-linking `crcbl-wgpu` from the wasm dropped a capability nobody has decided
about: **a browser without WebGPU now has no fallback at all.** The WebGL2 path
came from `wgpu`, which is no longer linkable there, so on such a browser the
engine has no backend to open rather than a slower one.

**Detect-and-message shipped**, which was the recommendation. `demo.js`'s `main`
answers a missing `navigator.gpu` with "This browser has no WebGPU" and the
browsers that have it, and — the case that actually happens — answers a
`requestAdapter()` that resolves to `null` with a sentence aimed at a person,
before the megabytes of wasm load. A blank canvas is now an explanation.

**What remains undecided is only the fallback itself**, and it is a real choice
rather than an oversight:

- **Accept it.** WebGPU shipped in Chrome, Edge and Firefox; Safari has it
  from 26. The floor rises on its own, and the engine now refuses gracefully.
- **A second artifact.** Build a wgpu/WebGL2 wasm alongside and pick at load
  time — keeps the fallback and the small default, at the cost of two builds,
  two toolchains (that build needs `wasm-bindgen` back) and a loader that
  chooses. Note this option **dies with `crcbl-wgpu`**: once that crate is
  deleted there is no WebGL2 path left to build, so choosing it later means
  reviving a deleted backend, not re-enabling a flag.

**Recommendation stands: accept it, and revisit only if someone reports a
browser that needs it.** Nothing in the samples requires WebGL2, and no
telemetry says anyone is on such a browser. Worth deciding before the deletion
rather than after, since the deletion forecloses the alternative.

### The browser probes cannot say why a readback failed

`Reply::ReadbackFailed` now carries a reason and `poll_readback` reports it, but
the eight readback state machines in `crates/crcbl-webgpu/src/probe.rs`
(`ReadbackProbe`, `DrawProbe` and their siblings) each end their `match` with
`_ => Self::Pending`. So a `ReadbackFailed` naming a probe's sequence still
reads as "not yet", and the probe re-polls until the gate's deadline instead of
reporting the browser's words.

Unchanged behaviour and the probe gate is green — nothing is failing today. But
it means the one harness whose job is diagnosing the seam is the one place a
failure diagnoses as a timeout. Extending the eight to absorb the new reply is
mechanical; doing it while adding a ninth probe would be cheaper than doing it
twice.

### glTF reaches the renderer — what it still cannot open

`crcbl_scene::gltf_render::build_render_scene` turns an imported document into a
`SceneDesc` and instances, proved by `crates/crcbl/tests/gltf_e2e.rs` drawing
one and asserting a colour per quadrant. What a real file can still hit:

- **`run-gltf-e2e.sh` is not in CI.** `ci.yml` runs `run-tiling-e2e.sh` on WARP,
  lavapipe and wgpu; the glTF runner needs the same four steps. Do it with the
  cross-platform work below, since both edit the same jobs.
- **`data:` URI buffers and images** — Blender's "glTF Embedded" export is
  exactly this, so the most common way an artist hands over a single file is one
  we skip. Probably the highest-value gap for the viewer.
- **Sparse accessors**, **`KHR_texture_transform`**, and **JPEG images** — no
  JPEG decoder exists in the workspace, so that one is a dependency decision
  rather than work (`crcbl-sprite` decodes PNG only).
- **The single-page limit.** Every base-colour image is resampled onto one
  square page, so a model with many large textures costs `layers x extent^2 x 4`
  on the device — a 20-texture 2048² document is roughly 335 MB. A real
  Sketchfab download will find this before anything else does.
- **Every fixture is still hand-assembled.** Nothing in the suite opens a file a
  DCC tool actually wrote, which is the sample's own acceptance test (">=90% of
  the Khronos glTF-Sample-Models suite"). Until that runs, "we import glTF"
  means "we import the glTF we generate".

### Run the WebGPU browser gates on Windows and macOS, not just Linux

~~Every WebGPU browser test in the repository runs in one job~~ — **the probe
now runs on three platforms.** What is still true, and is the live half:,
`pages/build` on `ubuntu-latest`: the five demo gates, the seam probe groups and
the golden parity harness. The browser backend is now what the samples ship on,
so its entire browser-side evidence comes from a single OS and a single Chromium
build. Windows and macOS runners already exist in `ci.yml` (`win32-e2e`,
`dx12-e2e`, `vk-e2e-windows` on `windows-latest`; `mtl-e2e` on `macos-latest`)
and none of them runs any of this.

That matters because the parts most likely to differ per platform are exactly
the parts a browser owns: which adapter Dawn picks, what
`getPreferredCanvasFormat()` returns, the limits an adapter advertises, and how
the canvas is composited. A backend that passes on Linux/SwiftShader is not
thereby proven on Windows/D3D12-backed Dawn or macOS/Metal-backed Dawn.

**Feasibility, in the order worth attempting:**

- **The golden parity harness (`web/run-render-harness-e2e.sh`) is the most
  portable and the most valuable.** It is headless, needs no Xvfb — it reads
  pixels out of wasm memory rather than snapshotting a canvas — and it is the
  only gate that compares against references. Its driver finds a browser by name
  and honours `CRCBL_CHROMIUM`, so a runner-specific path is the main work. Note
  nine of eleven scenes need more storage buffers per stage than a software
  adapter offers until the draw-args reduction lands, so on a GPU-less runner
  expect the two 2D scenes until then.
- **The demo gates (`web/run-browser-e2e.sh`) need a display on Linux via
  Xvfb**; on Windows and macOS headless Chrome should not, but the script wires
  Xvfb unconditionally for Linux and will need a per-OS branch. Both scripts are
  bash; `ci.yml` already runs bash steps on `windows-latest` (`shell: bash`), so
  that part is precedent rather than new ground.
- **The seam probe groups** ride the same driver, so they follow whichever of
  the above lands.

If a platform turns out not to support it in CI, run it locally on that machine
and say so plainly here rather than leaving the impression it is covered — a
gate that exists on one OS is coverage for one OS.

### The office PC's `VK_ERROR_OUT_OF_DEVICE_MEMORY` — wgpu only, so not worth fixing

On a dual-GPU laptop the web samples flood the console with
`vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY`, and in `breakout`
**every** allocation the frame makes fails, cascading into invalid buffers,
textures, bind groups and command buffers until nothing renders.

**It is the `wgpu` backend, not `crcbl-webgpu`.** The log says
`opened the wgpu GPU backend` / `crcbl_wgpu::errors`, and at the time it was
captured the deployed site was still the default `wgpu` build. **The site now
deploys `crcbl-webgpu`**, so the live question is whether that machine fares any
better on it — worth asking the reporter to retry. Since `crcbl-wgpu` is being
deleted at the end of the migration, the wgpu failure itself **is not being
fixed.** It is recorded only because of what it says about the machine and about
our own robustness.

**It is not an allocation-size problem, and the earlier guess that it was
`ArrayPages` allocating a large fixed texture array was wrong.** The failing
allocations are tiny — `ball staging` is 1536 bytes, `bricks staging` 4096,
`ui glyph atlas staging` 9984 — and they fail on a card reporting 14 MB used of
2.0 GB. When a 1.5 KB buffer cannot be allocated, the heap being asked for is
not the one with the free memory: the plausible causes are the dGPU (an MX550 on
a PCIe 1.0 x4 link, parked in P8) being unable to serve allocations in that
state, or Dawn selecting a device or memory type that has nothing available.
Chromium holds GPU processes on **both** the MX550 and the Intel UHD, so which
one Dawn opened is still unknown — the engine logs the adapter name as empty
(`wgpu adapter ""`, type `Other`), which is a real gap in its own right: a bug
report from a user cannot say which GPU produced it.

**What this is worth acting on:**

- **Find out whether `crcbl-webgpu` survives the same machine.** That is the
  backend we are keeping, and it is the only version of this question that
  matters. Nothing deployed uses it today, so there is nothing for the user to
  try — publishing a `webgpu`-backed variant of the site alongside the default
  one would make the new backend testable on real problem hardware instead of
  only on CI's SwiftShader and one developer's RDNA-3.
- **We degrade badly rather than failing loudly.** `crcbl-wgpu` does surface
  these through `crcbl_wgpu::errors` and `crcbl::web`'s `gpu error`, which is
  better than silence — but the engine keeps running for minutes with invalid
  textures and empty bind groups, rendering nothing. An unrecoverable device
  error at start-up should stop and say so. Worth carrying into `crcbl-webgpu`,
  where `WebGpuDevice::take_error` is still a documented `None` stub, so the
  same failure there would be entirely silent.

**Not the bug:** the `MaxListenersExceededWarning`, `ObjectMultiplex`,
`app-init-liveness` / `background-liveness` and `Extension context invalidated`
lines come from `contentscript.js` / `inpage.js` — a MetaMask content script
injected into the page. Unrelated to crcbl.

### No browser probe exercises debug markers or indirect dispatch

`insert_debug_marker`, `end_debug_label` and `dispatch_indirect` are wired and
covered by the node replayer's stubs, but **nothing drives them against a real
browser** — `web/probe/` has no group for either.

`DispatchIndirect` is the one worth a group and it is testable end to end: write
the three workgroup counts with `WriteBuffer`, dispatch off them, and read back
a storage buffer sized by those counts, so a wrong count is visible rather than
inferred. The debug ops produce no readable state and can only be seen in a
capture tool, so they may never be probe-able — say that rather than pretending
a group would prove something.

Note adding a group changes the check count `run-probe-e2e.sh` asserts (55
today), which is why the wiring slice deliberately left the probe alone.

### The sRGB encode is still unproven on dx12's and Metal's presented path

The swapchain now must offer an sRGB format and `preferred_format` must pick it,
asserted on every backend, and the clear test asserts the encoded bytes exactly.
Two gaps remain in the same bug class:

- **dx12's flip-model path is uncovered.** `buffer_format` creates linear back
  buffers and casts to an sRGB RTV — the same "the view is where the encode
  happens" mechanism that broke on WebGPU — but every GPU suite here creates
  `SurfaceTarget::Offscreen`, whose ring uses real sRGB resources and never
  exercises the cast. `crcbl-mtl`'s `CAMetalLayer` path versus its offscreen
  ring is the same shape. **No windowed GPU e2e exists on any backend** and
  there is no compositor in the test environment, so closing this needs a
  windowed harness, not another assertion.

### A hung fetch reports as "cancelled", and that is what the mystery runs were

Two runs today finished `cancelled` with **no newer push to supersede them**,
which reads like somebody pushed over the board. Neither was. Both were the
`shaders` job hanging on **"Install the pinned spirv-tools"** — one run sat
there from 07:49:47 to 08:10:05, twenty minutes, until the job's own
`timeout-minutes` fired. GitHub reports a job killed by its timeout as
_cancelled_, not _failed_, so nothing in the summary says a fetch stalled.

The cause was three unwrapped network operations in one step: a bare `curl` with
no `--max-time` and no retry, and two bare `apt-get` calls. `slangc` and `dxc`
had the same bare `curl`. All three are now bounded and retried, and the
`apt-get update` beside them is best-effort rather than chained to the install
that follows.

**This is the same argument `.github/actions/apt-packages` already makes** — "a
hang is not a failure, and never returns to be retried" — applied to the fetches
nobody had given it to. Worth checking the rest of the workflow for any other
network call without a bound; these three were found by grepping `curl` for the
absence of `--max-time`.

**And the diagnostic lesson**: a run that reads `cancelled` with nothing after
it is not noise. Three of them today were a real hang wearing the wrong label.

### The runner's apt mirror goes down, and now says so

Three jobs failed on it in one session — `x11 e2e`, `wayland e2e`, `cli e2e` —
across commits that changed only markdown. It is a GitHub runner mirror outage,
not a fault here, and re-running the failed jobs is the response.

**What was fixed is the legibility, and it is worth knowing why.**
`.github/actions/apt-packages` retries a hung mirror, and its whole argument is
that a hang should fail by name rather than look like somebody cancelled the
run. But its budget could not fit: callers cap the step at 8 or 10 minutes while
the timeouts allowed `240 + 3x360` plus backoff — twenty-three. The step was
killed mid-loop, so the later tries never ran and the failure surfaced as "The
action has timed out", which is exactly the illegible failure the retry exists
to prevent. The worst case is now `100 + 2x(40 + 100) + 30 = 410s`, and a real
outage prints
`the runner's mirror is unreachable, which is not a fault in this repository`.

**And the tightening was overdone — that is worth knowing before touching these
numbers again.** Fitting three tries inside the cap meant 100s per install, and
that turned a _slow_ mirror into a fatal one: the log showed
`mesa-vulkan-drivers [17.5 MB]` downloading and being cut off at the timeout,
three times over, on a mirror that was answering. Slow is the common failure
here, not silent. It is now one long attempt (300s) and one short retry, because
a large package needs the time and the retry is cheap —
`Keep-Downloaded- Packages` means whatever arrived is still on disk and a second
attempt resumes. Worst case 450s, inside the eight-minute cap.

**A third correction, and the same lesson twice.** The retry was written as
`update && install`, so when a slow `apt-get update` used its whole timeout the
`&&` short-circuited and **the retry install never ran** — the wayland job
failed thirty seconds after the refresh began, having attempted the packages
exactly once. The refresh is best-effort now (`|| echo …`): a stale index is a
reason to refresh, not a reason to skip the attempt.

Both this and the 100s regression before it are the same mistake — treating a
_slow_ mirror as if it were an _absent_ one. Timeouts here should be generous on
the thing that transfers bytes and never allowed to eat the attempt that
follows.

**Done, because the rate stayed high — five jobs in one session.** The `.deb`
files are cached and tried **first**, with `--no-download`, which forbids the
network outright: a job whose packages were fetched on an earlier run installs
from disk without asking the mirror at all. A miss falls through to the retry
loop, which then repopulates the cache.

Two things about the key are deliberate. It includes the package list, so jobs
wanting different sets do not fight over one entry, and the runner image,
because a `.deb` built for one release is not what another resolves to. There
are **no `restore-keys`**: a near-miss here would be the wrong package set, not
a slightly stale one.

**The image half was broken on the first attempt and the log said so.**
`${{ env.ImageOS }}` does not resolve inside a composite action, so the key read
`apt-Linux-X64--<hash>` — an empty segment, which would have restored one
release's `.deb` files onto another. It is read in a shell step now, falling
back to `/etc/os-release` when the variable is absent. Worth remembering
generally: a cache key with an empty segment fails silently and _looks_ like a
hit.

What it does not fix: the very first run after a cache eviction still needs the
mirror. The cache narrows the window rather than closing it.

### The seam audit — five places the seam is not backend agnostic

Found by auditing `crcbl-hal` against every backend's implementation, after the
rule was stated: anything on the seam that cannot work on all backends gets
refactored. Each entry below was checked in at least two backends' code. Ranked
by how badly it misleads.

**1. `StencilState::reference` — FIXED, and the exercise that missed it is the
lesson.** The field is off the seam and `set_stencil_reference` is the only
channel. What is worth keeping: `exercise_stencil_reference` **already read back
the reference that took effect** — it just bound the pipeline _first_, so a
bind-time clobber was invisible and every backend passed. One moved line makes
it catch the divergence. A guard can be present, correct in what it asserts, and
still ordered so it cannot fail.

Still unobserved: the _initial_ half of the rule — that a pass which never calls
`set_stencil_reference` draws against `INITIAL_REFERENCE`. It needs a second
pass in one command buffer with the plane cleared to that value and no call at
all, which `Raster::render` cannot express because it opens the pass itself.

Original finding, for the record:

**A silent 2-versus-2 divergence, and parity could not see it.** `crcbl-vk`
declares `STENCIL_REFERENCE` dynamic _unconditionally_ ("a pipeline that baked
it would make that call a no-op"), so the pipeline's value is dead and an
earlier `set_stencil_reference` survives a bind. `crcbl-webgpu` drops the field
in its writer, same result. But `crcbl-dx12` re-applies `OMSetStencilRef` at
every bind from `GraphicsPipelineEntry::stencil_reference`, and `crcbl-mtl`
calls `setStencilReferenceValue` the same way — both _overwriting_ what the
encoder set. So `set_stencil_reference(0x80)` followed by binding a pipeline
declaring `reference: 0` **draws with 0x80 on two backends and 0 on the other
two**, and `Capability::StencilReference` is `Support::Yes` on all four, so the
parity report is green. Verified by reading all four.

The fix follows the rule: `StencilState::reference` is a Vulkan/D3D12
static-state artefact — WebGPU and Metal have no such pipeline field — so it
comes off the seam, `set_stencil_reference` becomes the only channel, and the
seam states that a pipeline bind does not disturb it. `crcbl-render` never sets
it, so the blast radius is the backends and the tests.

**2. `MultisampleState::mask` — FIXED, removed from the seam.** Nothing in the
workspace ever set a partial mask; the only non-`!0` values were two wire-format
fixtures exercising a `u32`. Removing it also fixed a latent Vulkan bug nobody
had hit: the old code passed a **single** mask word, where the array must be
`ceil(samples / 32)` long — short at 64 samples. It is now an empty slice, which
the specification defines as all bits set. Original finding:

**A whole seam field Metal cannot honour, and nothing declares it.** Vulkan,
D3D12 and WebGPU all pass it through natively. `crcbl_mtl::pipeline`'s
`check_multisample` refuses any non-full mask outright: "Metal has no
per-pipeline sample mask … `MTLRenderPipelineDescriptor` has no counterpart at
all". There is **no `Capability` variant and no `DIVERGENCES` row** for it,
which is exactly the class `capability.rs` says every variant was derived from.
Either add the capability row, or make "every sample" the only portable value —
nothing in `crcbl-render` sets it to anything else.

**3. `Limits::max_bind_groups` with `max_push_constant_size` — ADDRESSED, and it
turned up a real bug.** The contract now says what is actually promised: each
field bounds the one quantity it names, and a descriptor respecting every field
may still be refused because three backends spend several of them from one
budget — D3D12's root cost, Metal's argument table, WebGPU's per-stage caps. The
numbers were deliberately **not** made conservative: a caller using no
descriptor tables really can spend D3D12's whole root signature on push
constants, and under-reporting would take that away to make a sentence true.

`a_pipeline_layout_at_every_reported_ceiling_is_served_or_refused_by_name` in
the seam suite now asks every backend for a layout at its own ceilings and
accepts only success or `InvalidDescriptor` — never a panic, another variant, or
a silent acceptance the API underneath rejected.

**The bug it found:** `crcbl-wgpu` had no `max_bind_groups` guard at all, so an
over-count reached `wgpu`, which files a validation error and still returns an
object — `Ok` plus a poisoned layout. Fixed.

**Still open, and it is a seam-shape call rather than a fix:** a caller cannot
ask "will this layout fit" without building it, so a renderer wanting a fallback
must construct both. A `Device::can_create_pipeline_layout` verb or a cost model
would answer it; neither was built. Also note the new wgpu guard has no standing
test of its own — the seam test only exercises its non-refusing side, and its
reachability was shown by a temporary edit rather than by a check that stays.

Original finding: `Limits` documents each field as "a hard ceiling the backend
guarantees". That is Vulkan's model. D3D12 leaves `max_bind_groups` at the floor
of 4 because the number does not exist there — `D3D12_MAX_ROOT_COST` bounds the
whole signature, and what a set spends depends on its contents — while reporting
the _entire_ root signature as `max_push_constant_size`. A layout inside both
reported limits can still be refused by `root::place`. Metal has its own shared
budget in `argument::plan`, and WebGPU caps per-stage binding counts, which the
seam has no field for at all. Vulkan is the only backend where the independence
assumption holds.

**4. `Limits::max_bindless_descriptors` — FIXED.** D3D12 now reports
`crcbl_dx12::binding::VIEW_DESCRIPTORS`, the heap it actually allocates, rather
than `D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2`. Original finding:

**D3D12 reported ~244x what it can serve.** It reports
`D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2` (a million) while
`crcbl_dx12::binding` allocates one heap of `VIEW_DESCRIPTORS = 4096`; exceeding
that is `OutOfDeviceMemory` at bind-group creation, not the `InvalidDescriptor`
the seam documents for exceeding a limit, and it depends on what else is live.
Metal's 8192 is honest and honoured. **This one is a one-line fix** in dx12's
`limits_of` and worth doing whatever else happens.

**5. `Limits::max_draw_indirect_count` — the same field means three things.** A
device fact on Vulkan, the type's ceiling on D3D12 (`u32::MAX`, argued), and a
deliberate policy budget of 8 on Metal. Honestly documented and enforced in
each, so this ranks last — but a caller sizing work off it portably will size
very differently per backend.

**Two smaller things, recorded so they are not rediscovered.**
`crcbl-hal/src/query.rs`'s "a backend without the feature returns zeros from
`query_results`" is unreachable — `create_query_set` refuses, so no caller can
hold a handle to ask with. And `Capability::OcclusionQuery` is unfalsifiable
everywhere: `CommandEncoder` has no begin/end-query verb, so nothing recorded
through this seam can write an occlusion result on any backend.

**Checked and genuinely uniform**, so not concerns: `MemoryLocation`, the
stencil read/write masks, push-constant alignment, `present_id`
non-monotonicity, `PolygonMode::Line`, `DepthClamp`, `SamplerAnisotropy`, and
the no-op present verbs (decided deliberately, with the caller handed a feature
bit to check).

### The nanosecond refactor is validated on every backend CI can run

Worth recording because the change was landed with a "dx12 and Metal are
type-checked only" caveat, and CI settled it in one run: `mtl e2e`, `dx12 e2e`,
`wgpu e2e` and the cross-backend image compare all passed on the first attempt.

The strongest single piece of evidence is dx12's
`d3d12_timestamps_advance_and_both_read_paths_report_the_same_ticks`, which
**passed on WARP**. It holds `resolve_query_set`'s GPU-side tick copy against
`query::timestamp_nanos` of the `query_results` read — so it fails both a path
reaching a different heap, range or stride _and_ a `query_results` that forgot
to convert. That is exactly the asymmetry the refactor introduced, checked on a
real device rather than reasoned about.

So the caveat is retired: the only unexercised half is Metal's, and that is
because Metal still refuses timestamp query sets entirely, not because the
conversion is unproven there.

### Metal's query rows: one of the two reasons is now retired

`Limits::timestamp_period_ns` is gone — the seam returns nanoseconds and each
backend converts. That **retires one of the two reasons** `crcbl-mtl` gave for
refusing `TimestampQuery`: reporting the flag no longer obliges a tick period
Metal cannot produce, since an implementation would correlate at read time and
owe the seam nothing it cannot measure.

What remains is real and unchanged: this backend builds **no
`MTLCounterSampleBuffer` at all**, and the CI Mac advertises **no
`MTLDevice::counterSets`** to build one from. So the rows stay, and no
capability was flipped inside the refactor — deliberately, because quietly
closing a parity row inside unrelated work is how four rows came to read closed
while running nowhere earlier today.

**What would close them**: write the counter sample buffer, then the rows become
`NotOnThisDevice` here rather than divergences — eight rows to six. It cannot be
verified on the hardware this project has, which is the whole of the remaining
argument.

One asymmetry the removal forced into the open and left there:
`resolve_query_set` copies ticks GPU-side with nothing to multiply by, so its
destination holds device units while `query_results` returns nanoseconds. Both
calls now say so. A seam that wanted symmetry would have to convert in a shader.

### The Metal ICB line of attack, and why it is closed

Kept because it cost four CI runs and the conclusion is worth not re-deriving.
`Capability::DrawIndirectCount` on Metal was attempted three times as an
**indirect command buffer** and hung the GPU every time — the same three
`render_e2e` goldens, with every encoder reporting `completed` and no API
violation in 8 MB of validation log:

| attempt | change                                                         | result           |
| ------- | -------------------------------------------------------------- | ---------------- |
| 1       | `executeCommandsInBuffer:withRange:`                           | Hang             |
| 2       | execution range read from GPU memory                           | Hang             |
| 3       | + blit `optimizeIndirectCommandBuffer` between kernel and pass | Hang, ~3x slower |

The isolated ICB probes pass on that exact device — a kernel encodes an ICB and
the ICB executes — so the mechanism works and something about a full frame does
not. **Nothing ever localised it**, which is why the branch
`try/mtl-icb-indirect-range` is kept rather than deleted: it is the reproduction
if anyone wants it.

**What shipped instead needs no ICB at all**, and that is the lesson: the
backend already issued plain indirect draws on a path that passed on this
runner, and a GPU-side count only needs the surplus draws to become no-ops.
`crcbl_mtl:: indirect_count` packs the arguments and zeroes the instance counts
past the count; the pass issues ordinary draws. It went green on the first CI
run, including the goldens that had hung three times.

Look for what the backend already does successfully before building new
machinery on top of it.

### DECISION NEEDED — dx12 mesh shading: WARP claims it and dies, hardware works

Three measurements now bracket this, and together they turn it from "our mesh
path is broken" into a question about what to report.

1. **The renderer's mesh path is correct on real hardware.**
   `CRCBL_GPU=vk crates/crcbl/tests/run-render-e2e.sh` on an AMD RX 7900 XTX
   (RADV NAVI31) passes 26/26, and
   `the_cube_scene_draws_the_same_frame_on_every_geometry_path` reports
   `cube on MeshShader against IndirectCount — 0 channel(s) differ, worst by 0`.
   So the amplification stage descending the cluster DAG, its bind groups and
   its indirect dispatch extents all produce a byte-identical frame through the
   mesh path on a real GPU.
2. **WARP reports `MeshShaderTier = TIER_1` and then loses the device** on that
   same path, with **zero debug-layer errors** — so it is not an API misuse the
   validation layer can see — and **zero DRED breadcrumbs**, so nothing names
   the operation.
3. **`crcbl-dx12`'s own mesh probe passes on that same WARP runner**, drawing
   through a mesh pipeline and an amplification stage and reading the attachment
   back. So D3D12-on-WARP can run _a_ mesh pipeline; it is the renderer's larger
   use of one that kills it.

**What is still not proven** is whose defect it is. (1) is Vulkan, not D3D12, so
nothing has run `crcbl-render`'s mesh path through `crcbl-dx12` on a
**hardware** D3D12 GPU. WARP failing where hardware succeeds is the likeliest
reading, but a dx12-specific bug that only a software rasteriser exposes fits
the evidence just as well.

So the decision:

- **(a) Withhold `MESH_SHADER` on software adapters and report it on hardware.**
  Exactly the shape `crcbl_mtl::quirk` already uses — that module's rule is that
  a quirk needs a measurement contradicting an unconditional API guarantee, and
  says what was measured, on which device, and what every other device does. It
  closes two blockers (nine rows to seven). The price: CI would report a
  capability **no CI job can ever exercise**, since every dx12 job is WARP. That
  is a real loss — it is the failure mode the parity mechanism exists to prevent
  — and it should be a deliberate choice, not a side effect.
- **(b) Keep both rows withheld** until a hardware Windows GPU can run them.
  Honest, costs two blockers that may never close on the hardware this project
  has, and matches how Metal's four unprovable rows are already treated.
- **(c) Narrow it first.** Build a smaller repro on WARP — the amplification
  dispatch alone, then with the DAG descent, then with the real bind groups —
  until one of them removes the device. That would say whose bug it is, which is
  what actually decides between (a) and (b). Costs a Windows debugging session
  measured in CI round trips rather than minutes, since nothing local can run
  it.

My reading is (c) then (a) or (b) on what it finds, because (a) taken now is
reporting a capability on the strength of a _different backend's_ evidence.

**The harness for (c) already exists**: branch
`diagnose/dx12-mesh-device-removal`, pushed and kept deliberately. It reports
`MESH_SHADER` unconditionally from `features_of` and neutralises the two
assertions that would otherwise fail the dx12 job before it reaches its "Draw a
frame through ForwardRenderer on WARP" step. Run it with
`gh workflow run ci.yml --ref diagnose/dx12-mesh-device-removal`; it is expected
red, and the job log is the deliverable. Never merge it.

The two hypotheses eliminated by reading are unchanged and still worth not
re-testing: the indirect argument size is right (`IndirectKind::DispatchMesh`
reports the 12 bytes `D3D12_DISPATCH_MESH_ARGUMENTS` wants), and the asymmetric
resource states on the mesh path are deliberate, not a bug.

### Reporting dx12 mesh shading removes the WARP device — measured, then reverted

**This was attempted and reverted, and the reason is a real defect rather than a
CI accident.** Reporting `Features::MESH_SHADER` from
`D3D12_FEATURE_DATA_D3D12_OPTIONS7::MeshShaderTier` routes `crcbl-render` onto
`GeometryPath::MeshShader` for every D3D12 adapter. On WARP the frame then never
completes:

```
the cube frame renders on MeshShader: HAL: ID3D12Resource::Map failed:
The GPU device instance has been suspended. Use GetDeviceRemovedReason to
determine the appropriate action. (0x887A0005)
```

`0x887A0005` is `DXGI_ERROR_DEVICE_REMOVED`. Four `render_e2e` tests fail that
way — `the_cube_scene_draws_the_same_frame_on_every_geometry_path`, the same for
`ao`, and both scenes' golden tests — and every one of them fails inside
`draw_and_readback`, so **the frame never renders**; no pixel is ever compared.

**The narrowing that matters:** `crcbl-dx12`'s own
`a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible`
**passes on the same WARP runner**, drawing through a mesh pipeline and an
amplification stage and reading the attachment back. So the backend's mesh
pipeline is not broken in general. What removes the device is `crcbl-render`'s
mesh path specifically — the amplification stage descending the cluster DAG, its
bind groups, and its dispatch sizes — none of which the probe exercises.

**Both of the hypotheses this entry used to list are now eliminated by reading,
and so is a third**, which matters because each would otherwise cost a Windows
session:

- **A `DispatchMesh` group count past TIER_1's ceiling.** The extents are
  `draw_gen.slang`'s: `MESH_ARG_GROUP_X` is `bucket_clusters(index)`,
  `MESH_ARG_GROUP_Y` is the surviving-instance count, `MESH_ARG_GROUP_Z` is `1`.
  D3D12 allows 65535 per dimension and 2^22 as a product; the ao and cube
  fixtures have single-digit clusters and instances. Not close.
- **A payload larger than the amplification-stage limit.** `ClusterPayload` is
  **two `uint`s — eight bytes** — against D3D12's 16 KB. The struct exists
  precisely so the mesh stage re-reads from buffers rather than copying a
  record, which is why it is this small.
- **A stale or uninitialised Y extent.** Plausible, since `GROUP_Y` is
  accumulated by atomic add and would be last frame's value if nothing reset it.
  `clear_counters.slang` does zero it every frame, and says so where it does.

So the remaining space is narrower and nastier than "some limit is exceeded":
whatever removes the device is not a count, a size, or an uninitialised word.
`crcbl-dx12`'s own mesh probe passing on the same WARP runner already said the
mesh _pipeline_ is fine, so what is left is the interaction — the amplification
stage descending the cluster DAG, its bind groups, and the indirect dispatch —
none of which the probe exercises. Confirming any of that needs a Windows
machine, which nothing here has.

**Two hypotheses have been eliminated by reading, so nobody spends a Windows
session on them:**

- **The indirect argument size is right.** `IndirectKind::DispatchMesh` reports
  12 bytes, which is `D3D12_DISPATCH_MESH_ARGUMENTS`' three `u32`s exactly. A
  wrong stride here would have produced garbage thread-group counts, which is
  the most obvious route to a removal.
- **The resource states are right, and deliberately asymmetric.** On the mesh
  path `crcbl-render`'s forward pass declares `draws.args_id` as a **shader
  read** and `draws.counts_id` as `ResourceState::IndirectArgument`, while the
  lesser path declares both as `IndirectArgument`. That looks like a bug and is
  not: the mesh path reads the draw arguments as shader data and executes its
  thread-group extents out of the _counts_ buffer, which is what
  `draw_mesh_tasks_indirect` is handed (`args: draws.counts`). The two live in
  different buffers precisely because a resource holds one state per pass, and
  the code says so where it splits them.

Also checked: the command signature passes no root signature, which is correct
for a `DISPATCH_MESH`-only layout — that argument kind writes no root argument,
so D3D12 requires null there.

**The next attempt should name the operation rather than the `HRESULT`.**
`crcbl_dx12::dred` now forces DRED auto-breadcrumbs on before the first device
is created and prints them beside `GetDeviceRemovedReason`, so a re-run of the
reverted commit on the WARP runner should say which command list stopped and on
which operation — `DISPATCHMESH` versus the `ExecuteIndirect` after it versus a
barrier is most of the narrowing above, answered from a CI log. That has **not**
been observed: see the DRED entry below for what is unverified about it.

**Why it is a revert and not a workaround.** Gating the report on the adapter
name would hide a real defect behind CI's specific device, and the flag is
either honest or it is not. `crcbl-vk` proves the paths _can_ agree: on an RX
7900 XTX the same test draws `MeshShader` against `IndirectCount` with **0
channels differing, budget 0**. dx12 must reach the same bar.

**What the attempt is worth keeping for.** The implementation, the tier+shader
model gate (`TIER_1` and SM 6.6 together, because the committed DXIL is built at
`6_6`), the `FeatureQuery` move out of `mod tests`, the `instance.rs` derivation
assertion, and the seam exercise are all written and reviewed; the revert is
`6fe2d41` and they can be recovered from it rather than rewritten. The blocking
question is only the device removal.

### DRED has now run, and WARP records nothing

Settled by a throwaway branch that re-reported `Features::MESH_SHADER` and let
one CI run reach the removal. Three of this entry's four open questions are
answered:

- **`D3D12GetDebugInterface` does answer for
  `ID3D12DeviceRemovedExtendedDataSettings` on a stock `windows-latest` runner
  with no Graphics Tools feature.** `crcbl_dx12::dred` logged "DRED
  auto-breadcrumbs and page-fault reporting are on". The module docs' argument
  for enabling it unconditionally rather than behind `CRCBL_DX12_VALIDATION`
  holds.
- **Breadcrumbs are NOT populated on WARP.** The report reads
  `DRED auto-breadcrumbs: 0 command list(s) with recorded work` on a genuinely
  removed device. This was listed here as the thing worth finding out, and the
  answer closes the avenue: DRED cannot name the failing operation on a software
  adapter. It costs nothing and stays enabled — on a hardware Windows GPU it is
  still the right tool.
- **The walk survives**: it ran against a real removed device and returned a
  report rather than faulting, though with an empty history it dereferenced
  little.

Still unknown: the `IN FLIGHT` marker's off-by-one, which needs a driver that
actually writes breadcrumbs.

**What the run also fixed.** The diagnosis never reached the caller: a readback
`Map` failure raised a bare `HalError::Backend`, and `debug::diagnosis` was
attached only to `Signal`, the fence waits and the submit paths. A `Map` is
where a removal surfaces, since it is the first call touching memory the GPU was
writing. All three `Map` sites now carry it. That is why the first two
diagnostic runs printed nothing but `0x887A0005`.

### dx12 mesh shading: the calls exist, the flag does not

`crcbl-dx12` now builds mesh pipelines and records both mesh draws — the
subobject stream, `DispatchMesh`, and an `ExecuteIndirect` of `DISPATCH_MESH`.
What is left is **reporting** `Features::MESH_SHADER` and `TASK_SHADER`, which
is deliberately a separate change because it is not a one-line flag flip:

- The read is `D3D12_FEATURE_DATA_D3D12_OPTIONS7::MeshShaderTier` from
  `crcbl_dx12::adapter`'s `features_of`. WARP measures `TIER_1`, so the software
  adapter CI runs on does support it.
- **The `FeatureQuery` impl for `D3D12_FEATURE_DATA_D3D12_OPTIONS7` is inside
  `adapter.rs`'s `mod tests`**, while the four production impls sit above it. It
  has to move up rather than be copied — a second impl of the trait for the same
  type is a coherence error, so the compiler enforces this rather than it being
  a preference.
- Reporting the flag flips `GeometryPath::from_features` to
  `GeometryPath::MeshShader` for **every** D3D12 adapter, and breaks the
  `IndirectCount` assertion in `instance.rs`.

  **There is no golden re-bless, and this entry said there was.** Checked: no
  golden is keyed on `(GeometryPath, BindingModel, LightingPath)` or on any part
  of it — `draw_scene_and_match_its_golden` takes the golden's name as a literal
  argument, the 27 files under `crates/crcbl/tests/golden/` carry no path or
  backend in their names, and nothing in `crcbl-golden` mentions `LightingPath`
  at all. What reporting the flag actually does is make
  `the_cube_scene_draws_the_same_frame_on_every_geometry_path` a _real_
  cross-path comparison on dx12 instead of a self-comparison: that test opens
  the device twice, once asking for the mesh-stage features and once with them
  subtracted, and asserts the two frames are byte-identical on one adapter. It
  already guards against the degenerate case — `best_path != lesser_path` must
  equal whether the adapter offers mesh shading, because "a self-comparison that
  reads as a cross-path one is worse than no test".

  So the requirement is that dx12's mesh path draw _exactly_ what its indirect
  path draws, which is a test that must pass rather than an image to re-bless.
  That makes this an ordinary slice, and the reason it did not ride along with
  the implementation is narrower than recorded: it is a behaviour change to
  every D3D12 adapter, and worth landing where it can be reverted on its own.

Retiring the `MeshShading` and `TaskShaderStage` dx12 divergences happens there
and not before: a row leaves on `Support::Yes`, and that answer is gated on the
flag. Their `why` strings now say the calls exist and the flag does not, rather
than claiming no stream is built.

Also still true after this work: `crates/crcbl/tests/hal_seam_e2e.rs` maps both
capabilities to `Exercise::Unexercised(NEEDS_MESH_ARTIFACTS)`, so even a
reporting dx12 would not be _driven_ until the seam suite grows a mesh exercise.

**And the ordering is now forced rather than merely tidy.**
`Capability::MeshShading` is defined as the pipeline being creatable and both
draws recordable — not as the feature flag being reported. dx12 satisfies that
definition today: it creates the pipeline, records `DispatchMesh` and the
indirect form, and its own suite draws through both on WARP. It nevertheless
answers `Support::No`, because reporting the flag re-keys the goldens. So the
seam suite's rule — a backend declaring something unsupported must refuse it —
is currently **unmet by dx12 and hidden only because the capability is
unexercised**. Writing the mesh exercise before the reporting slice would
therefore fail on dx12, and correctly. This is the same class CI twice caught as
a backend performing what it denies; it is latent rather than live because
nothing drives it.

Two honest ways out, and the second is the one the sequence already assumes:
report the flag and declare `Yes` (the slice above, with its golden re-bless),
or redefine the capability in terms of the reported feature rather than the
callable surface — which would weaken what it asserts for every backend to
accommodate one, and is worth naming only to reject.

### The graphics push-constant path is unrun on Metal and dx12

`exercise_push_constants_on_graphics` in `crates/crcbl/tests/hal_seam_e2e.rs`
closed the coverage gap this entry used to record: it draws twice in one render
pass with different `push_constant_raster.slang` blocks and asserts each draw
saw its own, with the vertex stage taking its rectangle from the block and the
fragment stage its colour. Verified on vk against real hardware (RADV Navi31),
and `Refused` on wgpu, which is that backend's answer at layout creation because
it never enables wgpu's `IMMEDIATES`.

**The Metal and dx12 arms were type-checked only** —
`--target aarch64-apple-darwin` and `--target x86_64-pc-windows-msvc` — so CI is
the first thing that runs them, and three pieces of arithmetic run there for the
first time:

- `crcbl_mtl::argument::plan` computing a block index for a layout with **no
  bind groups at all**. The index is zero, and `msl/push_constant_raster.metal`
  puts the block at `[[buffer(0)]]` in both entry points, but no push constant
  has occupied index 0 before — `push_constant_probe`'s sits behind one binding.
- `crcbl-mtl`'s **render** arm of `push_constants`, which records
  `RenderCommand::PushConstants` with a `Vec<u8>` copy and replays it as
  `setVertexBytes:`/`setFragmentBytes:`. That copy is exactly what the second
  draw's assertion is about, and nothing had ever made two draws either side of
  a `push_constants` on Metal.
- `crcbl_dx12::conv::shader_visibility` resolving `VERTEX | FRAGMENT` to
  `D3D12_SHADER_VISIBILITY_ALL`, and `push_constants` taking its
  `SetGraphicsRoot32BitConstants` branch rather than the compute one.

**`crcbl-render` still passes `push_constants: None`** at every render-pass site
it has, so the renderer never exercises the path in anger and the seam suite is
the only thing that does. Not a defect — recorded so the closed coverage gap is
not read as the engine having started using push constants.

### An unwritten timestamp query does not read back as zero

Measured while closing the render-only gap above (now closed — the exercise
times a compute pass beside the render one). With the compute pass's
`timestamp_writes` removed so queries 2 and 3 are never written, `query_results`
over the whole set came back **all four zero on vk — including the render pair
that was written**. Vulkan zeroes the entire read when any query in the range is
unavailable rather than reporting per query.

Two consequences worth keeping:

- **Zero is not a per-query sentinel.** A test priming its destination and
  checking "did this query get written" cannot rely on the others surviving. The
  timestamp exercise primes with `QUERY_POISON` _and_ checks zero, because which
  one an unwritten query keeps is the backend's business.
- **The whole-range zero is why the render branch fires first there.** The
  compute pair's own assertion is unreachable on vk for that particular break —
  it exists for the asymmetric case where a backend writes one pass kind and
  drops the other, which is exactly what `crcbl-webgpu`'s separate
  `begin_compute_pass` encoding makes possible and nothing else would catch.

### The doc gate does not cover private items

CI runs `cargo doc --workspace --all-features --no-deps` and it is green. Adding
`--document-private-items` to that same command fails with 104 diagnostic lines
on the Linux target, measured today — 20 of them in `crcbl-webgpu/src/probe.rs`,
then `crcbl-audio`'s `qoa.rs` with 3, and a long tail of one or two across
`crcbl-vk`, `crcbl-render`, `crcbl-webgpu` and `crcbl-rand`.

Not a regression and not currently a failure: these are private-item links and
sections nothing has ever checked. Worth knowing before anyone proposes turning
the flag on as a "small hardening" — it is a real cleanup, and the count is
target-dependent besides, since `crcbl-dx12`'s Windows-gated modules document
nothing on Linux and add their own diagnostics under
`--target x86_64-pc-windows-msvc`.

### What is still declared but not driven

The seam suite drives most of `Capability::ALL` with real GPU work; the tally it
prints on every run is the current answer and this entry does not restate it —
earlier versions did, and were wrong within a day.

**This is the practical ceiling, not a to-do list.** Of the four left, two wait
on work that is itself blocked (the dx12 mesh reporting slice, blocked on a
device removal only a Windows session can diagnose) and **two have no robust
observable at all** — they are not waiting on effort:

- `SamplerAnisotropy` — a conformant implementation may legally take fewer
  samples than `maxAnisotropy` asks for, so "the anisotropic image differs from
  the isotropic one" is not guaranteed even on a device that honestly reports
  the feature. A test asserting the difference would report a false
  `SilentlyIgnored`; one accepting either outcome asserts nothing.
- `BinarySemaphore` — the claim is ordering between two submissions, and
  demonstrating it requires the unordered case to actually lose a race. A test
  that depends on losing a race passes when it should fail, which is worse than
  the gap.

Both were considered and declined on those grounds rather than left undone, and
the shader toolchain being available locally (`slangc` 2026.14, `dxc` 1.9) does
not change either — the obstacle is the observable, not the artifact.

**Four remain unexercised, for three different reasons:**

- **A fixture that does not exist yet.** `SamplerAnisotropy` needs a shader that
  samples a minified texture at a grazing angle and a second to compare against;
  the committed raster artifact samples nothing.
- **An observable the seam cannot reach.** `BinarySemaphore`'s claim is ordering
  between two submissions, and on a one-queue backend a dropped binary semaphore
  is indistinguishable from an honoured one.
- **Artifacts per backend.** `MeshShading` and `TaskShaderStage` need committed
  mesh and task shaders for each backend that can run one.

**`TimelineWaitBeforeSignal` left that list** when `Device::signal_semaphore`
arrived: `exercise_timeline_wait_before_signal` submits a wait for a value
nothing on the queue will ever produce and opens it from the test thread.

**`BindlessDescriptorArray` left it** when `bindless_probe.slang` and
`exercise_bindless_descriptor_array` landed — the first committed artifact in
the tree to declare a resource array. What that leaves open is one arm nobody
here can run: see "The bindless exercise has never reached a D3D12 device"
below.

**The residual risk, written down rather than assumed away.** A backend that
accepts such a wait and then never releases it stops its queue with nothing in
any log, and `Headless::drop` calls `wait_idle` on the panic path, so the run
would wedge until nextest's `terminate-after` kills it rather than reporting.
Two things bound that: the exercise reads the counter back after its **first**
host signal and gives up before submitting anything if it did not move (proved
by red-checking a no-op `signal_semaphore` in `crcbl-vk` — the run failed and
`wait_idle` still answered `Ok`), and the only unbounded call left in the
sequence is `submit`, which no backend blocks in. What is _not_ covered is a
backend whose host signal moves the counter and whose queue-side wait never
observes it; on Metal and D3D12 that has not been run on real hardware from
here, only reasoned from the APIs.

**And two capabilities are unexercised on Metal alone**, which the tally shows
as a lower number there than elsewhere: both indirect exercises turn on which of
two argument structures a draw read, and CI's Metal device reports no
`max_draw_indirect_count` above one, so a single call can only reach the first.

**A ceiling worth not mistaking for a to-do:** `OcclusionQuery` and
`PipelineStatisticsQuery` can never be driven further than set creation by
anything a caller records, because `CommandEncoder` has no begin/end query verb.
Their `Yes` means a set can be made and read, which is what Vulkan's means too.

### The bindless exercise has never reached a D3D12 device

`exercise_bindless_descriptor_array` and `bindless_probe.slang` landed
2026-08-19 and were run on `CRCBL_GPU=vk` (RADV Navi31, Mesa 26.1.7) and
`CRCBL_GPU=wgpu` here. **The dx12 arm has run nowhere.** `crcbl-dx12` answers
`gated(DESCRIPTOR_INDEXING)` off `RawCaps::dynamic_resources`, and WARP has been
measured at binding tier 3 with SM 6.6, so on CI it will declare `Yes` and must
actually work. If it does not, that is a finding about the backend's bindless
path, not a reason to weaken the exercise.

**What to look at first if it goes red**, in order of how likely each is:

- **The register space.** Slang moves an _unbounded_ array into a register space
  of its own: without an explicit annotation `dxc -dumpbin` reported this
  binding at `t0,space1`, and `crcbl_dx12::binding::ranges` builds every
  descriptor range with `RegisterSpace: 0`. The source therefore pins it with
  `: register(t0, space0)` — the only `register` annotation in
  `crates/crcbl-shaders/shaders/`, documented at the declaration. `crcbl-dx12`'s
  `dxil` module asserts space 0 over every committed container and now lists
  `bindless_probe` in `registers_are_assigned_per_class_in_declaration_order`,
  but that test is Windows-only and has never executed. The `-dumpbin` reading
  is the only evidence the annotation worked.
- **The unbounded range's placement in the table.** The layout's two entries are
  a UAV (`destination`, binding 0) and an SRV (`sources`, binding 1) in one view
  table; `plan_layout` gives an unbounded range `count: 0` for the table-offset
  arithmetic and `declared: u32::MAX` for the root signature. D3D12 wants an
  unbounded range last in its table, which it is here — unverified.
- **The heap size.** `allocate_group` sizes the group from
  `BindGroupDesc::variable_count`, which this exercise sets to `SOURCE_COUNT`
  (4) against a declared ceiling of `BINDLESS_CEILING` (5), so five is what the
  root signature declares and four is what is allocated.

**Also unrun anywhere:** the Metal and WebGPU arms. Both declare `Support::No`
and refuse at `create_bind_group_layout`, so what CI proves for them is the
refusal, not the array.

### `exercise_push_constants_on_compute`'s RADV note is stale

Its doc comment still carries a section headed "The shader indexes its block
with a divergent index, and that does not work on RADV", saying the exercise is
"honest about a real defect" and that `push_constant_probe.slang` reads
`constants.values[index]` through a pointer. The shader has since been changed
to the `switch (index)` over `values.x`/`.y`/`.z`/`.w` that very paragraph
prescribes, and the exercise passes on this machine's RADV Navi31. The panic
message in `push_constant_dispatch` points back at the same stale claim ("which
is the driver reading the block with a divergent index and is not this backend's
doing").

Not fixed here because it is unrelated to the slice that found it. The
measurement itself is worth keeping — it is the reason the shader is written the
way it is, and `push_constant_probe.slang`'s own comment records it correctly —
so the fix is to rewrite the section as history rather than delete it.

### The seam cannot use occlusion or pipeline-statistics queries at all

Exercising the query capabilities turned up three defects in the query seam
itself. The declarations are honest — they claim only that `create_query_set`
works — but nothing a caller records can ever write to those pools.

- **There is no begin/end query verb.** `crcbl::hal::CommandEncoder`'s entire
  query vocabulary is `reset_query_set`, `resolve_query_set` and the pass
  descriptor's `timestamp_writes`. Every API scopes occlusion and statistics
  with a begin/end pair around a draw — `vkCmdBeginQuery`, D3D12 `BeginQuery`,
  Metal `setVisibilityResultMode:`, WebGPU `beginOcclusionQuery` — and the seam
  exposes none of them. So an `OcclusionQuery: Yes` means "a set can be
  created", and nothing more.
- **`query_results` and `resolve_query_set` assume one `u64` per query, which is
  wrong for pipeline statistics.** `crcbl-vk` enables three counters
  (`VERTEX_SHADER_INVOCATIONS | FRAGMENT_SHADER_INVOCATIONS | CLIPPING_PRIMITIVES`),
  so a Vulkan statistics pool is **24 bytes per query**, while `query_results`
  passes `out.len() * 8` and `resolve_query_set` strides by `size_of::<u64>()`.
  The validation layer says so directly:
  `VUID-vkGetQueryPoolResults-dataSize-00817: specified dataSize 16 which is less than 32`.
  **No `out` length satisfies both Vulkan and the seam's own
  `first_query + out.len() <= count` bound**, so a statistics pool cannot be
  read through this seam at all.

  **Confirmed independently on D3D12**, which makes it a seam defect rather than
  a Vulkan quirk: `D3D12_QUERY_DATA_PIPELINE_STATISTICS` is a fixed **88-byte**
  struct with no selection, so `crcbl-dx12` refuses `query_results` for a
  statistics set by name and says why. Two APIs, two different widths, neither
  of them one `u64`. The fix is a result width the seam carries — which is what
  the settled design above calls `QuerySetLayout::values_per_query`.

- **`resolve_query_set` sets `WAIT` unconditionally**, so resolving a query that
  was reset and never written blocks for ever rather than returning. That is a
  hang a caller can reach without doing anything unusual, and the seam method
  says nothing about it.

**The design is settled** (2026-08-18, read out of the resolved `ash`,
`windows`, `objc2-metal` and `wgpu-core` sources rather than recalled). What it
decided, and why each is not the obvious answer:

- **`WAIT` is not the defect — `WAIT` over a range this command stream did not
  write is.** Over a written range the wait is what makes results final and is
  bounded by work already recorded. `wgpu-core` solves it by tracking written
  slots per encoder and splitting a resolve into runs: written runs copy with
  `WAIT`, unwritten runs get a **zero fill**, so an unwritten query resolves to
  a defined value rather than stale bytes or a stall. D3D12 gets this free —
  `ResolveQueryData` has no wait parameter at all.
- **The stride comes off the set, not off a second declaration.** A new
  `Device::query_set_layout` answers `values_per_query` for the set the device
  actually built, and it is the only number the read and resolve paths may size
  from. `crcbl-vk` cannot keep using ash's `get_query_pool_results` helper: it
  derives `queryCount` from `data.len()` and the stride from `size_of::<T>()`,
  which _is_ the defect. The `statistics` a layout reports may be a **superset**
  of what was asked — D3D12's struct is a fixed 11 fields and Metal's a fixed 8,
  neither selectable.
- **`query_results` returns `Pending`/`Ready` and leaves `out` untouched**,
  mirroring `ReadbackState`. It replaces a zero-fill-on-`NOT_READY` that made
  "two zeros" mean both "unsupported" and "a wrong `Support::Yes`".
- **The narrowest backend picks the shape.** WebGPU cannot name the set at the
  begin call, so the set is named on `RenderPassDesc` — which is Metal's shape
  too (`visibilityResultBuffer` on the pass descriptor). Vulkan and D3D12 serve
  that trivially; the reverse is not true. Two verb pairs, not one generic
  `begin_query`, because occlusion is render-pass-only with the set on the pass
  and statistics is legal in either pass kind with the set at the call.
- **Two new capabilities, both from real refusals:** `PreciseOcclusionCount`
  (WebGPU's occlusion result is documented as 0/1, not a sample count) and
  `PipelineStatisticsSelection` (D3D12 and Metal hand back fixed structs).
  **Deliberately no capability for the wait** — a backend that reintroduces it
  would answer `Yes` and then hang, so the refusal channel cannot express it.
  That is what the test below is for.

**How a hang is caught by a test rather than by a user**, which is the part
worth reading twice: a hang does not fail, it never returns. The test converts
it into a value by never using an unbounded wait — resolve an unwritten range,
submit signalling a timeline semaphore, then `wait_semaphores` with a deadline,
which the seam already promises answers `Ok(false)` on timeout rather than
erroring. A wedged queue becomes `Ok(false)` in bounded time. **And the fixture
must be `mem::forget`ed before the failing assertion**: `Headless::drop` calls
`wait_idle` while panicking, which against a wedged queue hangs a second time
and destroys the output the `Drop` exists to produce. Without that line the test
detects the hang and then hangs. Backends with no timeline (dx12, WebGPU) get
the weaker form — a watchdog thread that aborts the process with a named message
— and they refuse query sets today anyway.

**Slice order** (1 and 2 are independent of the capability and test agents; 5
and 6 must sequence after them):

1. The types — `PipelineStatistics`, `QuerySetLayout`, `QueryState`,
   `query_set_layout`, `query_results` taking a `Range<u32>`. No behaviour
   change; `crcbl-render`'s `timing.rs` follows the signature.
2. **The hang fix** — written-slot tracking, run splitting, the correct stride
   in both read paths. Its layer-1 test is a pure function
   (`resolve_flags(Unwritten)` must not contain `WAIT`), which turns a
   wall-clock event into a value comparison that runs with no driver.
3. `RenderPassDesc::occlusion_queries` plus the occlusion verbs — mechanical
   across the `RenderPassDesc` construction sites, and the compiler finds them.
4. The statistics verbs.
5. The two capabilities and their `DIVERGENCES` rows.
6. The exercises and the hang test.

**One exercise is honestly out of reach here:** a non-zero occlusion count needs
a raster pipeline the seam suite does not have. What the suite _can_ prove is
that the pool was written — a zero-sample query is still a written query, so
zero-and-not-poison separates it from silently ignored — and the count assertion
belongs in `render_e2e`, which has the fixture. Statistics, by contrast, is
fully drivable there today: begin/end around the existing compute probe and
assert the invocation count equals `workgroups * workgroup_size`. That is why
the five-bit statistics set must include `COMPUTE_SHADER_INVOCATIONS`, which the
current hard-coded three do not.

**A separate defect found and not folded in:** `crcbl-vk` maps
`Features::OCCLUSION_QUERY` to Vulkan's `occlusionQueryPrecise`. Occlusion
queries are core in Vulkan and only the _precise count_ is optional, so a device
without it is currently reported as having no occlusion queries at all.

### `vk_e2e/mesh.rs` is a redesign, not a move — and the decision is taken

The white-box migration is otherwise finished: the seam suite (16),
`draw_gen_e2e` (12), `forward_e2e` (13) and `sprite_e2e` (12) all run on vk,
dx12, Metal and WebGPU in CI, and `crcbl-vk`'s own suite is down from 92 tests
to 55. The nine sprite goldens moved without a single re-bless — every one
matches on wgpu with zero differing pixels.

**`mesh.rs` (3180 lines, 15 tests, 4 goldens) cannot be moved as it stands.** At
least seven of its tests open the device demanding
`Features::MESH_SHADER | Features::TASK_SHADER` and then assert that
`GeometryPath::MeshShader` was the path selected — one failure message even says
"radv and lavapipe both report VK_EXT_mesh_shader". That path does not exist on
wgpu, WARP or Metal, so those tests assert a Vulkan fact, not a seam fact.

**Decision, taken rather than deferred: split each affected test in two.** The
claim about what the cluster DAG selected — which levels, which clusters, how
many survived — is backend-agnostic and belongs in the shared suite. The claim
that the _mesh-shader path_ produced it is Vulkan-specific and stays in
`vk_e2e`, alongside the existing mesh-shader capability test. Two tests where
there was one, each asserting something true everywhere it runs. The alternative
— moving them wholesale and gating the assertions on `Capability::MeshShading` —
would leave three backends running a test whose substance is skipped, which is
the shape this project keeps removing.

**A blocker to clear first:** `vk_e2e/draw_gen.rs` and `vk_e2e/queries.rs` still
import
`crate::mesh::{MESH_EXTENT, mesh_camera, place, place_cube, render_mesh}`, so
those helpers must be extracted within `vk_e2e` before `mesh.rs` can leave.

**And a measurement worth having before the goldens move:** the mesh goldens are
the loosest in the tree already — `mesh_clusters` reports 99.84% of pixels
differing on radv today, with a max channel delta of 6 and 0.66% over tolerance.
It passes, but with far less headroom than any sprite golden, so a cross-backend
move needs the numbers measured on each backend rather than assumed.

### Three capabilities are still declared `Yes` and exercised by nothing

The mechanism proves every backend _answers_ for every capability. It does not
prove the answers are true, and an audit found seven carrying a `Yes` on a
keeper backend with no test anywhere driving them. **Four have since been
closed** — `MsaaResolveAttachment`, `IndirectArgumentPaddedStride`,
`UpdateBindGroup` and `StencilReference` are all driven now, three of them by
exercises written specifically because the claim rested on nothing.

**Two survive**, and they are the ones a wrong `Yes` would reach a user through.
`StorageImageBinding` was the third and is now driven by
`exercise_storage_image_binding` in `tests/hal_seam_e2e.rs`, on every backend
and in both directions:

- **`SamplerAnisotropy`** — answered from a device flag and never set against a
  device. The one anisotropy test asserts a _limit refusal_, not a working
  anisotropic sampler.
- **`BinarySemaphore` on WebGPU** — self-declared unverifiable in the backend
  itself: `acquire_next_frame` answers `None` for both kinds, so nothing
  observes it.

### Four mesh tests stayed on Vulkan, and one degrades quietly

The mesh cluster split; what stayed did so for a reason worth keeping written
down, because "why is this still vk-only?" is the question a future reader asks.

**`per_cluster_culling_rejects_the_clusters_a_camera_hides`,
`a_scaled_instance_keeps_the_clusters_a_camera_can_see`,
`the_gpu_descends_the_dag_to_the_cut_the_camera_asks_for` and
`the_shadow_cascades_select_coarser_levels_than_the_camera_does` are not
splittable.** Each reads a buffer only the amplification stage writes —
`CLUSTER_SURVIVOR_WORD`, `cluster_selection`, `shadow_selection`. On a backend
with no mesh-shader path those words are never written, so the agnostic half
would be a counter nobody incremented: a test that passes because nothing ran.

**`the_mesh_dispatch_extent_is_the_culled_instance_count`** stayed for the
opposite reason — its agnostic half already exists as `draw_gen_e2e`'s
`a_bucket_fills_and_empties_as_its_instance_comes_and_goes`, and splitting would
have duplicated a test that already runs on all four backends.

**One live gap:**
`the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is` still degrades
when a device reports no `TASK_SHADER` — it now says so loudly on stderr and
refuses a _partial_ comparison rather than silently comparing less, but it does
not fail. Whether lavapipe reports the feature could not be checked from this
machine, so the choice was to make the degrade visible rather than to harden a
condition nobody has observed. If CI's lavapipe arm never prints that line, the
degrade path is dead and the test should simply require the feature.

### The Windows probe gate has no adapter; macOS is proven

**macOS works, first run, exactly as predicted.** `probe-macos` ran headless on
`macos-15` against a real Apple adapter — Chrome 150.0.7871.187, adapter mode
`hardware` resolved automatically on darwin, **57/57 over groups `G…AA`**. So
headless Chrome really does close the WebGPU canvas readback gap on macOS, and
that is the first proof of `crcbl-webgpu` on Metal-backed Dawn. Take
`continue-on-error` off, having served three green runs.

**Windows has no WebGPU adapter in the default mode.** The job found Chrome
151.0.7922.109 exactly where the registry said, resolved `hardware` on win32,
and died at `requestAdapter() returned no adapter — no GPU to drive`. So a
GPU-less `windows-latest` exposes nothing through D3D, and that mode gates
nothing. The runner reported it correctly — "the driver reported no checks — the
gate is not gating" — rather than passing on zero.

**The remaining route is SwiftShader, now being measured.** It is the one this
file warned against, and the warning is still true: it moves Dawn to SwiftShader
while Chromium's shared-image device stays on D3D11, so a canvas handed between
them reads back as uninitialised memory. But that is a _canvas_ fault, and most
of the probe is not a canvas — G through W and AA drive the command stream
against textures the replayer owns. **Expected: real seam coverage with X, Y and
Z failing.** If that is what comes back, the honest end state is a Windows job
that runs the groups Windows can serve and says which, not one that pretends to
run them all.

**Also seen and not chased:** `actions/download-artifact` logged
`digest-mismatch: error` on the Windows job while still extracting the site. It
did not cause the failure — the probe page loaded and asked for an adapter — but
nobody has looked at why the digest disagrees on that runner and not on macOS.

`pages.yml` runs the seam probe on `macos-15` (headless, against real Metal) and
`windows-latest` (headless, SwiftShader, with four groups expected to fail).

**Both jobs now gate.** macOS came off `continue-on-error` after three green
runs at 57/57. Windows came off once it was taught its four expected failures,
and passes at 53/57 with them excused — and a listed group that _passes_ fails
the run as stale, so the list cannot rot into a blanket suppression.

**What the first runs answer**, and the line to read for each:

- **macOS.** `browser: /Applications/Google Chrome.app/…` — absent means the
  image assumption is wrong rather than the gate. Then
  `probe e2e: adapter mode "hardware" (auto on darwin)`, then the `groups` line.
  `G…AA` and 57/57 means headless macOS really does close the canvas readback
  gap. A missing `X Y Z` means it does not, and macOS needs a headed session
  too.
- **Windows.** The open question is whether a GPU-less runner exposes any WebGPU
  adapter at all. No group letters means it does not; `X Y Z AA` missing while
  everything else passes means headed did not close the readback gap either, and
  Windows cannot host this gate.

**Coverage gap in what landed:** `render-harness-e2e.mjs` was moved onto the
shared launcher but its _launch_ path was never executed — only its startup
error path. Its imports resolve at link time and its call shape matches the two
that were exercised, but that is reasoning, not a run. It also remains wired
into no workflow at all.

**One behaviour change on Linux:** `--enable-unsafe-webgpu` is now passed in
hardware mode as well as swiftshader. Verified on real hardware here (57/57 and
43/43); unverified anywhere else.

### dx12's remaining blockers, ordered — and two reasons that were wrong

dx12's remaining rows are none of them API absences — its own `supports` comment
says so. Planned 2026-08-18 against the `windows` 0.62.2 bindings and
`wgpu-hal`'s dx12 backend on disk. **Two of the divergence list's own stated
reasons did not survive reading the code**, and both change what the work is:

- **The shader-visible descriptor heap already exists.** The three `Declined`
  fill rows say the obstacle is a heap `crcbl_dx12::descriptor` does not create.
  True of `descriptor.rs`, whose docs say the flag is "deliberately never set" —
  but `binding.rs` creates shader-visible CBV/SRV/UAV and sampler heaps lazily
  and hands them to `SetDescriptorHeaps`, and has since the bind-group slice.
  There is no descriptor-heap prerequisite to schedule.
- **No committed DXIL has a push-constant block.** The `PushConstants` row says
  nothing knows which root slot the committed DXIL puts one at. No `.slang` in
  the repo declares one — `ui.slang`'s was replaced by a bound uniform buffer in
  2026-08 — and every `push_constants:` field in `crcbl-render` is `None`. The
  obstacle is a missing test artifact, not a missing fact about a shader.

**The one real shared prerequisite is about five lines:** adapter caps are
computed before any device or queue exists, and `Features::TIMESTAMP_QUERY` is
withheld because the timestamp period needs `GetTimestampFrequency`, which needs
a queue. Let `Dx12Device::open` amend caps after the queue it already creates,
and move the queue-free flags into `features_of`. Both the sync and query slices
need it.

**The order, and why:**

1. ~~Depth copy~~ — **landed.** It also found a real defect nothing else would
   have: the row pitch came from the format's block size rather than the plane's
   texel size, so every copy of a multi-plane depth format read the buffer at
   the wrong stride.
2. ~~Sync~~ — **landed**, four rows in one slice. It also implemented
   `ReadbackDesc::after`, whose refusal reason ("`create_semaphore` refuses")
   the same change made false.
3. ~~Queries~~ — **landed.** The hidden-resolve-buffer shape it needed is in the
   commit; a statistics set still refuses `query_results`, which is the seam
   defect below rather than a backend gap.
4. ~~MSAA resolve~~ — **landed.** The trap was this backend's own: a `D2`
   render-target view addresses layer zero alone, so reading the layer count
   would have planned a resolve per layer for a descriptor covering one.
5. **Push constants.** Small implementation; the evidence costs a new committed
   shader artifact, since nothing in the engine would consume one. WGSL cannot
   carry it, so the artifact's target list excludes wgsl and both wgpu backends
   stay refused.
6. **Mesh, and only after a measurement.** Largest, riskiest, and the only one
   whose provability is unknown: `crcbl-dx12` never calls `CheckFeatureSupport`
   for `OPTIONS7`, so it reports no mesh support **by construction rather than
   by measurement**. Worse, it would be _silently_ unprovable — `MeshShading` is
   feature-gated — though a retirement now requires the backend to answer
   `Support::Yes`, so the row cannot be deleted on nobody's evidence any more.
   And reporting the flag flips `GeometryPath` to `MeshShader`, which re-keys
   every dx12 golden in four suites. Split it: probe-and-log first, then the PSO
   stream with the flag still withheld, then the flag and the re-bless only once
   a mesh frame has actually been drawn.

**Every slice must delete its rows from both `DIVERGENCES` and
`REVIEWED_BLOCKERS`** or the snapshot test fails — that is the mechanism working
— and must shrink the dx12-local test asserting every remaining refusal names
itself.

### Metal's five blockers, ordered — and a hole in the parity mechanism

Planned 2026-08-18 against `objc2-metal 0.3.2` and `wgpu-hal 30.0.0` on disk.

**The mechanism hole this plan found has since shipped.** Eight of Metal's nine
rows sit behind a `Features` flag `crcbl-mtl` reports on no device, and the old
rule excused a refusal whenever the device withheld the gate — so a row could be
retired by a device that could not have proved anything either way. `Support`
now separates the backend's refusal from the device's, and a plain `Support::No`
needs a listed row on **every** device, so a slice can retire its row only by
making the backend answer `Support::Yes`.

Three of those flags change what the renderer _builds_: `MESH_SHADER` and
`DRAW_INDIRECT_COUNT` both move `GeometryPath`, and `DESCRIPTOR_INDEXING` moves
`BindingModel`. The other five move nothing, which makes them the cheap ones.

**The order:**

1. **A counter probe that closes no rows and unblocks three.** One `#[ignore]`d
   test on the existing Metal CI job printing `supportsFamily` for the three
   families mesh shaders need, all five `supportsCounterSampling:` answers,
   every `counterSets()` name, and a `sampleTimestamps:gpuTimestamp:`
   correlation across a sleep. That last one is the measured
   `timestamp_period_ns` the adapter currently says Metal has no way to report —
   and `wgpu-hal` fabricates it as 1.0 on Apple silicon and 83.333 on Intel,
   calling it "the dangerous but easy thing" in its own comment. We do not have
   to guess. Needs one manifest line: `MTLCounters`, which the manifest
   currently calls deliberately absent.
2. **`PushConstants`** — small, moves no golden, and its unknown is measurable
   _here_ rather than on CI (see the corrections below).
3. **`TimestampQuery`** — the exercise already exists and only has to flip.
   Metal is _easier_ than D3D12 here: `resolveCounterRange:` reads on the CPU
   with no submit, where D3D12 needs a hidden resolve buffer and its own list.
4. **`OcclusionQuery`** — downstream of the seam's query redesign, which was
   already shaped by Metal's `visibilityResultBuffer` being pass-level. Metal's
   own part is small, and one decision is its own: use
   `MTLVisibilityResultMode::Counting`, not the `Boolean` wgpu-hal picks for
   WebGPU's sake, since the design's `PreciseOcclusionCount` capability exists
   exactly to let Metal answer `Yes` there.
5. **`PipelineStatisticsQuery`** — only if the probe finds the statistic counter
   set. This is the one slice with **no upstream to read**: `wgpu-hal`'s Metal
   `create_query_set` is `todo!()` for it.
6. **`DrawIndirectCount`** — largest non-mesh slice and likely a seam change
   (see corrections). `wgpu-hal`'s Metal version is an empty `//TODO`.
7. **`MeshShading` + `TaskShaderStage`** — last, splits in four, and re-keys
   Metal's goldens twice. The MSL is already committed and `xcrun metal` already
   compiles it in CI.

   **A measurement in flight may rule this out on CI entirely.** `wgpu-hal`'s
   mesh gate is not the three-family AND this plan first recorded — it is
   `family_check && (Metal3 || Apple7 || Mac2) && !is_virtual`, where
   `is_virtual` is the device name containing "virtual". CI's runner reports
   **`Apple Paravirtual device`**, so by that formula mesh pipelines are off the
   table on this runner whatever the families answer. The counter probe prints
   all four terms, so the next Metal CI run settles it. If it holds, the honest
   outcome is that Metal's mesh rows cannot be proved by this CI at all and need
   either a hardware runner or an explicit "unproven here" state — not a
   retirement.

8. **`BindlessDescriptorArray`** — last or `Declined`. The blocker is in
   `crcbl-shaders`, not `crcbl-mtl`, and closing it reopens the backend's
   barrier model.

### Five Metal divergence reasons that are wrong

Same exercise as the dx12 pass, and it found more. Two of these mislead about
_who owns the work_, which is what makes them expensive rather than merely
untidy.

1. ~~`PushConstants`~~ — **closed.** The obstacle never existed, and it was
   written down three times. Measured from the artifact the block lands at
   `buffer(1)`, _behind_ the bound buffer, so nothing shifted and the slice was
   smaller than its row implied. Left here as the record of what the wrong
   reason cost: three planning passes took it at face value.

   The original wording, for reference: The row, `MetalDevice::supports`,
   `features_of`'s docs and the pipeline-layout refusal all say the committed
   MSL puts a push-constant block at `buffer(0)`, ahead of every bound buffer.
   **No committed MSL declares a push-constant block at all** — `msl/ui.metal`
   has `vertices` at `buffer(0)` and `constants` at `buffer(1)`, and
   `binding.rs`'s own module docs say so, so two files in one crate contradict
   each other. This is the **same 2026-08 shader change** that made dx12's
   push-constant reason wrong: the block was replaced by a uniform buffer
   because WGSL cannot carry one. The obstacle is a missing test artifact — and
   it is the _same_ artifact dx12 needs, so build it once.

2. **`TimestampQuery`: the dependency it named is gone, and the row is not.**
   The row used to turn on whether a timestamp could be taken at an arbitrary
   point. The seam no longer asks for one: `PassTimestampWrites` names two
   queries in the pass descriptor and the backend samples where the pass opens
   and closes, which is exactly what `MTLRenderPassDescriptor`'s
   `sampleBufferAttachments` express. `Unclassified` is still the right kind —
   whether _this device_ advertises a counter set, and what its clock's period
   is, both need a Mac — but the placement question is settled.
3. **Mesh and task: the rows say the work is in the backend; part of it is in
   the seam.** `MeshPipelineDesc` carries no object- or mesh-stage threadgroup
   sizes, and `drawMeshThreadgroups:` requires both. That is the identical gap
   `ComputePipelineDesc::workgroup_size` was added to fill, argued in those
   exact terms — `wgpu-hal` escapes it by reflecting naga; this project commits
   pre-generated MSL and cannot. **`crcbl-mtl` cannot close those rows alone.**
4. **`DrawIndirectCount`: the shared constant omits what the backend states
   twice.** It describes the work as a compute kernel encoding into an
   `MTLIndirectCommandBuffer`. True and insufficient: that kernel must be
   dispatched **before the render encoder was opened**, and the seam calls
   `draw_indirect_count` inside the pass. The constant exists precisely so the
   list and the backend cannot drift, and they have drifted the other way — the
   backend now carries a material fact the shared sentence does not. It changes
   the estimate from "encode an ICB" to "split the render encoder, or change the
   seam".
5. **`BindlessDescriptorArray`: true reason, wrong owner.** It reads as backend
   work. The gating fact is that an argument buffer bound where a shader
   declared vertex data _silently reads descriptor words as vertices_, so it
   needs `crcbl-shaders` to emit different MSL. A second unstated cost: the
   backend's barrier model rests on every resource being automatically tracked,
   which argument buffers break.

**Two smaller drifts, noted:** `crcbl-mtl`'s manifest still says the
`macos-latest` runner hangs the command buffer, which CI's own comment
supersedes — macos-26 executes correctly and the hang was
`setDepthStencilState:nil`.

### WebGPU has no blockers left

Planned 2026-08-18 and closed the same day. `crcbl-webgpu`'s `TimestampQuery`
row is gone, `StorageImageBinding`'s went before it, and `parity_blockers()` now
names only `crcbl-dx12` and `crcbl-mtl`: every remaining WebGPU refusal is
WebGPU itself refusing, which is an `ApiAbsence` and not a blocker.

**The landmine this section warned about did not go off, and it is worth
recording why**, because the shape recurs. The warning was that the moment
`create_query_set` accepts `QueryKind::Timestamp`, `PassTimers::new` stops
returning `None` and the engine records query verbs every frame — and a verb
that `record_unsupported`s takes down the whole command buffer at `finish()`.
What kept it shut was gating **both** on one flag: `create_query_set` refuses
the timestamp kind on a device that opened without the browser's
`timestamp-query`, and `PassTimers::new` gates on the same
`Features::TIMESTAMP_QUERY`. A browser without the feature therefore builds no
timers and records no query verbs; one with it records verbs that are all wired.
The demo gate ran green with the hud demo creating a timestamp set and timing
every pass.

### What the pass-boundary timestamp slice left open

Written 2026-08-18, when `PassTimestampWrites` replaced
`CommandEncoder::write_timestamp`.

- **A copy pass is no longer timed, and nothing replaces it.** `PassKind::Copy`
  opens no scope, so it has no boundary for a backend to sample at and
  `PassTimers` gives it no query pair and no row. In a forward frame that is
  `cull-stats-readback`, whose cost now appears nowhere. Timing it would need a
  verb the seam does not have — Vulkan and D3D12 could take a timestamp either
  side of the copy, Metal and WebGPU could not — so the options are to leave it
  untimed (what happened), to give the graph a `PassKind::Copy`-only timing path
  that only two backends implement, or to make the copy a compute pass. Nobody
  has asked for it; recorded so the missing row is a decision rather than a
  surprise.
- **The report is the pass alone, where it used to include the pass's
  barriers.** `PassTimers` used to bracket each pass from before its barrier
  batch to after its close, and the module doc argued that was the more useful
  number. It is no longer available: a pass boundary is a pass boundary. What a
  profiler now cannot see is a pass whose transitions cost more than its draws.
- **`crcbl-mtl` and `crcbl-wgpu` refuse a pass carrying timestamps rather than
  accepting and dropping it.** Neither can create a timestamp set, so any set
  such a pass could name is dead or of another kind, and the seam's
  degrade-rather-than-break rule is discharged one level up: `PassTimers::new`
  gates on `Features::TIMESTAMP_QUERY` and builds nothing on either backend, so
  the refusal is unreachable from the engine. If a caller ever hand-rolls a
  timed pass without checking the flag, it will get a loud `HalError` rather
  than silent zeros — which is the intended direction, but it is a behaviour
  change from "accepted and dropped" and is written down here rather than only
  in the code.
- **The browser gate asserts `end >= start` and not `end > start`.** Chromium
  quantises timestamp-query results (100 µs at the time of writing) unless
  started with `--disable-dawn-features=timestamp_quantization`, so probe group
  AF's empty pass can legitimately open and close inside one quantum. The native
  suite times a pass that clears a megabyte and asserts the strict form there.
  Adding the flag to `web/tools/browser-launch.mjs` would let group AF assert
  the strict form too; it was not added because it changes every browser gate's
  launch for one check's benefit, and was not measured.
- **`crcbl-dx12`'s and `crcbl-mtl`'s halves are type-checked and not run.** Both
  were cross-compiled with `-D warnings` for `x86_64-pc-windows-msvc` and
  `aarch64-apple-darwin`; neither backend's e2e suite runs on this machine. The
  D3D12 timestamp test was rewritten to time a compute pass that dispatches
  rather than a buffer copy, because a copy cannot be inside a pass — that
  rewrite has never executed.

### Three WebGPU divergence reasons that are wrong

`StorageImageBinding` was the fourth and its row is gone:
`BindingKind::StorageImage` grew a `view_type` and a `format`, `crcbl-webgpu`
answers `Support::Yes`, and the design question the row hid is now its own
entry, `BindingKind::StorageImage` has no way to say "reads _and_ writes".

1. **`IndirectArgumentPaddedStride` is classified `ApiAbsence` and is already
   implemented.** True of the WebGPU call, false of this backend: `stride`
   crosses the stream whole and the replayer unrolls it into one `drawIndirect`
   per draw at `offset + i * stride`, so any stride is honoured, padded
   included. `Support::Yes` is the correct declaration. It is `ApiAbsence` so it
   is not in the 27, but it costs one probe check to make honest.

The `TimestampQuery` entry that used to sit here — "there is no arbitrary-point
write to narrow, implement rather than narrow" — was half right and the half it
got wrong is the interesting half. It reasoned that because every
`write_timestamp` in the repository already sat outside a pass, the replayer
could open an empty compute pass carrying `timestampWrites` around each one.
That would have worked and it would have been a _convention_ holding it up:
nothing stopped a caller putting a write somewhere the replayer could not wrap.
The seam took the other route and moved the two writes into the pass descriptor,
which is `timestampWrites`' own shape, so the replayer passes it straight
through and there is no free-standing call left to place wrongly. The feature
leak the same section named is closed the way it asked — by implementing.

**What has no browser-side evidence at all**, and matters because after
`crcbl-wgpu` is deleted these are claims resting on nothing: `DepthClamp` —
every probe pipeline sets it false; `BinarySemaphore` — unprovable by
construction and honestly declared so; and `SamplerAnisotropy`, whose `Yes` arm
is **unreachable** because the limit is pinned to 1.

### Two create-image paths accept a format the device cannot serve

Found while building the seam suite's raster fixture, which needed a
depth-stencil attachment and hit both:

- **`crcbl-vk::create_image` does not check format support.** Asking for
  `D24UnormS8Uint` as a `DEPTH_STENCIL_ATTACHMENT` on radv returns `Ok`, while
  the validation layer reports `VK_ERROR_FORMAT_NOT_SUPPORTED` from
  `vkGetPhysicalDeviceImageFormatProperties2` and then two more VUIDs at view
  and pipeline creation. The first draft of that fixture **passed on undefined
  behaviour** before the layer output was read.
- **`crcbl-wgpu::create_image` is not wrapped in `checked()`**, unlike
  `create_graphics_pipeline` next to it — a format the device did not enable
  arrives through the uncaptured-error handler and a live-looking handle comes
  back anyway.

**The root cause is a seam gap, not two backend bugs.** `DeviceCaps` carries
features and numeric limits and **no format table**, so a caller has no portable
way to ask which depth-stencil format is usable as an attachment. The fixture
works around it by trying `D32FloatS8Uint`, then `D24UnormS8Uint`, and checking
_both_ channels — the returned `HalError` and `Device::take_error` — which is
the shape every caller would otherwise have to reinvent.

Worth deciding as one question: does `DeviceCaps` grow a format-capability
query, or do the backends validate at `create_image` and refuse? The first is
more useful (a caller can choose), the second is cheaper and closes the UB.

### A wgpu pipeline refusal is `Backend`, not `Unsupported`

`crcbl-wgpu::create_graphics_pipeline` refuses through `checked()`, which
produces `HalError::Backend`. On a wgpu device lacking `POLYGON_MODE_LINE` or
`DEPTH_CLIP_CONTROL`, `Support::NotOnThisDevice` would meet a `Backend` error
and the seam suite's "a capability refusal is `Unsupported` and nothing else"
assertion would fire. Not reproducible on this machine — the adapter here has
both features — but the raster exercises made it **reachable where it was not
before**, so it is a live hazard on a poorer device rather than a theoretical
one.

### MEASURED — CI's Metal device can serve neither query, and no mesh

The counter probe ran. `Apple Paravirtual device`, macOS 26.5.2, and the answers
settle three open questions at once:

```
supportsFamily Metal3 = false   Apple7 = false   Mac2 = true
name contains "virtual" = true
supportsCounterSampling  AtStageBoundary=false  AtDrawBoundary=true
                         AtDispatchBoundary=true  AtTileDispatchBoundary=false
                         AtBlitBoundary=true
counterSets = 0
common set timestamp / stage utilization / statistic  present = false
```

The timestamp correlation answered too:
`wall_ns=53410875 cpu_delta=0 gpu_delta=0`. `sampleTimestamps:gpuTimestamp:` is
**inert** on this device — a real 53 ms of wall clock and neither clock moved.

**Both `Unclassified` rows are settled, and not in the direction the plan
hoped.** The device reports sampling at three points and then exposes **zero
counter sets**, so no `MTLCounterSampleBufferDescriptor` can name one and
neither a timestamp nor a statistics sample buffer can be built here at all.
`TimestampQuery` and `PipelineStatisticsQuery` on Metal are therefore
**implementable but unprovable on this CI** — the reason changes from "nobody
has looked" to "this device cannot, and CI has only this device". They are no
longer guesses, which is what `Unclassified` was for.

**The mesh precondition is settled too, and it is a no.** `wgpu-hal`'s gate is
`family_check && (Metal3 || Apple7 || Mac2) && !is_virtual`. This runner is
`Mac2` but its name contains "virtual", so mesh pipelines are excluded by that
formula regardless of the family. **Metal's `MeshShading` and `TaskShaderStage`
cannot be proved by this CI**, however the backend is written.

**A third thing this device cannot drive, found later:** both indirect
exercises. Their observable is which of two argument structures a draw read, and
this device reports no `max_draw_indirect_count` above one — so a single call
can only ever reach the first. `DrawIndirectCount` and
`IndirectArgumentPaddedStride` are therefore unexercised on Metal while every
other backend drives them. The exercise says so in its reason rather than
scoring a pass it did not earn, which is the right behaviour and also means the
Metal arm silently covers two capabilities less than the others.

**What this means for the goal**, and it needs a decision rather than more work:
four of Metal's five blockers — two query rows and two mesh rows — are now known
to be unprovable on the only Metal machine this project has. The options are a
hardware macOS runner, an explicit "implemented but unproven here" state that a
reviewer accepts once, or leaving them open indefinitely. Closing them on a
device that cannot execute them is not among the options.

### MEASURED — CI's Mac creates indirect command buffers; the device is not the blocker

`crcbl_mtl::adapter`'s
`a_device_reports_its_indirect_command_buffer_support_and_draw_indirect_count_ceiling`
ran on `mtl e2e (macos-latest)` on 2026-08-18. It was written to answer the
question that has to be settled _before_ anyone restructures this backend's
encoder for `Capability::DrawIndirectCount`. The answer is that the device can
do everything the ICB path needs:

- **Families:** `Apple1`–`Apple5`, `Mac2`, `Common1`–`Common3` all `true`, and
  **`Metal4`, `Metal3` and `Apple6`–`Apple10` all `false`.** That second half is
  a finding in its own right — see below.
- **ICB creation succeeds at every rung tried**, `64` through `1_048_576`, with
  `first refused = None` — no ceiling was found, and the top rung is
  `Limits::desktop()`'s `max_draw_indirect_count`.
- **Inheritance works both ways.** `inheritBuffers`/`inheritPipelineState` set
  (what a real `draw_indirect_count` needs, since the seam's pass has already
  bound its pipeline and argument tables) creates just as readily as the control
  with them clear. "Refuses ICBs" and "refuses _inheriting_ ICBs" were
  distinguishable and neither happened.
- **`max_draw_indirect_count = 1` is this backend's floor, not the device's
  answer**, which the probe prints in as many words: `limits_of` assigns the
  field nothing, and `Limits::minimum()` is 1 while `Limits::desktop()`
  is 1048576.
- **A compute kernel has room to encode them:**
  `max_compute_workgroups_per_dimension = 65535`,
  `max_compute_invocations_per_workgroup = 256`,
  `max_storage_buffer_range = 3758096384`, and `COMPUTE = true`.
  `MULTI_DRAW_INDIRECT` and `INDIRECT_FIRST_INSTANCE` are already reported;
  `DRAW_INDIRECT_COUNT` is the one that is not.

**So the row's obstacle is exactly what it always said and nothing more:** the
architecture, not the hardware. Metal's only count-from-memory execution needs
commands a compute kernel wrote, and that kernel must run before the render
encoder `draw_indirect_count` is called inside was opened. **`crcbl-mtl` no
longer encodes straight through** — `crcbl_mtl::command` records a render pass
and encodes it at `end_render_pass`, so the compute encoder can be opened ahead
of the render one. That restructuring was the prerequisite, it has landed as a
pure refactor, and what remains is the ICB kernel itself.

**The one-byte-per-command caveat is closed by measurement.** Every ICB reported
`size` equal to its `maxCommandCount` — one byte per command, implausible for
real command storage — so creation succeeding was never evidence that
`executeCommandsInBuffer:` would run anything. `crcbl_mtl::device`'s
`an_indirect_command_buffer_executes_the_triangle_the_direct_draw_paints` now
settles it: it encodes `ink_msl`'s triangle into a one-command ICB from the CPU,
executes it with `executeCommandsInBuffer:withRange:`, and compares the readback
against the same pipeline drawn directly on the encoder. **It passed on
`mtl e2e (macos-latest)` on 2026-08-18** — 66 of 66 — so this device executes an
ICB draw and produces a byte-identical canvas. The nominal `size` is a reporting
quirk, not a hollow allocation.

### MEASURED — Metal's mesh rows are unprovable on the Mac CI has

The ICB probe's family list answers a question the mesh rows had only guessed
at. CI's Apple Paravirtual device reports **`Metal3 = false`**, and `false` for
every Apple family above `Apple5`; the highest it claims are `Apple5` and
`Mac2`.

**Metal mesh shading is a Metal 3 feature**, gated on
`supportsFamily:MTLGPUFamilyMetal3` — Apple's own check, and what
`MTLGPUFamily.metal3` documents itself as covering. So a
`MTLMeshRenderPipelineDescriptor` cannot be created on that device at all.

**What this changes, and what it does not.** `Capability::MeshShading` and
`Capability::TaskShaderStage` for Metal stay `Unwritten`: `crcbl-mtl` genuinely
builds no mesh pipeline, and that is a fact about the backend rather than the
runner. What changes is the _cost_ of writing them — the work could be done and
CI could never show it working, so it would land as code no gate exercises. That
is the same shape as the two `Unclassified` counter-sampled query rows, which
wait on a Mac that advertises a counter set.

Both rows' `why` in `crates/crcbl-hal/src/capability.rs` now carry the
measurement, so the parity report says it rather than leaving the next reader to
rediscover it.

**It also bounds the deletion-bar decision.** Of Metal's six blockers, **four**
are now measured unprovable on available hardware — the two mesh rows here and
the two counter-sampled query rows — which is the concrete content behind option
(2) in "what bar the deletion clears": every row either closed or _measured_
unprovable, with `Support::NotOnThisDevice` already able to say so.

### DECISION — should the `mtl-e2e`-off build be a CI gate?

The break itself is fixed: `Barriers`, `ImageBarrier` and `ResourceState` are
used only by `draw_canvas_over`, which is `#[cfg(feature = "mtl-e2e")]`, so
their `use` is now gated the same way — beside the one that already gates
`CompareOp`, `DepthBias`, `DepthStencilAttachment` and `Viewport`. Both
configurations now pass:
`cargo clippy -p crcbl-mtl --all-targets --target aarch64-apple-darwin` with and
without `--all-features`.

**What is still open is whether anything stops it happening again.** No CI job
builds `crcbl-mtl` with the feature off — every `cargo clippy` in `ci.yml`
passes `--all-features` — so this broke silently and was found by an agent
running a narrower command by hand. It is the configuration a developer gets by
default.

The trade-off: gating it costs a matrix entry on a macOS runner (the slowest and
scarcest), for a class of defect that is compile-only, always a one-line fix,
and never reaches a user. The same hole exists for `crcbl-dx12` on Windows and
was not checked. Cheaper middle ground: add `--no-default-features` /
feature-off builds to the _Linux_ clippy job for the crates that build there,
and accept that the two platform backends keep the hole.

### A measurement test must not read a truthful zero as a broken apparatus

The Metal counter probe reddened CI twice on its own assertions, and both were
the same mistake in different clothes. It asserted that a device claiming
counter sampling must expose a counter set, and that a CPU timestamp must move
across a sleep — each on the reasoning that otherwise the test never reached a
device.

Both fired. Both were wrong: the runner printed its device name and then
answered zero counter sets, and `cpu_delta=0 gpu_delta=0` across 53 ms of real
wall clock. It reached a device; the device's counter infrastructure is inert.

**A zero is data.** The honest reachability signal is the one thing that cannot
be a measurement — an empty device name — and that assertion stays. Worth
remembering when the next probe is written, because a measurement that fails on
an unexpected answer stops being a measurement and becomes an assumption with a
stack trace.

### MEASURED — where a push-constant block actually lands, per target

The artifact that three separate divergence rows described and that did not
exist now ships: `push_constant_probe.slang`, emitted for spirv, msl and dxil,
whose dispatch writes the constants into a buffer word by word. Read out of the
**emitted files**, not assumed:

| target | the block lands at                                                       | the bound buffer |
| ------ | ------------------------------------------------------------------------ | ---------------- |
| SPIR-V | `PushConstant` storage class, member offset 0, 16 bytes — no set/binding | set 0, binding 0 |
| MSL    | `buffer(1)`                                                              | `buffer(0)`      |
| DXIL   | `cb0` — register `b0`, space 0, size 16                                  | `u0`             |

**This corrects Metal's row, and makes that slice smaller.** The MSL puts the
block **behind** the bound buffer, not ahead of it. `ui.slang`'s old artifact
put it first only because it declared the push constant first, and the
declaration-order lint now has its first shipped exercise — so `crcbl-mtl` can
bind the block one past the last binding rather than shifting every table entry.

Two of the three are asserted by a test that reads the committed artifact;
DXIL's is read but not asserted, because its resource table lives in the DXBC
`RDEF` chunk and this crate is dependency-free by design. The test asserts the
container ships and the doc says which claim is which.

**WGSL is excluded and that was checked rather than asserted:** slangc _does_
emit a WGSL artifact for this source, and naga refuses it for a missing binding
decoration. So the validation sweep is not weakened — a shader declaring no wgsl
target contributes nothing to it, and one that declares it again is caught.

**What this unblocked:** dx12's `PushConstants` row is closed; Metal's still
stands, and its slice is smaller than planned because the block lands behind the
bound buffer rather than ahead of it. Plus the re-homing of `crcbl-wgpu`'s
push-constant-range exercise, which is one of the two coverage losses still
standing between here and that crate's deletion.

### A divergent index into a push-constant block reads lane 0 on RADV

Found by the seam suite's new push-constant exercise, and worth remembering
because of the shape rather than the bug. The probe shader read
`constants.values[index]` with `index` from the dispatch ID. That is valid
SPIR-V — `spirv-val --target-env vulkan1.3` is clean — and it reads correctly on
llvmpipe. On real hardware it is wrong: NIR defines a `load_push_constant`
offset as dynamically uniform, so ACO scalarises a divergent one, RADV emits
`v_readfirstlane_b32` on the byte offset, and every invocation reads the block
at lane 0's index. All four words came back holding `values[0]`, on a discrete
Navi31 and an integrated Raphael alike.

**CI's Vulkan arm is lavapipe, which gets it right.** So this would have been
green on every runner and wrong on every AMD card — the asymmetry is the part
worth carrying forward, not the fix. A local hardware run is not a nicety here;
it is the only thing that sees this class of defect.

Fixed by naming each component through a `switch` rather than indexing, which
keeps the `uint4` layout and makes each invocation's offset a compile-time
constant.

### No GPU job in CI runs on real hardware, and one defect has already proved it matters

Enumerated from the workflows on 2026-08-18, every adapter every GPU job opens:

| job                          | backend    | adapter                               |
| ---------------------------- | ---------- | ------------------------------------- |
| `vk-e2e`                     | vk         | **lavapipe** (`CRCBL_VK_ICD` pins it) |
| `wgpu-e2e`                   | wgpu       | **lavapipe**                          |
| `cross-backend-e2e`          | vk vs wgpu | **lavapipe** both sides               |
| `dx12-e2e`                   | dx12       | **WARP** (`CRCBL_ADAPTER=cpu`)        |
| `mtl-e2e`                    | mtl        | **Apple Paravirtual device**          |
| Pages probe (Linux, Windows) | webgpu     | **SwiftShader**                       |
| Pages probe (macOS)          | webgpu     | Apple Paravirtual, through Metal      |

**There is no real GPU anywhere in it.** Every golden, every seam exercise and
every capability claim is verified against a software rasteriser or a
paravirtual device. That is a deliberate and reasonable choice — hosted runners
have no GPUs — but it decides what "verified" means here, and it is not what a
reader assumes when a suite is green.

**It is not theoretical.** The push-constant exercise landed green on every CI
arm and was wrong on every AMD card: a divergent index into a push-constant
block reads lane 0 on RADV and reads correctly on lavapipe. The only thing that
caught it was running the suite on this workstation's discrete card. Metal's
`DepthClamp` is the same shape from the other side — the paravirtual device
ignores a mode real Metal honours, so CI was _failing_ on something no user
would hit.

**What follows, and it is a working practice rather than a fix:** a slice
touching shader behaviour, driver-visible state or anything a compiler can
scalarise is not verified until it has run on this machine's hardware adapter as
well as the software path. `CRCBL_GPU=vk` with no `CRCBL_ADAPTER` is the
hardware run; `CRCBL_ADAPTER=cpu` is what CI sees. **Both, and the difference
between them is the interesting part.**

**The risk is bounded, and that was measured rather than assumed.** All five
golden suites — render 26, draw-gen 12, forward 13, sprite 12, mesh 9, 72 tests
— were run on both the hardware adapter and the software path and pass
identically. So the goldens are not adapter-sensitive within
`Tolerance::RASTERISER`, and the lane-0 defect was specific to a shader path
rather than a general property of the software-only board. What the gap
threatens is _new_ shader and driver-state work, not the existing picture.

The alternative — a self-hosted runner with a real GPU — would close it properly
and is the same class of decision as the hardware macOS runner the Metal rows
want. Worth pricing the two together if either is ever taken.

### The seam does not say when a bind-group update is legal, and backends differ

Found while driving `UpdateBindGroup`. Two gaps, and the second is the sharper
one.

**1. Mid-recording timing is unstated, and the backends would not agree.**
`Device::update_bind_group` covers the _pending_ case — a rewrite while command
buffers referencing the group are still in flight — and says nothing about a
rewrite while a command buffer that binds the group is still **recording**.
`crcbl-dx12` writes descriptors straight into the shader-visible heap a recorded
bind already points at, so the change lands. `crcbl-mtl` copies the values onto
the encoder at `bind_group`, and its own docs say an update "does not reach a
command buffer that already bound the group". Those are opposite answers to a
question the seam never asks, so that timing **cannot be exercised portably
until the seam picks one**. The exercise drives the timing every backend agrees
on — between two submitted-and-waited command buffers.

**2. Two backends accept a rewrite the seam documents as refused.**
`Device::update_bind_group` documents `HalError::Unsupported` for a layout
without `UPDATE_AFTER_BIND`. `crcbl-mtl` and `crcbl-dx12` accept it. On Metal
that is the **only kind of layout there is** — it withdraws
`DESCRIPTOR_INDEXING` because its bind groups are flat argument tables — so
enforcing the documented rule there would make Metal's `Support::Yes` describe a
call that can never succeed. Either the doc is wrong about which backends the
rule binds, or Metal's declaration is. Worth settling before anyone writes a
caller that branches on it.

**Also noted:** `crcbl-vk`'s own update test claims to cover "the one call the
seam permits while command buffers referencing the group are still pending". It
does not — it waits idle first, exactly as the new agnostic exercise does. So
the pending case this capability is defined around is driven by nothing at all.

### DECISION NEEDED — what "parity holds" has to mean before `crcbl-wgpu` goes

The stated order is: reach parity, then delete `crcbl-wgpu`. Goal 2 is done and
gated. Parity is at **14 blockers**, down from 29, and the composition now
matters more than the number — because **a strict reading of "all blockers
closed" is not reachable with the hardware this project has.**

Sorting the remaining rows by what actually stands in the way:

**Ordinary work.** Everything not named below — dx12's two mesh rows (now known
provable, see the probe result above), Metal's remaining rows other than the
four measured unprovable, and WebGPU's two. Both `PushConstants` rows have since
closed, which is why this reads as a description rather than a count: the number
moves every slice and the snapshot test is where it lives.

**Measured unprovable on the only hardware available — 4 rows.** Metal's
`TimestampQuery` and `PipelineStatisticsQuery`: CI's device reports three
counter sampling points and **zero counter sets**, so no sample buffer can be
built there at all. Metal's `MeshShading` and `TaskShaderStage`: the runner is
`Apple Paravirtual device`, and `wgpu-hal`'s own gate excludes a device whose
name says virtual. These can be _implemented_ and can never be _proved_ here.

**~~Unknown~~ — settled, and favourably.** A probe now runs on the WARP job and
answers: **`MeshShaderTier = TIER_1`**, shader model 6.6 offered (6.8 by
descending probe), `ResourceBindingTier = 3`. So dx12's `MeshShading` and
`TaskShaderStage` are **ordinary work and provable on this CI** — they move into
the eight above, making it ten. The committed `ms_6_6` DXIL will load. Nothing
in the blocker list is unknown any more.

**Awaiting a decision already raised — 3 rows.** dx12's fills, below.

**Metal's end state is now calculable, which makes the choice below concrete
rather than a matter of taste.** Its six rows split three ways, all measured:

| row                                         | state                                                                                        |
| ------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `MeshShading`, `TaskShaderStage`            | **unprovable here** — the runner answers `Metal3 = false`                                    |
| `TimestampQuery`, `PipelineStatisticsQuery` | **unprovable here** — no `counterSets`, and no honest `timestamp_period_ns`                  |
| `BindlessDescriptorArray`                   | writable — Metal's argument buffers do carry resource arrays; this backend binds flat tables |
| `DrawIndirectCount`                         | writable, and now fully de-risked — the device creates _and executes_ ICBs                   |

**`BindlessDescriptorArray` on Metal WORKS, and the remaining question is the
shader, not the device.** The probe in `crcbl_mtl::binding` ran on CI's
`Apple Paravirtual device` with Metal API and GPU Validation both on, and read
all sixteen words back exactly: `0xBD0000..3`, `0xBD0100..3`, `0xBD0200..3`,
`0xBD0300..3`. So on that device an argument buffer written directly as a table
of `MTLBuffer::gpuAddress` values works, `gpuAddress` answers non-zero, and
`useResource:usage:` residency is enough for the kernel to read through it.

Getting there took two corrections, both from measurement:

- `StructuredBuffer<uint> sources[] : register(t0, space0)` — the current
  `bindless_probe.slang` — makes `slangc -target metal` **exit 0** while
  emitting a binding index of `(uint64)-1` and an unattributed entry-point
  parameter.
- Recast as a `ParameterBlock` with an **unbounded** array, Metal's own front
  end refused it:
  `program_source:7:19: error: flexible array members are a C99 feature`. Slang
  lowers `items[]` to a C99 flexible array member, which MSL does not define.
- A **bounded** array in a `ParameterBlock` lowers to
  `array<uint device*, int(64)>` — an ordinary `metal::array` — and that is what
  runs.

**The cost is a descriptor-set move, and it is measured.** `ParameterBlock` is a
descriptor set in SPIR-V, so the array goes from set 0 / binding 1 to **set 1 /
binding 0**, with `destination` left in set 0. Bounding the array _without_
`ParameterBlock` keeps both in set 0 but hands Metal an entry-point parameter
with no `[[buffer]]` attribute — invalid MSL again. So `ParameterBlock` is not
optional if Metal is to have an artifact at all.

That move is arguably the right layout rather than a concession: a bindless
table is bound once per frame while other sets change per draw, which is why
engines give it its own set. But it does mean recasting `bindless_probe.slang`
changes the bind groups `crcbl-vk` and `crcbl-dx12` build for it, and those pass
today.

**Both halves have landed.** `bindless_probe.slang` is a bounded
`ParameterBlock` of `SOURCE_CAPACITY = 7` at set 1 / binding 0 shipping
`spirv, msl, dxil`, and `crcbl_mtl::binding` binds the table as a Metal argument
buffer of `gpuAddress` values with `useResource` residency. The capability is
`granted(DESCRIPTOR_INDEXING)`, reported when the device answers argument-buffer
`Tier2` **and** `Metal3`, so a lesser Mac says `NotOnThisDevice` rather than the
backend saying `No`. **The row is gone from `REVIEWED_BLOCKERS`** — nine rows,
dx12 4 and Metal 5.

The capacity is not a taste: it must exceed `SOURCE_COUNT` or the layout's count
stops being a ceiling, and Vulkan requires a sized array's layout to declare the
whole array, so `PORTABLE_STORAGE_BUFFERS_PER_STAGE` (8) minus the destination
caps it at 7.

**Nothing in `crcbl-mtl` has executed**, and the flip landed with the
implementation rather than after it because the two cannot be separated: the
capability is gated on `DESCRIPTOR_INDEXING`, and withholding the feature makes
`check_entries` refuse the layout, so the split would be "nothing now,
everything later". What only the `mtl e2e` run can settle: that the CI Mac
reports `Tier2` and `Metal3` at all; that Metal takes the table at
`[[buffer(1)]]` with the destination at `[[buffer(0)]]`, the reverse of the
indices the probe proved; that `useResource` on the compute encoder covers
buffers a preceding blit wrote in the same command buffer; and that a
zero-filled tail passes GPU validation when never read. **The render-side
`useResource:usage:stages:` arm is compiled and unproven anywhere** — the only
bindless shader is compute-only.

If it goes red, the revert is three hunks: restore the `Divergence` row and the
`REVIEWED_BLOCKERS` tuple in `capability.rs`, and delete the `Tier2 && Metal3`
block in `crcbl_mtl::adapter`'s `features_of`, which turns the path off at the
gate without touching `binding.rs`.

So **four of six can never go green on the hardware this project has**, whatever
anyone writes. "Metal fully closed" is not reachable by working harder; it is
reachable only by buying a Mac runner. That is the whole content of position (1)
versus (2), and it is worth seeing before choosing between them.

**And the two writable rows are not free.** `DrawIndirectCount` needs commands
written by a kernel that must run before the render encoder opens. The backend
can now express that ordering — see the deferral below, which has landed — so
what is left is the kernel, the ICB and the flag rather than the encoding model.

**It does split into safe slices, and an earlier note here said it did not.**
That note reasoned that implementing without reporting would fail the seam suite
as "declares unsupported and then performed it". It would not:
`crcbl-vk::command`'s `indirect_count` refuses with `HalError::Unsupported`
whenever `Features::DRAW_INDIRECT_COUNT` is absent from the device's own caps,
and `crcbl-mtl` doing the same keeps the capability honestly `No` while the
machinery exists underneath. The exercise sees `Refused`, which is what a `No`
owes. So the dx12 shape — land the calls, withhold the flag, flip it in a
separate revertible change — transfers exactly.

**The flip is still the dangerous half**, and knowingly so: reporting
`DRAW_INDIRECT_COUNT` moves every Metal adapter onto
`GeometryPath::IndirectCount`, which is the same class of change that removed
the WARP device when dx12 reported `MESH_SHADER` today.

**One real design decision sits inside the first slice**, and it should be made
deliberately rather than discovered:

1. **Defer render-pass encoding.** Record a pass's commands and encode them at
   `end_render_pass`, so a compute encoder can be opened ahead of the render
   encoder when the pass turns out to contain a `draw_indirect_count`. Correct
   and costs nothing at run time, but it touches _every_ Metal draw — the whole
   backend's encoding model — and is verifiable only on CI. **Done:**
   `crcbl_mtl::command`'s `RenderCommand`/`RenderRecording` and the pure
   `crcbl_mtl::pass`.
2. **Split the pass at the call.** End the render encoder, run the kernel in a
   compute encoder, reopen the render encoder with load actions preserving the
   attachments, execute the ICB. Local to the one call and far smaller, but a
   pass split stores and reloads every attachment, which is a real per-call cost
   on a tiler, and it silently drops any in-pass memory guarantee across the
   split.

(1) is the performant answer and (2) is the cheap one; the seam calls
`draw_indirect_count` once per bucket per frame, so (2)'s cost is not
hypothetical. (1)'s free prerequisite — the deferral landing on its own as a
pure refactor with no capability change at all — **has landed**, and the
existing Metal e2e and render jobs are what prove it: nothing about it can be
executed off a Mac, and the local gates only type-check it.

### BLOCKED ON THE SHADER TOOLCHAIN — Slang cannot write an ICB

**This overturns the decision below, and it was found by trying to build the
kernel.** The design picked deferred encoding so a compute kernel could write
the `MTLIndirectCommandBuffer`. **Slang cannot express that kernel.** Its Metal
target has no indirect-command-buffer support: an ICB parameter is silently
dropped rather than diagnosed, and Slang's own "Metal-Specific Functionalities"
page lists what the target does implement — mesh shaders, parameter blocks as
argument buffers, `SubpassInput` framebuffer fetch, specialization constants as
function constants, address spaces — with no `command_buffer` or
`render_command` anywhere, and an explicit unsupported list that does not
mention them either because they were never in scope.

Every shader in this repo is Slang; `crcbl-shaders`' whole build is `.slang` →
spirv/msl/dxil/wgsl, and the `shaders (committed artifacts match their sources)`
CI job hashes each source against its artifacts.

**So `DrawIndirectCount` on Metal is not "unwritten" in the ordinary sense.** It
is blocked on something outside this repo, and the honest options are:

1. **Hand-write one MSL kernel.** The stated cost here was that "the
   artifact-hash job needs an exception mechanism it does not have" — **and that
   is wrong.** `crcbl-mtl` compiles MSL through
   `newLibraryWithSource:options:error:`, so MSL is _source at runtime_, not a
   precompiled artifact; and `tools/compile-shaders.sh` is source-driven,
   walking `shaders/*.slang` and emitting each one's declared targets, then
   refusing orphans **inside `msl/`**. A kernel that never lives in `msl/` is
   invisible to it, with no exception mechanism required.

   The precedent already shipped and is green: `crcbl_mtl::binding`'s bindless
   probe carried its whole kernel as a `const BINDLESS_MSL: &str` in Rust. Note
   what that precedent is and is not — it is about _where_ MSL may live, not
   about hand-authoring it; that text was slangc's own output with a test tying
   it to its source.

   So the real cost is the honest half of the original objection: a hand-written
   kernel has no `.slang` to regenerate from, and nothing checks it against a
   source because there is none. That is a maintenance cost, not a CI-mechanism
   problem, which makes this option materially cheaper than recorded.

2. **Wait for Slang.** Out of our control and unscheduled; the row stays open
   indefinitely and the deferral refactor sits with no consumer.
3. **Take MoltenVK's route** — read the count back and loop on the CPU. Correct
   and simple, and it stalls the frame, which is why it was rejected above. It
   would close the row with a performance regression on the path
   `crcbl-render`'s GPU-driven work exists to make fast.
4. **Leave the row open with this as its reason**, reclassified from `Unwritten`
   to something that says "the toolchain cannot express it", which is closer to
   `ApiAbsence` in spirit than to work nobody has done.

My reading: (4) now and (1) only if something else also needs hand-written MSL,
because one exception to a hashed pipeline is a rule with an asterisk forever.
(3) fails the standard this project sets.

**And it makes the deferral refactor speculative for now.** `crcbl_mtl`'s
`RenderRecording` landed and is verified green on CI, and it is
behaviour-neutral — but its consumer was this kernel. It is not wasted (it is
the prerequisite for _any_ pre-pass work on this backend, including a
hand-written kernel) and it is not harmful, but it currently has no caller,
which is worth saying plainly rather than leaving it to look load-bearing.

**Decided below: (1), and the research is why rather than taste** — recorded as
it stood, because the reasoning about MoltenVK, pre-encoding and pass-splitting
is still correct and still decides the shape _if_ a kernel can ever be written.

- **MoltenVK does not take the ICB route at all.** It implements
  `vkCmdDrawIndirectCount` by reading the count back to the CPU and looping
  `drawIndexedPrimitives`, which needs a CPU–GPU synchronisation to see a number
  the GPU wrote. That is correct and simple and it stalls the frame, so it fails
  the performance half of this project's bar. It is worth knowing the reference
  Vulkan-on-Metal implementation gave up here — it is why nobody should read
  Metal's silence on this as "there is an easy way we missed".
- **A third option does not exist: the commands cannot be pre-encoded once.**
  `MTLIndirectRenderCommand`'s draws take literal vertex and instance counts, so
  an ICB encoded on the CPU cannot serve draws whose arguments the cull pass
  writes into GPU memory each frame. A kernel has to write them, which is what
  forces the ordering in the first place.
- **(2) is disqualified by the hardware this targets.** Ending and reopening a
  render encoder on a tiler stores every attachment out of tile memory and
  reloads it — per call, and the seam makes one call per bucket per frame. On
  Apple silicon that is the expensive operation, not a rounding error.

**What the deferral left behind, stated as a gap rather than a footnote.**

- **Nothing about it executed a Metal call before it was pushed.** The refactor
  was written and gated on Linux, where `crcbl-mtl` compiles for
  `aarch64-apple-darwin` and runs nothing. The only proof it works is
  `mtl e2e (macos-latest)` and the Metal render job — every draw, every bind,
  every golden comparison — so a red run there is the first real signal, not a
  regression against a local pass.
- **One failure moved, and only one.** `renderCommandEncoderWithDescriptor:`
  returning nil is now a `HalError::DeviceLost` recorded at `end_render_pass`
  instead of at `begin_render_pass`; both are before `finish`, which is where
  the seam observes it. The visible difference is confined to the case where a
  copy is recorded inside a pass whose encoder would have been nil — then the
  first error is the copy's `InvalidDescriptor` rather than the `DeviceLost`.
  Nothing can produce a nil encoder here without a lost device, so it is
  documented in `crcbl_mtl::command` rather than worked around.
- **The command list's replay order is not unit-testable off macOS.** Every
  `RenderCommand` payload is an Objective-C object, so only `crcbl_mtl::pass`'s
  scissor plan and debug-group stack are covered by `cargo test` on any host.
  Ordering is a `Vec` push and drain with no branch in it, which is why that was
  accepted rather than mocked.

- **The one semantic change the deferral makes is the one nothing exercises —
  and the gap is not Metal's.** `RenderCommand::PushConstants` carries
  `bytes: Vec<u8>`, a copy taken with `shadow.bytes().to_vec()` rather than a
  pointer into the live shadow, because `setBytes:length:atIndex:` copies
  immediately when encoding straight through while a recorded command would
  otherwise see whatever a _later_ `push_constants` spliced in. The copy is
  correct and was checked by reading. Nothing tests it — see the entry below,
  which is the general version of the problem.

**And one widely repeated claim is false, which our own measurement settles.**
Several sources say fragment shaders cannot be used with indirect command
buffers.
`an_indirect_command_buffer_executes_the_triangle_the_direct_draw_paints`
executed a full vertex-and-fragment pipeline through an ICB on CI and produced a
canvas byte-identical to the direct draw. Had that been believed rather than
measured, the whole approach would have been abandoned for a stall.

**So the question is what bar the deletion clears.** Three honest positions:

1. **Every row closed.** Requires a hardware macOS runner for four Metal rows,
   and possibly a hardware Windows runner for the two mesh rows. Real money, and
   it would close the loop completely.
2. **Every row either closed or _measured_ unprovable here**, with the
   unprovable ones implemented and marked. `Support::NotOnThisDevice` and the
   report's _unprovable here_ state already express this exactly — it is what
   Metal's `DepthClamp` does today. The claim becomes "every backend implements
   it, and the CI we have proves it wherever it can".
3. **Delete now on the coverage argument.** `crcbl-wgpu` closes zero capability
   gaps — every capability its 13 rows name is also refused by a keeper backend
   — and on Linux it runs Vulkan underneath, so agreement with `crcbl-vk` proves
   less than it appears. Two of the four exercises its deletion would have cost
   are already re-homed; the other two are one slice each.

**My reading, for what it is worth:** (2) is the honest bar and (1) is the same
thing plus hardware. (3) is defensible on its own terms and is the only one that
does not make the deletion wait on Metal's mesh support, which nothing in the
engine uses on that backend today. But it is a scope call, not a technical one,
and it is the last thing standing between here and goal 3.

## The numbers, restated — the eight rows and what each would take

The list is **eight**, down from eleven, and it is worth seeing that only two of
the eight are work anybody could do today:

| rows                                              | what they are                                                                               | what closes them                                                                                     |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| dx12 `BufferFillRepeatedByte`, `BufferFillWord`   | **Declined, reviewed.** `ClearUnorderedAccessViewUint` needs a UAV of every fillable buffer | a decision to pay `ALLOW_UNORDERED_ACCESS` everywhere, which was judged worse than a clean `No`      |
| dx12 `MeshShading`, `TaskShaderStage`             | the WARP device removal                                                                     | **a Windows session.** Three hypotheses eliminated by reading; the branch that reproduces it is kept |
| Metal `MeshShading`, `TaskShaderStage`            | the runner answers `Metal3 = false`                                                         | **hardware.** Unprovable here whatever anyone writes                                                 |
| Metal `TimestampQuery`, `PipelineStatisticsQuery` | no `MTLCounterSampleBuffer` is built, and the runner advertises no `counterSets`            | writable, but **unverifiable here** — it would land as `NotOnThisDevice`                             |

So under bar (2), the reachable end state is **six rows**, not zero: writing
Metal's counter sample buffer moves two rows to `NotOnThisDevice`, and the other
six need either hardware or a decision already taken. Under bar (3) the deletion
happens now.

**What is no longer an argument either way:** every seam-audit finding is
closed, so `crcbl-wgpu` is not holding any contract honest that the other
backends do not. It caught one real bug this session — a missing
`max_bind_groups` guard that returned `Ok` plus a poisoned layout — which is a
point _for_ its coverage, and that guard now exists and would survive its
deletion.

**What has changed since this was written, and it sharpens the question rather
than answering it.** `crcbl-webgpu` now has **zero** divergences —
`REVIEWED_BLOCKERS` does not name it at all, after `StorageImageBinding` gained
the seam field it needed and `TimestampQuery` was closed by moving timestamps
into the pass descriptor. The blocker list is **nine rows across two backends**:
dx12 4, Metal 5.

That matters because the browser backend is the one `crcbl-wgpu`'s deletion is
_about_. The replacement for the path `crcbl-wgpu` used to serve now implements
the whole seam with nothing outstanding, which is the strongest form position
(3) could ever take: the question is no longer "is the replacement ready" but
"does an unrelated backend's unfinished work gate an unrelated crate's removal".
Every remaining row belongs to dx12 or Metal, neither of which `crcbl-wgpu` has
ever served — it runs Vulkan underneath on Linux and is not built for macOS or
Windows CI at all.

Position (1) and (2) are now really about **when parity is declared done**, not
about whether the deletion is safe. They can be separated: nothing about
deleting `crcbl-wgpu` makes dx12's mesh reporting or Metal's counter sets
harder, and keeping the crate does not make either easier.

### The two valued dx12 fills stay declined, and the zero one shipped

The decision this entry asked for was taken: the middle position it named. The
zero-buffer copy — one small zeroed device-local resource created with the
device, then `CopyBufferRegion` over the range, which is what `wgpu-hal`'s dx12
backend does — landed, so `BufferFillZero` reports supported and the blocker
list went from eleven to ten.

`BufferFillRepeatedByte` and `BufferFillWord` stay `Declined`, and the price is
unchanged: `ClearUnorderedAccessViewUint` needs a UAV of the destination, so
closing them costs `ALLOW_UNORDERED_ACCESS` on every device-local allocation or
a fill that works only on `STORAGE` buffers — a capability that works only
sometimes, which is worse than a clean `No`. The pattern-buffer route (an
encoder-owned upload arena) remains the option if they are ever wanted; nothing
asks for them.

Their recorded reason was also wrong and is corrected in the parity record
itself: the crate _does_ build shader-visible heaps in `crcbl_dx12::binding`, so
the obstacle is descriptor provenance and lifetime rather than a missing heap.

**Proven on WARP.** The seam suite ran it: `BufferFillZero supported`, 17/17,
with the two declined rows printing the corrected reason and a non-zero fill
refusing by name — so both directions of the parity contract are exercised, not
just the supported one.

### Smaller things the WebGPU work surfaced and did not fix

- **A device error names no command.** `Reply::DeviceErrors` carries the
  browser's prose, but `uncapturederror` arrives with no sequence, so nothing
  says _which_ encoded command caused it — which is why the errors ride a
  command that asks rather than being pushed. `docs/plan/41-webgpu-stream.md`'s
  attribution section is still open. It is the difference between "the device
  reported an invalid bind group" and "command 4,182 did".
- **`take_error` answers a frame late, by construction.** Nothing here may block
  on a browser, so the reply to this frame's ask lands in the next one:
  `Gpu::acquire` refuses to record a frame when it answers `Some`, so the frame
  _after_ a failure still records and the one after that stops. Documented at
  the call site. Not fixable without blocking; worth knowing when reading a
  trace.
- **`GPUDevice.lost` is deliberately unwatched.** It means the device is gone
  and this seam has no device-lost path, so feeding it to the error queue would
  report a dead device as one more line to log and carry on from. Revisit when
  the HAL grows a device-lost story.
- **The error queue's carry-over path is untested through a browser.** The JS
  log caps at 64 pending and one reply carries at most 64, so the log's cap
  always bites first and the reply's "wrote fewer than asked, carry the rest"
  branch is exercised only by the Rust and JS writer unit tests. Raising the log
  cap would exercise it for real.
- **`web/tools/wasm-loader.js` is now ours to maintain.** Dropping
  `wasm-bindgen` replaced a generated 72 KB glue with a hand-written 3 KB
  loader. It is small and does one thing, but it is a hand-rolled module loader
  on the path every demo boots through — worth remembering it exists when a
  browser changes how `WebAssembly.instantiateStreaming` behaves.
- **The Pages deploy for the un-link commit never ran.** GitHub returned 503 on
  `actions/deploy-pages` during an outage; the _build_ job passed every gate.
  The site on Pages is therefore one commit behind until that deploy is re-run.

### Non-zero `fill_buffer` is refused on the WebGPU backend

`crates/crcbl-webgpu/src/command.rs`'s `Command::FillBuffer` and its
`gpu-replay.js` `#fillBuffer` arm map `value == 0` to WebGPU's `clearBuffer` and
route any other value to the device error queue — WebGPU offers no valued
device-side fill, so this matches the `crcbl-wgpu` backend's `fill_buffer`
(`crates/crcbl-wgpu/src/command.rs`, "wgpu only offers a zero fill").

**The consequence for scenes.** If any `render_e2e` scene issues a non-zero
`fill_buffer`, that scene cannot render on the WebGPU backend — the fill lands
on the error queue and the buffer it meant to initialise keeps its previous
contents. The parity gate (slice 10) is where that surfaces as a WebGPU/native
divergence. **It is unverified whether any scene actually issues a non-zero
fill**: the scenes were not audited for it in this slice. When slice 10 lands,
check the scene set for non-zero `fill_buffer` calls before treating a WebGPU
parity failure as a backend bug rather than an unsupported operation.

### The probe module-doc export table is missing the readback shims

`crates/crcbl-webgpu/src/probe.rs`'s `# Exports` table lists every shim through
`__crcbl_web_gpu_probe_surface_caps_has_extent`, then jumps to the draw shims
this slice added — the five `__crcbl_web_gpu_probe_readback*` exports from the
prior readback slice were never added to the table. The functions and their doc
comments exist; only the summary table skipped them. Left out of the draw slice
as out of scope: add the readback rows between the surface-caps and draw rows so
the table is whole again. Verified by reading the table (lines ~27–65) against
the `pub extern "C"` shim fns.

### Nothing on the WebGPU backend can ever say "try the next adapter"

`surface_caps`'s contract obliges a caller doing selection to read an `Err` as
"try the next adapter" rather than as fatal. `crcbl::engine`'s
`GpuContext::start_device` is the loop that does it — **not** `apps/sandbox`,
which the HAL doc and `crcbl-vk`'s own comment both still name; the loop moved
into the engine and those two sentences did not follow it.

**The WebGPU backend can never produce that answer.** Its capability query names
no adapter — it is an argument-less instance-level question, because the record
depends on neither the surface nor the adapter — so there is no adapter for a
refusal to be _about_, and the wire carries one failure cause, `Backend`,
meaning the query itself broke.

So a selection loop running on this backend always takes adapter 0 and never
reaches its second iteration. Vulkan is what gates that loop: it is the only
backend that asks a driver per call and the only one that can refuse a real
pairing. Worth knowing before anyone reads a green browser gate as covering it.

### Two bind-group-layout rules are stated but not observed on a browser

Both are believed on the specification's word, and the browser gate cannot see
either. Written down so a green group M is not read as covering them.

- **A duplicated `binding` number.** `web/engine/gpu-replay.js` says the browser
  refuses one, and the corpus carries such a layout — but only
  `web/tools/gpu-replay.mjs` replays it, against a stub that does not validate.
  The browser probe's descriptor has unique bindings. A probe entry would close
  it. `BindGroupLayoutDesc::check_entries` rejects duplicates on the near side
  anyway, so this is about whether the far side is a second net or no net.
- **An empty `visibility`.** Measured, not assumed: setting `visibility: 0` on
  every entry and running the gate left it at 72/72 — Chromium accepts a binding
  visible to no stage. So group M catches a **missing binding-type member** and
  does not catch a lost visibility word. That mapping is held by
  `gpu-replay.mjs`'s bit-by-bit table and by the fixture instead.

### WebGPU cannot carry a fractional depth-bias constant

`crcbl_hal::DepthBias::constant` is an `f32`, and the field's own doc tunes the
reversed-Z shadow-acne bias as a float against a float depth buffer. **WebGPU's
`GPUDepthBias` is an `i32`.** The two slope fields (`slope_scale`, `clamp`) are
floats and map directly, but the constant cannot: WebIDL's `[EnforceRange] long`
conversion throws on a non-integer, so a fractional constant cannot reach
`createRenderPipeline` at all.

The WebGPU replayer refuses a fractional or out-of-`i32` constant loudly, naming
it, rather than truncating — `1.9` silently becoming `1` would change a tuned
bias invisibly. So an integer constant works and a fractional one is rejected on
this backend only; the other three carry the float.

The decision this needs is not the backend's to make: either
`DepthBias.constant` becomes an integer on the seam (matching WebGPU and D3D12's
`DepthBias`, which is also an `INT`, while Vulkan's `depthBiasConstantFactor` is
a float), or WebGPU is documented as a lower-fidelity target for depth bias and
the engine keeps its shadow constants integral. `pipeline.rs`'s `DepthBias` doc
discusses sign and magnitude as floats and does not mention that a quarter of
the backends cannot carry a fractional one — that is where the outcome belongs
once it is decided.

### The stream decoder caps a SPIR-V module at 65 536 words

`crcbl-webgpu`'s decoder bounds `ShaderModuleDesc::spirv` by `MAX_ELEMENT_COUNT`
(`1 << 16` words = 256 KiB), the same cap every counted list on this stream
uses. A real SPIR-V module can be larger — a big compute shader clears 256 KiB
easily — so pointing the decoder at one would refuse it as `InvalidLength`.

**It cannot bite the WebGPU path**, and that is why it was left. A browser
consumes only `wgsl`; on a browser build the engine hands `create_shader_module`
a descriptor whose `spirv` is empty, so the field the cap guards is never
populated on the only path this decoder runs. The ceiling is reachable solely by
aiming the Rust decoder at a stream carrying real SPIR-V, which is a test or a
tool, never production.

If a future use does stream SPIR-V through this decoder, the fix is a per-field
cap sized to a shader rather than the shared element count — the two limits
answer different questions and `MAX_ELEMENT_COUNT` was sized for the second.

### Anisotropy: the limit says one, the replayer passes more through

`halLimitsFor` reports `max_sampler_anisotropy: 1` and withholds
`Features::SAMPLER_ANISOTROPY`, while `webgpuMaxAnisotropyFor` passes an ask
above 1 straight to `createSampler` and lets the device clamp. Both halves are
argued where they are written and neither is a bug, but together they mean a
caller who respects the reported limit never exercises the pass-through, and one
who ignores it gets whatever the device does.

The alternative is refusing everything above 1, which would make the seam's
anisotropic filtering permanently unreachable on WebGPU. That is why it was not
done — but it is a decision worth confirming rather than one that should sit
implicit in two files that do not reference each other. WebGPU has no query for
the maximum a device supports, which is why the reported limit is 1: it is "no
ceiling this backend can guarantee", not "more than one is refused".

### A `GPUSampler` reports nothing but its label, so no browser check can confirm one

Group L asserts `instanceof GPUSampler` and an empty device error queue, and
that is the ceiling of what is observable: `GPUSampler` exposes no filters, no
address modes, no clamps and no comparison. Everything the seam sends is
verified against the node stub in `web/tools/gpu-replay.mjs` and nowhere else.

So a translation bug that produces a _valid_ sampler with the wrong filtering
would pass the browser gate. The only thing that would catch it is a rendered
frame that depends on the sampling — which is what the parity gate in roadmap
slice 10 is, so this closes there rather than needing its own machinery. Stated
because "group L is green" must not be read as "the sampler is right".

Related and smaller: the probe uses `anisotropy: 1.0`, so the pass-through above
is exercised only against the stub. Nothing verifies what a real Dawn does with
`maxAnisotropy: 16`. Deliberate — the probe exists for the `lod_max` sentinel,
and an anisotropy probe would be measuring the machine rather than the seam.

### `ImageDesc` has no `view_formats`, so sRGB reinterpretation cannot work on WebGPU

`ImageViewDesc::format` documents itself as free to differ from the image's "for
sRGB reinterpretation". **On WebGPU that documented capability does not work.**
`GPUTextureDescriptor.viewFormats` is the list a texture may be reinterpreted
as, it is fixed at creation, and WebGPU refuses a view whose format is neither
the texture's own nor in that list. `ImageDesc` has no field to carry one, so
the replayer creates every texture with the default empty list and any
reinterpreting view is refused by the browser.

The replayer deliberately does **not** invent a list — granting a permission the
caller never asked for is worse than the refusal, and on a real driver it costs
optimisations the texture would otherwise get.

The fix is a `view_formats` field on `crcbl_hal::ImageDesc`. It is not
WebGPU-specific: Vulkan wants the same list through
`VkImageFormatListCreateInfo`, and D3D12 has the equivalent. So this is a seam
gap the WebGPU work surfaced rather than a WebGPU workaround, and it wants doing
where every backend can use it. Recorded in `web/engine/gpu-replay.js` above
`GPU_TEXTURE_USAGE` and on the probe's view descriptor.

### `ImageDesc` and `ImageViewDesc` state contracts nothing enforces

Three of them, found while putting both descriptors on the wire. Each is prose
in `crates/crcbl-hal/src/resource.rs` with no check anywhere and no named owner:

- **`ImageDesc::mip_levels` and `samples` have no documented floor.**
  `BufferDesc::size` says "must be non-zero" and these say nothing, so whether a
  zero is a seam violation or a backend's problem has no answer. The stream
  carries them through verbatim — the encoding refuses malformed streams, never
  invalid descriptors, which `41-webgpu-stream.md` now states as a rule — so the
  question lands on whatever creates the image.
- **`ImageViewDesc::format` "must be compatible with" the image's format**, and
  nothing defines compatibility or says who checks it.
- **`Extent3d::height` says "`1` for `ImageType::D1`"**, also unenforced.

All three land on the WebGPU replayer in the slice that executes these commands,
and it has nothing to enforce them against: it sees the descriptor and not the
seam's intent. What is needed is a decision about where each belongs — the
descriptor's constructor, a debug assertion in the HAL, or explicitly the
backend's — not more prose.

### The sweep for test-restated isolation defaults is finished, and three candidates were declined

Every sample was reviewed on 2026-08-16 for the shape horde's `Setup` had: a
knob that keeps a test off real hardware, defaulting to the production value, so
that every test restates the test value and a test that forgets it opens an
audio device or writes to the developer's disk. Two instances were found and
fixed — horde's `Setup::default`, and the `--headless` that `breakout_null` and
`sandbox_null` did not pin. What follows is what was looked at and left, so the
idea does not get re-proposed from scratch.

**`Audio::new(true)`, restated 24 times — declined.**
`apps/asteroids/src/audio.rs` has 8, `apps/horde/src/audio.rs` 7,
`apps/flappy/src/audio.rs` 6 and `apps/breakout/src/audio.rs` 3, none behind a
per-file helper. It is repetition, but not this shape: `headless` is a
**required positional argument** to `Audio::new`, so no default is fighting
anyone and a test cannot forget it, only actively type `false`. There is no
silent path to a device. Wrapping it in a per-file helper would be churn for
symmetry.

**`crcbl::args::Common`'s `headless: false` — must not be flipped.** It looks
like the horde case and is the opposite of it. `Common::consume` parses by
mutating a default-constructed `Common` — `"--headless" => self.headless = true`
— so production genuinely depends on the `false`, and `crcbl::args`'s own tests
already assert the default is windowed. The test-side restatement it would
otherwise cause is already absorbed: every app with a loop has a
`headless(frames)` helper in its `src/app.rs`, so each app states it once. The
same argument covers `apps/sandbox/src/args.rs::parse` against that app's
`Options::default`.

**horde's `Setup::workers: None` — declined.** It is the production value
sitting in a default, and it does hand every horde unit test a machine-wide
thread pool. But no test site overrides it, so nothing is being fought, and a
pool is not a device or a file. Revisit only if the pool starts costing suite
time.

Apps confirmed to have none of the shape, so nobody need look again:
`asteroids`, `flappy` and `breakout`'s in-crate `game.rs` tests all funnel
through a single `Harness::new` that passes `headless` once; `hud` and `lumen`
have no audio, score file or headless knob in their game code; `sim`'s binary
has no shell, audio or store, so its e2e helper needs no isolation flag at all.
`crcbl-shell`'s `WindowDesc::default` has `visible: true`, but its callers are
either real window creation or e2e suites whose point is a real window.

### Counted claims in doc comments have not been swept, only spot-fixed

`CLAUDE.md`'s rule is that a count of code elements — "four impls", "the three
call sites" — loses the number entirely, because nothing recomputes prose. Two
present-tense violations were found and fixed on 2026-08-15: `crcbl::args`'s
`Common` said a shared flag "reaches four games" when six samples hold it, and
`crcbl`'s umbrella test doc said it builds "the loopback pair the four games
build" — wrong in composition as well as count, since `apps/horde` has no
transport at all and `apps/hud` does.

**The sweep was not finished.** A regex over every doc comment in `crates/`
returns roughly a hundred counted phrases, and most are legitimate and must not
be "fixed": "two backends" and "three backends" usually name the real backend
set, "three samples" is often an MSAA sample count, `crcbl-ui/src/budget.rs`
means statistical samples, and a great many are past-tense accounts of what a
duplication looked like before it was extracted — which the rule explicitly
exempts and which are worth keeping.

So the remaining work is not mechanical. What would make it tractable: restrict
to **present-tense counts of `apps/*`**, which is the population that actually
grows. Candidates noticed but not verified: `crcbl-render/build.rs` ("five times
— the four samples and `crates/crcbl-render`", "three games"),
`crcbl-ui/src/debug.rs` ("four samples each remembering to add the same
module"), `crcbl-ui/src/menu.rs` ("three samples the same arithmetic"). Each
needs reading before editing — the count may be right, and the past-tense ones
are correct as they stand.

### rustc's lint suppression inside external macros is a hazard for every forwarding macro we write

`unconditional_recursion` is a rustc lint and this workspace runs `-D warnings`,
so a hand-written `fn counters(&self) -> FrameCounters { Self::counters(self) }`
with no inherent `counters` to reach fails the build. **The same code produced
by a `#[macro_export]` macro from another crate does not** — rustc suppresses
its lints in external macro expansions — so it compiles clean and recurses
forever at run time.

Measured, not assumed: deleting the inherent `counters` from
`apps/hud/src/gpu.rs` warns with the block written out and compiles silently
once `crcbl::impl_game_gpu!` produces it.

`impl_game_gpu!` handles its own case with a `const _` block that coerces each
inherent method to a function pointer in a scope where neither trait is
imported, so path syntax cannot reach the trait method and a missing one is
`E0599`. **The general lesson is not handled anywhere**: any future macro that
expands to `Self::method(self)` forwards has the same hole and will not be
warned about it. `web_exports!` is not affected — it forwards to free functions
in `crcbl::web`, not to same-named methods — but it is the kind of macro that
would be.

Worth deciding whether the guard shape becomes a convention with a name, or
whether a `trybuild`-style compile-fail test is worth a dev-dependency to pin
it. Neither has been done.

### Two audits' worth of doc drift is fixed; the mechanism that produced it is not

The 2026-08-15 sweep corrected `docs/plan/ROADMAP.md` and most of the numbered
stage docs, and the pattern behind the drift was uniform: a doc says work is
missing, the work lands, and nothing connects the two. Three modules
(`crcbl_render::counters`, `crcbl_render::cull_stats`,
`crcbl_shaders::declaration_order`) already quote the sentence they close, in
their own headers — which is what made them findable — and that convention is
worth spreading rather than leaving to whoever happens to remember.

Not proposed as a lint: what would have to be checked is prose. Recorded so the
next sweep starts from "who quotes their plan doc" rather than from nothing.

### `crcbl-dx12` points at a backlog note about `crcbl::screenshot` that is not here

`an_offscreen_ring_draws_reads_back_and_comes_round_again`, in
`crates/crcbl-dx12/src/swapchain.rs`, explains its closing `TransferSrc` →
`Present` barrier with "see the note about `crcbl::screenshot` in
`docs/backlog.md`". There is no such note, and
`grep -n screenshot docs/backlog.md` finds nothing about barriers at all — it
was either never written or deleted with something else.

The defect it was about is fixed:
`crcbl::screenshot::OffscreenSetup::draw_and_readback` now brackets its copy
with `Present` → `TransferSrc` and `TransferSrc` → `Present`, and
`every_readback_barrier_declares_the_state_the_image_is_actually_in` replays the
null backend's recorded stream to hold it there. So what is left is a dangling
cross-reference in a doc comment, in a crate outside the paths that fix owned.
Either repoint it at that test or drop the clause; it is a one-line edit and
needs a Windows-crate touch, not a decision.

### The Win32 pointer-clip tests are held out of the ordinary sweep

Three flakes across the session, on **two different tests** and **three
different assertions**, every time for a commit that touched no shell code
(`d9ee566`, `28fc1b7`, `0354eec` — all renderer or CI changes).

One shape underneath all of them: the test compares a clip rectangle the system
applied against one this process computed, and both operands move. The desktop
repositions windows, the foreground is contended by whatever else the runner is
doing, and this runner changes its display set mid-run — the behaviour that made
`refresh_clip` refuse a degenerate refresh.

Two rounds of narrowing helped and did not fix it: reading through
`confined_to_client` immediately before asserting, and re-deriving the rectangle
after a restore rather than reusing one from before the minimize. The prediction
in this entry was "if it flakes a third time, stop patching and quarantine" — it
did, so both are now `#[ignore]`d.

**They are not disabled.** `run-win32-e2e.ps1` passes `--run-ignored all` and
runs on a real interactive desktop, so they still gate there — the only place
their preconditions actually hold. What they stop doing is failing the workspace
sweep, where nothing guarantees a foreground window or a stable display set.
Deleting them would have been worse: a process that keeps the cursor clipped
after losing focus has taken the desktop hostage.

Still open and unrelated: the **focus** flake (three instances), where
`focus_and_confirm` loses the foreground.

**A fourth instance, on a third test, and it found a real bug (2026-08-15).**
`warping_the_pointer_moves_it_to_a_position_in_the_window` failed on `5889a3c` —
a commit changing one JavaScript file and two markdown files — reading the
cursor back at exactly the client origin against an expected origin-plus-offset.
So the warp had not moved the pointer at all, and the same signature as the
other three: no shell code in the commit.

The cause is the one this family keeps pointing at. Windows refuses
`SetCursorPos` from a process that is not in the foreground, and
`win32::input`'s `warp_to_client` **discarded the `BOOL`** — along with a failed
`ClientToScreen` and a bad window handle — so a warp that moved nothing returned
`()` and the seam's `warp_pointer` reported `Ok`. The mismatch then surfaced as
a coordinate wrong by precisely the offset requested, which reads as a
conversion bug and is not one. It now returns `ShellError::Backend` naming the
foreground requirement; the two internal convenience warps
(`recentre_if_near_edge`, and the initial centring in `set_pointer_mode`) log
instead of propagating, because in both the pointer mode is already established
and only the courtesy move failed.

The test is now `#[ignore]`d with the other two. **That does not make the fix a
quarantine:** the code change stands on its own — three swallowed failures in
one function — and would be right if the test never flaked again. What the
`#[ignore]` buys is that the _precondition_ is asserted where it holds.

**Worth noticing about the family as a whole:** three of these have now been
diagnosed as environmental and one turned out to be a real defect hiding behind
an environmental symptom. The lesson is not that the quarantine was wrong; it is
that "flaky on a shared runner" and "swallows the error that makes it flaky" are
the same finding seen from two ends, and the next one in this family deserves a
look at what the failing call ignores before it is filed as the desktop's fault.

### `GpuInstance::flags` is a bare `u32`, not `bitflags`

**Recommendation taken, yours to override.** `LIVE = 1 << 0` is the first
defined bit and more are coming (§3.3's own needs, topic 18's per-instance
toggles), so `crcbl_hal::Features`-style `bitflags` is the house pattern and
would normally win.

It lost on one fact: the type has to live where the layout lives, and
**`crcbl-shaders` has no dependencies at all, deliberately** — its `Cargo.toml`
says so, because the library is what a not-yet-written backend consumes.
`bitflags` would be its first, and taking a new dependency is your call. The
alternative — a wrapper type in `crcbl-render` — would be a second
representation of the same word, which is the drift `crcbl-shaders` exists to
prevent.

So: an associated const `GpuInstance::LIVE`, documented as bit 0. Revisit when
topic 18's toggles land; the cost of the switch is one dependency on
`crcbl-shaders` and nothing else.

Calls made on judgement during the 2026-08-09 planning session, listed so they
can be confirmed or reversed without re-deriving anything. **Each says what was
decided, why, and what reversing it costs.** Delete an entry once it is
confirmed; the rest of this file assumes them.

- **`naga` added as a dev-dependency of `crcbl-shaders`, taken without asking.**
  A new dependency is normally the user's call. Taken because the alternative
  was leaving three of four committed shader artifacts validated by nothing —
  the gap that let `wgsl/ui.wgsl` ship for months with a `var<uniform>` carrying
  no binding decoration, which `crcbl-wgpu` could never have loaded. naga is the
  tool that would have caught it, is already in `Cargo.lock` through wgpu at the
  same version, and is dev-only so it does not ship. `git diff Cargo.lock` is a
  three-line dependency edge and **no new package entered the graph**;
  `cargo deny` is clean. **To override:** drop the dev-dependency and the WGSL
  artifacts go back to being unchecked, or find a validator that is not naga.

- **The shader manifest's section order was locale-dependent, and that broke
  `main` a third time.** `compile-shaders.sh` iterates `shaders/*.slang`, and a
  glob is sorted by the caller's collation: `en_US.UTF-8` ignores the
  punctuation and puts `mesh_shader.slang` before `mesh.slang`, while `C`
  compares bytes and puts `mesh.slang` first. So the committed manifest carried
  one developer's locale, CI regenerated the other order, and the byte
  comparison refused it — with every artifact identical and only the section
  order differing. `export LC_ALL=C` fixes it and the manifest was regenerated.

  Worth keeping because the class is general: **this project pins its compilers
  and now its validator, and the environment those run in is provenance too.**
  Any glob, sort, `uniq` or `tr` in a build script has the same exposure. It
  went unnoticed for months because no two shader filenames collided this way
  until `mesh_shader.slang` arrived next to `mesh.slang`.

- **CI's `spirv-val` is pinned to a fixed `.deb`, after an unpinned one broke
  `main` twice.** The shader job installed whatever `spirv-tools` the runner
  image carried. On 2026-08-09 that was **SPIRV-Tools v2025.1**, which rejects a
  valid mesh shader over `VUID-PrimitiveTriangleIndicesEXT-…-07054` — claiming
  the indices decoration is used without `OutputTrianglesEXT`, on a module that
  declares that execution mode on both entry points reading the decorated
  variable.

  **This was established rather than argued.** Both validators were fetched and
  run locally against the same artifact: v2025.1 rejects it, v2026.1 and v2026.3
  accept it, and radv and lavapipe both render it correctly. So it is an
  upstream validator bug fixed between those releases, and the artifact is
  sound. The pin is Ubuntu's own `.deb` at a fixed version, because LunarG
  publishes no repository for the SDK originally reached for — that first
  attempt 404'd and failed the job a second time, which is its own lesson about
  pinning to a URL nobody checked.

  **To override:** if you would rather not depend on a validator version, the
  alternative is to stop two mesh entry points sharing one
  `PrimitiveTriangleIndicesEXT` variable — split `amplifiedMeshMain` into its
  own `.slang` — which makes the artifact acceptable to v2025.1 too. That works
  around a fixed upstream bug in shader structure, which is why it was not
  chosen.

- **Metal's `DRAW_INDIRECT_COUNT`: the seam was _not_ reshaped.** This reverses
  an explicit instruction ("update the seam and get all features supported in
  all the native backends"), on evidence found after it was given: `wgpu-hal`
  declines the same feature on Metal — `wgpu-types` documents
  `MULTI_DRAW_INDIRECT_COUNT` as DX12 and Vulkan only, and its Metal backend
  contains no multi-draw code at all. Two independent implementations reached
  the same conclusion, so it is a Metal API fact rather than a gap. With mesh
  shaders as the primary geometry path, Metal sits on the primary path anyway
  and the count only affects the fallback. **To override:** the seam grows a
  "record indirect work before the pass opens" step and Metal builds an ICB from
  the count buffer in a compute kernel — real work, and it makes the seam less
  Vulkan-shaped, which `crcbl-hal` has resisted so far.
- **Shader pipeline: four independent Slang lowerings kept, plus guardrails.**
  Godot's SPIR-V-as-single-IR model would make the `SV_InstanceID` class of
  divergence structurally impossible, but costs two vendored C/C++ translators
  and cannot serve the WGSL leg anyway (naga's SPIR-V frontend rejects
  `DrawParameters`). Recorded as reopenable in `docs/plan/02-vulkan-backend.md`
  with a named trigger. **To override:** adopt SPIRV-Cross and spirv-to-dxil for
  the native targets.
- **`crcbl-wgpu` should report capabilities honestly rather than pinning to a
  low tier.** wgpu on native exposes bindless, multi-draw-indirect-count, ray
  query and mesh shaders; the reduced set belongs to the browser, not the crate.
  **To override:** keep it deliberately limited as a pure triage backend, and
  say so in its crate docs so the pinning is not read as a bug.
- **The editor is native-only.** `10-wasm-webgpu.md` called editor-in-browser a
  stretch that "should mostly work by construction"; the asset browser, OS
  drag-drop and the file watcher are all native-shaped and nobody examined it.
  **To override:** scope what a browser editor would actually do about those
  three.
- **`crcbl_ui::hud` gets deleted, not extended.** It has no consumer, and the
  obvious fix (a `color` on `Label`) builds on the model topic 7's CSS rewrite
  replaces. **To override:** add the field and have the samples adopt it,
  knowing it is throwaway.
- **towers co-op and arena are native LAN**, and arena's prediction work is
  therefore validated against injected latency only. towers loses its
  mixed-native/browser marquee session. **To override:** host something.
- **The `delve` sample was folded into `shard` before it was written.** It was
  proposed to fill the web-flagship vacancy; shard's web milestone fills it, and
  two samples in one genre is duplication. **To override:** split them again.
- **New phase and gate names are inventions**: P7B (raster twin), P7C
  (ray-traced path), S4B (lumen), S4C (quarry), S6B (shard web slice), S7
  (bracket). So are the sample names lumen, quarry, shard and bracket.
- **Point-light shadows moved into MVP.** They were post-MVP; the raster twin
  has to cover every light type ray-traced shadows cover, so they follow from
  the parity decision rather than being a separate call.
- **bracket keeps a single-player web demo** — client and matchmaking server
  in-process over `InMemoryTransport` — rather than shipping no web build.
  Preserves sample rule 7 and demonstrates the matchmaker and rating curve; only
  the transport is absent.

## Owed

The S1B findings in `docs/plan/ROADMAP.md` were the substantive list — six
places two unrelated games were pushed into the same workaround. **All six are
now closed**: 1 by `SpriteRenderer` (P4B), 2 by `crates/crcbl/src/web.rs`, 3
inside the phase that found it, 4 by `crcbl::store::record::Record`, 5 by
`crcbl_audio::mixer` reached through the blanket `impl AudioSource for Arc<T>`,
and 6 by the umbrella's re-exports — verified by reading each sample's manifest
and the crates named above, not by trusting the roadmap's own status column,
which still carries the pre-closure narrative for 2. What is left below has no
phase attached to it.

- **`rect` and `uv` are both `[f32; 4]` and adjacent in `Sprite::new`, and
  nothing but a picture catches a swap.** This is the hazard `SheetDesc`'s doc
  comment names — "two adjacent `u32`s that can be swapped at the call site are
  a bug the compiler cannot see" — reintroduced knowingly when `Sprite` gained a
  constructor, because the alternative (defaulting `uv` to the whole sheet)
  trades a swapped-argument wrong picture for a silently-whole-sheet one. What
  catches it today: `the_instance_layout_is_exactly_what_the_shader_reads`
  asserts the two lanes at their byte offsets from distinct values, and the
  sprite goldens catch it at any call site the golden scenes reach. **A call
  site no golden covers is not covered** — every sample's `art.rs` is in that
  set.

  The real fix is newtypes — a `WorldRect` and a `SheetUv`, so the compiler
  refuses the swap — and it ripples through every `[f32; 4]`-returning helper in
  the samples, which is why it was not attempted inside an API refactor. Worth
  doing when something else is already touching those helpers.

- **`SheetDesc` was considered for the same treatment and declined.** Its own
  doc argues it is already the safe form: five named fields is what a positional
  constructor would be a regression from, and its `label`/`pixels` borrow `'a`,
  so a `new` would carry the lifetime into a builder chain for nothing. The
  measurement agrees — every construction names all five fields, so there is no
  "the default is fine here" population for `with_*` to serve. The half worth
  having is `#[non_exhaustive]` **alone**, and that is a decision to take when a
  sixth field (a mip count, a swizzle) is actually proposed; taking it now would
  break every call site to buy nothing today.

- **`SpriteInstance` in `crcbl_render::sprite_pass` is the same public-literal
  exposure, on the GPU-side twin.** It cannot take `#[non_exhaustive]` without
  thought, because its `bytemuck::Pod` derive and the `..Default` idiom around
  it are load-bearing. Nothing constructs one outside `crcbl-render` today.
  Noted, not investigated.

- **The listener standoff moved from the emitters onto the listener, and the
  subtraction changed precision with it.** Every sample used to compute
  `compute_cue([0,0,0], [dx, dy, 1.0])` — the listener at the origin, and "one
  unit in front" added to _each emitter's_ Z with the same comment copied into
  three files. That standoff is a fact about the camera, so it now sits on the
  listener (`LISTENER_STANDOFF` in each sample, listener at `z = -1`, emitters
  at their true Z).

  `emitter − listener` is arithmetically the same, but **not bit-identical**,
  and the agent's report claiming it was is wrong. The samples that subtracted
  first — horde and flappy — did `(at.x - listener.x) as f64→f32`, one rounding;
  the new path casts each coordinate to `f32` and subtracts inside
  `compute_cue`, two roundings. The error is bounded by the coordinate
  magnitude, and horde's arena is `ARENA_HALF_WIDTH` 48 by `ARENA_HALF_HEIGHT`
  36, so it is on the order of 1e-5 on a direction that gets normalised — far
  below audibility and below every assertion in the suite, which is why nothing
  moved. Recorded because "bit identical" is what someone would otherwise assume
  when reading the diff, and it would be the wrong thing to rely on if these
  coordinates ever grow.

- **`CueGrammar` is a parameter of `Mixer::cue` that every call site passes
  `&CueGrammar::default()` to** — five of them. By the workspace's own rule that
  is a parameter nothing varies, and putting the grammar on the mixer beside the
  listener would collapse `cue(emitter, &CueGrammar::default())` to
  `cue(emitter)`. Deliberately not taken with the listener: "this mixer's
  grammar" is a bigger claim than "this mixer's listener", and it was not part
  of the decision that was delegated.

- **`Listener` has a position and no orientation**, so `compute_cue` still
  hard-codes "the listener faces +Z" and its module docs say so. That is the
  field the type was made `#[non_exhaustive]` to be able to gain; nothing needs
  it until a game turns its camera.

- **`docs/code-review.md` cites two latent panics in `play_panned` that are not
  there and now cannot be.** The entry names
  `apps/breakout/src/audio.rs:130,169` for an `id as usize - 1` underflow and a
  `fade_env` underflow. The first was already fixed before this session — the
  current code takes `bank.create_voice(id)` behind a `let Some(…)` guard and
  its comment says in so many words that the `id - 1` index it used to have is
  gone — and `play_panned` itself no longer exists, so the citation is doubly
  stale. Left alone because it is unclear whether that file is a living findings
  list to be pruned or a dated snapshot to be preserved; **that is the decision
  needed**, and it applies to the whole document rather than this one entry. The
  `fade_env` half was not re-checked.

- **Nothing has listened to the migrated cues on a real device.** Every sample's
  audio was rewritten onto `crcbl_audio::mixer` and the checks are all
  structural: buffer shapes, pan ordering, voice counts, loop seams. Two
  audible-only risks are unverified. Asteroids' engine changed from a pulsed
  one-shot to a continuous loop at `ENGINE_GAIN` = 0.25 against the one-shots'
  0.5, and that ratio was chosen by reasoning rather than by hearing it. The
  loop seam is asserted to be a bare tone with no envelope, which is the right
  property, but nobody has heard whether ten joins a second is inaudible in
  practice. Both want a person with headphones.

- **How many props a view holds is not pinned by anything.** The count over the
  whole arena is —
  `game::tests::the_scatter_is_sparse_and_never_pens_the_player_in` asserts it
  lands between 30 and 70 over 64 seeds — and so is the fact that the layer is
  culled, but the number a 960 × 720 window actually shows is only observable
  from `SceneStats::props` in a running frame. A test in the shape of
  `the_visible_ground_is_bounded_by_the_view_and_not_the_arena` would pin it and
  would make `PROP_DENSITY`'s "a handful in a view" checkable rather than
  asserted. Not written because the honest bound is wide — the scatter is random
  and the player's own glade sits in the first view — and a wide bound on one
  seed is a weak test.

## The samples depend on `crcbl` and `std` — met, with one exception

Reached on 2026-08-03: every one of
`apps/{bare,breakout,flappy,asteroids,horde,sandbox}/Cargo.toml` names `crcbl`
and nothing else under `[dependencies]` — the nine simulation crates are
re-exported, `glam` is `crcbl::math` and `log` is `crcbl::log`. What is left:

- **The `crcbl-sprite`/`bake` build-dependency is the one exception, and it was
  taken rather than decided.** The four game manifests carry
  `crcbl-sprite = { features = ["bake"] }` under `[build-dependencies]`, with
  the reason written in each: `crcbl` re-exports the `.crpix` _reader_ and not
  the PNG _encoder_, and cargo's `resolver = "3"` keeps the bake feature out of
  the shipped binary because build-dependency features resolve separately. The
  alternative — making the umbrella's heavy re-exports optional so a build
  script can take `default-features = false, features = ["bake"]` — buys a
  literal zero-exception rule at the price of a feature matrix on the umbrella's
  public surface. **Still a decision nobody has made**; the exception is
  defensible and is what ships today.

## Frame pacing sleeps on the monotonic clock, which is not what a display does

`crcbl::engine::Pacing` chooses a present mode (`Auto`/`Vsync` → `Fifo`,
`Adaptive` → `FifoRelaxed`/`Mailbox`, `Off` → `Mailbox`/`Immediate`; `Auto` may
then rebuild onto `Adaptive`'s once the display has been read) and `FrameLimit`
paces the loop by sleeping the difference between the last frame's length and a
period, on `std::time` — `Clock::Real` in `crates/crcbl/src/engine.rs`, where
the `wasm32` arm of `sleep` is deliberately a no-op because the browser paces
frames itself. That is the whole mechanism, and it is open loop: it never learns
when a frame was actually shown.

**The seam, the engine wiring and the Vulkan backend now exist.**
`Features::PRESENT_FEEDBACK`, `PresentInfo::present_id` and
`Device::wait_until_presented` are the capability-named seam the note below
asked for, and `GpuContext::acquire` calls the wait for the present
`FRAMES_IN_FLIGHT` behind the frame it is about to start
(`GpuContext::present_to_wait_for` is the arithmetic, tested without a GPU). A
device that does not advertise the flag answers `Ok(())` immediately, which is
what lets the call site have no branch on which backend is underneath.
`crcbl-vk` answers it with `vkWaitForPresentKHR` where the driver has
`VK_KHR_present_id` + `VK_KHR_present_wait`; the other four `Device` impls are
still the immediate answer, and `FrameLimit` is untouched and still the only
thing pacing a loop on a device without the capability.

What is still owed:

- **`vkWaitForPresentKHR` is verified on radv only, and CI does not run it at
  all.** `VK_KHR_present_wait` is a driver-conditional extension: this
  developer's radv exposes it, lavapipe does not (`vulkaninfo` lists it under
  both RADV devices and under neither llvmpipe entry), and lavapipe is what CI
  runs for both `vk e2e` and the wayland sandbox pass. So every CI leg exercises
  the _absent-capability_ path and proves nothing about the wait. `run_sandbox`
  in `crates/crcbl-shell/tests/run-wayland-e2e.sh` says so on stderr rather than
  passing quietly, and asserts the two halves agree when the extensions are
  there — the backend's own `vkWaitForPresentKHR on present` line has to appear,
  which is the only thing that tells a real wait from the immediate `Ok(())`.
  Closing this needs a CI leg with a driver that has the pair; nothing else will
  do it.

- **The windowed half of the `wait_until_presented` id check now has a check.**
  `run-wayland-e2e.sh` runs the sandbox with `--wait-unpresented`: on its first
  tick the sandbox calls `Device::wait_until_presented` with `u64::MAX` on its
  real swapchain and logs whether the device answered at once, and the
  present-feedback block asserts the success line. Falsified on radv: with the
  id guard removed, the wait blocks the whole 60 s timeout and the pass goes
  red; with it, it answers in microseconds. What is still not independently
  checkable is the offscreen guard alone — an offscreen entry never records an
  id, so the id guard answers for it, and the vk e2e
  (`the_offscreen_ring_answers_a_present_wait_with_no_swapchain_to_wait_on`)
  only goes red when **both** are removed (removing both segfaults radv on a
  `VK_NULL_HANDLE` swapchain).

- **Metal's `addPresentedHandler:` path is verified by nothing, on any
  machine.** This is the wider of the two coverage holes in present feedback,
  because unlike Vulkan's it is not driver-conditional — it is simply that no
  automated run anywhere has a drawable. `crcbl_mtl::swapchain`'s
  `attach_presented_handler` is the only code in the capability that is not
  plain Rust, and the only test that reaches it is
  `a_layer_swapchain_acquires_a_drawable_and_presents_it`, which numbers each
  present and waits for its number. That test is `#[ignore]`d **and** excluded
  by name from the `mtl-e2e` job's filter, because a headless runner's detached
  `CAMetalLayer` vends no drawable at all — so the job says nothing about it and
  never will.

  What would close it: **a person on a real Mac running
  `crates/crcbl-mtl/tests/run-mtl-e2e.sh`**, which needs a window server. That
  is the only thing that has ever executed a Metal present at all. Two outcomes
  are worth writing down when someone does. If it passes, the handler fires for
  a _detached_ layer and the capability is confirmed end to end. If it times out
  on the first wait, the handler does **not** fire for a layer outside a view
  hierarchy — which would be a real constraint, not a bug in the ledger, and
  would mean the test needs an `NSWindow` rather than a detached layer. Neither
  is known today; nothing has run it.

  Everything on this side of the callback _is_ covered and runs on every host:
  `crcbl_mtl::present`'s tests pin the immediate answers, the strictly
  increasing id, the out-of-order report, the reset across a reconfigure, the
  lapsed timeout, and that a blocked wait is genuinely woken rather than left to
  time out and re-check its condition. That module is compiled off macOS under
  `cfg(test)` for exactly that reason.

- **`Condvar::wait_timeout_while` reports success after a timeout it should have
  failed, and that is not a Metal fact.** Found while falsifying
  `crcbl_mtl::present`: with `record_shown`'s `notify_all` removed, every
  present wait still returned `Ok`, because `wait_timeout_while` re-tests its
  condition _after_ the deadline lapses and reports "not timed out" when it has
  since become true. A missing wake-up therefore costs a whole
  `PRESENT_WAIT_TIMEOUT` per frame and raises no error anywhere. The test now
  bounds the elapsed time from above as well as below, which is what catches it.
  Audited for the rest of the workspace on 2026-08-06: the only other condvar is
  `crcbl-jobs`' pool, and it calls the bare `Condvar::wait` with **no timeout**
  — a missed wake-up there is throughput, never a reported success — so this is
  the only `wait_timeout` pairing in the tree and no other instance needs the
  bound.

- **Read the real present mode with `VK_EXT_present_timing`.** The seam, the
  Vulkan backend and the engine's use of them now exist; **what no run has
  produced is an answer other than `Unknown`.** `DisplayTiming` and
  `display_timing_from_refresh_nanos` in `crcbl_hal::swapchain`,
  `Features::PRESENT_TIMING` and `Device::display_timing` are the capability-
  named seam; `crcbl_vk::present_timing` is the hand-written FFI (`ash` still
  has no bindings — rechecked against the pinned 0.38.0+1.3.281);
  `GpuContext::settle_pacing` in `crates/crcbl/src/engine.rs` is the caller,
  asking once after the first present and resolving `Pacing::Auto` against the
  answer with `Pacing::resolve`. The extension is **ratified**, not provisional
  as this entry previously said: `supported="vulkan" ratified="vulkan"` in
  `vk.xml`, revision 3, which is what `/usr/include/vulkan/vulkan_core.h`
  declares and what RADV exposes here.

  Still owed:
  - **Only the `Unknown` arm has ever executed against a driver, and that is now
    a measured result rather than an untested path.**
    `crates/crcbl-shell/tests/run-wayland-e2e.sh` drives the sandbox on RADV (RX
    7900 XTX, Mesa 26.1.6) against a nested headless sway 1.12. The chain
    negotiates —
    `crcbl-vk: present timing enabled (VK_EXT_present_timing + VK_KHR_present_id2)`
    — the query reaches the driver after the first present, and the answer is
    `hal: display timing Unknown; asked for Auto, pacing Vsync`, in both display
    modes. `Fixed`, `Variable` and `Stepped` are still covered by unit tests on
    the pure mapping and by **nothing else on any machine**. The script asserts
    only that the engine _asked_; it shouts when the arm is `Unknown` rather
    than asserting a cadence nobody has seen.
  - **The pacing resolution therefore runs on one input everywhere it runs for
    real.** `Pacing::resolve` in `crates/crcbl/src/engine.rs` maps (requested,
    observed) to the pacing in force, and
    `auto_is_the_only_pacing_the_display_can_change` walks all sixteen pairs —
    but only `(Auto, Unknown)` has ever executed against a driver, on any
    machine. Everything the `Variable`/`Stepped` branch claims (that the rebuild
    onto `FifoRelaxed`/`Mailbox` is right, that it improves anything on a real
    VRR panel, that one rebuild during start-up is not visible as a hitch) is
    argued, not measured. A VRR panel driven by a compositor that reports a
    cadence is what would settle it, and that is the same missing machine the
    entry above needs.
  - **That `Stepped` resolves to `Adaptive` is a judgement call, not a measured
    one.** The reasoning is in `Pacing::resolve`'s doc comment: a quantised
    cycle is not a fixed one, so a fixed-vblank wait is wrong there in the same
    way it is on a free-running panel. No driver here has ever emitted `Stepped`
    at all (see the mapping gap below), so nothing distinguishes this from the
    other choice — pacing a stepped panel on vsync at its current multiple —
    except the argument.
  - **Why RADV answers `Unknown` there is partly determined.** Verified: that
    sway session advertises `wp_presentation` and neither
    `wp_commit_timing_manager_v1` nor `wp_fifo_manager_v1` (`wayland-info` on
    the session the script starts), while `libvulkan_radeon.so` contains
    bindings for both of those names — so the Wayland WSI looks for protocols
    this compositor does not offer. **Not determined:** whether
    `vkGetSwapchainTimingPropertiesEXT` returned `VK_NOT_READY` or `SUCCESS`
    with zeroed figures, since `crcbl-vk` maps both to `Unknown` and logs only
    the mapped arm; and whether RADV would report a real cadence on a compositor
    that does advertise those protocols. Hyprland 0.56.2 (installed here)
    implements both, but it cannot be nested inside sway for a safe headless
    experiment — it demands `xdg_wm_base` v6 and sway 1.12 offers v5 — and
    running it on DRM would take over this machine's console. A DRM-backed
    session, or the same run under a compositor with commit-timing, is what
    would settle it.
  - **The four-state reading of `refreshDuration`/`refreshInterval` is taken
    from the proposal text, not from a driver.** In particular the `Stepped`
    case (interval non-zero, not `UINT64_MAX`, not equal to the duration) is a
    shape no driver here has been observed to emit, and the contradictory-input
    arm — an interval that does not divide the duration, mapped conservatively
    to `Unknown` — is a guess about driver bugs rather than a response to one.
  - **One present may not be enough on some platform, and the engine will never
    find out.** The query is one-shot by design — the extension may answer
    `VK_NOT_READY` until an image has been presented, and a driver that answers
    `Unknown` forever must not be re-queried forever — so a platform that needs
    _two_ presents before it will speak reads `Unknown` here and stays on
    `Pacing::Vsync`. No such platform is known; the alternative (retry until it
    answers, or until some count) was declined because the count would be a
    number nobody could justify and the failure it guards against has never been
    seen. A caller on such a platform asks for `Pacing::Adaptive` by name.
  - **The observation is not refreshed, ever.** A window dragged from a fixed
    panel to a VRR one, or a laptop entering power saving, keeps the pacing
    start-up settled on. Declined for this slice: re-reading on every
    reconfigure is the per-frame query on any driver that answers `Unknown` (it
    would re-run on every resize), and a panel that flaps between cadences would
    rebuild the swapchain each time. `GpuContext::set_pacing` is the escape
    hatch, and a game with a monitor-changed event of its own can call it.

  Note that `wait_until_presented` deliberately does **not** return a timestamp
  — a caller that needs one needs a second method, and `VK_EXT_present_timing`'s
  `vkGetPastPresentationTimingEXT` is where it would come from. Only
  `vkGetSwapchainTimingPropertiesEXT` is bound today.

Considered and declined while shaping the seam, so it is not re-argued: having
the wait return an enum distinguishing "waited" from "this device cannot observe
presents". The distinction has exactly one consumer — a log line — and
`caps().features` already answers it once at start-up, where the engine now logs
it. Also declined: refusing with `HalError::Unsupported` on a device without the
capability. It was tried as a falsification and the engine's frame loop fails
every frame under it, which is the argument against it in one line.

## `--pacing` and `--fps` reach the engine; three quarters of what they can ask for is unexercised

`crcbl::args::Common::pacing` and `::limit` carry the two values,
`Common::gpu()` hands the first to `GpuContextDesc` through `GpuOptions`, and
`Common::loop_config()` hands the second to `Loop::new`, which applies it to the
clock. `run_sandbox_paced` in `crates/crcbl-shell/tests/run-wayland-e2e.sh`
proves the whole path on a real Vulkan swapchain — the flag, the present mode,
the logged limit and the measured frame time. What that leaves:

- **The `adaptive` e2e pass runs in CI, and proves only that the present mode is
  reachable.** `run_sandbox_paced` takes the pacing and its expected present
  mode, and the wayland suite opens a second pass with `--pacing adaptive`,
  asserting `asked for Adaptive, pacing Adaptive` and a `FifoRelaxed` swapchain
  — the mode a VRR panel wants, which no run had ever opened one on (its
  coverage was unit tests and nothing else). What is still a missing machine,
  tracked in the `Pacing::resolve` entry above, is whether adaptive _improves_
  anything on a real VRR panel — this pass proves only that the present mode is
  reachable.

- **`--fps` is unobservable on every CI leg without a compositor.** The limiter
  lives on `Clock::Real` by construction, so a headless run takes the flag and
  correctly does nothing with it — which means the macOS, Windows and null-
  backend legs say nothing about it, and neither does the browser, where every
  entry point builds `Clock::manual` and `requestAnimationFrame` is the pacing.
  The wayland pass is the only place it is real.

- **`apps/sandbox` now duplicates eight shared flags rather than six.** Its
  parser is deliberately its own (`crates/crcbl/src/args.rs`'s module docs make
  the case, and it is a real one: the sandbox takes `--camera` and `--title` and
  no `--seed`). The cost is that the sandbox is also the **only** sample the
  Wayland and X11 harnesses drive, so every flag added to `Common` is either
  written twice or untestable against a window system. `--pacing` and `--fps`
  were written twice. Worth deciding once: either `apps/sandbox` consumes
  `Common::consume` for the shared set and keeps its own arms for the rest, or a
  `Common`-consuming sample joins the harness scripts. Not attempted here
  because either is a change to what the harness runs, which is not something to
  do inside a flag slice.

## The CLI scaffold gate has thin timing margin, observed once

`a_scaffolded_project_builds_lints_and_runs_headless` in
`crates/crcbl-cli/tests/cli_e2e.rs` scaffolds a project into a temporary
directory and points `CARGO_TARGET_DIR` at another one, so **every run is a cold
build of the whole engine** — deliberately, because an inherited target
directory would deadlock against the lock the test runner already holds.
`.config/nextest.toml` gives it
`slow-timeout = { period = "60s", terminate-after = 4 }`, a 240s ceiling, and it
has measured **`TIMEOUT [240.174s]`** once and `36.367s` on the rerun — a 6x
spread, cause undetermined, "seen once, unexplained". It has not failed in CI.
What is worth knowing: the margin is a wall-clock budget on a from-scratch
engine build, and it moves with every dependency the engine gains and every
runner GitHub retires. If this job ever goes red on a timeout, the fix is the
`period`/`terminate-after` pair rather than anything in the scaffold.

## P5B — the job system, and the two decisions in front of it

`crates/crcbl-jobs` carries the spawn seam (`Spawn`, `Threads`, `Inline`,
`default_spawner`), the design's two communication primitives — `mailbox`
(latest-wins triple buffer, for states) and `ring` (bounded SPSC, for streams) —
and the work-stealing `pool` with `par_for` in both modes. The order is forced:
the spawn seam and its single-threaded fallback came first (a pool on
`std::thread` would silently have no browser story — spawning _compiles_ on
wasm32 and returns `UNSUPPORTED_PLATFORM` at run time), then the pool, then
adoption. **What is still owed is the worker backend behind the seam.** The
adoption slice found **one consumer, not four**, and that is a fact about the
samples rather than a shortfall: `apps/horde`'s `steer_enemies` is on `par_for`,
and every other candidate collection is smaller than a single chunk — breakout
has forty bricks, asteroids at most forty-four rocks — so a `par_for` over them
would be the serial loop plus a pool. **The "two samples freeze a seam" rule has
therefore not been met**, and `Spawn::threaded` returning a `bool` is still the
most likely thing to give.

- **Only `overlap_sphere_into` has the shared form.** `cast_ray`, `sweep_sphere`
  and `overlap_aabb` are still `&mut self` — nothing parallel calls them yet,
  and `sweep_bolts` (the obvious candidate) reduces into a shared hit list in an
  order the scheduler would choose, so it needs a design decision before it
  needs an API.
- **`STEER_CHUNK` was chosen by argument, not by measurement.** Sixty-four
  enemies a chunk keeps the split independent of the worker count and stays
  under the pool's 1024-slot queue up to 65 536 enemies. Nothing has swept the
  value, and the right time to is when there is a benchmark that isolates the
  pass.

**The atomics are checked by Miri and by nothing else.** x86-64 is
total-store-order, so a `Release` store and a `Relaxed` one compile to the same
instruction and weakening one is invisible to any test on this machine — which
is why the Miri job is load-bearing. It runs **weekly, in `cron.yml`**, a
deliberate choice: the full crate list is minutes of interpretation per PR, and
the per-commit value is concentrated in one small crate. The consequences to
keep in mind: **an ordering regression can sit on `main` for up to a week**, so
any change to the atomics is expected to be run under
`cargo miri test -p crcbl-jobs` (~23 s for its 40 tests) before it is pushed —
written into the crate docs where somebody editing the atomics will see it — and
**after a dependency lands, trigger the cron manually**
(`gh workflow run cron.yml`) rather than waiting for Monday; the weekly job went
red on 2026-08-03 for want of a `libasound2-dev` install and was found only
because it was briefly on the per-PR path.

Also still open: **nothing runs the primitives on a weakly-ordered machine.**
Miri models the memory ordering, which is a stronger check than any test on x86,
but it is a model — an aarch64 runner exercising the same stress tests natively
would be independent evidence, and GitHub offers one. Not attempted, and the
cost is a second `test` leg rather than anything subtle.

**The pool's own gaps**, none of which is a defect:

- **The lost-wakeup window is argued, not tested.** A worker reads the
  submission count under the lock, searches once more, and only then sleeps
  while that count is unchanged; the second search is what closes the gap
  between the first search and the read. Making a test land inside that window
  needs an injection point the pool does not have. It is bounded rather than
  frightening: **waking a worker is throughput, never correctness** — the
  driving thread runs the chunks itself until they are gone, so a missed wakeup
  costs parallelism for one call and cannot hang it.
- **Nothing benchmarks the pool in isolation.** A harness that times the pass
  alone, and sweeps the chunk length, is what would let `STEER_CHUNK` be chosen
  rather than argued.
- **A mode comparison cannot catch a defect that is symmetric across modes**,
  and this was measured rather than assumed: dropping the last chunk of every
  `par_for` leaves both worker-count tests green, because a pool with no workers
  drops it too. Eight other horde tests go red on that mutation, which is what
  actually covers it — worth knowing before anyone reaches for the worker-count
  tests as a general correctness net.
- **One deque, not one per worker.** Only the driving thread pushes today, so a
  per-worker deque would be a queue nothing ever puts anything in. What needs
  them is `scope(|s| …)` fork-join (the design lists it for BVH build), where a
  running chunk spawns more work. Not written, and nothing calls it.
- **`Mutex` + `Condvar` for the sleep, in the frame path.** The design's rule is
  no mutexes in the frame path; this takes one per _submission_, not per job,
  and a worker takes it only on its way to sleep. A futex-style parking scheme
  would remove the lock, and needs the profiler to say whether it is worth the
  reasoning.
- **Considered and declined: aborting the remaining chunks when one panics.**
  Running them all instead lets the panic be reported by chunk index — the
  lowest wins — so a panicking `par_for` fails identically with and without
  threads.
- **A broken completion count hangs the suite rather than failing it.** Three
  mutations each wedge `par_for`'s wait loop instead of going red, because a
  chunk that never finishes is exactly what that loop waits for. A deadline in
  the wait loop would fix the symptom by putting a timeout in the frame path,
  which is a worse trade; the honest note is that this class of defect looks
  like a hang.
- **Considered and declined: `crossbeam-deque`.** It would be the right one, but
  neither it nor any `crossbeam-*` nor `rayon` is in `Cargo.lock`, and a new
  dependency is the user's call. Worth revisiting if one arrives for another
  reason: its growable deque would remove this one's capacity ceiling, past
  which `par_for` runs the extra chunks on the driver.

**`ring` does not implement drop-oldest**, though `21-jobs.md` lists it beside
drop-newest as an overflow policy. It cannot be done from the producer: the read
cursor belongs to the consumer, and a producer advancing it to make room would
be a second writer to it, which is exactly what makes an SPSC ring cheap. `push`
hands the item back and counts the refusal instead, leaving the policy to the
caller. If a real consumer turns up wanting drop-oldest, the honest options are
a consumer-side drain-and-discard or an MPSC design, not a flag on this one.

**The seam has one consumer, and one is not two.** `apps/horde` has used the
spawn seam without asking for anything back, which is evidence and not the two
samples this workspace's rule wants before a seam is frozen. `Spawn::threaded`
returning a `bool` rather than a richer capability is still the most likely
thing to give — horde works around it by handing `Pool::with_workers` an
`Inline` spawner when `--workers 0` is asked for, which is a caller saying
"threads, but none" through the only channel there is.

- **Cross-origin isolation is proved locally, and the worker backend is blocked
  on the pinned nightly's `rust-src`.** The isolation half is done:
  `web/tools/serve.mjs` sends `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`, `web/build.sh --serve` runs it,
  and `run-browser-e2e.sh` refuses a run whose output does not contain the
  `crossOriginIsolated === true` check by name. Two cautions for whoever next
  touches the gate: the browser readback table in `web/run-browser-e2e.sh` was
  measured on Chromium 150 and this machine has 151, where the `Xvfb` +
  SwiftShader row no longer holds — **re-measure it before concluding the gate
  is broken**; and the Pages question is deliberately still open (GitHub Pages
  cannot set either header, `coi-serviceworker` is third-party JS) — if the shim
  is declined, the demos run single-threaded through `Inline` and the roadmap's
  `crossOriginIsolated` gate should be struck rather than left unmeetable.

- **Blocked 2026-08-06: the wasm worker backend needs `rust-src` on
  `nightly-2026-07-02`, which is not installed.** The plan stands — a nightly
  pinned by date for that one target, in the shape `decoder-fuzz` already uses,
  because `rust-toolchain.toml` pins an **exact stable** (`1.97.0`) on purpose
  and its own comment calls a floating channel a broken promise. Measured:
  `rustup component list --toolchain nightly-2026-07-02 --installed` lists
  `cargo`, `rust-std` and `rustc` and **not** `rust-src`, and the build fails
  with
  `library/Cargo.lock does not exist, unable to build with the standard library`.
  No backend was written against a toolchain that cannot build it:
  `default_spawner` still yields `Inline` on wasm, which is a whole answer
  rather than a stub.

  To unblock:
  `rustup component add rust-src --toolchain nightly-2026-07-02-x86_64-unknown-linux-gnu`,
  then re-check `21-jobs.md`'s finding 1 before building on it. Beyond the
  toolchain, three things are known to be in the way and are worth deciding
  before writing code:
  - **A wasm module cannot start its own worker** (`21-jobs.md` finding 2), so
    the backend is an `extern "C"` import the `web/` half implements — the same
    hand-written ABI shape `crcbl-audio`'s `web` module and `crcbl-store`'s OPFS
    path already use, and the reason no engine crate depends on `wasm-bindgen`.
    A `web-sys`/`js-sys` backend would be a new dependency in a wasm graph that
    currently has zero third-party crates, which is the user's call.
  - **The demos are built without atomics**, so the worker path cannot be
    exercised by the existing site at all. Proving it needs a threaded artifact
    the `DEMOS` loop in `web/build.sh` does not build, and
    `wasm-bindgen --target web` has to emit glue that accepts a shared
    `WebAssembly.Memory` — untested here against the pinned 0.2.126 CLI.
  - **The fallback has to be automatic and loud**, because every GitHub Pages
    visitor is in the non-isolated state. The observable to assert is the one
    `apps/horde`'s determinism tests already use — `pool.workers()` and two
    distinct thread ids — not "a worker backend was selected".

## What the scaffold's gate does not cover

`crcbl new`'s template now hosts `crcbl::engine::Loop`, and the scaffold e2e
compiles it, lints it, runs its three unit tests, runs it headless, and — since
the sway start-up moved into `crates/crcbl-shell/tests/sway-session.sh` — runs
it **windowed** against a private headless compositor, asserting the summary
reports the wayland shell at the size the template asks for. Against the null
backend by default and against lavapipe in CI (`CRCBL_CLI_E2E_BACKEND=vk`). What
is left:

- **Nobody has looked at the scaffold.** The windowed pass asserts the window is
  the size it asked for and that frames were presented; it does not assert what
  is _in_ them. Whether the pause menu, the fullscreen toggle and the debug
  panel look right in a generated project is still `cargo run` on a desktop,
  then `ESC`, `F11`, `F3`. Same class as every other "nothing has looked at it"
  entry in _Coverage gaps_.

- **Vulkan validation is not gated on it.** Run by hand against lavapipe with
  `CRCBL_VK_VALIDATION=1 CRCBL_VK_SYNC_VALIDATION=1`, the template's graph is
  clean — 30 frames, no layer messages. It is not in CI because a validation
  error only _logs_: `crcbl run` still exits zero, so the step would be a check
  that cannot fail. Gating it needs the scaffold e2e to read the child's stderr
  for layer messages, or `crcbl-vk` to grow a "fail on validation error" mode
  the sample harnesses could share.

Every combination of {born borderless, toggled with `F11`, imposed by the window
system} × {honoured, refused} is now executed by a harness on both Wayland
(`run-wayland-e2e.sh`) and X11 (`run-x11-e2e.sh`, with and without
`CRCBL_E2E_X11_WM`) — and the X11 F11 pass asserts the summary line's extent
after a clean `WM_DELETE_WINDOW` close as well as the engine's own mode line.
`crates/crcbl-shell/tests/bin/send_key_wayland.rs` and `send_key_x11.rs` are
what drive `F11` at a running sample from outside its process. What is still
uncovered:

- **The null GPU backend is excluded from every mode assertion, and correctly.**
  It presents by doing nothing, so no `wl_buffer` is attached, so the surface
  never maps: `swaymsg -t get_tree` lists no `app_id` for a null-backend run
  where a Vulkan one lists `sh.kryptic.crcbl.sandbox` — observed, not inferred.
  An unmapped surface gets no fullscreen configure, so any mode assertion there
  would be checking a window the compositor does not have.
- **`F11` is only pressed at the sandbox, never at one of the four games.** The
  games take the same engine-owned path (`crcbl::engine::FULLSCREEN_KEY`), but
  no harness presses the key at a running game — only at the sandbox, on both
  platforms.
- **macOS and Windows have shell backends now, and neither has a game-level mode
  pass.** P5C built both, and each has an end-to-end suite that opens a window,
  flips its mode and reports injected input — but that is the _shell_ being
  driven directly. Nobody presses `F11` at a running sample on either, the way
  `run-wayland-e2e.sh` does: Windows would need the key sender pointed at a game
  rather than at the suite's own window, and macOS has no renderer to run a game
  with until `crcbl-mtl` can present — permanently so, since the 2026-08-05
  decision makes Metal the only Apple path. See the platform sections below.

## The docs gate reads more files than any other, and it reads them on wasm32

CI runs `cargo doc --workspace --all-features` on the host **and**
`--target wasm32-unknown-unknown`, both under `RUSTDOCFLAGS: -D warnings`. Three
consequences that cost a round trip each if you do not know them:

- **An intra-doc link to an item that is `cfg`-ed out on the other target is an
  error there.** Write it as a code span instead. `#[cfg_attr]`-ing two versions
  of the sentence puts the same prose in two places and guarantees they drift.
- **`--all-features` builds Linux-only targets on every platform.** A
  feature-gated helper whose `use` resolves only on Linux compiles nowhere else.
  Give it a `#[cfg(not(target_os = "linux"))] fn main` that fails and says why,
  rather than a `cfg` that quietly compiles it to nothing —
  `crates/crcbl-shell/tests/bin/send_key_wayland.rs` is the worked example.
- **Rustdoc is the only gate that notices a public type nobody exported.** A
  `pub` field whose type is `pub` inside a private module is readable and
  unnameable: a consumer can get the value out and cannot write it down.
  `cargo clippy`, `cargo fmt` and the whole test suite pass straight through it,
  because nothing in the crate itself needs the path. Rustdoc reports it as
  `public documentation for X links to private item Y`, which reads like a
  formatting nit and is an unusable API. `RenderState::player_facing` and
  `RenderState::props` shipped that way and were caught only in CI.

None of the three is reachable from a local `cargo clippy --all-targets`, which
is what makes them worth writing down rather than rediscovering. **Run
`cargo doc` before pushing**, both targets, or CI will run it for you.

## Cross-test state, found by adding a window manager and a second monitor

**Both e2e suites run every test in its own process against one long-lived
display, and both had state that survived between them** — a _tail_ of tests
that passed alone and failed in a full run, moving whenever anything was
reordered. Three instances were found and fixed: the X11 pointer (`XTEST` leaves
it wherever the last test put it; `Session::open` parks it at the centre), the
X11 window manager's idea of what is still alive (`Session`'s `Drop` withdraws
and destroys its windows and then **waits for `_NET_CLIENT_LIST` to drop them**;
graded evidence, eight of eight runs clean against two and five for the earlier
attempts), and the Wayland focused workspace (a `FocusedWorkspace` guard puts it
back after a test fullscreens onto the second output).

**The rule that falls out: anything a test moves and does not move back belongs
in `Session`, not in the test.** The pointer, the input focus, the clipboard
owner, the focused workspace and the compositor's idea of which clients exist
are all this kind of thing.

Two blind alleys, recorded so they are not re-run. Neither is the fix and both
looked convincing: giving the `_NET_ACTIVE_WINDOW` message a real server
timestamp instead of `CurrentTime` (`Peer::server_time`, kept — it is correct
EWMH), and asking `openbox` less often or clicking the frame instead. The click
made it measurably _worse_: five runs, 3-5 failures each.

## What the Win32 backend has and has not been run against

The whole of `crates/crcbl-shell/src/win32/` and its e2e suite were written on a
Linux machine and are cross-checked with
`cargo check`/`cargo clippy --target x86_64-pc-windows-msvc`, which do not link
and do not run — **a cross-check proves the code typechecks and nothing more**.
The e2e suite has since run on `windows-latest` in CI (W1–W4 and the rounds
after), so the window lifecycle, input, clipboard and mode flips are executed;
everything below is what those runs still do not reach.

### The runner is a real, non-idle desktop

Any Windows test written from now on has to hold under all of this: the display
is **1024×768** — smaller than `WindowDesc::default`'s 1280×720; a cursor is
always over the window and keeps moving (a genuine `WM_MOUSEMOVE` arrives before
a test sends anything); the foreground is contested and `SetForegroundWindow` is
granted only under narrow rules — the e2e suite's `desktop::take_foreground`
pulls `SPI_SETFOREGROUNDLOCKTIMEOUT` plus `AttachThreadInput`; and **messages
arrive that this process did not cause**, every few milliseconds
(`WM_DWMNCRENDERINGCHANGED`, real `WM_MOUSEMOVE`), so an idle window with a
drained queue does not exist on that runner. The rule that cost three flaky
runs: **identify your own events by their payload, never by their index in the
sequence.**

### Unverified, in the order it would hurt

- **Structure layouts in `win32/ffi.rs` are asserted by size and offset, which
  catches a missing or wrong-width field but not a reordering of two same-width
  fields** — and a field whose width shrinks into its own trailing padding moves
  no offset and is not caught either (narrowing `DropFiles::p_files` from `u32`
  to `u16` leaves every assertion green). `DEVMODEW` is the one with two unions
  in it and the one to re-read if a refresh rate ever looks implausible;
  `RAWMOUSE` if a raw delta does.
- **No input has ever been delivered by a real device.** The suite drives the
  window procedure with `SendMessageW` — the real procedure against the real
  cached state, but not the real message stream. Nothing has confirmed that a
  real keyboard's `lParam` carries what `keys::scancode` expects, or that
  `GetMessageTime` answers for the message being dispatched.
- **`WM_INPUT` is untestable from CI and is untested.** A raw report needs an
  `HRAWINPUT` only the system can produce, so `input::read_raw_mouse` and the
  `RIM_TYPE_MOUSE` check have never run; the absolute path needs a machine that
  produces absolute reports (a remote-desktop session or a tablet). W4's
  `injected_motion_arrives_as_raw_relative_motion_for_mouselook` assumes
  `SendInput` feeds the raw stack on a `windows-latest` image — the single
  assertion most likely to be answered by the runner rather than by the backend;
  if it fails with the ordinary `PointerMotion` present and `raw_delta` absent,
  the finding is about the image, not the backend.
- **`ClipCursor` and `SetCursorPos` are restricted to the foreground process.**
  The two pointer tests call `SetForegroundWindow` first; if a GitHub runner
  refuses the foreground, both fail — a finding about the runner, not the
  backend.
- **`MapVirtualKeyW(.., MAPVK_VK_TO_CHAR)` is assumed to answer the uppercase
  letter**, which `input::unshifted` then lowercases so a rebind menu reads the
  same as on Linux; if the call already answers lowercase, the lowercasing is a
  no-op and nothing changes. The test asserts the lowercase result either way.
- **The `ShowCursor` balance is asserted through the count itself**
  (`cursor_display_count` reads it by moving it and putting it back). That test
  is the only thing standing between this backend and an invisible cursor for
  the rest of a session, so it is the one to keep rather than relax.
- **Auto-repeat is not the driver's.** Windows typematic comes from the
  keyboard, and an injected key does not repeat; the repeat test sends two
  presses and reads bit 30, which the _system_ sets — the same bit a real hold
  sets, so the claim is sound; what is untested is the driver's timing.
- **No file has ever been dragged onto a window.** The drop test builds a
  `DROPFILES` block by hand — this project's idea of what the shell sends, not
  the shell's, and a real drag needs a source application's mouse. If shell32
  rejects the block it reads exactly like a backend bug: `ffi::DropFiles`'s size
  assertion is the first thing to re-check, and `f_wide` the second.
  `DragAcceptFiles` and the `WS_EX_ACCEPTFILES` round trip are asserted through
  the style word; a _real_ drag being offered is a decision the shell makes in
  another process.
- **No other process has ever contended for the clipboard**, so `Opened::After`
  and `Opened::Refused` have never been produced and the retry loop itself is
  unexercised; only the budget arithmetic is covered, on Linux. The clipboard
  tests also share the desktop's clipboard — two Windows suites in parallel
  would interfere, which is why the e2e suite is `--test-threads 1`.
- **No sample-level pass, and there cannot be one yet.** The Linux suites run
  the sandbox and press F11 at it; that needs a renderer, and `windows-latest`
  has no Vulkan device, so this waits on a D3D12 path.

### Owed on the Win32 backend

- **Drag feedback: there is only a drop, never a conversation.** `WM_DROPFILES`
  is a notification; `DragEnter`/`DragOver`, a drop cursor, non-file formats and
  copy-versus-move all need `RegisterDragDrop` and an `IDropTarget` — COM, with
  `OleInitialize` on the pumping thread and an apartment this crate does not
  own. Considered and declined for W3 (it buys feedback rather than drops, and
  `ShellEvent::DroppedFile` is what the seam actually names); owed before the
  editor's asset browser wants a drop target that looks like one (P12).
- **`MimeType::UriList` is a registered format, not `CF_HDROP`.** A "copy file"
  from Explorer puts `CF_HDROP` on the clipboard, and this backend does not read
  it: a request for `text/uri-list` finds whatever was published under that
  registered name and is otherwise `Empty`. Rendering `CF_HDROP` as a
  `text/uri-list` blob means _encoding_ URIs, and the shared decoder
  `clipboard::parse_uri_list` cannot round-trip a Windows path (`file:///C:/a`
  decodes to `/C:/a`, which is not a file). Closing this means either a
  Windows-aware `file:` encoder plus a matching decoder, or delivering
  `CF_HDROP` as paths through a different route. Not attempted; named so the gap
  is not rediscovered as a bug.
- **A clipboard payload whose last bytes are NUL loses them.** Payloads are
  written NUL-terminated and read back with trailing NULs trimmed, which is what
  makes a `GlobalSize` larger than the request harmless — and what would
  truncate an `Other("image/png")` offer ending in NUL. Recorded in
  `win32::clipboard`'s module docs as well.
- **`WindowDesc::app_id` is validated and never applied.** Win32's equivalent of
  `WM_CLASS` is the Application User Model ID, set process-wide with
  `SetCurrentProcessExplicitAppUserModelID` from `shell32`; it decides taskbar
  grouping and which shortcut a window matches, and W1 rejects a NUL and
  otherwise ignores it, so a Crucible window groups under whatever Explorer
  infers. Wiring it means a third system library (`shell32` is already linked
  for the drop calls) and a decision about _where_ a process-wide property is
  set when a host application embedded the engine.
- **`ShellCaps::TEXT_IME` is clear although typing works.** `WM_CHAR` is
  handled, including surrogate pairs, and the default IME does deliver a
  committed CJK string through it — but nothing touches the `WM_IME_*` family,
  the seam cannot tell a pre-edit from a commit, and there is no way to place
  the candidate window at the caret. Matching Wayland's standard means handling
  the `WM_IME_*` family and giving the seam a pre-edit event, which is its own
  slice. The argument is written out in `Win32Shell::caps`.
- **`DeviceId` names a device kind, not a device.** Windows is better placed to
  fix this than X11 is — `RAWINPUTHEADER::hDevice` identifies the physical
  device on every `WM_INPUT` — but turning a handle into a stable `DeviceId`
  needs a handle table and a hotplug story, and raw input would have to become
  the source of button and wheel events too rather than only of motion.
- **A modal drag-resize accumulates raw motion.** `WM_INPUT` keeps arriving
  while Windows runs its own message loop, so a three-second edge drag delivers
  a few thousand `PointerMotion` events in one `pump`. Bounded, not a leak, and
  the two obvious fixes are both wrong: coalescing loses the per-event timing
  `docs/plan/19-input.md`'s pattern evaluator is a function of, and dropping
  needs a "we are inside a modal loop" flag nobody else would consume.
- **Refresh rate is a whole hertz, so 59.94 Hz reports as 60.**
  `EnumDisplaySettingsW`'s `DEVMODEW::dmDisplayFrequency` is an integer, and
  `MonitorInfo::refresh_millihertz` exists precisely because that rounding
  matters to frame pacing. The exact figure is in `QueryDisplayConfig`'s
  `DISPLAYCONFIG_RATIONAL` — worth closing now that frame pacing is real.
  **Closed 2026-08-07**: `win32::monitors` now reads the exact rate from
  `QueryDisplayConfig` (path walk → `DisplayConfigGetDeviceInfo` source name →
  the target mode's `vSyncFreq`, `vSyncFreqDivider` applied), falling back to
  the integer path when the walk cannot answer. The first CI run on
  `windows-latest` caught the virtual-display case: the runner's desktop reports
  a placeholder rational (1 mHz), which the exact path now refuses
  (`MIN_PLAUSIBLE_REFRESH_MHZ`, so the seam's documented "0 = cannot determine"
  is what such a display reports); the e2e's refresh band permits that zero.
  What is still unobserved: the exact rate of a _physical_ display — the
  `win32: exact refresh for …` info line is the only record of which path a
  machine took, since a broken walk silently falls back and every test stays
  green.
- **A window frozen during a user drag-resize is accepted, not fixed.** Windows
  runs its own modal loop between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`, so
  no frame renders until the mouse is released. The usual fix — `SetTimer` plus
  a frame rendered from `WM_TIMER` — cannot be built in this crate: the shell
  deliberately has no `Shell::run(closure)`. Closing it needs a second seam,
  "render one frame now", which is a decision above `crcbl-shell`.

## What the AppKit backend has and has not been run against

`crates/crcbl-shell/src/appkit/` was written on a Linux machine and
cross-checked with `cargo check`/`cargo clippy --target aarch64-apple-darwin`,
which do not link and do not run — **a cross-check proves the code typechecks
and nothing more**. The window lifecycle and the injected-input suite have since
run on `macos-latest` through `tests/appkit_session.rs`; everything below is
what that pass still does not reach.

### The one rule to know before writing any macOS test

**A `#[test]` can never drive an AppKit window, and that is measured rather than
assumed.** AppKit is main-thread-only and enforces it by raising
(`-[NSApplication nextEventMatchingMask:...]` throws
`NSInternalInconsistencyException`; an Objective-C exception unwinding through a
Rust frame is undefined behaviour), and Rust's `libtest` always runs a test body
on a thread it spawns — so the thread and app state a test needs are exactly
what `#[test]` does not supply (a green `#[test]` asserting every `NSCursor`
selector failed on the runner with `+[NSCursor "arrowCursor"] answered nil`).
The window suite therefore lives in
`crates/crcbl-shell/tests/appkit_session.rs`, a `harness = false` target that
owns its `main` and runs _as_ the process; it is not feature-gated (off macOS it
prints why it did nothing rather than reporting a pass it did not earn), and it
answers libtest's `--list` protocol before anything else — a `harness = false`
target has to be verified with `cargo nextest list` as well as
`cargo nextest run`, because `cargo test` does not enumerate and CI uses
nextest. A host `#[test]` is fine for the Objective-C runtime (thread-safe,
needs no application), CoreGraphics, and the pure modules; anything that creates
an AppKit object needs the session target.

What the session covers that is easy to miss: every `objc_msgSend` signature
shape this backend transmutes is dispatched against a class built at runtime
(`CrcblFfiProbe`) and against Foundation classes; the main-thread refusal is
exercised; every `NSCursor` selector `pointer::cursor_selector` names is checked
from the session; the pasteboard round-trips through a second process
(`pbcopy`/`pbpaste`, so text only — `application/x-crcbl+ron` is not
round-tripped cross-process, and if an engine-to-engine paste ever misbehaves on
macOS this is the gap it would hide in); and the pure modules (`geometry`,
`events`, `keys`, `pointer`, `TimeBase`) run on every host.

### Uncovered, and why each one is uncovered

- **`Borderless { monitor: Some(..) }` lands on the named screen's origin by
  construction and not by observation.** The runner has one display, so a
  backend that ignored the named monitor entirely would pass every assertion.
  Needs a two-display machine.
- **A window created borderless is untested.** The session creates its window
  windowed and flips; `create_native_window`'s borderless arm (placing the
  window with `initWithContentRect:` rather than `setFrame:display:`) has never
  run, and the presentation options are applied by `refresh_presentation` on the
  first `set_mode`, not at creation — whether that ordering matters for a window
  born borderless has not been measured. Re-read the ordering rule in
  `appkit::window`'s module docs before adding a test here.
- **`injection_skipped` is written and unrun**, because the runner granted
  activation. It stays for the case that produced it — a developer running this
  as a background process on their own machine — and prints the `Activation`
  evidence rather than a bare timeout if it is ever taken.
- **The harness, not the backend, asks for activation.** A GitHub macOS runner
  will not hand the foreground to the cooperative `-[NSApplication activate]`
  the backend is right to be limited to; `frontmost::ask` in the session uses
  `-[NSRunningApplication activateWithOptions:]` with
  `NSApplicationActivateIgnoringOtherApps`. That split — a harness may arrange a
  precondition a backend must never arrange for itself — is the same shape as
  Win32's `desktop::take_foreground`.
- **The sample-level F11 pass.** Needs a renderer, and macOS has no Vulkan at
  all — permanently, per the 2026-08-05 decision that Apple platforms are Metal
  only. It waits on `crcbl-mtl` reaching a swapchain, not on a gate.
- **A real drag and drop.** A drag needs a _source_ application with a mouse
  held down over a Finder item, which `CGEventPost` alone does not provide;
  `performDragOperation:` and the real `draggingLocation`/pasteboardItems
  conversion have never been watched. What M4 adds is that the registration is
  read back off the real window (`-[NSView registeredDraggedTypes]`), so the
  gate is a mechanism rather than a promise.
- **Only `public.file-url` is read.** `NSFilenamesPboardType` and
  `com.apple.pasteboard.promised-file-url` are not; the promised form needs the
  receiver to name a destination directory, which the seam has no way to ask
  for. Closing it is a seam question ("where should a promised drop land?"), to
  be answered once for every platform that has the concept.
- **macOS 15's pasteboard-access prompt has not been met.** It gates _reads_,
  not writes, and it does not turn a read into an error — but if a future runner
  image shows it, the session's `clipboard()` would block rather than fail,
  which reads as a hang. Recorded so a mysterious ten-second timeout in
  `paste()` is diagnosed rather than rediscovered.
- **IME composition is unverified.** The view conforms to `NSTextInputClient`
  and every key goes through `interpretKeyEvents:`, which is the structural
  standard Wayland is held to; that a Japanese input method actually commits
  through it is unverified, because a GitHub runner has no IME installed.
- **The horizontal scroll sign.** Vertical is settled; horizontal is passed
  through on the same reasoning without a trackpad to confirm it. If a
  two-finger swipe right turns out to scroll left, the fix is one negation in
  `pointer::scroll` and the test beside it.
- **Reference counting is reasoned, not observed.** `releasedWhenClosed` is
  turned off and `appkit::shell::release_window` is the single matching release
  for the window and the layer; there is no leak check anywhere in this
  workspace and Instruments is not in CI.
- **`AXIsProcessTrusted()` is not called**, although it would say outright
  whether TCC is the reason for a failed injection. It lives in
  `ApplicationServices`, which this crate does not link; if the first run's
  diagnosis is ever ambiguous, that is the next instrument to add. (TCC does
  **not** gate `CGEventPost` back to the posting process — settled by
  observation — so the `postEvent:atStart:` fallback should not be written.)
- **The `NSTrackingArea` is still a structural claim.** The posted pointer
  motion goes to the key window through `mouseMoved:` rather than through a
  tracking crossing, so nothing yet _requires_ the tracking area to have been
  registered.
- **Every type encoding on `CrcblView`'s methods.** The runtime reads them only
  when it forwards a method through an `NSInvocation`, which nothing in this
  crate does; a wrong one is a wrong-width read in a path CI never enters. The
  mitigation is that the encodings are written from one place (`ffi::ENC_RANGE`,
  `ENC_RECT`, `ENC_POINT`) rather than spelled out per method.

### Considered and declined

- **`PointerMode::Confined` is not implemented, and `POINTER_CONFINE` is
  clear.** macOS has no confine API; the only technique is warping back after
  the cursor has already crossed, which runs a frame late, fights the user's
  motion and manufactures events a consumer cannot tell from real ones.
  Approximating it would set a capability bit with no mechanism behind it. **Do
  not revisit without a public API to point at.**
- **`RAW_POINTER_MOTION` is set although the deltas are accelerated.** `NSEvent`
  deltas satisfy the half of that bit that decides whether a camera works and
  not the "unaccelerated" half; GLFW answers
  `glfwRawMouseMotionSupported() == false` on this platform for the same reason.
  Closing it properly means IOKit, a slice of its own.
- **`DeviceId` is a constant per device _kind_, as on X11 and Win32.** An
  `NSEvent`'s `deviceID` identifies a tablet and is meaningful only for the
  tablet family; the real answer is IOKit, the same slice as above.
- **The IME candidate window is placed at the window's origin, not at a caret.**
  The seam does not model a caret — nothing above `crcbl-shell` says where text
  is being typed. Closing it needs a seam addition ("the caret is here"), a
  decision above this crate to be taken once for every backend with an IME.
- **Lazy pasteboard provision (`pasteboard:provideDataForType:`) is not used,
  and it is structurally unavailable.** The callback arrives on the main run
  loop driven by the pasteboard server on behalf of a reader in another process
  — between two `Shell::pump`s an engine is rendering, so there is no run-loop
  turn to service it in — and a lazy owner must stay messageable until the
  flush, leaving the server holding an unretained pointer if the host process
  survives the shell. The same refusal `win32::clipboard` makes about
  `WM_RENDERFORMAT`. **Do not revisit without a seam that gives the shell a
  run-loop turn it owns.**
- **The engine's own format is published under its mime string, not a `dyn.*`
  UTI.** A pasteboard type is an arbitrary string, the mime is unique to this
  engine by construction, and it is byte-identical to what the other three
  backends name the same format with. Only text uses a system UTI.
- **Drag and drop _out_ is not implemented on any backend.** `15-windowing.md`
  scopes drag-and-drop to "file paths in"; `NSDraggingSource` is absent by plan
  decision rather than by gap.
- **No menu bar.** An unbundled Regular-policy application gets the system's
  default menu bar — enough to be focusable, not enough to ship (no ⌘Q).
  Building one is `NSMenu`/`NSMenuItem` and a decision about what belongs in it,
  which is above this crate.
- **`HW_UPSCALE` is clear although macOS has it.** A `CAMetalLayer`'s
  `drawableSize` is independent of its bounds, exactly what `wp_viewport` buys —
  but **the seam has no way to ask for it**. Setting the bit would be a claim
  with no mechanism behind it; closing it is a seam change (a render-scale
  request on `Shell`), a decision above this crate to be taken once for both
  backends.
- **`app_id` has nowhere to go.** macOS's equivalent is `CFBundleIdentifier` in
  an `Info.plist`, which cannot be set by a running process; the descriptor is
  validated for a NUL byte so a rejected descriptor is rejected here too, and is
  otherwise unused.
- **A live resize drag freezes the window**, on the same terms as the Win32
  modal loop and with the same unavailable fix.

## Not covered on either backend

- **A window manager that does not respect the requested size.** `openbox` with
  the packaged default theme decorates around the client area, so the client
  keeps the size it asked for. One that shrinks the client, or a tiling manager
  that ignores the request entirely, would break the sandbox passes' extent
  assertions — a real configuration, not exercised.
- **X11 multi-monitor.** The Wayland suite now declares two outputs and asserts
  that a fullscreen request naming the second one lands on the second one; the
  X11 suite still has a single `Xvfb` screen, so `move_to_monitor` and
  `Borderless { monitor: Some(..) }` are unit-tested only on the backend that
  can actually honour them. `Xvfb`'s RANDR exposes one CRTC and
  `xrandr --setmonitor` defines RANDR 1.5 _monitors_, which `crcbl-shell`'s
  enumeration does not read — it goes through `GetScreenResourcesCurrent` and
  `GetCrtcInfo`. Two ways forward: read `RRGetMonitors` first and fall back to
  CRTCs (what GTK and Qt do, and it makes the headless split testable), or run a
  real `Xorg` with the dummy driver configured for two heads in CI. The first is
  a backend change with its own slice; the second is a CI dependency.
- **Pixels.** Every display-mode assertion on both backends is a summary line, a
  log line, or the compositor's own tree. That a fullscreen frame is _composed_
  at the new extent, rather than merely built at it, is unchecked.

## Five sample `gpu.rs` files, two of them identical

`apps/breakout/src/gpu.rs` and `apps/flappy/src/gpu.rs` differ in **nothing**
but the game's name: rename `breakout`/`Breakout` to match and `diff` reports
zero lines. Both are 622 and 619 lines. `apps/asteroids` and `apps/horde` differ
substantially (352 and 487 lines against breakout's) and `apps/sandbox` almost
entirely, so this is a two-file duplication rather than a five-file one.

Not acted on because the seam is not obvious. The shared shape is "orthographic
camera + sprite pass + menu pass + UI pass over `GpuContext`", which is a
plausible `crcbl-render` bundle — but breakout's camera is fixed and flappy's
scrolls, and the two files agreeing today may be the coincidence of two 2D games
at the same stage rather than one piece of knowledge written twice. Revisit when
a third game wants the same bundle; a helper with two callers that then needs a
flag per caller is the failure mode.

## `run-vk-e2e.sh` pins no ICD by default, so nobody was running CI's gate

The script's header says it "is what a developer runs to see what CI sees". That
was not true unless the developer happened to set `CRCBL_VK_ICD`: the pin block
is wrapped in `if [ -n "${CRCBL_VK_ICD:-}" ]`, and with the variable unset the
script exports nothing and the Vulkan loader picks whatever is installed. On a
workstation that is the discrete GPU. Measured here: a bare
`crates/crcbl-vk/tests/run-vk-e2e.sh` reports
`adapter "AMD Radeon RX 7900 XTX (RADV NAVI31)"`, while CI's job sets
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` and gets llvmpipe.
The suite printed the adapter it got all along, but an adapter line is not
something anyone reads looking for an absence.

The script now prints a warning naming the gap and the command that closes it,
and still runs — testing against real hardware deliberately is worth doing, and
this is how. **It is still not a hard failure**, which is a judgement worth
revisiting: the alternative is defaulting `CRCBL_VK_ICD` to lavapipe so the bare
invocation is CI's invocation, and requiring an explicit opt-out to use the real
GPU. That is probably the better default and was not taken here because it
changes what an existing command does.

## `ImportedImage::initial` is a contract nothing enforces, and it has been wrong twice

`crcbl_render::graph`'s `ImportedImage::initial` documents what state an image
is already in when the frame imports it, and a declaration that lies produces a
barrier with no source scope — `Undefined` maps to
`(stage NONE, access NONE, layout UNDEFINED)` — so nothing waits for the
previous frame's reads. That is a write-after-read hazard, and **CI's validation
layer is the only thing in this project that reports it**.

Got wrong twice now: `crcbl::screenshot`'s readback barriers (fixed earlier this
session) and `crates/crcbl-vk/tests/vk_e2e/depth_probe.rs`'s 1x1 shadow
placeholder, which imported `Undefined` on every call while a previous
`render_probe` had left it in `ShaderRead`. Both were the same shape and neither
was caught locally.

**The enforceable version**: record each imported image's `final_state` in
`TransientPool` and reject a contradicting `initial` at `compile()`. What stops
it being a small change is that a swapchain image legitimately declares
`Undefined` every frame — the acquire semaphore makes it true — so telling a
legal `Undefined` from a lie needs a new field on `ImportedImage` and an update
to every construction site, `apps/bare/src/lib.rs` included.

**Coverage note:** `crcbl_render::forward`'s
`the_shadow_atlas_enters_each_frame_in_the_state_the_last_one_left_it` now
replays the null backend's recorded stream over several laps and asserts each
barrier's `from` equals what the previous one left, so the engine's own atlas is
guarded on any machine. **Test fixtures are not.** `run-vk-e2e.sh`'s header
claims the graph-compile suite covers this class; that is true of the engine and
not of fixtures like `DepthProbe`, which is exactly where the second instance
lived.

## This machine's validation layer cannot see the two-submission hazard

`crcbl-vk`'s offscreen-ring write-after-read (fixed — see `CHANGELOG.md`) is
reported by CI's layer and by nothing here, and the difference is the **layer**,
not the ICD. Measured under CI's own ICD, so the ICD is ruled out:
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` (which the script
resolves to Arch's `lvp_icd.json`), adapter
`llvmpipe (Mesa 26.1.6-arch1.1 (LLVM 22.1.8))`, layer
`VK_LAYER_KHRONOS_validation spec 1.4.357` — and
`synchronisation_validation_catches_a_missing_barrier` still reports
`record-time=yes one-submission=yes cross-submission=no`.

`syncval_submit_time_validation` **defaults to true** in that layer build (read
out of `/usr/share/vulkan/explicit_layer.d/VkLayer_khronos_validation.json`),
and setting it explicitly through `VK_LAYER_SETTINGS_PATH` and through the
`VK_KHRONOS_VALIDATION_*` environment form both left the measured reach
unchanged. So the switch is not the explanation and there is no known way to
widen it here. What that costs: the **two-submission** instance of this bug can
only be observed in CI.

What stands in for it locally is
`reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it` in
`crates/crcbl-vk/tests/vk_e2e.rs`, which provokes the same missing dependency at
record-time distance — a one-image ring, both trips recorded into one command
buffer — where every layer build sees it. It was falsified by disabling the
widening in `VkCommandEncoder::pipeline_barrier`: red, with the layer naming
`vkCmdPipelineBarrier2 performs image layout transition on the VkImage ... which was previously read by vkCmdCopyImageToBuffer`.

**A CPU wait was tried first and does not work**, which is worth keeping because
it is a plausible idea. `acquire_next_frame` blocked on the retire timeline
before handing a reused image back. Instrumentation confirmed it ran (`reuse=3`
and `reuse=4` on the third and fourth frames of the failing test), and CI
reported the identical hazard anyway: a host-side wait establishes real ordering
but is not a queue dependency, and syncval reasons about submitted commands. It
also costs exactly the frame overlap the ring exists to provide. It was removed
rather than kept alongside the barrier.

**`crcbl-wgpu`'s offscreen path does not have this gap** — checked 2026-08-04,
and the untested assumption that used to sit here is deleted. Its acquire does
report `acquire_semaphore: None`, but the hazard needs the discarding transition
and that transition cannot reach wgpu: `WgpuCommandEncoder::pipeline_barrier` is
a no-op, so the `ResourceState::Undefined` the seam records is dropped at the
backend boundary, and wgpu-core inserts its own transitions from its usage
tracker — `command/transfer.rs`'s `transition_textures(&src_barrier)` before a
texture→buffer copy, and `device/queue.rs`'s
`insert_barriers_from_device_tracker` in front of each submitted command buffer,
which is what carries a texture's state across submissions (read in wgpu-core
30.0.0, the resolved version).

`reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it` in
`crates/crcbl-wgpu/tests/wgpu_e2e.rs` is the check: a one-image ring, trip one
clears and copies out, trip two clears the same image to the reversed colour,
and the staging buffer must still hold trip one's. Green on radv, on lavapipe
and on the GL backend. Falsified both ways — writing trip two's colour in trip
one, and deleting the copy — each red, and each for its own reason.

**And the layer agrees, with a control that proves the layer was listening.**
Sync validation is not something wgpu-hal requests, so it was forced at layer
level: a settings file with `khronos_validation.validate_sync = true` reached
through `VK_LAYER_SETTINGS_PATH`, which makes the layer print
`Current Validation Enabled: … Synchronization` at `vkCreateInstance`. Under it
the wgpu test reports no hazard. The control is the same file and the same ICD
against `crcbl-vk` with the widening in `pipeline_barrier` disabled: red, with
`SYNC-HAZARD-WRITE-AFTER-READ … previously read by vkCmdCopyImageToBuffer`. So
the silence on the wgpu side is a verdict rather than an absence.

**Worth knowing before the next investigation**: `CRCBL_VK_SYNC_VALIDATION` is
what turns sync validation on for `crcbl-vk`, and with it unset the vk test
above stays green with the fix removed — the suite says
`CRCBL_VK_SYNC_VALIDATION is not set; skipping the sync-hazard probe`, which is
easy to run past. `run-vk-e2e.sh` defaults it to `1`; a raw
`cargo test`/test-binary invocation does not.

## The demo site's social preview is blank

No `og:image`, so every link posted to Slack, Discord or a Mastodon timeline
renders as a bare title and description. `web/templates/layout.html` is where
the tag would go and the site already carries `og:title`, `og:description`,
`og:type`, `og:url` and `og:site_name`, so the tag itself is one line — the
missing half is the image. It cannot be the `favicon.svg`: the platforms that
matter want a raster of about 1200×630, and several ignore SVG entirely.

Deliberately not invented here, because the choice is a design one: a rendered
frame of a demo, a wordmark on the site's own background, or a per-demo card.
Whatever it is has to be baked from a committed source the way
`crates/crcbl-render/assets/menu.crpix` is, not a PNG dropped into `web/` with
no way to regenerate it.

## `/favicon.ico` is still a 404, deliberately

`web/favicon.svg` is declared by the layout, which is what stops the browsers in
the requirements list (Chrome/Edge 113+, Safari 18+, Firefox) asking for
`/favicon.ico` at all — verified: `curl` against the live site returned 404 for
that path before the change, and the built pages now carry
`<link rel="icon" href="/favicon.svg">`. A browser that ignores the declaration
still gets a 404 and no icon.

Not fixed because an `.ico` is a binary blob and this repo bakes its art from
committed text. `web/build.sh` has no image toolchain and adding one for a 16×16
icon is a worse trade than the miss. `web/tools/browser-e2e.mjs` still filters
`favicon.ico` out of its 404 assertion for the same reason.

## Always run the browser gate with `--build`

A run without `--build` uses whatever is in `target/site`, and that directory
outlives the commit it was built from — a stale site is how a green run gets
reported for code that is red. This has cost real time once already: both demos
were signed off at "25/25" against a site built before the focus, pause and menu
work landed, and the gate had in fact been red the whole time. Pass `--build`,
or delete `target/site` first.

## Should the click that refocuses a canvas reach the game at all?

**Behaviour that surprised us, deliberately left alone.** A canvas has no title
bar, so `web/engine/shell.js` gives it the keyboard from its own `pointerdown`
handler — which makes the click that "clicks back into the window" also a press
at a real position inside the game. With the pause menu on screen and `RESUME`
under the cursor, clicking back in resumes. That is each half behaving correctly
and the combination being surprising; it is what put the browser gate at 23/25
for a slice, because section E clicked the canvas's _centre_ to restore focus
and the menu is centred there.

The alternative is click-to-focus **activation blocking**: the first press after
a focus gain restores focus and is swallowed rather than delivered, which is
what several desktop toolkits do. Not done, and not obviously right — swallowing
a click is its own surprise, and for a paused game the current behaviour is
arguably the friendlier one (the player clicked on `RESUME`; they got a resume).
It needs a decision rather than a patch, and it would have to be decided for
native and web together, since `Loop` cannot tell the two apart.

What holds the line meanwhile:
`a_focusing_click_off_every_button_leaves_the_game_paused` in all four games'
`app.rs` asserts the corner is over no button and the centre is over `RESUME`,
so a menu that grew until it reached the corner fails a fast Rust test rather
than the slow browser one. Four copies of it, plus horde's
`a_focusing_click_off_every_button_leaves_the_title_screen_up`, because the menu
geometry is per-sample even though `FOCUS_CLICK_INSET` — 8 pixels, in
`web/tools/browser-e2e.mjs` — is not. The loop around them is
`crcbl::engine::Loop` now, so this is one of the few things still written out
per sample, and it is per sample for a reason rather than by omission.

## The sprite system, and what is left of the retrofit

The pipeline is joined up end to end for both games: each of `apps/flappy` and
`apps/breakout` authors `.crpix` text under `assets/`, a `build.rs` bakes it,
and an `art::Scene` draws it through `SpriteRenderer` on a layer stack. What is
left:

- **The tick rate is one constant now, and the guard around it is still weak in
  three of the five.** `bake_dir` writes `ART_TICK_HZ` into the generated table,
  so the loader reads the rate the art was actually baked at and the `build.rs`
  value is its only source — the two halves cannot disagree, and the five
  hand-written copies beside the loaders are gone. What survives is the
  _conversion_ pair: a `.crpix` counts holds in ticks, a sidecar counts
  milliseconds, and each game's `the_art_bakes_to_the_sheets_it_declares`
  asserts an authored hold makes the round trip. **Breakout's, asteroids' and
  `crcbl-render`'s are weaker than flappy's and horde's**, because nothing they
  draw is animated: they can only assert the default hold of one tick, which
  survives a fairly wide range of wrong arithmetic. Asteroids' ship and rocks
  _turn_, which is a rotation applied to a still frame and not a clip, so it
  does not help. Each gets real the moment that game has a clip.

- **Breakout's paddle is a plain frame, not a nine-slice.**
  `game::PADDLE_HALF_WIDTH` is a `const` and nothing shadows it, so the paddle
  is 10 world units across on every tick of every run and a stretch would have
  had no caller. If a widening power-up is ever added, `assets/paddle.crpix`
  wants `nine: 12 12 0 0` and `art::paddle_rect` already produces the target
  rectangle `expand` would take.

- **`Sprite::rotation` has no pivot offset, and that was a decision.** The angle
  turns the quad about the centre of its own `rect` and there is no way to name
  another point. Considered and declined: the sheet lane has exactly one
  component left, an offset needs two, and the case it would serve is
  expressible without it — a rectangle rotated about an outside point is the
  same rectangle rotated about its own centre and then translated, so a caller
  wanting an orbit computes the translated `rect` and gets it exactly. Reopen it
  if something wants a pivot that is _animated_ independently of position, which
  is the one shape the translation trick makes awkward; it would need a fifth
  `float4` on the instance, taking it from 64 bytes to 80.

- **A nine-slice cannot be rotated, and neither can a menu or a button skin.**
  `NineQuads::sprites` hard-codes `rotation: 0.0` with a comment saying why: the
  nine quads are stretched against each other, so turning each about its own
  centre opens a gap at every band boundary, and turning the frame as a whole
  needs one pivot shared by all nine — which is a different feature from
  `Sprite::rotation`, and would be `expand`'s job rather than the instance's.
  Nothing has asked for it. If something does, the shape is a rotation on
  `NineSliceSource::expand`'s target that it applies to all nine quads about the
  target's centre, which needs the per-sprite pivot above or a rect-plus-angle
  that is not the rect's own centre.

- **The angle a sample interpolates is the sample's own, and the third one will
  copy it.** Asteroids answered the open question — `game::lerp_angle`, a
  shortest-arc scalar lerp between the previous tick's angle and this one,
  driven by `FrameClock::alpha` — but it answered it _in the sample_. Nothing in
  `crcbl-render` or `crcbl-client` offers it: `Client::interpolate` lerps
  `Transform`s, whose `DQuat` no sprite path reads, and `crcbl-phys` still has
  no angular velocity, so game code owns every angle. The next sample that turns
  something writes the same twenty lines. Worth promoting only when there is a
  second caller — the shape would be a `lerp_angle` in `crcbl-core`'s math, or a
  `Sprite` that could take the pair and the alpha, and the second is a much
  bigger claim than it looks.

## The debug overlay, and what is left of it

The modular panel is built and all three samples switch it on with F3 (or
`--debug-overlay`): `crcbl_ui::debug` owns `DebugPanel`, `DebugSection`,
`DebugModule`, `FrameStats` and `DebugOverlay`; `crcbl-render` contributes the
`gpu` section by implementing `DebugModule` for `FrameTimings`. What is left:

- **There is no network module, and no sample could show one yet.** The panel is
  ready for it — a module is a `DebugModule` impl and one `add` call — but
  nothing was written, for two reasons. The first is that `23-netcode.md`'s
  netgraph list (RTT, jitter, loss, send/recv bandwidth, snapshot size, resend
  counts, tick-lead) is **not measurable today**: `InMemoryTransport` has no
  timing, no loss and no byte accounting, and `Client` exposes only
  `is_connected`, `session_id`, `last_applied_tick`, `baseline_entity_count`,
  `baseline_system_count`, `processing_error_count`, `auth_failure_count` and
  `rate_limited_message_count`/`rate_limited_byte_count`. Those are real numbers
  and a module could show them, but they are a connection-health readout, not a
  netgraph, and shipping them under that name would make the P10 work look done.
  The second is placement: the module belongs in the crate that owns the numbers
  (`crcbl-client`, following `crcbl-render`'s example), which means a
  `crcbl-client → crcbl-ui` dependency. That is not obviously wrong — `crcbl-ui`
  depends on nothing but `glam` and `bytemuck`, so there is no cycle — but it is
  the first time a simulation crate would depend on the UI, and it is a call
  worth making deliberately rather than in passing. **What it would take**: the
  transport growing byte and timing counters, then a `DebugModule` impl beside
  them, then one `add` line in each sample that has a connection.
- **The overlay starts hidden in a release wasm build.** The default is
  `cfg!(debug_assertions)`, which is sample rule 4's "on by default in dev
  builds" taken literally; the demos on `crcbl.kryptic.sh` are release builds,
  so a visitor has to press F3. Whether the published demos should default it on
  is a product decision nobody has made. `web.rs` builds `Options::default()`,
  so turning it on there is one field.

## Coverage gaps

- **The mixer adoption was not verified by ear, and two of its choices are
  audible-only.** See the entry under _Owed_ above. Structurally everything is
  pinned; nothing has been listened to.

- **The `wasm32` audio path is not built by the local verification loop.**
  `AudioStream::open` on `wasm32` goes through `web::install`, and the blanket
  `impl AudioSource for Arc<T>` is what makes an `Arc<Mixer>` acceptable there
  too. The browser gate (`web/run-browser-e2e.sh --build`) covers the four demos
  end to end, which is the only place that path runs.

- **`html-validate` reports `require-sri` on every `<link>` and `<script>`, and
  it is being ignored.** Subresource Integrity guards a resource served by
  someone else; the stylesheet and the demo shims are same-origin files this
  repo builds in the same step as the page that names them, and a hash pinned in
  the layout would have to be regenerated on every edit to `style.css`. Recorded
  so the next person to run the validator does not re-litigate it.
- **No visual regression baseline for the site.** The browser gate captures the
  _canvas_ — deliberately, since the page's chrome is not what it tests — so a
  stylesheet or template change that breaks the layout around the canvas would
  pass all 26 checks. The screenshots taken during the 2026-08-02 audit were
  looked at by a human and thrown away.
- **The menu golden cannot see an inset larger than the one authored.**
  `menu_frame_two_sizes` compares the two panels' corner blocks pixel for pixel,
  which catches a corner that _scaled_ with the target — measured: making the
  panel's insets a function of the target width failed it. It cannot catch an
  inset that grew uniformly, because `menu.crpix`'s panel is uniform fill past
  texel 3 on both axes, so a `nine` of 6 draws exactly the same picture as a
  `nine` of 4. That number is pinned instead by
  `crcbl_render::menu::the_shipped_art_has_the_insets_the_layout_assumes` and by
  the layout tests, both of which go red on it.
- **The golden's reference is weak on small-area art changes.** The image is
  416×576, so recolouring a one-texel band moves under 1% of the pixels and
  compares inside `Tolerance::RASTERISER` — measured: swapping the panel's
  shadow colour for its own channels reversed passed the reference. The pixel
  assertions carry that weight instead (`assert_menu_pixels` samples the
  highlight and the shadow on all four edges, and that _did_ catch it). Worth
  knowing before adding a claim to this golden that only the reference would
  hold.
- **Nothing has looked at a menu over a real game.** The golden renders the menu
  over a flat clear colour on an offscreen ring; no test composites one over
  breakout's brick grid or flappy's course, and no human has confirmed the scrim
  reads well over either. The browser gate reaches a paused demo but only counts
  HUD lines. The browser gate's canvas capture is the closest thing there is,
  and it happens to fire while the pause menu is up — a human has now looked at
  it for both demos and it reads fine, but nothing asserts it.
- **No test and no tool captures a native sample's pixels.** `breakout` and
  `flappy` take `--headless --frames N` and print a summary; neither has a
  screenshot path, so "the Vulkan build of the game draws the right picture" is
  reachable only by running it on a desktop. The multi-sheet bug lived in the
  shipped samples for that reason: the evidence that it is gone is
  `every_batch_draws_its_own_instances_rather_than_the_first_batchs` exercising
  the same `SpriteRenderer` on radv, plus both samples running 120 headless
  frames with `CRCBL_VK_VALIDATION` and sync validation clean — **not** a
  picture of either game. What it would take: a `--capture <path>` on the sample
  front ends, reading the swapchain image back the way `vk_e2e.rs`'s
  `render_sprites` does.
- **Flappy's swept-sphere collision is exercised, not demonstrated.**
  `game::fatal` sweeps the bird's path with `PhysicsSystem::sweep_sphere`
  because that is the correct query, but at this game's speeds a point test at
  the end of the tick catches every pipe the swept one does — measured, down to
  a tick rate of 3 Hz, where the bird still ends each tick inside the pipe it
  would have tunnelled through. Tunnelling needs a step wider than a pipe plus a
  bird (2.3 units, so under about 2.6 Hz), which is not a rate this game is
  coherent at. Closing it honestly means a faster consumer, not a contrived test
  here.
- **No golden image covers the play field's framing, in either sample.** The
  browser gate reads the canvas back and asserts it is neither blank nor still.
  Flappy's `gpu::the_playable_band_is_on_screen_at_every_aspect_ratio` now puts
  the world through the real view-projection at five aspect ratios, which is
  stronger than the hand-written mapping it replaced, and is still not a pixel
  check that would catch the framing drifting.
- **The overlay has never been looked at.** Every test over it is draw-list
  strings and rectangles; no golden image, and no human has confirmed the panel
  is legible over a lit scene or a bright sprite background. The layout maths
  (value column past the longest label, panel inside the screen at two sizes) is
  asserted; the _appearance_ is not.
- **Nothing has looked at either sample's art come out of a GPU.** Every test
  over it is `Sheet` data, sprite rectangles and layer membership; the pictures
  in both retrofit reports were composited in software from the same sprite
  lists, so they say the scenes are assembled correctly and nothing about the
  shader, the sampler or the blend. `crcbl screenshot` cannot help — it renders
  the sandbox cube through `ForwardRenderer`, which **neither** sample uses now.
  Closing this means either a golden through the sprite pass with a sample's own
  sheets, or an offscreen path the samples can drive. It is the same gap for
  breakout as for flappy and is not worth two entries.
- **Nobody has _looked_ at a fullscreen window.** The mechanism is gated end to
  end now — see _Display-mode coverage_ — but every assertion is a summary line
  or a compositor's tree, not a picture. That the frame is composed correctly at
  the new extent, rather than merely built at it, is unchecked, and is the same
  gap as every other "nothing has looked at it" entry here.
- **The browser's fullscreen path has never been exercised.**
  `web/tools/browser-e2e.mjs` covers focus and pause in a real browser — it
  blurs the canvas, checks the status becomes `STATUS_PAUSED` and that the HUD
  heartbeat stops, and that Escape brings it back — but not F11. Headless
  Chromium's fullscreen behaviour under Xvfb is its own question, and
  `Input.dispatchKeyEvent` is not a user gesture for the purposes of
  `requestFullscreen`, so the check would need `Browser.setPermission` or a
  headed run to mean anything. What is covered instead is the seam:
  `crcbl-shell`'s `fullscreen_is_the_pages_answer_not_the_engines_request` and
  `the_mode_is_right_whichever_order_the_resize_and_the_change_arrive_in` drive
  `__crcbl_web_fullscreen` directly, and `check-exports.mjs` confirms the symbol
  ships and that the shim calls it.
- **Nothing has looked at the pause menu.** It is asserted as draw-list strings
  — the `PAUSED` heading and the first hint line reaching the UI pass — and no
  human or golden image has confirmed it is legible over either game's art, or
  that the full-frame dim reads as a dim rather than as a bug. It is
  deliberately the crudest possible version, behind `draw_pause_menu`, because
  the next slice replaces it.
- **The changelog starts mid-project.** `CHANGELOG.md` covers changes from
  2026-08-01 onward; everything before it is in `git log` only. Worth doing at
  the first tagged release, or not at all — there are no releases yet for a
  reader to be missing entries from.

## What `crcbl-phys` owes, found by writing asteroids

`apps/asteroids` is the P6 physics slice's first consumer. Two of the questions
P6 left open are answered below and no longer open; the rest are what building
against the crate turned up.

**Answered — the wrap's broadphase rule.** P6 left "when is a move a teleport"
to whoever wrote the wrap. Asteroids' `teleport` (in
`apps/asteroids/src/game.rs`) chose: **a wrap is a teleport, and a teleport is a
remove-and-re-insert**, done by calling `PhysicsSystem::set_collider` again,
applied uniformly to everything in the broadphase with no distance threshold —
"did the position change discontinuously" is what a wrap knows and a threshold
would only guess at it.

**Corrected — the wrap is not a correctness bug.** The old entry here said a
teleported body "leaves its ancestors' bounds stretched across the whole field",
implying collisions break. They do not, and this was checked by falsification:
swapping `set_collider` for `set_transform` in that function leaves the whole of
`apps/asteroids`' 49-test suite green, because `Bvh::update_aabb` refits every
ancestor on the way to the root and a stretched ancestor is a conservative
_superset_ — bigger than it should be, never smaller, so it prunes nothing. What
it costs is **tree quality**, not answers.

- **The per-entity routes exist now, and `PhysicsSystem` still has no per-entity
  providers.** `DampingForce::world_force(velocity, mass, dt)` and
  `DragForce::world_force(velocity)` joined `ThrustForce::world_force`, and
  asteroids' hand-rolled `damping_force` — `-k·v` plus the `mass/dt` clamp,
  written out because a provider is global — is deleted. What was **not** taken
  is the other option that entry named: letting `PhysicsSystem` hold providers
  that apply to one entity rather than to every body. Three `world_force`
  methods is the cheaper shape and it stops being so the moment a fourth force
  wants one, or a game wants several forces on one entity and has to call each
  by hand every tick.

  A trap worth keeping, found while testing it: `apply` **delegates** to
  `world_force`, so a test asserting the two agree cannot fail — deleting the
  cap from `world_force` left it green. The test asserts the model written out
  (`-velocity * min(k, m/dt)`) and, in the capped regime, that one step lands
  the velocity exactly on zero. Measured: that version goes red.

- **There is no "what does entity E overlap" query.**
  `PhysicsSystem::overlap_sphere` takes a free centre and radius. An entity that
  is only ever the _subject_ of overlap tests therefore has no reason to be in
  the broadphase at all — asteroids' ship carries no collider, because a leaf no
  query is allowed to return would have needed filtering back out of every
  result by entity id. That is fine here and will not be for a game where two
  things test against each other. What is wanted is an entity-shaped overlap
  with an exclusion list; the same exclusion list is what `sweep_sphere` needs
  and what breakout and flappy both work around by removing the sweeper's own
  collider and putting it back.

- **`PhysicsSystem::overlap_sphere` still fabricates its `ShapeHit`.** `t: 0.0`,
  `normal: DVec3::Y`, `started_inside: true` for every result. Asteroids only
  asks _whether_ anything is there, so it discards the hit outright — which
  means the type is promising a contact that no caller in the workspace can use.
  Either compute a real deepest-point normal or change the return type to entity
  ids. `PhysicsWorld::overlap_sphere` underneath is honest.

- **Rotational dynamics are absent.** `Transform` carries a `DQuat` and
  `ThrustForce` reads it, but there is no angular velocity, no torque and no
  quaternion integration: `RigidBody` has `velocity` and `force_accum` and
  nothing angular. Asteroids' ship integrates its own heading in `turn_ship` and
  writes it through `set_transform`. That is right for this game — a turn rate
  is a constant, not a physical response — and wrong for the inertia tensor the
  design doc describes. Whoever needs real torque adds `angular_velocity`,
  `torque_accum` and an inertia term to `RigidBody` and a rotation step to
  `SemiImplicitEuler`.

- **No benchmark, and no rebuild policy.** Churn cost was measured as tree
  _depth_, not as time: the claim "insert/remove beats a rebuild" is an
  algorithmic one (one root-to-leaf path against `O(n log n)`) and was not
  timed. There is also no policy that ever rebuilds a churned tree — the AVL
  bound is what makes one unnecessary, but a bulk build still produces a tighter
  tree by surface area than incremental insertion does, and nobody has measured
  the query-cost difference between the two. The horde sample (P8, 10k bodies)
  is where that stops being academic.

## `GameModule::tick` runs after the ECS sweep, so a game's destructions lag

`crcbl_ecs::World::tick` runs the schedule and then `sweep`s the deferred
destruction queue. `crcbl_server::Server::tick` calls `world.tick()` **and
then** `module.tick(&mut world)`. So every entity a `GameModule` despawns sits
in the pool for one more tick before the pool lets go of it — and a game reading
`World::entity_count()` between ticks sees a count that is high by however many
things died last tick.

Found by asteroids, whose leak test compares `entity_count()` against
`1 + rocks + bullets` on every tick and failed immediately; it now adds
`World::dead_queue_len()` to the sum. `apps/flappy`'s equivalent test asserts a
`<=` ceiling, which tolerated this without noticing it.

Two possible fixes, and the choice is the engine's:

- **Sweep after the module**, i.e. `Server::tick` calls `world.sweep()` between
  the module and `emit_snapshot`. Arguably more correct anyway: today's snapshot
  is emitted while entities the module destroyed are still in the pool.
- **Leave it and document it**, and have `World::entity_count` grow a sibling
  that excludes the queue, so a consumer is not obliged to know.

Not worked around in asteroids beyond making the test honest. **Horde hits it
harder**: its leak invariant is checked on every tick of a soak, and the queue
is non-empty on any tick something died — which at a hundred spawns a second is
most of them.

## What `crcbl-phys` owes at scale, found by writing horde

`apps/horde` runs `N` broadphase overlap queries per tick — one per enemy, for
separation — plus one for contact damage and one for aiming, so it is the first
consumer where the _per-query overhead_ rather than the query's answer is the
cost. Provisional numbers and their conditions are in
`docs/plan/sample/03-horde.md`; both entries below sit in front of them and
neither was taken, because either is an API change to `crcbl-phys` and this
slice was the sample.

- **The allocations are gone and nobody has measured what they cost.**
  `overlap_sphere_into` runs through all three layers now, and the pass
  allocates nothing once its buffers have grown: `steer_enemies` keeps one
  `QueryScratch` and one neighbour list per thread in a `thread_local!`, which
  is what a `par_for` closure can reach, and the `&mut self` callers still use
  the `QueryScratch` on `PhysicsSystem` and `PhysicsWorld`. It used to be
  **three** `Vec`s per enemy per tick — `overlap_sphere`'s own,
  `PhysicsWorld::overlap_sphere`'s, and `Bvh::traverse_aabb`'s — which at the
  plan's ten thousand is 1.8 million allocations a second, every one dropped
  immediately.

  **What is not known is whether it mattered.** No before/after number exists,
  for the reason under _What horde still owes_: ten thousand enemies kill the
  player in under a second, so a wall-clock run measures a simulation that has
  stopped, and this repository has no allocation counter and no benchmark
  harness. The change is justified by the count, not by a measurement, and
  anybody quoting it as a speed-up is quoting something nobody ran.

- **A broadphase query costs what its _answer_ costs, so the tick's cost tracks
  local density rather than entity count.** The same ten thousand enemies cost
  14.66 ms a tick spread over the arena and 84.09 ms after eight seconds of
  converging on the player — measured, both columns, in
  `docs/plan/sample/03-horde.md`. This is not a complaint about `crcbl-phys`; it
  is the fact any budget stated in "N agents" is wrong about, and it is why
  18a's provisional 8–9k figure was both too optimistic (it never let the crowd
  converge) and taken on a fixture that at ten thousand described a field larger
  than the arena. **Anything that quotes a per-agent cost for this crate has to
  say what the neighbourhoods looked like.**

- **The neighbour sum's order is the BVH's traversal order, and horde chose to
  live with it.** Floating-point addition is not associative, so the separation
  vector depends on the order `overlap_sphere` returns neighbours in. That order
  is deterministic — the tree is a pure function of the insert/remove sequence,
  which is a pure function of the seed and the script, and
  `the_same_script_replays_bit_identically` covers it — but it is deterministic
  _because of the tree_, not independently of it. Sorting each neighbourhood by
  entity id would make it independent, at the price of a sort per enemy per
  tick. **Declined** for that reason. It becomes a real question the moment the
  tree's build order stops being reproducible — a parallel insert, or a rebuild
  policy that depends on timing — and whoever adds either should read this
  first.

## What horde still owes

S3 is done — the core loop, the art and progression, and now audio, the longest
run, the browser demo and the scale measurement. `docs/plan/sample/03-horde.md`
carries the numbers and their conditions; this is what was raised and not
finished. Entries the measurement closed have been deleted rather than
annotated.

- **`--prefill` starts its own run, and that coupling is not obvious.**
  `assemble` in `apps/horde/src/app.rs` queues a start edge when
  `options.prefill > 0`, because the scale fixture would otherwise measure a
  `run_tick` that returns on its second line. It is one call beside
  `Game::stage_field` and `a_prefilled_run_does_not_wait_at_the_title_screen`
  holds it. Anything else that stages a board before the first frame — a replay
  header, a future demo mode — has to do the same or it will measure nothing and
  say it measured everything.

- **The plan's exit criteria are internally inconsistent and need rewriting, not
  answering.** "10 000 enemies at 60 Hz tick" is true of a crowd spread over the
  arena (14.66 ms of a 16.67 ms budget) and false of the same crowd converged on
  the player (84.09 ms). The difference is a factor of 5.7 at a fixed count,
  because separation is a broadphase query whose cost is the size of its answer;
  a horde converges by construction, so the second number is the one the game
  spends its time at. Whoever owns the criterion has to say **which crowd**.

- **"Playable and mildly fun for 5 minutes" cannot be true of this arena at the
  plan's count.** Ten thousand enemies in 96 × 72 units is 0.82 units apart,
  several inside `PLAYER_RADIUS` on frame zero, and contact damage is a rate
  summed over everything touching — so `--prefill 5000` and above kills the
  player in under a second. A default run, spawner only, dies at about 24
  seconds with 46 things on the field. Two ways out and neither is obviously
  right: **a bigger arena** (the density falls as the area grows, and the follow
  camera and `clamp_to_arena` already handle any size, but a 300 × 225 arena
  changes what the game _is_ — the walls are what makes kiting finite), or
  **admit the count is a benchmark target** and let the exit criterion carry two
  numbers, one for the budget and one for the game. Nobody has decided.

- **The CPU cull is still per-sprite and still `N` per frame**, and it is now
  measured: 28 µs at ten thousand, of a 16.67 ms budget. That is the work P7's
  GPU culling exists to delete, and the measurement says deleting it is worth
  0.17 % of a frame to this sample. Keep it as the reason P7 exists for _other_
  scenes; it is not the reason it exists for this one.

- **`crcbl-audio` has no voice limit, no priority and no stealing.** Still true
  after the mixer-adoption slice, which deliberately left it there.
  `apps/horde/src/audio.rs` caps itself at `MAX_VOICES` = 16 and refuses the
  newest voice, counting refusals in `Audio::dropped()`. Refusing the newest is
  the crudest answer that is honest and it is audibly wrong in one case: a
  player's _death_ cue can be refused by sixteen kill cues raised on the same
  tick. Wanted in the crate: a voice budget with a priority, so an important cue
  steals the oldest cheap one. The refusal count is on the debug panel now (the
  `audio` section's `dropped` row), so the pressure is visible while the
  crate-level budget stays undecided.

  One detail changed with the adoption and is worth knowing before anyone moves
  the cap into the crate: horde now reads `Mixer::voice_count` and then calls
  `Mixer::play`, two lock acquisitions where the hand-rolled queue held one
  across both. Only the game thread adds and only the audio thread removes, so
  the count can be stale **low** and never stale high — the cap can refuse a cue
  that had just been made room for, and can never let the count past
  `MAX_VOICES`. A cap inside the crate would not need the two-step at all.

- **Nothing has listened to the five cues**, on any device. They are synthesised
  deterministically from a fixed seed, so a golden buffer is possible and there
  is not one. What the tests assert is that each cue fires, that it carries the
  position of the thing that raised it, that the listener is the player rather
  than the origin, and that the level cue actually sweeps in pitch. No test can
  tell a good kill sound from a bad one.

- **The HUD line can still outgrow its backdrop at extreme settings.**
  `the_hud_fits_the_panel_it_is_drawn_on` measures both lines through the real
  `FontAtlas` at a stated worst case — a five-minute run at the shipped enemy
  cap, level 18, 2 048 kills — and requires them inside `HUD_PANEL_RIGHT`. It
  does **not** bound `--max-enemies 10000` with a twenty-minute soak behind it:
  five-digit fields are wider than the panel and the text would run off the end
  of it, which is what the browser gate's canvas capture caught the last time
  the panel was too narrow. A real fix is a HUD that measures itself and sizes
  its own backdrop, which is a `crcbl-ui` widget rather than a `DrawList::rect`.

- **Nothing enforces that the arena is a plane.** Positions are `DVec3`,
  everything the game produces sits at `z = 0`, and `clamp_to_arena` passes `z`
  through untouched. A body given a non-zero `z` would separate in depth and
  never be brought back — which a test fixture using `DVec3::splat` did, and
  which is how this was noticed. Either clamp `z` too or make the fact a type.
  Not a live bug: no production path can produce one.

- **The horde does not avoid the walls, it is pushed into them.** Seek is a
  straight line to the player and separation knows nothing about the arena, so a
  crowd chasing a player along an edge piles into it and the clamp holds it
  there. It reads acceptably and it is not pathfinding — which is a hard
  non-goal — but "walk around the obstacle" is the first thing a player will
  expect if props ever land in the arena.

- **Contact damage has no invulnerability frames**, by choice: it is a damage
  _rate_ summed over whatever is touching, so a stack of enemies is worse than
  one and there is no per-enemy timer on the hot path. The consequence is now
  measured rather than predicted — see the density entry above — and it is what
  makes the plan's count unplayable in this arena.

- **Nothing pulls a gem towards the player.** `Upgrade::Magnet` widens the
  radius the collection query runs at, so a gem inside it is banked on the tick
  it comes into range and one outside it is not. The genre's version drifts the
  gem to the player, which reads far better and which would be `P` steering
  updates a tick on top of the `N` the sample measures. Left out deliberately:
  the point of this slice's pool is that each upgrade is one line.

- **A gem that is never collected is lost, silently.** `MAX_PICKUPS` is 512 and
  a kill on a full field drops nothing; `Game::pickups_dropped` counts the
  refusals and nothing shows them. It is bounded and deterministic, which is
  what it was for, and a player kiting away from a heap of loot in a long run
  will not be told why their level-ups stopped. A HUD line, or dropping the
  _oldest_ gem instead of refusing the newest, would both fix it. **A potion is
  refused by the same ceiling**, which is worse than losing a gem — the rarest
  drop in the game can be eaten by a field of litter and the player is told
  nothing. `drop_pickup` says why it is not special-cased: a kind that could
  jump the queue is an unbounded population wearing a bound. Dropping the oldest
  gem would fix this half too, and is the option to weigh first.

- **Nothing on screen tells a potion from a gem before you reach it, except the
  picture.** There is no minimap, no pickup outline and no HUD line;
  `art::tests::a_potion_is_not_a_gem` is what says the two silhouettes are
  distinguishable at all, and it measures the baked frames rather than what a
  player at a distance can resolve. Nobody has looked at a field of both on a
  real display. The claim being made is about shape, not colour, precisely
  because red-against-green is the one pair a large minority of players cannot
  use — but that reasoning has not been checked against a simulated deficiency
  either.

- **A potion always lands beside a gem, and both are taken together.** A brute
  drops both, one pickup diameter apart, so a player who walks over one almost
  always takes the other in the same tick — the collection radius is wider than
  the gap. Nothing is lost by that (both pay out) and it does mean the potion is
  never a separate decision to walk to, which is half of what a rare pickup is
  for. Placing it further out would need a bound nobody has stated against
  `clamp_to_arena` and against a brute dying in a corner.

- **`POTION_DROP_CHANCE` was tuned against the autopilot, not against a
  player.** The kiting `autopilot` in `game::tests` walks a fixed circle and
  takes steady chip damage, which is not how a run is actually lost; the rate
  was settled by finding where `a_long_run_leaks_nothing` stops reaching a death
  (one brute in ten survives the whole soak on single-figure hit points, one in
  twenty dies and restarts). That makes the number defensible and not the same
  as playtested. Nobody has played a run with potions in it.

- **The level-up screen has no way out but forwards.** There is no "skip", and a
  choice out of range is ignored, so a run that reached `LevelUp` stays there
  until one of the three digits is pressed. The loop's Escape still pauses over
  it and the death menu cannot be reached from it — nothing can kill the player
  while the field is frozen, so this is not a soft-lock, but it does mean a
  browser demo left on the level-up screen looks stopped. `browser-e2e.mjs`
  watches the once-a-second `[HUD]` heartbeat, which keeps firing, so the gate
  itself is fine. Headless runs reach past the screen with `--choose <N>`, which
  is what took the potion-drop measurements out of `game::tests`.

- **The upgrade pool is repeatable without limit.** `RapidFire` has a floor
  (`FIRE_COOLDOWN_FLOOR`) and the other five do not, so a very long run has an
  unbounded weapon range, walk speed and hit-point ceiling. It is a five-minute
  game and nobody has played it for twenty; caps are a balance decision, not a
  bug, and they are not there.

- **Enemies do not turn to face anything.** Every silhouette is deliberately
  non-directional — a lump, a four-legged X, a horned slab — so no sprite
  rotation is needed and no `atan2` runs per enemy per frame. It is the right
  trade at 10k and it does mean the crowd has no sense of heading. The _player_
  turns, and it does it by reversing the frame's `u` range (`art::mirrored`)
  rather than by rotating — which would cost nothing per enemy either, if a
  future enemy ever wants a heading.

- **The wizard's walk cycle has never been watched.** `art::mirrored`'s reversal
  is now rasterised — `crates/crcbl-vk/tests/vk_e2e/sprite/mirror.rs` renders a
  frame and its mirror and compares the two images column-reversed, bit-exact on
  radv, which is the shader-side evidence the older entry asked for. What is
  still unverified is the _animation_: nobody has seen the walk cycle play in a
  running window, and the browser gate's canvas capture that a human looked at
  predates the flip.

- **A wizard walking into a wall keeps walking on the spot.**
  `RenderState::player_walking` is the intent, not the velocity after
  `clamp_to_arena`, so a player holding a direction against the arena edge
  animates while going nowhere. Deliberate — it is what the player is doing, and
  taking it from the velocity would make the wizard freeze mid-stride against
  every wall — but it is the one place the animation and the movement disagree,
  and it is worth knowing before someone "fixes" it.

- **Not measured, not reviewed: the windowed native path.** It is compiled and
  never run — there is no display in this environment — so the follow camera,
  the sprite pass, the three menus and the HUD layout have been checked by test,
  by argument, and now by a **browser**: the gate's canvas capture at 26/26 is a
  picture of the real game, and a human has looked at it. What has still never
  been seen is the _native_ window, and the fullscreen toggle against a real
  compositor is the same gap the other three samples carry.

- **Every scale number was taken on an offscreen image ring, not a swapchain.**
  `--headless` gives `crcbl-vk` a `SurfaceTarget::Offscreen` rotation of images,
  which exists precisely so that it is the same acquire/record/submit/present
  path — but it is not a windowed present, it is not vsynced, and it is 960
  × 720. A windowed 1440p run would raise the sprite pass's fill by about four
  times, which on a 0.023 ms pass is still nothing, and nobody has taken it.

- **There is no Tier B / browser scale number.** The exit criteria ask for one
  ("Tier B/wasm gets its own smaller recorded budget") and the only browser this
  repository can drive is Chromium's SwiftShader under Xvfb, which measures a
  software rasteriser. It needs a machine with a real browser GPU and a way to
  read `PassTimers` out of a wasm build — the second of which does not exist:
  the demo has no way to report its frame timings to the page.

- **Nothing checks that a `.crpix` texel lands on a whole screen pixel.** At
  `TEXELS_PER_UNIT` = 20 and a 720-pixel-high view of 28 world units, one texel
  is 1.286 screen pixels, so `SampleMode::Pixel`'s nearest sampling drops and
  doubles rows as the camera moves. Every sample has this and none of them
  addresses it; the fix is an integer-scaled render target, which is a renderer
  feature nobody has asked for.

## What asteroids itself still owes

S2 is done — simulation, art, audio, persistence and the browser demo. What is
not:

- **No golden buffer for the cues.** The three sounds are synthesised
  deterministically — `audio::noise` runs splitmix64 from a fixed seed — so a
  golden buffer is _possible_, and there is not one. What the tests assert is
  that each cue fires, that it carries the position of the thing that raised it,
  and that the explosion decays and is not a tone. Nobody has listened to the
  result on a real device and no test can tell a good explosion from a bad one.
- **The 10-minute soak in the exit criteria was not run.** What runs in CI is
  `hundreds_of_spawns_and_deaths_leak_nothing`: 18,000 ticks (five minutes of
  simulated play), 337 rocks spawned, 1,221 bullets fired, six waves cleared,
  checking the entity and collider accounting on **every** tick. Ten minutes of
  wall-clock soak with the inspector open, and the "no stale-handle panics with
  entities selected as they die" criterion, both need the entity inspector,
  which this sample does not use yet.
- **The overlap query does not know about the seam.** Ship-versus-rock is a
  single `overlap_sphere` at the ship's position, so a ship straddling an edge
  does not see a rock straddling the opposite one until one of them has wrapped.
  A full answer queries up to four offset positions. Deliberately not done: it
  costs four broadphase queries a tick to fix a one-tick artefact at a boundary
  both bodies cross constantly, and no test could tell the difference without
  being written to.
- **No golden image covers a single asteroids pixel.** Weaker than it was: the
  browser gate now loads the demo in a real Chromium, opens a WebGPU device and
  reads the canvas back with every check green, so "the frame is not blank, not
  one flat colour and changes between frames" is checked — 89 distinct colours
  across a 959×463 canvas on the SwiftShader adapter. What is still unchecked is
  whether it is the **right** picture, and in particular whether a rotated
  `SampleMode::Pixel` sprite looks right on a real driver.
  `crates/crcbl-vk/tests/vk_e2e.rs` has sprite goldens including a rotated one,
  so the shader path is covered; the game's own frame is not. There is also no
  display in the build environment, so the _windowed_ native path is compiled
  and never run. The art was checked by eye against the baked PNGs, and that is
  the honest report of it.
- **Tuning constants are compiled in.** The plan's milestone 3 wants them from a
  data file after stage 6. Every one of them is a `pub const` in `game.rs` with
  its reasoning written beside it, which is the form that survives being moved
  into a file.

## Deferred decisions

Questions that came up mid-slice and were answered by judgement rather than by
asking. Each is the question, the answer taken, and **what would change it** —
because the point is that a later reader can reopen one cheaply instead of
rediscovering that it was ever a question. An entry here is not a complaint
about the answer; most of these are probably right.

Distinct from _Considered and declined_ below, which is for ideas rejected on
their merits and expected to stay rejected. These are answers taken under
uncertainty.

- **Should `SoundBank` hold `Arc<[AudioSample]>` rather than `Vec`, so
  `create_voice` stops copying the sound?** _Yes, and it is why horde adopted
  the bank at all._ `SoundBank::create_voice` cloned the whole sample buffer per
  voice, which at horde's cue rate — up to about forty a second, each an
  allocation the size of the sound — was the one measured reason to keep the
  hand-rolled `Arc<Sound>` bank instead. Changing `Voice::data` to
  `Arc<[AudioSample]>` and `create_voice` to `Arc::clone` deletes the reason,
  and `Voice::new(Vec)` still compiles for every existing caller.
  `a_bank_shares_one_buffer_with_every_voice_it_makes` in
  `crates/crcbl-audio/src/mixer.rs` pins it on `Arc::strong_count`. **What would
  change it:** a bank that wants to hand out _mutable_ sample data, which
  nothing does.

- **Should `AudioStream::open` have kept taking `impl AudioSource` by value, or
  changed to `Arc<dyn AudioSource>`?** _Kept — the sharing went into a blanket
  `impl<T: AudioSource + ?Sized> AudioSource for Arc<T>` instead._ Changing
  `open`'s signature would have broken every existing caller, including the
  `wasm32` `web::install` path and the crate's own tests, for a case a blanket
  impl serves without touching any of them:
  `AudioStream::open(Arc::clone(&mixer))` now type-checks and a non-shared
  source still moves in as before. The cost is one redundant `Arc` layer on the
  shared path — `open` wraps whatever it is given in an `Arc` of its own — which
  is a pointer chase per block, not per sample. **What would change it:** a
  source that needs to be reached from the stream _and_ from two other places
  with different types, where the double `Arc` stops being the only wart.

- **Should the voice cap have moved into `Mixer` while the samples were being
  migrated?** _No — horde keeps `MAX_VOICES` and its refuse-newest policy._ The
  crate has no cap, no priority and no stealing, and the honest version of that
  feature is a voice budget with priorities so a death cue can steal a kill cue,
  not a bare count. Shipping the bare count in the engine would have frozen the
  crude policy as the crate's answer and taken the evidence for the good one
  with it, since horde's `Audio::dropped()` is the only measurement of the
  problem anyone has. **What would change it:** a second sample needing a cap,
  which would make it a pattern rather than one game's answer.

- **Should the samples' spatial assertions read the mixer, or the rendered
  audio?** _The mixer, through `Mixer::voice_mixes`._ Rendering a block and
  measuring left against right is the stronger observable and it was tried: it
  races the null stream's polling thread, which is draining the same mixer every
  five milliseconds and will have eaten an unpredictable prefix of any voice by
  the time the test looks. The gain-reaches-the-output half is checked once, in
  the engine, where a test can own a `Mixer` with no stream attached —
  `set_mix_re_aims_a_voice_that_is_already_playing`. **What would change it:** a
  headless `Audio` that opens no stream at all, which would make the render
  check deterministic in every sample.

  **Same race, second test, observed once:** asteroids'
  `the_engine_is_one_looping_voice_that_outlives_its_buffer` failed on
  `macos-latest` 2026-08-07 with "the engine's release block was cut" and passed
  on the immediate rerun and on both the preceding and following CI runs — no
  macOS-relevant diff separated them. Its release-block check has the same
  window as the spatial assertions above: a headless `Audio` opens the null
  stream, whose polling thread can consume the one release fade between
  `set_thrust(false)` and the test's own `fill`, so the test sees silence and
  blames the backend. The fix is the same one — a headless `Audio` with no
  stream — and it is the same decision, per sample.

- **Where does the menu art live?** Taken: **`crates/crcbl-render/assets/`**,
  baked by that crate's own `build.rs`. `apps/*` cannot depend on each other, so
  per-sample art is the same window authored three times and three games that
  look like three engines. The rejected alternative was a shared `assets/`
  directory both build scripts reach into: it shares the `.crpix` and nothing
  else — each script still bakes, each `art.rs` still loads, each game still
  writes the layout — and it puts a `../../..` path outside a package's own
  tree, which cargo does not track for rebuilds the way it tracks a package's
  files. It also gives `crcbl-vk`'s suite nothing, because that crate cannot see
  `apps/`, so the golden would be a picture of a replica. _Changes it_: art that
  is genuinely one game's. A sample that wants its own frame should author it
  under its own `assets/` and pass its own `MenuArt`; the shape for that is a
  constructor beside `MenuArt::register` taking a `Sheet`, not a fork of this
  one.

- **What size is the menu drawn at?** Taken: **the largest whole scale in 1..=4
  whose panel fits inside 90% of the framebuffer**, a pure function of the
  extent and the menu's own contents (`Menu::layout`). Whole numbers because the
  art is pixel art and a fractional scale puts a nine-slice corner on a half
  pixel; a fit rather than a constant because a fixed size is either lost on a
  4K screen or off the bottom of a 1440×400 canvas. _Changes it_: a settings
  screen with a UI-scale slider, at which point the scale is the player's and
  `layout_with` is already the entry point that takes one.

- **Does the menu shadow a key a sample had bound?** Taken: **yes, once** —
  flappy's `ArrowUp`, which is the _second_ binding of its flap action beside
  `Space`. The three menu keys (Up, Down, Enter) are the same three in every
  sample, for the reason F3, Escape and F11 are; two of the three are free in
  every game and this one is not. Space is never shadowed, is what the HUD has
  always named, and is printed on every button that flaps. The keys are consumed
  only while a menu is on screen. _Changes it_: a sample that binds Enter or the
  vertical arrows to something a player uses _while a menu is up_ — which today
  is nothing, because a menu is only on screen when the simulation is stopped or
  waiting. **Asteroids is the second, and it shadows `ArrowUp` too** — its
  second thrust binding, beside `KeyW`, which is not shadowed. Same trade, one
  sample later, and it costs less there: a menu is up only on a frame the ship
  is not being flown.

- **Does the world keep drawing behind a menu?** Taken: **yes, and it is
  dimmed** by a scrim sprite the menu's own pass draws. A frozen screenshot
  would need a captured frame and a second code path; a menu with nothing behind
  it loses the player's place. The scrim is a _sprite_ and not a `DrawList`
  rectangle because the UI pass runs after the sprite pass, so a UI-pass scrim
  would dim the menu's own frame along with the game. _Changes it_: a menu that
  wants the game genuinely stopped in the background — a settings screen over a
  paused multiplayer session, where the world is still ticking and the motion is
  a distraction.

- **Does a looping ping-pong replay its end frames?** Taken: **no.** A looping
  ping-pong's period is `2n - 2` — four frames run `0 1 2 3 2 1` and then `0`
  again — while a one-shot is `2n - 1`, precisely because that trailing `0` is
  no longer the next cycle's first, and an out-and-back that stopped on frame 1
  would look truncated. `Clip::steps` and `Clip::step` in `crcbl-sprite`.
  _Changes it_: art that wants a beat held at an end — a wing pausing at the top
  of its stroke. Today that is spelled by giving the end frame a longer `hold`,
  which works and is per-frame; if it turns out to be the common case rather
  than the exception, the answer is a hold on the return leg, not a global flag.

- **Does `reverse` reverse a clip's holds too?** Taken: **no** — a hold belongs
  to the frame it holds, not to the position in the list, so a reversed clip's
  tick pattern is the forward one read backwards. Documented on `Clip::step`.
  _Changes it_: a consumer wanting the reversed clip to have the same
  tick-by-tick timing _profile_ as the forward one (slow-then-fast staying
  slow-then-fast). Nothing has asked, and the current rule is the one that makes
  a frame's timing a property of the frame, which is easier to author against.

- **What does a nine-slice do when the target is smaller than its corners?**
  Taken: **the fixed bands shrink in proportion and the stretched band
  vanishes.** `NineSliceSource::expand`. The two alternatives were both worse:
  _refusing_ — emitting nothing, or clamping the target up to the minimum —
  makes a pipe squeezed below its caps either disappear at one size and not
  another or spill outside the rectangle it was handed; _letting the corners
  overlap_ inverts the middle band, which with no backface culling rasterises a
  mirrored quad rather than nothing, and double-blends the overlap. Shrinking
  keeps the three properties that matter more than corners staying literally
  fixed at a size where they arithmetically cannot: the quads still tile the
  target exactly, nothing is drawn outside it, and the picture is continuous —
  at exactly the minimum size this path and the ordinary one agree. _Changes
  it_: a caller that would genuinely rather draw nothing than draw squashed
  corners. That is `NineSlice::fits_in` at the call site, not a change here.

- **Should nine-slice edges tile instead of stretching?** Taken: **no tiling
  mode at all.** Two concrete costs: a tiled band is `ceil(extent / inset)`
  quads rather than one, so the instance count stops being bounded by nine and
  starts depending on how big the thing was drawn — a pipe stretched to a tall
  gap would quietly become hundreds of instances — and doing it in UV space by
  letting `u1` run past 1 needs a repeating sampler, while `SpriteRenderer` has
  exactly one sampler, `ClampToEdge`, shared by every sheet. _Changes it_: art
  whose edge is a repeating motif that stretching visibly smears — a chain, a
  rope, a brick course. Then it is a new mode with its own quad emitter, not a
  flag on `expand`.

- **Teach the UI pass a second texture, or draw button skins as sprites?**
  Taken: **sprites**, and an older backlog entry that called this "blocked on
  the UI pass sampling a second texture" was deleted as wrong rather than
  satisfied. The UI atlas is a single-channel `R8Unorm` glyph _coverage mask_
  sampled into alpha only — every fragment's RGB comes from the vertex colour —
  and a button skin is RGBA colour art. Routing it through would need a second
  bound image in a second format, a per-quad branch between two samplers, a
  UV-carrying draw command `DrawList` does not have, and an RGB path added by
  hand to both tier permutations of `ui.slang`. `SpriteRenderer` already is an
  instanced RGBA pass with alpha blending, and a skinned button is nine sprites.
  _The cost paid_: the caller owns the ordering. `RenderGraph` runs passes in
  declaration order with no topological sort, and both passes load rather than
  clear, so `SpriteRenderer::add_pass` must precede `UiRenderer::add_pass` or a
  skin paints over its own label — enforced by nothing but the order of two
  lines. _Changes it_: a UI element needing colour art _interleaved_ with text
  rather than behind it, which two passes cannot express at any ordering.

- **A fixed backdrop for breakout, or a parallax band?** Taken: **fixed.**
  _(Moved here from Considered and declined — it is a judgement about this
  game's camera, not an idea rejected on its merits.)_ Breakout's camera never
  moves — the field is fixed and the whole of it is on screen — and `Parallax`
  is `(1 − factor) × camera`, so with a camera at the origin every factor
  produces the same offset of zero. A "distant" layer and a world-locked one
  would be the same picture, and a band that scrolled anyway would be motion the
  player has no reason for. `art::Scene`'s two layers are both `Parallax::WORLD`
  and exist for depth ordering, which is the half of a `LayerStack` that still
  means something here. _Changes it_: breakout gaining a camera that moves — a
  screen-shake on a brick break would be the obvious one, and is currently a
  scope violation under the sample's "no juice" cap.

- **Commit the baked PNGs beside the `.crpix` text?** Taken: **no.** _(Moved
  here from Considered and declined.)_ It would make the build faster and the
  art reviewable in an ordinary diff, and it would create two sources of truth
  for one picture — the one a reviewer reads being the one that is not loaded.
  `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input, and both
  samples' `build.rs` keep it that way. _Changes it_: a build where baking is
  slow enough to be felt, or a review workflow that genuinely cannot read
  `.crpix`. Neither is true today — the baker is a strip blit — and the honest
  fix for the second would be rendering `.crpix` in review, not committing PNGs.

- **What phase are the eleven sprite slices?** Taken: **P4B**, by analogy with
  P4A audio, which was the same shape — a subsystem that was not in the original
  phase table, delivered between numbered phases. It is written into both the
  roadmap's status table and its phase table. _Changes it_: a preference for a
  different label. Nothing depends on the string except the roadmap's own
  cross-references and this file.

- **What stays at P10 now that the frame-timing core is built?** Taken: **the
  rest of it.** The core shipped early, out of P10, because both existing
  samples wanted it and two more are planned before P10 — leaving it there would
  have guaranteed a third and fourth per-sample HUD, the shape `web.rs` already
  took twice. What P10 still owes is the rest of `07-ui-debug.md`'s suite
  (inspector, console, culling stats, debug-draw controls, UI inspector) and
  `23-netcode.md`'s netgraph, which is unbuildable before the transport can
  measure itself. _Changes it_: a sample that needs one of those sooner, which
  is the same argument that moved the frame-timing core.

- **How does a module register with the panel — retained list or per frame?**
  Taken: **per frame**, `DebugPanel::add(&dyn DebugModule)` once per system the
  frame actually has, matching the crate's immediate-mode authoring. A retained
  registry would need the panel to hold borrows or `Rc`s of every system that
  reports, which is the plugin framework `07-ui-debug.md` explicitly does not
  want, and it would make "a section appears because the system is present" into
  "a section appears because someone remembered to register and to unregister".
  _Changes it_: a module whose data is expensive enough to want gathering off
  the frame path, which would want a handle rather than a per-frame call.

- **What does the panel's FPS number mean?** Taken: **frames divided by the time
  they took** over a rolling 120-frame window, not the mean of the per-frame
  rates. The two agree only when every frame is the same length: 10 ms and 30 ms
  average to 67 FPS as reciprocals and to 50 FPS as `2 / 40 ms`, and the second
  is what the window actually ran at. 120 frames is two seconds at 60 Hz — short
  enough to react while you are looking at it, long enough to read. _Changes
  it_: wanting a 1%-low figure, which needs the sorted window this deliberately
  does not keep.

- **Which samples are exempt from the pixel-art rule?** Taken: **hud, viewer and
  sparks**, on the ground that each one's _subject_ is something other than
  pictures — a widget gallery, the user's own glTF, and a particle workbench —
  so authored sprite art in front of it would be showing the wrong system. hud
  still authors its button skins as `.crpix` because a skinned widget is a
  widget. Every other sample on the ladder is in scope. _Changes it_: a sample
  arguing itself out, which sample rule 11 requires it to do in its own doc with
  a reason.

- **Is `docs/code-review.md` a record or a description of current state?**
  Taken: **a record**, and left unedited except for a line in its header saying
  so. It is dated 2026-08-01, was added in one commit and never amended, and the
  roadmap already says its findings were fixed across eight commits. Several of
  its findings now describe code that no longer exists — the `paddle_model`
  finding is the clearest, since breakout has no forward pass at all. _Changes
  it_: a decision to keep it live, which would mean re-running the review rather
  than patching the findings that happen to have been noticed.

- **What does a paused frame do to the fixed-tick accumulator?** Taken: **update
  the clock and drain the accumulator without stepping the game.** The three
  candidates only differ after a long pause. _Not calling `update`_ freezes
  `FrameClock::last_update`, so the first update after the pause measures the
  whole of it and the `DEFAULT_MAX_CATCH_UP_TICKS` cap turns it into eight ticks
  in one frame — measured, not reasoned: falsifying the drain that way makes
  `resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst` report "ran 8
  ticks" in all three samples. _Updating but not draining_ saturates the
  accumulator at the same cap and lurches identically, also measured. Draining
  leaves only the sub-tick remainder, so the first live frame runs the one tick
  it is owed, and it keeps `render_dt` real so the debug overlay's frame graph
  does not flatline at whatever it read when Escape was pressed. The cost is
  that `FrameClock`'s `TickId` advances during a pause; nothing in any sample
  reads it. _Changes it_: a consumer that does — a networked sample whose tick
  ids have to line up with a server's — which would want an explicit
  `FrameClock::reset` rather than a drain loop.

- **Is pause a `GameState` variant or the app loop's?** Taken: **the loop's.**
  Both samples' `GameState` lives inside `GameLogic`, which the authoritative
  server's `GameModule` mutates from inside a tick and which the client
  replicates; a `Paused` variant there would make the server's state depend on
  which window a player's compositor has focused, and would put a value in
  `Summary::state` that a headless scripted run could reach. Pause is not
  something the simulation does — it is the loop declining to advance it — so it
  is the loop's — `crcbl::engine::Loop::is_paused`, reported out through
  `RunSummary::paused` and from there into each game's own `Summary`. _Changes
  it_: a pause the _simulation_ has to know about, which in a multiplayer build
  it would: pausing a shared world is a server decision and would be a state on
  the server, not a client's window losing focus.

- **Does regaining focus resume?** Taken: **no.** A player who clicks back into
  the window would otherwise arrive mid-ball with no warning, and the pause menu
  exists to be dismissed on purpose. This also keeps the two edges asymmetric on
  purpose: focus loss is a thing the platform does _to_ the game, resuming is a
  thing the player does. _Changes it_: a sample where pausing costs the player
  something (a timed run), where the two-step would read as a penalty.

  **Read this together with "Should the click that refocuses a canvas reach the
  game at all?" above.** "Focus does not resume" is about the focus _event_. In
  a browser the gesture that delivers it is a click inside the game, so clicking
  back in onto `RESUME` does resume — one step, not two. The decision above is
  intact; the gesture is not the same gesture on every platform.

- **Which key pauses, given that a browser reserves Escape?** Taken: **Escape
  anyway.** Neither sample's action map binds it — breakout declares arrows,
  Space and R; flappy declares Space, Up and R — and it is what a player tries
  first. In a fullscreen browser demo Escape both leaves fullscreen and pauses,
  because `requestFullscreen` reserves the key and no page can decline it. That
  is one keystroke doing two reasonable things rather than a collision worth
  designing around. _Changes it_: a sample that wants Escape for something else,
  or a pause menu with a back-navigation stack where "leave fullscreen" and
  "close the menu" would want to be separate steps.

- **Who calls `requestFullscreen` in the browser — the shell or the page?**
  Taken: **the page.** A browser grants fullscreen only from inside a
  user-gesture handler; the shim's `keydown` listener is one and a
  `requestAnimationFrame` callback is not, and the engine reads a key on the
  frame _after_ the `keydown` that carried it, by which time the gesture is
  over. Calling it from Rust would also mean the wasm module's first non-`wbg`
  import, which `web/tools/check-exports.mjs` exists to prevent. So
  `web/engine/shell.js` binds F11 itself and reports the outcome through the new
  `__crcbl_web_fullscreen` entry point, exactly as a compositor answers
  `Shell::set_mode` with a configure rather than obeying it. The cost is that
  `FULLSCREEN_KEY` is spelled in four places — three `app.rs` files and
  `shell.js` — with nothing but a comment holding them together. _Changes it_: a
  second key wanting a gesture (pointer lock is the obvious one), which would be
  the point to give the shim a small table the engine can publish rather than a
  second hard-coded key.

- **Does the sandbox get a pause too?** Taken: **yes.** It has no game, and it
  does have a cube on the fixed timestep — the one thing in it a player can see
  stop — and the samples' standing rule is that a facility switched on in one is
  switched on the same way in all of them. It costs about fifteen lines.
  _Changes it_: nothing likely; if the sandbox ever became a pure benchmark
  harness, pausing it would be noise.

- **Should `MenuSet::activate` and `MenuSet::point` return the game's own
  `MenuAction` rather than a `WidgetId`?** _No — they return the id, and each
  sample maps it._ Returning the action needs a trait
  (`fn from_id(WidgetId) -> Option<Self>`) that every game with a menu must
  implement, to save one `.and_then(MenuAction::from_id)` at two call sites in
  `app.rs` and two test helpers per sample. The id is also what the layer
  beneath actually deals in: `Menu::activate` and `Menu::point` both return
  `Option<WidgetId>`, so the set passing it through adds no translation of its
  own. _Changes it_: a consumer that threads the action through several layers,
  where the `and_then` would start appearing at call sites that have no business
  knowing about ids.

- **Should the sandbox get a `MenuKind` enum for symmetry with the other four,
  instead of keying its set by `bool`?** _No._ `MenuSet<bool>` is what its one
  menu actually is, `false` is the state with no entry, and
  `apps/sandbox/src/app.rs` already called `self.menus.show(self.paused)` — the
  `bool` was always the key. An enum would be code added to make five files
  rhyme. _Changes it_: the sandbox growing a second panel, or the loop
  extraction turning out to need one `K` across all five.

- **Where does horde's "has the offer changed?" guard live now that the
  container is the engine's?** _In a `LevelUpOffer` type in
  `apps/horde/src/menu.rs`, held by the game itself — the `Horde` struct's
  `offer` field, rebuilt from `HostedGame::menu_kind`._ `MenuSet::replace`
  rebuilds unconditionally and drops the capture; deciding _when_ a panel is
  stale needs `built_from: Option<(u32, [Upgrade; 3])>`, which the engine cannot
  hold because it knows nothing about upgrades. The alternative was putting that
  field on the loop and inlining the comparison in `draw_menu`, which is the
  same state in a place where it could not be unit tested — and which is no
  longer even available, since the loop is `crcbl::engine::Loop` and a sample
  cannot add a field to it. _Changes it_: a second sample growing a rebuilt
  panel, at which point the guard is a shape and not horde's alone.

## What the horde Pages flake left behind

The flake itself is fixed and deleted from this file (2026-08-03, diagnosed from
the run's uploaded page log: the gate clicked the canvas **centre** to hand the
page its keyboard, which pressed `PLAY` — horde's centred first item — and
destroyed the run the `Space` after it was meant to start). Two things outlive
it.

- **A check that passes in the failure mode is not a control, and two of them
  agreeing is not corroboration.** Group E of `web/tools/browser-e2e.mjs` was
  read as evidence for ninety seconds of a contradiction that did not exist:
  `heartbeats()` counts any `[HUD]` line and horde logs one in every state
  including `WaitingToStart`, so every check in that group passes on a game
  sitting on its start screen. The theory that survived was the one nothing in
  the harness could refute.

- **Coverage gap: the title-screen inset test is horde's alone.**
  `a_focusing_click_off_every_button_leaves_the_game_paused` exists in all four
  games; `a_focusing_click_off_every_button_leaves_the_title_screen_up` is in
  `apps/horde/src/app.rs` only, because horde is the one game whose start-screen
  first item is destructive. The other three would need it if a start screen
  grew one.

## Considered and declined

- **Adopting `crcbl_ui::hud`'s `Hud`/`HudPanel` in the four samples.** It was on
  the audit's list as "the engine feature was already bought", and it is not:
  the type does not do what any of the four HUDs needs.

  **`Label` has no colour.** Colour lives on `Style`, one per panel, so a
  panel's labels are all one colour. Every sample draws its stat line yellow,
  its state line pale blue and — breakout — its lives line green, which is three
  colours in one panel and is not expressible. That alone ends it.

  Two smaller mismatches behind it. `HudPanel` sizes itself from its content,
  where horde's backdrop width is a **measured** constant with a test putting a
  stated worst-case run through the real `FontAtlas` and requiring it to fit;
  auto-sizing throws that guard away. And `Hud::render` routes button clicks,
  which a read-only stat panel has no use for.

  **What is actually shared between the four is not the drawing.** Each has a
  private `HudStrings` that rebuilds its strings only when the numbers behind
  them change — the caching avoids the `format!` work each frame. **It does not
  stop the frame allocating, though four doc comments say it does** (corrected
  2026-08-15): `DrawList::text` takes `impl Into<String>` and stores
  `text.into()` into a `DrawCommand::Text { text: String }`, and every sample
  calls it with `hud.score.as_str()` — so a fresh `String` is allocated per text
  command per frame and dropped by `DrawList::clear`.
  `apps/breakout/src/app.rs`'s `draw_hud` names "the sandbox's 'a steady-state
  frame allocates nothing' property" as the reason the cache exists, and that
  property is not delivered by this mechanism. **This does not change the
  decline below** — the `Label`-has-no-colour argument ends adoption on its own
  — but it removes the strongest stated reason the samples' version is worth
  keeping, and it means the real fix would be on `DrawList` (a borrowed command,
  or an arena) rather than in any sample. But the structs differ in their fields
  and their cache keys, because each game shows different numbers: that is
  duplicated _shape_, not duplicated knowledge, and the logic under it is three
  lines. Extracting it would be an abstraction over a coincidence.

  **The finding this leaves is about the engine, not the samples**:
  `crcbl_ui::hud` has no consumer anywhere in the workspace. It is either owed a
  `color` on `Label` and an optional explicit panel size — at which point the
  samples could adopt it — or it should be deleted. Not decided here, because
  adding a field nothing uses is the speculative-machinery mistake and deleting
  a module is not a call to make inside an adoption task.

- **Building the demos' export names in `web/engine/demo.js` from the sample's
  slug.**
  `exports[\`**crcbl\_${sample}\_frame\`]`would delete the thirty-line`bind`block from each`web/demos/<name>/main.js`and is the obvious way to write it. Declined because it defeats the gate:`web/tools/check-exports.mjs`learns which exports the JS depends on by scanning for a literal`.**crcbl\_…`and fails when one is missing from the artifact. Verified both directions — with the names spelled out, renaming`\_\_crcbl_breakout_frame`to`…\_framee`in`main.js`fails the check with that symbol named; behind a template literal the scan sees nothing and a typo becomes a`TypeError`
  in somebody's browser. The per-sample file is the price of keeping the check
  able to fail.
- **Folding the demo pages' "what is actually running" prose into a partial
  too.** Its opening paragraph differs between breakout and flappy by two words
  ("high score" / "best score") and its second paragraph differs materially —
  flappy's explains the seeded course, breakout's names swept-sphere collision.
  Templating it would mean the layout carrying three prose variables, which is a
  generator, not a partial. The shared blocks are the ones that are identical
  and structural: the window, the loop's keys, and the console note.
- **Reformatting `web/tools/browser-e2e.mjs` with prettier.** It is not
  prettier-clean at the width the rest of `web/` uses — confirmed against the
  version at `HEAD`, so it predates this work — and this slice touched only a
  three-line comment in it. Reformatting a 1400-line gate file to fix a
  whitespace complaint would bury that comment in a diff nobody can review.
  Worth doing on its own, with the gate run either side of it.
- **Fixing the multi-sheet sprite bug in the shader, by adding
  `SV_StartInstanceLocation` back on.** It works, and it is one line:
  `sprites[instance + base]` with `uint base : SV_StartInstanceLocation`
  restores the `BaseInstance` that `SV_InstanceID` subtracts, giving the
  absolute index that the old `draw(0..6, batch.instances)` needed. Measured
  with slangc 2026.14: the SPIR-V comes out with the `OpIAdd` next to the
  `OpISub` and no extra capability beyond the `DrawParameters` the file already
  declares.

  Declined for two reasons. First, `slangc` **rejects that semantic for WGSL** —
  `error[E55202]: system value semantic 'sv_startinstancelocation' is not supported for the current target`
  — so the source would have to be `#if`-split per target, and there is no
  target macro to split on (probed: `__TARGET_SPIRV__`, `SLANG_SPIRV`,
  `__SPIRV__`, `__TARGET_WGSL__` are all undefined; only `__SLANG_COMPILER__`
  is), so `tools/compile-shaders.sh` would have to start passing its own `-D`
  per target. Second and worse, the WGSL half would then be correct **because
  Slang's two lowerings disagree**: `SV_InstanceID` becomes
  `InstanceIndex - BaseInstance` on SPIR-V and a bare `@builtin(instance_index)`
  on WGSL, and only the SPIR-V one matches HLSL. A Slang release that made WGSL
  consistent with the rest would silently break the browser, with nothing in
  this repository pointing at the cause. Always drawing from instance 0 depends
  on neither lowering.

- **A dynamic offset on the instance _storage_ buffer rather than a per-batch
  constant block.** The obvious shape — bind `sprites` with `dynamic: true` and
  offset it to the batch — needs the binding's declared **size** to be fixed at
  bind-group creation while `offset + size` must stay inside the buffer, so the
  size would have to be "the largest batch", which is a per-frame quantity the
  group is not rebuilt for. Batches would also have to be padded to
  `min_storage_buffer_offset_alignment` (256 on WebGPU) rather than packed at
  `INSTANCE_STRIDE`. The constants block is 80 bytes and fixed, so the same
  mechanism costs nothing there.

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer. **Vindicated**: both layers were built where the evidence
  said they belonged — `crcbl_audio::synth` and `crcbl::store::record::Record` —
  and the samples adopted them.
- **A `visible` check inside `DebugPanel::layout`.** It was written, and it
  could not be made to fail: `add` refuses to gather while hidden and
  `set_visible` drops what was gathered, so a hidden panel has no sections and
  the emptiness check already returns `None`. A guard that no test can reach is
  a guard that reports "passed" for reasons unrelated to what it guards, so it
  was deleted and the reasoning left in its place.
- **A `DebugSection::row` taking `String`s.** It takes `fmt::Arguments` instead,
  so a module writes `row("fps", format_args!("{fps:.1}"))` and formats straight
  into a `String` the section already owns. The ugly signature buys a
  steady-state section rebuild that allocates nothing, which matters for the one
  widget whose job is not to disturb the thing it is measuring.
- **Tinting one brick sprite four ways instead of authoring four frames.** It is
  the cheaper sheet and it is what `app.rs`'s colour table used to do. Four
  frames is what lets the rows differ in their _shading_ — a lit top edge and a
  shaded bottom in each row's own hue — which a single tinted rectangle cannot
  express, and it is what a sprite sheet is for. The cost is 96 × 8 texels
  instead of 24 × 8.
- **Re-randomising flappy's course from a clock.** A restart advances the seed
  deterministically (`course_seed(seed, runs)`) instead. A clock would make the
  course unreproducible, and the sample's exit criterion is that a recorded
  script replays to the same score.
- **Authoring flappy's background bands at one texel per sprite unit.** They are
  drawn at `art::BACKGROUND_SCALE` = 2 instead. At `TEXELS_PER_UNIT` = 20 a hill
  wide enough to read as a hill is a couple of hundred texels of hand-written
  rows for a silhouette with two bumps in it; the pipe is deliberately **not**
  scaled, because its caps are measured in texels and scaling would stretch
  them. If the bands ever gain detail that the doubling makes obvious, redraw
  them rather than adding a second scale knob.

## Full-codebase review 2026-08-04

Scope: working tree was clean (branch `crcbl-worktree` == `origin/main`, commit
050f570), so per the review/audit/perf conventions the **entire workspace** was
reviewed — `crates/*` and `apps/*`, ~216k lines of Rust. Correctness, security
and performance passes were split per crate across read-only review passes;
every finding below was re-verified against the code it cites (re-traced to the
return path, guard chain checked, string/length arithmetic applied) before being
published. **47 findings: 16 medium, 31 low, no critical or high — and all 47
were closed on 2026-08-04**, one commit each (pushed to `main`; `git log` is the
record, and each fix shipped with a test that failed on the old code or an
honestly stated gap). What survives below is the part of the review that is not
a closed finding.

### Cleared (the expensive half)

Per-crate review passes explicitly disproved these before publishing anything:

- **crcbl-net**: decoder panics on hostile bytes (every decoder length-gated
  through `ByteReader`); unbounded allocation from length fields (delta/system
  counts checked against remaining bytes before `with_capacity`); ReplayWindow
  edges; HMAC vs RFC 4231 vectors, constant-time compare; rate-limiter overflow
  (u128/saturating); reflected authenticated packets (disjoint direction tags
  fail the codec decode); repair-ack loop; `handle_ack` monotonicity.
- **crcbl-vk**: acquire-semaphore reuse (safe only because of the
  `slots = image_count + 1` throttle); surface refcount balance across every
  swapchain path; `Drop for DeviceInner` ordering; handle-tagging collisions;
  `write_buffer` bounds; submit-counter ordering; SPIR-V parser bounds.
- **crcbl-shell appkit**: pointer-capture revert on error; enqueue coalescing
  against the BackingChanged+Resized pair; retain/release balance; warp/flip
  math; CAMetalLayer Retina sizing; pool handle reuse.
- **crcbl-shell win32**: WM_CAPTURECHANGED guard; resize-coalescing order;
  WM_PAINT termination; 0×0 WM_SIZE handling; WM_DPICHANGED nesting; X_BUTTON
  decode; RAWINPUT sizes; TimeBase wrap; Drop ordering.
- **crcbl-shell x11**: GeGeneric sizing and `full_sequence` offset (verified
  against libxcb layout); xcb reply/event free-exactly-once at all ~20 sites;
  get_property chunk loop; Atoms pipelining; INCR state machines (terminator
  always emitted, ack-by-delete ordering); fp3232 fraction; blank_cursor
  lifetime; SelectionClear ordering; set_pointer_mode grab failure.
- **crcbl-shell wayland**: same-offer selection re-send (verified against
  wlroots source); fd close-exactly-once on every path; protocol decode overruns
  (libwayland signature validation); keymap size-vs-length check before mmap;
  drag drop/teardown double-destroy ordering; TimeBase rebase wrap; repeat-rate
  caps; axis gating.
- **crcbl-hal**: `Extent3d::full_mip_levels`; Format block/texel sizes for all
  29 formats; `needs_barrier` discriminant logic; readback poll contract; device
  outlives instance; create_device default loop; reversed-Z consistency;
  swapchain extent obligations.
- **crcbl-wgpu / null**: null ring rotation; poll_readback slice bounds; wgpu
  lock ordering; generational handle reuse; destroy_readback on Failed; present/
  reconfigure/destroy present the outstanding SurfaceTexture on every path;
  double-submit detection; semaphore promotion.
- **crcbl-render**: tonemap bind-group cache (destroyed-after-use is safe via
  the retire queue + generational handles); cross-frame barrier ordering;
  nine-slice geometry (traced against tests); camera math; texture row pitch;
  sprite-batching instance addressing; UI tier split; timer ring; graph state
  tracking.
- **crcbl-core / ecs / input**: arena aliasing (bumpalo-style argument),
  zero-size allocs, generation wrap (checked_add retires at u32::MAX),
  stale/foreign handles, System::detach swap-remove, input key up/down pairing,
  WASD normalization, WorldPos rebase math (Sterbenz), splitmix64 vectors,
  FrameClock accumulator. All non-test panic sites are unreachable from within
  the invariants.
- **crcbl-phys**: AVL rotation (traced all four shapes); BVH slot recycling;
  refit-only update_aabb; ray_vs_capsule piece tests; select_hit branches;
  entity churn; determinism; DampingForce cap; swept-TOI arithmetic.
- **crcbl-audio / store**: QOA bounds (verified against qoa.h byte-for-byte),
  allocation bomb rejected before reserve, WAV parser chunk arithmetic, mixer
  data races (single mutex, immutable samples, atomic ids), web resampler phase
  math, crash-ring wrap agreement, save/replay parser length gates, OPFS framing
  checksum + generation restore, URL/key allow-list containment.
- **crcbl-ui**: HUD snapshot vertex counts (136 hand-verified), double-applied
  scale (all callers pass 1.0), RectOutline geometry, menu centring math, fit
  loop, FrameStats windows, UTF-8 codepoint handling, widget_id collisions,
  click-capture correctness.
- **crcbl-sprite / wl-scanner / shaders / golden**: crpix header let-else,
  palette `#` handling, XML entity DoS (no DTD), quote-aware start-tag scan,
  emit identifier gating, SHA-256 vs FIPS 180-4 + NIST vectors, golden PNG size
  guard (the pattern load.rs should copy), JSON surrogate pairs.
- **crcbl-cli / engine**: semaphore value-0 semantics, cargo invocation (no
  shell, args via `Command::arg`), screenshot channel order, replay tick bounds,
  `new` template escaping, App::frame stage machine, readback arithmetic,
  GpuContext teardown order, FrameBudget cap.
- **apps**: breakout bounce data (real sweep, not fabricated), per-tick
  high_score.raise early-return, brick-neighbour geometry, asteroids wave/split/
  tumble index spaces, perimeter_point catch-all, save-file parsing,
  pause/focus/ dt handling in the engine loop; sandbox/sim/bare: sim tick-drift
  (ManualTime whole-tick drain), headless tick-count assertions, seed
  determinism, f32 hashing, frame-budget edges.

### Hardening (correct today, fragile — explicitly not defects)

- **net**: `baseline_tick = 0` is wire-ambiguous (delta.rs:824/866-869;
  unreachable — the server never encodes against tick 0); a forged `Accept`
  permanently wedges the client (unauthenticated handshake by design); `Reject`
  `msg_len` is u16 with a silent cast on encode (codec.rs:399); key rotation on
  reconnect trusts a cleartext token (documented); reject messages disclose
  server identifiers pre-auth.
- **vk**: acquire path waits on the armed fence with `u64::MAX` _while holding
  the device lock_ — a compositor that never returns an image hangs every device
  call; semaphore-reuse safety depends on the `slots = image_count + 1`
  throttle; `submit` never checks the CB's pool family matches the queue;
  `untag`'s `unreachable!` panics on a forged handle.
- **appkit**: the field drop order is the _opposite_ of what the comment claims
  (shell.rs:149-150 vs 1605-1607 — `shared` drops first; nothing dereferences it
  between the drops today, a future field or an AppKit call in `Drop` makes it a
  UAF); the first-responder-after-`setStyleMask:` hazard is logged, never
  re-issued (known open item); the borderless-origin defect is the tracked open
  item in this backlog.
- **win32**: `ScreenToClient` return ignored in the wheel arm (proc.rs:679);
  `GlobalLock` failure reads as `ClipboardContent::Empty` (documented);
  registered-format payloads lose a trailing NUL; 0×0 descriptor creates a
  frame-only window (doc overstates); `Limits` stale for one pump after
  `WM_DPICHANGED`.
- **x11**: `handle_selection_notify` phase routing times out pathological
  owners; a second keyboard's held key reads as repeat; `create_window` clamps
  width/height to u16::MAX; `warp_to` clamps out-of-i16 to (0,0); `modifiers()`
  allocates per key event without a keymap; consumer offers are not size-capped
  before `ChangeProperty` (trusted caller only).
- **wayland**: `PendingConfigure` never cleared (protocol-violation-only);
  `Conn::drain` treats any negative return as a permanent disconnect; e2e
  `attach_shm_buffer` stride×height truncates to i32 (test scaffolding); a 4 GB
  keymap file costs a 4 GB virtual mapping.
- **hal**: `ColorAttachment::resolve`'s required state is never documented; the
  reference frame destroys the command buffer right after present (wrong pattern
  to copy); `write_buffer`'s error doc says "not host-visible" but the
  requirement is `HostUpload` specifically; `query_results` "returns zeros
  without TIMESTAMP_QUERY" is unreachable (create_query_set errors first);
  `present`'s queue must be present-capable but the seam never says so;
  `AcquiredFrame` carries no swapchain identity.
- **wgpu**: unclosed pass / draws-outside-pass silently no-op where the null
  backend records validation errors; creation calls not routed through
  `checked()` (descriptor errors surface one `take_error` drain late); null
  `create_image_view` never validates format/subresource; `set_scissor` with
  `rect.x == i32::MIN` overflows `x - rect.x`; abandoned encoders leak one pool
  entry; `create_buffer` size within 3 bytes of u64::MAX panics on alignment;
  `copy_layout` bytes_per_row wraps on adversarial extents; `write_buffer`
  alignment differs between backends; offscreen surface formats differ
  (Rgba16Float offered by wgpu, refused by null); `SwapchainSlot::suboptimal` is
  dead state; two pending signals of the same timeline value pass the check;
  null `semaphore_value` always returns 0.
- **render**: cross-frame mixed-state transient handoff (single-mip production
  transients only); cross-frame queue-ownership release dropped (no second queue
  in use); `begin_frame`'s `atlas` argument is layout-only; pool transient view
  covers every mip; per-frame CPU allocations are small and documented;
  `upload_texture`'s expected-size math can overflow u64 (unreachable with real
  memory).
- **core/ecs/input**: wrong-kind bindings silently produce permanently idle
  actions (user-profile typo, no diagnostic); `set_enabled(true)` doesn't
  resolve immediately; `FrameClock::new(tick_hz > 1e9)` panics with a misleading
  message; `FrameArena` doc claims "neither Send nor Sync" (it is Send, only
  !Sync); `with_capacity(usize::MAX)` overflow is pre-empted by the vec capacity
  check; `Held` duration quantizes to f32 after ~19 days uptime.
- **phys**: `world_mut()` lets a caller desync `collider_to_entity`;
  `ThrustForce` fields are pub (unnormalized direction silently scales thrust);
  negative collider radii bypass the constructors; per-tick `Vec<Entity>` in
  `step` is negligible.
- **audio/store**: `opfs.rs` write-before-ready can be replaced by a later
  generation restore; `settings.rs get` falls through on a type error in a
  hand-edited file; `voice_mixes()`/`voice_count()` take the audio thread's
  mutex (HUD polling can stall audio); qoa.rs:349 saturates where the reference
  wraps (adversarial LMS weights only).
- **ui**: `FrameStats::with_window` aborts on a huge caller-supplied window;
  public float style fields are unclamped (0/negative → inverted geometry);
  `Text` top-left-anchor holds only for the built-in metrics; trailing-newline
  labels measure one line too tall; per-frame allocations are documented.
- **sprite/wl-scanner**: JSON recursion depth (~30-50k nested objects overflow
  the stack; sidecars trusted); `emit::KEYWORDS` omits
  `self`/`Self`/`super`/`union` (loud compile error, not silent mis-generation);
  `worst_pixels` collects all differing pixels then truncates (up to ~230 MB on
  an all-different 4K frame); `escape_ident`/`camel_case` collisions name the
  generated file, not the XML line.
- **cli/engine**: `channel_order`'s `_ => Rgba` arm would silently mislabel a
  future non-8-bit format (unreachable today); F11 toggle runs before the
  `destroyed` check; the pointer hit-test runs before `draw_menu`
  (one-frame-late menu clicks); failed `PendingGpuContext`/`GpuContext::finish`
  drops surfaces without `destroy_surface` (vk cleans up with a warning);
  `request_open`/ `start_device` accept a (0,0) extent (swapchain creation fails
  loudly); sandbox `--frames 0` accepted while bare rejects it; sandbox
  `--backend` usage text names only vk/null while more parse.
- **apps**: asteroids score is u32 (debug panic after ~43M small rocks); muzzle
  spawn wraps to the far side at the field edge; fire press during respawn is
  consumed (no edge buffering); breakout destroys a brick even when not
  approaching (unreachable with current geometry).

### Coverage

Scope: the whole workspace (clean tree at 050f570). Reviewed in full, per crate:
`crcbl-net` (+fuzz), `crcbl-server`, `crcbl-client`, `crcbl-shell` (appkit,
win32, x11, wayland, linux, web, shared), `crcbl-vk`, `crcbl-hal` (+null),
`crcbl-wgpu`, `crcbl-render` (+tests), `crcbl-core`, `crcbl-ecs`, `crcbl-input`,
`crcbl-phys` (+tests), `crcbl-audio` (+tests), `crcbl-store` (+web), `crcbl-ui`,
`crcbl-sprite` (+tests), `crcbl-wl-scanner` (+tests), `crcbl-shaders`,
`crcbl-golden`, `crcbl-scene` (empty), `crcbl-cli` (+tests), `crcbl` (engine;
non-test code), and apps asteroids, breakout, sandbox, sim, bare, horde.

GAPS — reported honestly:

- **apps/flappy: reviewed by a sub-agent whose report was never delivered** (the
  agent twice claimed delivery of a report that never arrived; only a summary
  fragment was received). The horde finding (45) was independently verified
  against the code; **flappy's zero-finding verdict is the agent's claim, not
  independently confirmed** — nothing in flappy was verified by me.
- **`crates/crcbl/src/engine.rs:3301-5105` (the test module)** and
  `crates/crcbl-ecs/src/{world,schedule}.rs` internals were not read by any
  review pass.
- **`crates/crcbl-net/fuzz/corpus/`** binary seeds — exercised via
  `include_bytes!`, not read as code.
- **wgpu internals** (the wgpu/wgpu-core dependency) were consulted for specific
  claims (resolve_target, tight packing, output_buffer_size) but not audited.
- No build/test run was performed during the review passes (read-only
  constraint); every finding above is static-verified. The project's CI gate
  (`cargo fmt/clippy/build/nextest`) was not run as part of this review.

## What MTL1 left open on the Metal backend

- **It advertises Tier B, and a tier-aware caller will believe it.**
  `DeviceCaps::tier` is derived, and `DRAW_INDIRECT_COUNT` /
  `MULTI_DRAW_INDIRECT` stay off until the command slice picks Metal's indirect
  path (indirect command buffers, per `docs/plan/09-backends-metal-dx12.md`'s
  mapping table). Correct today and documented in the crate docs, but it is
  visible behaviour: once anything selects on tier, macOS takes the Tier B
  branch.
- **The engine has never stated a minimum macOS version.** `adapter.rs` sends
  `supportsBCTextureCompression` among others, which dates the floor to macOS
  11; `objc2` does not gate on availability, so an older system raises an
  unrecognised-selector exception rather than answering wrongly. Loud, but
  undecided — the same question the AppKit shell backend has been carrying
  unstated since P5C.
- **`DeviceType::Virtual` is unreachable on Metal.** There is no virtualisation
  query, so a paravirtual GPU answers every question exactly as the built-in one
  and enumerates as `Integrated`. Stated as a gap, not fixed.

## Considered and declined: an OpenGL / GLES backend

**Decided 2026-08-05.** GL is a dying support surface and the engine will not
grow a `crcbl-gl`. The platform matrix is Vulkan for Windows, Linux and Android;
Metal for macOS and iOS; DX12 for Windows as the second Windows path. Nothing
else — see the Apple decision below, taken the same day, which closed the
MoltenVK option this entry originally listed alongside them.

Reasons, so this is not re-argued:

- **GL is already reachable and nobody needs a crate for it.** `crcbl-wgpu`
  enumerates `wgpu::Backends::all()` and wgpu's default feature set includes
  `gles`, so a GL device is enumerable through the existing backend today. It is
  present and unproven rather than supported — nothing in CI exercises it — but
  the cheap experiment would be pointing the existing wgpu e2e suite at a GL
  device, not writing a backend.
- **The blocker is above the seam, not at it.** `RendererTier` declares exactly
  two tiers, and Tier B is not a low bar: per-batch indirect draws, indexed SSBO
  lookups, and culling still running in compute. GLES 3.0 has no compute, no
  SSBOs and no indirect draw — those arrive in 3.1 — so the old hardware GL
  would be added _for_ cannot reach even Tier B. A Tier C is a renderer change
  with a third draw-emission path and a third set of golden images, which is far
  more expensive than the backend crate it would sit under.
- **GL fights this seam specifically.** No command buffers (the seam hands out a
  `CommandEncoder` and submits; GL executes immediately), thread-affine contexts
  against a seam that requires `Device: Send + Sync` on native, no explicit sync
  to map `pipeline_barrier` onto, and reversed-Z — locked engine-wide — needing
  `glClipControl`, which is core in GL 4.5 but only an extension on GLES.
- **It is the wrong tool for mobile anyway.** iOS is Metal-only and has
  deprecated GL ES since iOS 12; modern Android ships Vulkan. The Android gap is
  a `crcbl-shell` surface backend, not a HAL backend — `crcbl-vk` already exists
  and is the best-tested path in the workspace.

## Considered and declined: Vulkan on macOS and iOS

**Decided 2026-08-05. Apple platforms are Metal only.** `crcbl-vk` is not
expected to run there, MoltenVK is not a shipping path, and the MoltenVK spike
`docs/plan/09-backends-metal-dx12.md` scheduled as P14's first task **will not
be run** — the gate it was meant to inform is closed by this decision instead.

What that buys, and what it costs:

- **One macOS path instead of two.** The alternative was shipping on MoltenVK
  while native Metal caught up, which means two GPU paths to test on the
  platform with the least CI capacity, and bug reports that begin with "which
  one were you on".
- **iOS was never in question.** There is no Vulkan loader or ICD story on iOS
  at all; MoltenVK is linked directly into the app. Metal is the only path
  there, so choosing it for macOS as well makes the whole Apple side one
  backend.
- **The cost is that `crcbl-mtl` is now load-bearing rather than an
  optimisation.** Until it can present a frame, macOS has no native GPU path —
  `crcbl-wgpu` is the only thing that runs, at Tier B. That raises the stakes on
  MTL3 (first pixel) and MTL5 (swapchain) and is the reason they are the two
  slices worth watching.

The technical question the spike would have answered is recorded here because it
is the same question `crcbl-mtl` itself has to answer, and the answer is now
expected from the Metal side rather than the Vulkan one: `crcbl-vk` demands
`Features::TIER_A` outright rather than degrading, that set includes
`DRAW_INDIRECT_COUNT`, and `crates/crcbl-vk/src/adapter.rs` reads it straight
off `VkPhysicalDeviceVulkan12Features`. **Metal has no native indirect-count
draw**, which is exactly why `crcbl-mtl` reports Tier B today and why MTL6's
indirect-command-buffer work is what moves it. MoltenVK would have hit the same
wall from the other side.

One framing note kept because it explains why "the user installs MoltenVK" was
never the shape this would have taken: MoltenVK ships **bundled with the
application**. The Vulkan SDK's macOS installer places an ICD for development,
but a shipped app embeds `libMoltenVK.dylib`. It describes a developer's
machine, not a player's.

## Confirmed: DX12 stays, alongside Vulkan on Windows, and last

**Decided 2026-08-05**, closing a question that had been half-answered twice —
`docs/plan/09-backends-metal-dx12.md`'s original text justified DX12 as old-iGPU
coverage, its 2026-07-27 correction retracted that and substituted the Xbox door
plus Windows tooling, and neither pass weighed it against simply using
`crcbl-vk` on Windows.

**Windows keeps both backends. DX12 is never a replacement for Vulkan there.**

### The asymmetry that settles the "instead of" framing

`crcbl-vk` has to exist regardless — it is the Linux path and, per the same
day's platform decision, the Android one. Windows support falls out of it at
approximately zero marginal cost, because it is the same code reaching a
different loader. So dropping Vulkan _from Windows_ saves nothing: the crate,
its tests and its maintenance all stay. Replacing it with DX12 would pay for a
new backend to obtain a working path that already exists.

It would also cost the one thing Windows is uniquely placed to give:
**cross-backend differential debugging on identical hardware.** "Does it repro
on the other backend?" is reason #1 in `crcbl-hal`'s own argument for dynamic
dispatch and for compiling two backends into one binary, and Windows is the only
platform where both can run against the same GPU.

### Why it is still worth building

- **Xbox.** The only item here obtainable no other way.
- **A GPU device on the Windows CI runner.** Every software-rasteriser job in
  `ci.yml` is `ubuntu-latest`/lavapipe; `windows-latest` has no device at all,
  which is why Windows has no golden images and no sample-level render pass.
  WARP is D3D12's software rasteriser and ships in Windows, so this would be
  Windows' lavapipe. **See the open question below — this benefit is
  unconfirmed.**
- **Robustness against a missing or stale vendor ICD.** D3D12 is part of the OS;
  Vulkan is not.
- **Windows-on-ARM**, where D3D12 is first-class and Vulkan is patchier.
- **PIX and DRED**, and DXGI's waitable swapchain object — a mature answer to
  the closed-loop frame pacing this backlog already has open, where the Vulkan
  side needed `VK_KHR_present_wait` (bound in the pinned `ash`) and
  `VK_EXT_present_timing` (ratified, but not in `ash` at all, so genuine
  hand-written FFI) — both of which have since landed.

### Why it is last

- It maps near-1:1 onto the Vulkan-shaped seam, so **it finds no HAL leaks**.
  That is a cost saving and a value reduction at once: Metal is the backend that
  stresses the abstraction, which is why the plan orders it first.
- Its value is infrastructure and optionality, not capability. Nothing renders
  today that it would render better.
- It is a crate comparable in size to `crcbl-vk`, the largest in the workspace,
  plus a third shader artifact (DXIL) in `crcbl-shaders` and its manifest, plus
  another pinned toolchain in the `shaders` job, plus a second Windows path to
  test permanently.

Ranked below finishing Metal — which after the same day's decision is the _only_
Apple path — and below an Android surface in `crcbl-shell`, which is the largest
coverage win available and needs no new HAL backend at all.

### Open question, worth an afternoon before the phase starts

**Does WARP clear Tier A?** Specifically SM6.6 dynamic resources, which this
backend is specced around. If it does, the Windows CI argument is real and DX12
buys golden-image coverage on a second OS. If WARP is Tier B only, `crcbl-wgpu`
already covers that on Windows and the CI half of the justification collapses,
leaving Xbox and tooling. Cheap to check, and it changes how much of the above
is true — so check it before committing the phase, not during it.

## Considered and deferred: console backends

**Decided 2026-08-05. No console support now; open to it if someone asks for
it.** Nothing is being built speculatively, and nothing in the engine forecloses
it. The canonical platform matrix is in `docs/plan/01-foundations.md`.

### What each console would actually need

- **Xbox — comes free with DX12.** It is D3D12X through the GDK rather than
  desktop D3D12, so it is not literally the same backend, but `crcbl-dx12` is
  the prerequisite and the delta is small. This is already the strongest item in
  DX12's justification (see the DX12 entry above).
- **PlayStation — a private crate.** There is no Vulkan on PlayStation, ever.
  PS5 is AGC (with a GNM compatibility layer), PS4 is GNM/GNMX, and shaders are
  PSSL. **The blocker is legal rather than technical**: the SDK, its headers and
  the API's detailed shape are under NDA, and downloading any of it requires
  licensed-developer status with an approved concept. So it cannot live in this
  repository and cannot be written speculatively by anyone.
- **Switch — probably `crcbl-vk` with a shell backend.** It has a working Vulkan
  driver. NVN is the faster native path and what shipping titles use, but Vulkan
  is a genuine bring-up route, which makes Switch by far the cheapest console to
  reach and the only one needing no new HAL backend.

### Why this costs nothing to defer

**The seam is what makes a console backend possible at all.** A closed crate
implementing the public `crcbl-hal` traits drops into a private workspace as a
path dependency, with zero changes above the seam — the renderer, ECS, UI and
every game compile unchanged. That property is already load-bearing for the four
public backends; consoles just exercise it under an NDA.

AGC is also close to the shape already built: explicit command buffers, explicit
sync, bindless descriptors, GPU virtual addresses. The Vulkan-flavoured seam is
roughly right for it, for the same reason DX12 maps near-1:1.

The genuinely new axis is **shaders**. PSSL is HLSL-like and the platform
toolchain consumes HLSL-ish input, so the path is Slang → HLSL → PSSL — a fourth
artifact after SPIR-V, WGSL, MSL and DXIL, and the only one whose compiler could
never run in public CI.

### `BackendKind` would need a variant — and that is not a problem

`crcbl_hal::BackendKind` is a closed enum —
`Vulkan | Wgpu | Metal | Dx12 | Null` — so a console backend needs a new variant
(naming a console is not an NDA breach) or a `Custom(&'static str)`, because a
private crate cannot add one to a public enum it does not control.

**Add it when a console backend actually exists.** This was first written up as
something to settle before the seam freezes, on the grounds that a new variant
is a breaking change to a public API. That reasoning does not apply here: the
workspace is `0.1.0` with no tags, everything so far is unreleased, and the
project's own convention is that below 1.0 a breaking change bumps the minor.
Breaking changes are routine and expected, so there is nothing to buy by
deciding early — and adding a variant nothing implements would be the
speculative machinery this codebase deletes rather than keeps.

## What MTL2 left open on the Metal backend

- **`MTLTextureUsagePixelFormatView` is set on every colour image,
  unconditionally.** It can disable lossless bandwidth compression on some Apple
  GPUs. Narrowing it needs the seam to carry intended view formats the way
  WebGPU's `viewFormats` does, which is a HAL change and so was not made.
  Recorded in `conv::texture_usage`'s docs.
- **Metal validates descriptors by raising, not by returning nil**, and an
  Objective-C exception crossing into Rust aborts the process. `create_image`
  guards the rules confirmable from the headers and deliberately invents no rule
  that could not be confirmed — **so a caller passing `HostUpload` for a depth
  image could abort rather than receive an `Err`.** How far descriptor
  pre-validation should go on this backend is undecided, and it is a question
  the other backends do not have.
- **`conv`'s `ALL` format list is hand-maintained.** The staleness guard asserts
  the last sorted entry is `Bc7RgbaUnormSrgb`, which catches an appended format
  and not an inserted one. Small, and the compiler catches the half that
  matters.

## `render_area` does not exist in Metal, and clears diverge because of it

**The closest thing to a seam leak the Metal backend has hit.** Found in MTL3,
not fixed, and `crcbl-hal` was deliberately not changed.
`MTLRenderPassDescriptor` has no render-area rectangle, so `crcbl-mtl` turns
`render_area` into the render encoder's **scissor**, set only when it is a
genuine sub-rect. The consequence is a real behaviour difference: **a
`LoadOp::Clear` clears the whole attachment on Metal, where Vulkan clears only
the render area.** Nothing above the seam depends on it today (the render graph
always passes the full attachment), so this is latent rather than broken.
Options, none taken:

1. Document `render_area` as affecting rasterisation only, and require a caller
   wanting a partial clear to draw one. Cheapest; makes the seam honest about
   the weaker guarantee.
2. Have the Metal backend emulate a partial clear with a draw when `render_area`
   is a sub-rect and the load op is `Clear`. Costs a pipeline in the backend.
3. Drop `render_area` from the seam entirely and give the encoder a scissor
   call. Largest change, and closest to what Metal, DX12 and WebGPU all do.

Wants a decision before anything starts relying on the Vulkan behaviour. Both
backends must then be re-verified.

## What MTL3 left open

- **The GPU-side wait is not proven to gate.** The test proves a wait does not
  wedge the queue and that an unsatisfiable wait is refused up front. Proving
  the wait actually orders two submissions needs an observation _between_ them,
  which is a race rather than an assertion, so it is not attempted. Stated as a
  gap.
- **Query sets stay refused**, deliberately and with the argument in
  `create_query_set`'s docs: this backend builds no `MTLCounterSampleBuffer` on
  any device, the CI Mac advertises no `counterSets`, and reporting the feature
  would oblige a `timestamp_period_ns` Metal has no fixed answer for.
  Half-building it would give real timings on some Macs and zeroes on others.
  The seam's own half is no longer an obstacle — `PassTimestampWrites` asks for
  a sample only at a pass boundary, which is where Metal samples.
- **`device.rs` is 4057 lines and should be split** — the pools, the resource
  create/destroy pairs, submission and readback are separable responsibilities.
  Wants to be a move-only change so a reviewer can see it is only a move.
- **`DepthStencilAttachment::read_only` is read and deliberately not acted on.**
  Metal has no image layouts, so there is nothing to set.

## What MTL4 left open

Most of MTL4's blocking list is closed by later slices — bind groups exist as
flat argument tables, compute pipelines build, index buffers bind, and the
engine's own `triangle.slang` draws through a bind group (it is one of the
quarantined draw tests above). What remains:

- **The pipeline object is only half of `GraphicsPipelineDesc`.** An
  `MTLRenderPipelineState` carries the shaders, the colour attachment formats
  and blending — and **not** cull mode, winding, fill mode, depth clip, depth
  bias, the depth/stencil state, or the primitive topology. Those are encoder or
  draw-call state in Metal; `crcbl-mtl` stores them in a `RasterState` beside
  the pipeline and replays them at bind. A future slice that binds pipelines
  through a different path has to replay them too, or half the descriptor
  silently stops applying.

## What MTL5 left open on the swapchain

- **`nextDrawable` and `presentDrawable:` are proven by nothing automated.**
  Everything else in the layer path runs on CI, because a detached
  `CAMetalLayer` needs no window server, `NSView` or run loop; acquiring an
  actual drawable does. That one test is behind `mtl-e2e` and `#[ignore]`, so a
  person on a Mac is the only thing that has ever run it. Whether a detached
  layer vends a drawable on the current hosted image is an open question, cheap
  to settle in a throwaway workflow the way the shader question was.
- **`surface_caps` can never return the `Unsupported` branch its own contract
  requires** — on Metal every device can drive any layer, so the branch is
  unreachable and **the contract is untested here by construction**, not merely
  untested. `crcbl-vk` is where that path is exercised.
- **`CompositeAlpha` offers only `Opaque`.** `CAMetalLayer` has `opaque` and can
  composite with alpha, but nothing verified the non-opaque behaviour, so it is
  not offered rather than offered untested.

## The Win32 shell tests share the desktop, and it keeps costing red builds

`hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for` failed the
`build + test (windows-latest)` leg on 2026-08-05; re-running the same job
passed clean on a macOS-only commit, so it is environmental — the shared
runner's foreground being contended — rather than a defect. The assertion is
doing its job (it fails at the point focus was lost, naming why nothing after it
can be trusted, instead of asserting against an unfocused window), but it is
still a red build on an unrelated change, which trains readers to re-run rather
than read. Options, none taken: retry the focus acquisition with a longer budget
than 8 attempts; move the focus-dependent assertions into the feature-gated e2e
suite where `desktop::take_foreground` already pulls the foreground levers; or
mark the test as allowed-to-retry if nextest's retry support is acceptable here.
**Third instance, 2026-08-09, and it is a _different test_:**
`win32::shell::tests::confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back`
failed on `assert!(shell.window_state(window).focused)` — the assertion right
after `make_foreground` + `send_focus`, before the confine is even attempted. So
this is not one brittle test but the whole class the backlog already names:
`ClipCursor` and `SetCursorPos` are foreground-only, several tests arrange the
foreground to use them, and a shared runner does not always grant it.

It failed on a **revert commit**, whose code was green two commits earlier —
which is about as clean a demonstration as this gets that it is environmental
rather than a defect. A re-run of the same job was taken to unblock `main`, and
that is the third time a re-run has stood in for a decision.

**The decision is overdue, and the options have not changed:** retry the
foreground acquisition with a longer budget; move the focus-dependent assertions
into the feature-gated e2e suite where `desktop::take_foreground` already pulls
the levers; or allow this specific test a retry. Doing nothing means every
unrelated commit carries a chance of a red Windows leg, which trains readers to
re-run rather than read — the exact habit that makes a real failure invisible.

**Fourth instance, 2026-08-09, and it is a different _resource_:**
`win32::shell::tests::an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not`
failed on `assert!(!clipboard_is_open())`. So the class is wider than focus —
these tests use **shared desktop resources** (the foreground, the clipboard) on
a runner that contends for them.

**The clipboard half is fixed.** That assertion sat at the end of a test and
meant "our code closed the clipboard", while `clipboard_is_open()` was
`!GetOpenClipboardWindow().is_null()` — whether **any process** holds it. Its
scope was wider than its intent, so a foreign process failed it while our code
was correct. `clipboard_held_by(hwnd)` replaces it, every caller now asks the
narrower question, and that is both non-flaky and a **stronger** assertion. The
"nothing is open before we start" precondition was deleted rather than narrowed:
that test is _about_ contention — it asserts the open was not refused — so a
foreign holder is the case its retry budget exists for, not a reason to fail
before starting.

**The focus half is not fixed** and remains the open decision below.

Two failures in one session, both on commits that had nothing to do with
windowing (a `Revert` and a mesh-shader reland), both cleared by re-running the
same job unchanged.

**Wants a decision rather than another re-run.**

## What MTL6 settled, and what it leaves for a decision

Metal's last planned slice. **The backend still reports Tier B**, and the reason
moved rather than went away:

- **Needs the user: `dispatch` is blocked on the seam, not on Metal.**
  `MTLComputeCommandEncoder` is otherwise ready. Metal takes
  `threadsPerThreadgroup` at `dispatchThreadgroups:threadsPerThreadgroup:`, but
  SPIR-V, DXIL and WGSL bake the workgroup size **into the shader**, so MSL
  declares it nowhere and `ComputePipelineDesc` carries no field for it. There
  is no number the backend could pass that is not a guess about the kernel, and
  a wrong one runs the shader with the wrong thread count rather than failing.
  **Resolved**: `ComputePipelineDesc` carries `workgroup_size`, sourced from the
  `WORKGROUP_SIZE` constant `crcbl-shaders` publishes beside each compute
  shader, so no caller restates a number the `.slang` already declares.
- **Needs the user: `block2` is now a direct dependency of `crcbl-mtl`.**
  `objc2-metal` types the `addPresentedHandler:` parameter as a
  `block2::DynBlock` and re-exports nothing, so the callback the seam needs
  cannot be written without naming the type. The case is argued next to the edge
  in `crates/crcbl-mtl/Cargo.toml`. It adds no package to the graph — `block2`
  was already in `Cargo.lock` as an `objc2` sibling — but a direct dependency is
  a decision, so it is flagged here for ratification rather than kept silently.
- **`DESCRIPTOR_INDEXING` was withdrawn, deliberately.** Bind groups exist as
  flat argument tables now, so there is no runtime-sized array;
  `create_bind_group_layout` refuses every `BindingFlags`, and a backend
  refusing them must not report the feature. Nothing above the seam is blocked;
  it returns with argument buffers, which need Slang to emit
  argument-buffer-shaped MSL — if the flag is wanted back, the honest route is
  scheduling that shader work, not flipping the bit.
- **`DRAW_INDIRECT_COUNT` is unreachable in this backend's shape.** The count
  lives in GPU memory, and Metal's only execution that reads one is
  `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over an
  `MTLIndirectCommandBuffer` whose commands must **already exist** — from the
  CPU, which does not know GPU-side draw arguments, or from a compute kernel,
  which would have to run before the render encoder was opened. The emulation
  (issue `max_draw_count` draws) is silently wrong. Closing this needs either
  deferred command recording or a seam that hands the backend its indirect work
  before the pass opens.

  **Superseded 2026-08-09, and independently corroborated.** "Metal stays Tier
  B" no longer means anything — there are no tiers, per
  `docs/plan/39-capabilities.md`. Metal reports the flag clear and the renderer
  selects another `GeometryPath`; with mesh shaders as the primary path this
  affects only the fallback. And the finding is not a `crcbl-mtl` limitation:
  `wgpu-types` documents `MULTI_DRAW_INDIRECT_COUNT` as DX12 and Vulkan only,
  and `wgpu-hal`'s Metal backend contains no multi-draw code at all. Two
  implementations, one answer. The seam-reshape option stays on the table and is
  logged under _User decisions_ at the top of this file.

- **A partially filled bind group leaves its unfilled argument-table slots
  holding whatever the previous bind put there.** Not checked, because
  `update_bind_group` makes create-then-fill a legal pattern. Vulkan leaves the
  same hazard to its validation layer.

## WARP clears the bindless bar — measured, 2026-08-05

The question this file told the DX12 phase to settle is settled: the
`windows-latest` runner reports
`ResourceBindingTier=3  HighestShaderModel=6.8 sm66-dynamic-resources=yes` for
both the DXGI lists and `EnumWarpAdapter`, and `crcbl_dx12::device`'s
`a_pulled_triangle_is_drawn_and_read_back_texel_by_texel` has since passed there
— so WARP supports SM6.6 dynamic resources **and executes a shader**, which
closes the coverage hole that `windows-latest` has never had golden images or a
render pass: Windows can have a software rasteriser, the way Linux has lavapipe.
What that does not cover is hardware: WARP is one implementation with one set of
tolerances, and no D3D12 code in this workspace has run on a GPU.
`renderer-tier=B` in the run's lines is the backend's own gap — `COMPUTE`,
`TIMELINE_SEMAPHORE` and the two indirect features wait on calls no slice has
written.

Deferred inside DX4, each with what it would take:

- **Compute pipelines.** `create_compute_pipeline` still refuses. The compute
  half is a `D3D12_COMPUTE_PIPELINE_STATE_DESC` over the same root signature
  plus `SetComputeRootSignature`/`SetComputeRootDescriptorTable`, which are the
  compute twins of calls `bind_group` already makes.
- **Indexed and indirect draws.** `bind_index_buffer`, `draw_indexed` and the
  four indirect entry points refuse. Indexed needs `IASetIndexBuffer` and a
  `D3D12_INDEX_BUFFER_VIEW`; indirect needs an `ID3D12CommandSignature`, which
  is a per-argument-layout object with no counterpart in the seam.
- **Dynamic offsets.** `create_bind_group_layout` refuses a `dynamic` binding by
  name. A descriptor table has no offset to apply — D3D12's answer is a _root_
  CBV/SRV/UAV carrying a raw GPU address, which changes the root parameter type
  for every binding in the set rather than adding to it.
- **Push constants / root constants.** D3D12 _has_ the feature; what is missing
  is knowing which root parameter slot the committed DXIL expects one at — the
  same gap `crcbl-mtl` names for `setVertexBytes:`.
- **Register-space mapping is verified for set 0 only.** Every committed shader
  declares `[[vk::binding(N, 0)]]`. **Settled by measurement, and the
  expectation was wrong**: `[[vk::binding]]` is Vulkan-only, and `dxc` numbers
  each register class from zero in declaration order across the whole source,
  all in space 0 — `sprite`'s set-1 texture is `t1` in space 0, not `t0` in
  space 1. Read out of the `PSV0` resource table of every committed container. A
  multi-set layout is still only checked by that artifact test, never by a
  driver.
- **The shader-visible descriptor heaps do not grow.** One heap per type at a
  fixed capacity, and `HalError::OutOfDeviceMemory` past it, because a bind
  group's GPU handle is an address inside the heap it came from. A real
  suballocator is a slice of its own.

Two things DX1 decided that a later slice may have to undo:

- **`DESCRIPTOR_INDEXING` is reported ahead of a call** — the opposite of what
  `crcbl-mtl` ended up doing, deliberate because `adapters()` is where the WARP
  question is asked, so the flag has to be derivable before any device exists.
  The binding slice must withdraw it if D3D12 bind groups cannot deliver a
  runtime-sized array, exactly as Metal's did.
- **`driver` comes from `CheckInterfaceSupport(IID_IDXGIDevice)`**, documented
  as a Direct3D 10 interface check, with a fallback string when it refuses. WARP
  is the adapter most likely to refuse it; if the CI line shows the fallback on
  real hardware too, the field needs a different source.

**Do not write a LUID into code or an assertion.** DXGI's `AdapterLuid` is
per-boot — two CI runs reported different LUIDs for the same two adapters. It is
an identity _within_ one enumeration and nothing more: fit for de-duplicating a
list, unfit for a fixture, a golden value or a comparison across runs.

## `Format::ALL` cannot be made airtight on stable

The seam owns the canonical format list as `Format::ALL` in `crcbl-hal::format`,
and all three backends — `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` — now drive
their injectivity tests off it rather than a copy each kept beside the mapping.
What is left is one thing.

**The list is hand-maintained, and the gap that remains is a variant appended to
the enum and not to `ALL`.** `the_format_table_is_in_declaration_order` compares
each entry's discriminant against its index, so an entry inserted or duplicated
anywhere but the very end is caught. An appended one is not, and **nothing on
stable Rust can count an enum's variants**. Closing it properly needs either a
declarative macro generating the enum and the list from one source, or a
proc-macro dependency — and a new dependency is the user's call, so this is
recorded rather than decided. Both `ALL`'s doc comment and the test say this
outright rather than implying full coverage.

The same shape applies to `Command`'s list in `crcbl-hal/src/null/record.rs`,
except that there an exhaustive `match` in the test forces an author who adds a
variant to visit the file — which is the mitigation `Format` cannot have,
because its list lives in a different crate from its consumers.

## `crcbl-vk` does not enforce cross-kind pass scoping, where `null` does

`begin_compute_pass` checks only whether a compute pass is already open, and
`begin_render_pass` only whether a render pass is — so a compute pass opened
_inside_ a render pass, or the reverse, is accepted. `dispatch` checks no scope
at all. The null recorder rejects every one of those as `NestedPass` or
`OutsidePass`.

This may be deliberate: the seam says a backend **may assume** the scoping rules
hold, which makes the null backend the strict reference and `crcbl-vk` the
permissive one. So no behaviour was changed and no test asserts the absence of a
check — that would pin a gap in place. Recorded because it is the second place
the mock is stricter than the backend it models, after the cross-instance
surface bug, and that pattern is worth watching rather than rediscovering.

The illegal _commands_ are still caught by the validation layer at record time.
The illegal _pass bookkeeping_ is caught nowhere.

## The Tier B arms of the indirect-draw tests have never run

`draw_indirect_count` and `draw_indexed_indirect_count` have a Tier A path and a
fallback, and the tests cover both arms and assert which one they took. But
lavapipe on Mesa 26.1.6 reports **tier A**, so on this machine and in CI the
fallback is compiled and unrun. Reaching it needs a genuinely Tier B driver, and
we do not currently have one anywhere — CI's "software rasteriser" leg is no
longer the Tier B leg it was assumed to be.

One arm was salvaged: the `update_bind_group` refusal is a layout rule rather
than a tier rule, so that test runs its refusal path on Tier A devices too.

## Modules of `--test vk_e2e` need `#[path]`, which is not obvious

`crates/crcbl-vk/tests/vk_e2e.rs` declares its modules with
`#[path = "vk_e2e/<name>.rs"]`. Without it a crate root resolves `mod foo;`
beside itself — `tests/foo.rs` — and Cargo would then compile that file as its
own separate test binary. Verified: plain `mod harness;` fails with E0583 naming
`tests/harness.rs`. The alternative Cargo supports is `tests/vk_e2e/main.rs` as
the target root, which needs no `#[path]` at all; it was not taken because the
slice's brief pinned `tests/vk_e2e.rs` as the root file.

## Vulkan's cross-submission barriers are unverified on this machine

`run-vk-e2e.sh` reports its own reach, and on a local run it prints
`sync-validation reach: record-time=yes one-submission=yes cross-submission=no`.
So a green local run against radv says nothing about a missing barrier _between_
submissions, which is the class every missing cross-frame barrier falls into.
CI's layer configuration has caught one that a local run cannot see.

Neither environment subsumes the other: the local run has a real driver, a
discrete GPU and a real async-compute queue that CI has never had; CI has the
cross-submission checking. Treat "green locally" as insufficient for anything
touching barriers, and rely on `cargo nextest run -p crcbl-render` for that
class — it compiles consecutive frames against one pool and needs no layer.

### Two things DX1 decided that a later slice may have to undo

- **`DESCRIPTOR_INDEXING` is reported ahead of a call**, which is the opposite
  of what `crcbl-mtl` ended up doing. It is deliberate and the reversal
  condition is written into `adapter.rs`: the binding slice must withdraw it if
  D3D12 bind groups cannot deliver a runtime-sized array, exactly as Metal's
  did. The reason for the asymmetry is that `adapters()` is _where the WARP
  question is asked_, so the flag has to be derivable before any device exists.
- **`driver` comes from `CheckInterfaceSupport(IID_IDXGIDevice)`**, which is
  documented as a Direct3D 10 interface check, with a fallback string when it
  refuses. WARP is the adapter most likely to refuse it. If the CI line shows
  the fallback on real hardware too, the field needs a different source.

## What DX2 left open

The device-and-resources slice. Everything below is in `crates/crcbl-dx12`.

- **An image view's format must equal its image's on DX12, and the seam says it
  need not.** `create_image_view` refuses a differing `ImageViewDesc::format`
  outright, so the sRGB reinterpretation `crcbl-hal` documents — and `crcbl-mtl`
  delivers — is unavailable on this backend. D3D12 permits the cast only from a
  typeless resource, which costs compression on every render target, or where
  `CastingFullyTypedFormatSupported` is reported, which would make the seam's
  promise depend on the machine. Both were declined for those reasons and the
  argument is in the code. **This wants a decision before the bind-group
  slice**: either the seam narrows its promise, or DX12 pays one of the two
  costs. A sampled _depth_ image is not affected — it is stored typeless with
  the depth-stencil view and the shader view each naming a concrete format.

- **`create_image` does not check `mip_levels` against the extent.** More mips
  than the extent admits reaches D3D12 as `E_INVALIDARG` and surfaces as
  `HalError::Backend` rather than the `InvalidDescriptor` it is. Every other
  descriptor rule in the slice is checked up front precisely because the
  `Create*View` calls return `void`; this one was left to the runtime.

- **`max_sample_count` and `max_storage_buffer_range` are still at the seam's
  floor.** Both need per-format `CheckFeatureSupport` queries, which the format
  table DX2 added now makes possible — the reason `adapter.rs` gives for
  deferring them no longer holds.

- **`device.rs`'s implementation half is still over the size a module should
  reach.** The seam DX2 named — the descriptor validation, `check_image`,
  `check_view_type` and `build_views` — was split into `validate.rs` on
  2026-08-07 as a move, and the file is still the implementation half of the
  device. The next seam is not named; `crcbl-mtl`'s device.rs carries the same
  warning with its own candidates (the pools, the create/destroy pairs,
  submission, readback).

- **The slice runs on WARP and nowhere else.** Its whole suite passed on
  `windows-latest` on the first attempt, which is a behavioural result and not a
  compile: `D3D12CreateDevice`, the DIRECT queue, `CreateCommittedResource`, the
  four `Create*View` calls, the descriptor-heap strides, `Map`/`Unmap` and the
  fence wait all did what the code assumed. **What that does not cover is
  hardware.** WARP is one implementation with one set of tolerances, and the
  `Create*View` calls return `void` — a descriptor a real driver rejects and
  WARP accepts fails as a dead view, silently, on the machine nobody tested. No
  D3D12 code in this workspace has run on a GPU.

- **The concurrent-`wait_idle` ordering is not provable by test.**
  `concurrent_waits_each_signal_once_and_all_return` pins that every call
  signals exactly once, which is deterministic. It does not pin that the signals
  reach the queue in increasing order — that is what the lock on
  `DeviceInner::idle_value` exists for, and its failure mode is a hang caught by
  `slow-timeout`, not a red assertion. A passing run is not evidence the race
  cannot happen.

## What DX3 left open

The command-list, render-pass and clear slice. All in `crates/crcbl-dx12`.

- **`StoreOp::Discard` is recorded as a store.** `OMSetRenderTargets` has no
  store op; the API that has one is
  `ID3D12GraphicsCommandList4::BeginRenderPass`, plus `DiscardResource` with its
  own state constraints. Correct but slower than it needs to be, and
  `crcbl_dx12::command`'s module docs say what it does rather than what it
  should do.

- **Read-only depth attachments are refused.** `create_image_view` builds one
  writable DSV per view, and the seam has no field asking for
  `D3D12_DSV_FLAG_READ_ONLY_DEPTH`, so a view cannot know which pass will only
  read it. **Needs a decision**: a second descriptor per depth view, or a field
  on `ImageViewDesc`.

- **Descriptor-heap slots are not retired, and the belief behind that is
  unverified.** The encoder retains an attachment's _resource_ but not its
  RTV/DSV slot, on the reading that D3D12 consumes those descriptors at record
  time. If that is wrong, a `destroy_image_view` followed by a new view while
  work is in flight overwrites a descriptor the GPU is still reading. Nothing
  tests it and WARP without the debug layer may not object either way.

- **`pipeline_barrier` skips transitions on host-visible buffers**, because
  upload and readback heap resources are pinned to one state for their lifetime
  and recording a transition on them is illegal. The seam has no vocabulary for
  that, and neither Vulkan nor Metal needs it. There is **no test**: no
  deterministic observable for a skipped barrier was found.

- **`ReadbackState::Pending` is never exercised.** Every test waits before
  requesting, so `poll_readback` is `Ready` on its first poll. Forcing `Pending`
  needs work genuinely in flight, which is a race rather than a test.

- **The retire queue's "does not release early" half has no device-level test.**
  After `submit` the fence may already have passed, so asserting `pending() > 0`
  would be racy. The device tests pin the two deterministic halves — the bytes
  arrive despite a destroy, and the queue drains to empty when idle — and the
  _ordering_ is pinned by `retire.rs`'s pure unit tests instead.

- **A WARP that hangs while executing still fails as a timeout, not a named
  stage.** The test helper panics with `stage=finish`, `stage=submit` or
  `stage=wait_idle`, and the readback poll has its own deadline — but a hang
  inside execution blocks in `wait_idle`'s `INFINITE` fence wait, so it surfaces
  as nextest's SLOW-then-SIGKILL. The test name identifies it; the stage marker
  does not.

- **`device.rs` grew again.** The descriptor-validation split DX3 was waiting
  for landed on 2026-08-07 (see the DX2 entry above); the file is still the
  implementation half of the device.

## What the DX12 swapchain and present-feedback slice left open

Surfaces, a flip-model swapchain, acquire/present/reconfigure, and
`Device::wait_until_presented`. `crcbl_dx12::present` holds everything that is
arithmetic (host-testable, and its tests run on any `cargo test`);
`crcbl_dx12::swapchain` holds everything that needs DXGI.

- **Whether the pacing wait genuinely blocks is measured, not asserted.**
  `a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd` prints
  whether each `wait_until_presented` returned or lapsed, and accepts either,
  because a window nobody is looking at is a state the compositor may retire
  frames differently for and the seam calls `SurfaceError::Timeout` expected
  traffic. Any _other_ error fails. Closing this needs a run whose output is
  read: if the waits return promptly there, the tolerance can become an
  assertion.

- **Whose handle `GetFrameLatencyWaitableObject` returns is unsettled**, so it
  is never closed. `SwapchainEntry::waitable` argues the asymmetry — a double
  `CloseHandle` is a process fault, a leaked handle is bounded — and records
  that `wgpu-hal` 29's D3D12 backend does not close it either (checked in the
  vendored source, not recalled). Settling it means reading the current DXGI
  documentation for that method; if the caller owns it, `destroy_swapchain` and
  `reconfigure_swapchain` are the two places that have to close it.

- **A present id is matched against a count, and the mapping is deliberately
  coarse.** DXGI's waitable object answers "fewer than `SetMaximumFrameLatency`
  presents are outstanding" and carries no id, so `PresentLedger` records only
  the highest id this swapchain object was given and a wait for any id at or
  below it blocks **once**. The seam licenses exactly this — its guarantee is
  "the weakest of the three" and it names this shape — but the consequence is
  worth writing down: a caller asking about a frame far back blocks as long as
  one asking about the frame before last, and the first `frame_latency` waits on
  a fresh swapchain return immediately because the object starts signalled that
  many times. An exact id→completion map would need a bounded ring of in-flight
  ids and a soundness argument about how many presents DXGI can have
  outstanding; it was **considered and declined** for this slice as machinery
  ahead of a caller that needs it.

- **A command buffer the caller has not destroyed blocks a resize.** The encoder
  retains every resource it records against, so a `CommandBufferHandle` still in
  the caller's hand holds a reference to a back buffer and `ResizeBuffers`
  refuses. `reconfigure_swapchain` waits for the queue and destroys the
  swapchain's own views and images, which is everything it can reach; the rest
  is the caller obeying the seam's existing rule about destroying finished
  command buffers. Nothing tests it — the test helper destroys its command
  buffer every frame — and the failure is a DXGI refusal with no field named,
  which is the class of thing `crcbl_dx12::swapchain`'s `check` otherwise turns
  into something readable.

- **`reconfigure_swapchain` refuses a format change**, by name. `ResizeBuffers`
  does take a new format, but the entry's `format` is what the views are built
  from and what a _later_ reconfigure resizes with, so changing it means
  threading the new format through the failure path too. Destroy and recreate
  works today.

- **Offscreen surfaces are still refused.** `SurfaceTarget::Offscreen` names an
  unwritten slice rather than a permanent refusal, so `crcbl screenshot` and the
  golden-image e2e cannot reach D3D12 — which is the same gap that keeps this
  backend out of `crcbl`'s registry. A ring of plain images through the same
  acquire/present path is what `crcbl-vk` and `crcbl-mtl` both build.

- **The sRGB-through-the-view path is the one place this backend performs the
  differing-format cast `create_image_view` refuses**, and it is legal only
  because a flip-model back buffer is the case D3D12 permits it on. DX2's entry
  about that refusal is unaffected — it still applies to every image a caller
  creates — but the two now disagree in the same crate, and the "needs a
  decision before the bind-group slice" note should be read with this in mind.

- **`MakeWindowAssociation(DXGI_MWA_NO_ALT_ENTER)` is called per swapchain, on
  the caller's window.** It stops DXGI's own message hook taking Alt+Enter into
  a fullscreen transition nothing above the seam can see. It is a window-global
  side effect a HAL backend arguably should not have; nothing above the seam
  asked for it and nothing can turn it off.

- **Nothing above the seam uses any of this yet.** No crate outside `crcbl-dx12`
  names `Dx12Instance`, so the backend is not in `crcbl`'s registry and no
  `crcbl-shell` window has ever been handed to it. Wiring it up is its own slice
  and would make the win32 shell e2e a real end-to-end D3D12 path.

## Raised 2026-08-09 and not finished

New gaps and deferrals from the planning session. The plan docs carry the
decisions; these are the things nobody has done.

### Deferred: browser multiplayer over WebRTC

**The only route that survives the no-infrastructure constraint**, and it is
recorded rather than refused so the decision is reopenable.

Data channels with **manually exchanged connection codes**: peer A creates the
connection, waits for ICE gathering to complete so candidates are embedded in
the SDP, and the compressed base64 of that is a "code" pasted to peer B, who
answers with one of their own. No signalling server. It maps onto topic 23's
channel semantics **better than WebSocket would have** — DataChannel offers both
ordered-reliable and unordered-unreliable, so the unreliable channel survives.

Against it: a third transport to maintain; a JS shim owning `RTCPeerConnection`
(the same `extern "C"` shape `crcbl-audio`'s web module already uses, so no
`wasm-bindgen` in any crate); a code that is hundreds of characters rather than
a room code; two round trips of copy-paste with both players in live contact
elsewhere; two peers realistically, since full mesh is N(N−1) exchanges; STUN
needed off-LAN and TURN needed behind symmetric NAT, which is the part that
costs money. Free public STUN is plentiful and the Open Relay Project offers a
free TURN tier.

**Do not fold it into bracket** — manual pairing is the antithesis of
matchmaking. If it is ever built it wants its own small sample whose subject is
the transport seam over a third transport shape.

### Coverage gaps and unbuilt things the plan assumes

- **Persistent mapped buffers are a native-only design principle**, and the
  browser path had no stated answer until now. `00-overview.md`'s first core
  principle names them alongside bindless and multi-draw-indirect; `wgpu`
  exposes `MAPPABLE_PRIMARY_BUFFERS` on native only. Every browser upload is a
  staging copy. Nobody has measured what that costs.
- **Wasm modules lose NaN canonicalization and fuel in a browser.** The
  equivalence gate ("bit-identical native _and in-browser_") is unguarded
  against NaN payload divergence, and hostile-module containment has no browser
  equivalent. Survivable because untrusted modules run server-side on native; a
  browser-hosted single-player game with mods has no containment. See
  `16-wasm-modules.md`.
- **MSL is validated by nothing, anywhere.** `spirv-val` runs on the SPIR-V;
  WGSL, MSL and DXIL are unchecked. MSL cannot be checked off macOS at all —
  `xcrun metal` is macOS-only, `newLibraryWithSource:` needs a device, and no
  open-source tool parses MSL. `xcrun metal` on the existing macOS CI leg is the
  cheap fix and is not done.
- **`crcbl-render` has no render-scale or upscale path**, though
  `15-windowing.md` locks borderless as "internal render target upscaled to the
  native surface" and `18-render-features.md` orders the post chain around it.
  `ShellCaps::HW_UPSCALE` exists and nothing can ask for it. This is a locked
  display mode with its renderer half missing.
- **L0's character controller and static trimesh/heightfield colliders do not
  exist.** `05-physics.md` puts both in L0 (MVP); the ROADMAP marks "P3 L0" done
  against a narrower list. towers demands both.
- **`crcbl-audio` has no bus graph and no limiter**, though `13-audio.md`
  specifies `master ← sfx/music/ui/voice` with per-bus gain and a soft-knee
  limiter, and its delivery table puts buses in P4A. Mix snapshots and ducking
  at P10 depend on them.
- **No golden audio buffers exist**, though the exit criterion asks for one per
  sample that emits sound and asteroids and horde both synthesise
  deterministically from fixed seeds.
- **The transcendental policy is two conflicting policies.** `05-physics.md`
  requires the `libm` crate; `13-audio.md` requires own polynomial
  approximations plus a CI deny. Neither exists. `libm` would be a new
  dependency and therefore a user decision.
- **`DeviceId` is per-kind on every backend**, which blocks local-multiplayer
  device assignment that `19-input.md` says is supported "from day one". A test
  asserting two devices are distinguishable would pass vacuously.
- **`21-jobs.md`'s threaded-wasm finding is not reproducible today**: it needs
  `rust-src` on `nightly-2026-07-02`. Unblock with
  `rustup component add rust-src --toolchain nightly-2026-07-02-x86_64-unknown-linux-gnu`.

### Owed by the shader guardrails

- **The differential render gate is still vk↔wgpu only.** Rule 5 asks for every
  backend; the gate now covers three scenes (cube, sprite, ui) across two, which
  closes the shaders with a history of divergence and leaves **Metal and D3D12
  entirely outside it**. A `sprite.slang` or `ui.slang` that means something
  different on MSL or DXIL would not be caught by anything. Both are blocked on
  the same prerequisites as everything else on those backends: `crcbl-mtl`'s
  draw tests are quarantined on a GPU hang, and `crcbl-dx12` refuses offscreen
  surfaces so it cannot read a frame back at all.

- **The cross-backend CI job's timeout was left at 30 minutes while its work
  tripled** (4 renders to 12). The renders are seconds on lavapipe and the
  compile dominates, but that is a local timing judgement, not a runner
  measurement. If that job ever times out, this is the first thing to look at
  rather than the last.

- **The declaration-order lint is stricter than the rule it guards.** Metal
  assigns indices per argument _table_; the lint asserts one global ascending
  order across all sets. So it can ask for a move that would have been harmless
  — swapping two resources in different tables trips it without changing any
  Metal index. Deliberate and documented in the module header: the per-table
  rule needs the lint to model Slang's table assignment, which is more of
  Slang's behaviour than is worth encoding for a guard whose false positives
  cost one declaration move. Reopen if a real shader finds it costly.

### Metal compute works, confirmed on hardware

`ComputePipelineDesc` carries `workgroup_size`, `crcbl-mtl` implements
`bind_compute_pipeline`/`dispatch`/`dispatch_indirect`, and the macOS CI job ran
all three new tests on a real device:
`a_compute_dispatch_writes_the_values_it_ was_asked_for`,
`an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer` and
`the_compute_pass_opens_an_encoder_and_its_calls_fail_only_as_themselves` all
PASS (112 tests run, 6 skipped). **Compute is no longer a Vulkan-and-wgpu
capability.** `indirect_count` is a separate Metal refusal and still stands.

The 6 skipped are the pre-existing draw tests that fault on that runner
(excluded by name in `.github/workflows/ci.yml`) — unrelated to compute, but
worth knowing the device is not fully healthy before reading any green macOS
run.

**A wrong workgroup size is caught on Vulkan and nowhere else.**
`crcbl_vk::spirv::require_workgroup_size` reads `OpExecutionMode … LocalSize`
and refuses a descriptor that disagrees with the shader. Metal cannot (MSL
declares no thread count, which is why the field exists) and wgpu keeps no
module source after `create_shader_module`. Safe only while every compute shader
is also run under Vulkan, which is true today and will not always be.

### Settled: `setDepthStencilState(nil)` hung every Metal draw

**Found by bisect, fixed in `8e40f55`.** For months every draw `crcbl-mtl`
recorded hung on GitHub's macOS runner with
`kIOGPUCommandBufferCallbackErrorHang` while render-pass clears succeeded, and
six tests were quarantined for it. Two hypotheses were wrong before this one.

The final round: ten probes, each the known-good hand-encoded pass plus exactly
one call, with a known-red and a known-green control. **7 passed, 3 failed**,
and the three failures are precisely the ones passing `nil` to
`setDepthStencilState:`. Its twin — same selector, real state object — passed,
as did `setCullMode`, `setFrontFacingWinding`, `setTriangleFillMode`,
`setDepthClipMode` and `setDepthBias:slopeScale:clamp:` individually.

**The fix makes `None` unrepresentable** rather than substituting at the bind
site: a pipeline without depth-stencil state holds a default object the device
builds once at open, so nothing in the crate can produce nil and the type says
so. Every descriptor field is set explicitly — `Always`, no depth write, `Keep`
on all three stencil outcomes — because `objc2-metal` is a generated binding
that documents no defaults, and guessing them would trade a hang for wrong
pictures.

Things worth carrying out of this investigation:

- **Three hypotheses, two wrong, and both wrong ones were "what's left standing"
  arguments.** The render-target format and the long draw forms were each the
  last candidate after eliminating others. What settled it was a _controlled
  comparison_ — one call reproducing the hang and its near-identical variant not
  — rather than an elimination.
- **The bug was invisible to every picture-based test by construction.** All six
  replay calls are image-neutral for a pipeline with no culling and no depth
  attachment. No golden could ever have caught it; only a device that faults.
- **Carry a known-red and a known-green control in any bisect** whose baseline
  would otherwise be a previous log. Without them "everything passed" cannot be
  distinguished from "the runner changed".
- The probes were deleted once they answered. A diagnostic that keeps running
  after it has reported is noise in the next run's signal.

**Unverified at time of writing**: the five released draw tests have not run
since the fix. If they pass, `crcbl-mtl` has working draws in CI for the first
time and the Metal arm of `render_e2e.rs` becomes worth wiring.

### Vulkan on Windows: the loader ignores its environment when elevated

**The cause, in the loader's own words** (`44bdf32`, with `VK_LOADER_DEBUG=all`
finally on the right job):

```
[Vulkan Loader] INFO: Loader is running with elevated permissions.
                      Environment variable VK_DRIVER_FILES will be ignored
                      … VK_ICD_FILENAMES will be ignored
                      … VK_ADD_DRIVER_FILES will be ignored
                      … VK_LAYER_PATH will be ignored
[Vulkan Loader] ERROR | DRIVER: vkCreateInstance: Found no drivers!
```

**GitHub's Windows runners run elevated, and the Vulkan loader discards every
environment-variable driver and layer path when the process has elevated
privileges** — deliberately, so a lower-privileged caller cannot inject a DLL
into one. `VK_DRIVER_FILES` was set correctly the entire time and the loader was
throwing it away by design. No shell and no path form was ever going to work:
neither the `cygpath -w` fix nor the move to `pwsh` could have mattered.

**The fix is registry registration** — `HKLM\SOFTWARE\Khronos\Vulkan\Drivers`
for the ICD and `…\ExplicitLayers` for the validation layer, each a `DWORD 0`
named by the manifest's full path. That is where a normally-installed driver
registers itself and what an elevated loader still reads.

**`CRCBL_VK_EXPECT_ADAPTER` is now the only thing that can prove which driver
answered**, since the pin no longer works through the environment. It was worth
building for exactly this.

Three rounds of diagnosis went to two causes that were real but not sufficient
(the `C:/…` path form; variables not crossing from Git Bash) and one that was
never measured at all — see the retraction below. **The loader could have said
this on round one.** `VK_LOADER_DEBUG` cost one line and answered immediately
once it was set on the job being debugged.

**Retracted, and the error was mine:** an earlier version of this entry
concluded "a loader ignoring its own debug switch is not reading its
environment". The switch had been inserted by matching the first step named
`Run the suite against lavapipe`, and both the Linux and Windows jobs have a
step with that name, so it landed on Linux. That conclusion was drawn from a
variable never set on the job it described.

**Still unknown after this fix:** whether the goldens hold.
`Tolerance::RASTERISER` was calibrated radv-versus-one-lavapipe and this is a
second, Windows Mesa build — unmeasured between two lavapipes.

**Owed:** `run-vk-e2e.ps1` and `run-vk-e2e.sh` are two harnesses over one suite,
and their guards are duplicated knowledge that will drift.

### Metal draw coverage in CI: what the ecosystem does

Researched 2026-08-10, because "is this just us?" was worth answering before
buying hardware. It is a real and widely-hit gap, but **it is not our failure**
— see the entry above.

- **GitHub's own position**: "Add support for Metal in macOS images" is an open
  discussion; a GitHub staff reply says _"There is no ETA for now but it's on
  our radar."_ Real GPU passthrough for hosted macOS runners is an open feature
  request.
- **Godot hit the paravirtual device too**, differently: it aborts with
  `-[AppleParavirtDevice newArgumentEncoderWithLayout:]: unrecognized selector`
  on `Apple Paravirtual device (Apple5)`. Closed unresolved; the reporter asked
  only for a graceful error. So the device is genuinely feature-poor — but ours
  fails on draws it demonstrably supports.
- **The asymmetry that matters for the plan**: Linux and Windows both have
  software rasterisers CI can install — lavapipe, which we already use, and
  **WARP** on Windows. macOS has no equivalent, which is why this gap is
  macOS-shaped rather than general.

**Actionable consequence, cheap:** `crcbl-dx12` has no e2e at all, and the
Windows runner has a real desktop session. **WARP is the D3D12 software
rasteriser that would close that gap the same way lavapipe closes Vulkan's** —
no hardware purchase, no self-hosted runner. That is the better next investment
than a Mac mini, and it is not blocked on anything.

### Settled: the render layer runs on all four backends

**D3D12 drew the cube frame on `4907b7e`**, and with the tightest golden match
of any backend:

```
dx12 selected IndirectCount / Bindless / Rasterised
device on adapter 0 "Microsoft Basic Render Driver" type=Cpu (CRCBL_ADAPTER=cpu)
golden cube on dx12 — 256x192: max channel delta 1, 0 over tolerance (0.0000%),
ssim 0.999879
```

So `render_e2e.rs` now passes on **Vulkan, native wgpu, Metal and D3D12**, and
the step is a real gate — the `continue-on-error` is gone. One golden, blessed
on lavapipe, matched by four independent implementations.

The two causes, both found by asking the device rather than reasoning about it:

1. **A constant buffer view outran its buffer.** D3D12 requires a CBV's
   `SizeInBytes` be a multiple of 256; `crcbl-dx12` rounded the _view_ up while
   the allocation stayed 16 bytes. The allocation is padded now, only for
   `UNIFORM` usage.
2. **Three draw-generation buffers were on an upload heap and bound writable.**
   D3D12 refuses `ALLOW_UNORDERED_ACCESS` on that heap at creation and pins the
   resource to a state no shader can write from. They are `DeviceLocal` now, and
   the frame zeroes them with a clear dispatch.

**The second was not a D3D12 bug at all** — it was a compromise this file had
already recorded under GPU-driven draw generation, kept because `fill_buffer` is
legal only outside a pass and the graph had no fill step. Vulkan tolerated it
for months. Worth remembering: **a portability compromise that one backend
accepts is not a compromise, it is a latent failure with a delay on it.**

Also settled by that work: a graph-level fill was the obvious fix and the wrong
one. `fill_buffer` is four separate backend promises — Metal repeats a byte,
wgpu clears only to zero, `crcbl-dx12` refuses it entirely — so it would have
moved the blocker one call later. A dispatch's portability is held by
construction.

**Two follow-ups this leaves:**

- ~~The `dx12 e2e (WARP)` job is misnamed~~ — renamed to
  `dx12 e2e (software adapter)`. `CRCBL_ADAPTER=cpu` selects the single
  `DeviceType::Cpu` adapter, and on that runner it is **Microsoft Basic Render
  Driver** rather than WARP. Naming the job after a specific implementation
  claimed something the pin never asked for.
- ~~`crcbl-dx12::fill_buffer` wants recording as a deliberate non-fix~~ — done,
  at the refusal itself. D3D12's fill needs a shader-visible descriptor heap
  this backend does not create, and nothing in the workspace needs it now that
  the counters are cleared by dispatch. A caller who wants it should say why a
  dispatch will not do.

### What WARP has actually proven

Worth separating from what is merely implemented, because this backend is
written blind and only CI ever executes it.

Proven on hardware:

- Compute dispatch, indirect dispatch, and a workgroup size refused for
  disagreeing with the container's `[numthreads]`.
- **Indexed draws, indirect draws, and indirect-count draws reading a GPU-side
  count** — all four passed on `c4e8655`.
- The root-signature register fix, implicitly: `compute_probe`'s pipeline could
  not have been created at all under the old `[[vk::binding]]`-derived rule.

Not proven on any device: dynamic offsets, offscreen surfaces, and a recorded
frame.

**A rot to expect.** `c4e8655` reddened WARP on
`the_slices_that_have_not_arrived_still_refuse_and_name_themselves`, which
asserts the unimplemented calls still answer `Unsupported`. Three of them had
just started working. That test's own comment calls it "the half that rots" —
every DX12 slice from here has to prune it, and the failure is legible when it
happens.

Also: `crcbl-dx12`'s crate docs still say bind groups and pipelines refuse,
which the code contradicts.

### Owed by GPU-driven draw generation

§3.3 is wired end to end — `cull` → `draw-args` → `forward` — and every golden
is bit-identical through it. What is not settled:

- **Metal and D3D12 have never run `draw_gen.slang`.** The MSL compiles, its
  bindings land at `buffer(0..8)` in declaration order, and it has no
  module-scope `threadgroup`. Nothing has executed it. Note the arm Metal needs,
  `IndirectPerBatch`, is proven **on Vulkan hardware, not on Metal** — a forced
  selector on one backend is not the same evidence as the backend that actually
  degrades to it.
- **radv appears to ignore `drawCount == 0`.** Seen while falsifying:
  `vkCmdDrawIndexedIndirect` with `draw_count: 0` still drew the geometry, and
  only removing the call blanked the frame. Nothing depends on it — the
  per-batch arm always passes 1 and relies on `instance_count == 0` — but a
  future empty-draw optimisation must not assume the count is honoured. Not
  investigated further; unknown whether it is radv, the loader, or our own
  recording.
- **`GeometryPath::MeshShader` has no tail** (§3.5). It degrades to an indirect
  one and logs, exercised on the null backend only.
- **Per-bucket capacity is the whole instance capacity**, so _N_ buckets cost
  _N_ × 16K × 4 bytes per frame slot. §3.3's own correction wants scene-stat
  sizing plus an overflow counter before the bucket table grows.
- **The mesh→bucket lookup is a linear scan** in `draw_gen.slang` — correct at
  any size, O(buckets) per instance. A mesh→bucket map is what a large table
  wants.
- **The three counters are host-visible only because the seam allows a fill
  outside a pass and the graph has no fill step.** A graph-level fill, or a tiny
  clear dispatch, would let them be device-local.
- **The browser will take `IndirectPerBatch`** — WebGPU has neither indirect
  feature — and that has not been tested there. Native wgpu selects
  `IndirectCount`, so the browser's arm is not the one CI exercises.
- **A golden is not sufficient for this stage.** Breaking `first_index` to zero
  left the cube golden **bit-identical** and was caught only by the argument
  readback. Worth remembering before treating an unchanged picture as proof that
  a draw-generation change was correct.
- Incidental: `mesh.png` has only **4 distinct colours** (flat-shaded faces),
  which is why the cross-path anti-vacuity floor is `> 4` and not the
  cross-backend script's 16.

### Owed by the GPU cull pass

- **The visible list has no consumer**, deliberately: indirect draw generation
  is its own slice. The pass is built in the e2e rather than in
  `ForwardRenderer` precisely because Metal would refuse it in a live frame.
- **Dead instance slots.** `InstancePool::remove` does not rewrite the element
  and the pass iterates `0..instance_count`, so a removed slot is culled on
  stale contents. The liveness bit wants to be `GpuInstance::flags`, which is
  still reserved and defines nothing. Today the count is the caller's problem.
- **No WGSL execution of `cull.slang` anywhere.** It compiles for wgsl and is
  run only on radv and lavapipe; the cross-backend script compares rendered
  scenes and this pass renders nothing. A compute-only differential harness is
  the missing piece, and it will be needed again for every later compute pass.
- `cull.slang` **re-declares** `GpuInstance` and `GpuMesh` because the compile
  script hashes one source per artifact and there is no shared header. A drift
  test compares the field lines of both files; a shared-include mechanism would
  remove the need for it.

### Owed by the GPU mesh table

The table (`MeshPool::table_buffer`) is what §3.3's cull pass will build
indirect draws from, so these two are due before a compute pass walks it, not
after.

- **A mesh id is a bare `u32` with no generation.** Freeing clears the entry to
  the empty range, so a _freed_ id resolves to `index_count == 0` and draws
  nothing. A **reused** slot is the gap: a stale id then names whichever mesh
  took the space, silently and plausibly. `MeshHandle` is the generational type;
  the id that reaches the GPU is not. Fix by putting a generation in the id or
  an epoch word in the entry — the choice interacts with how wide the cull pass
  wants its instance record, so decide them together.
- **The table has no resident bit.** Residency is a CPU gate today
  (`MeshPool::mesh`, `MeshPool::table_index`), which suffices only because the
  CPU records every draw. Once a compute pass reads the table itself, an entry
  written at upload but not yet flushed is reachable, and `index_count == 0` is
  the only signal it has.
- `crcbl-vk`'s `depth_probe` hand-builds a one-entry mesh table, so any further
  binding added to `mesh.slang` has to be mirrored there. A second hand-built
  copy of a layout is the kind of thing that drifts; worth folding into a shared
  test helper the next time either moves.

### Settled: base vertex and base instance never reach a shader

Recorded because it is a rule for every future shader here, and because it is
the first time the differential render gate caught a real divergence rather than
a hypothetical one.

`SV_VertexID` and `SV_InstanceID` mean **different things per target**, measured
rather than assumed: SPIR-V subtracts `BaseVertex`/`BaseInstance` (HLSL's
meaning), DXIL passes them through with D3D12 excluding both bases, and WGSL and
MSL index raw builtins that _include_ them. A pooled mesh at a non-zero base
vertex therefore rendered a correct pyramid through wgpu and a corrupted slab
through Vulkan — one source, two pictures — and `run-cross-backend-e2e.sh`
failed on it at 10.09% of pixels with a structural mismatch.

**The rule: every draw passes zero for both bases, and the real values arrive in
a per-draw constants block.** Zero is the one value all four lowerings agree on,
so nothing in the picture depends on how a target lowers a builtin.
`sprite.slang` reached the same conclusion independently for its own case; this
makes it the pattern rather than one shader's workaround.

The gate only caught it because `Scene::Cube` was changed to draw a second mesh
at a non-zero base. **A path nothing exercises is a path the gate cannot see** —
which is the general form of this and worth remembering before trusting any
green run over content that does not use the feature.

### Owed by the mesh-shader path

- **Slang's Metal backend materialises every global shader parameter in every
  entry point, and that once broke `main`.** Worth keeping because it constrains
  how any future shader here is written, and because the first diagnosis was
  wrong.

  Slang 2026.14 builds a `KernelContext` struct holding a pointer to every
  global shader parameter, and materialises it — with **all** of its globals —
  in every entry point, used or not. `mesh_shader.slang` had no global shader
  parameter until a `StructuredBuffer<Vertex>` was added for the vertex pull;
  that switched the machinery on, which dragged the module-scope
  `groupshared Amplification` into all four entry points including the fragment
  one, and `xcrun metal` refuses a threadgroup declaration in a fragment
  function. Fixed by making the payload a **local** in `taskMain` so no
  `groupshared` global exists for the context to carry. Slang lowers that to the
  same `TaskPayloadWorkgroupEXT` storage class in SPIR-V and to a stack payload
  `dxc` accepts.

  **The hypothesis recorded here first — that the fragment entry point sharing
  the vertex struct was the cause — was wrong**, and was falsified by trying it:
  a separate fragment input struct leaves the declaration exactly where it was.
  So were reordering the globals and function-local `groupshared` (which Slang
  rejects outright, `E31201`). Recorded so the wrong lead is not followed twice.

  **The rule that falls out:** a module-scope `groupshared` in a file that also
  has any global shader parameter is invalid on Metal. Nothing checks this — the
  `xcrun metal` CI step catches it after the fact, which is how it was found.

- **Not reproduced: a lavapipe SIGSEGV in CI that does not occur locally.**
  `retire::two_submissions_referencing_one_destroyed_buffer_keep_it_alive`
  segfaulted on CI's lavapipe during the same run, in a test unrelated to mesh
  shaders. The same commit runs 62/62 on this machine's lavapipe (Mesa 26.1.6,
  LLVM 22.1.8), so CI's Mesa build differs. Unexplained, seen once. If it recurs
  it is a real bug and the driver version is the first thing to compare — **CI's
  lavapipe is as unpinned as its `spirv-val` was.**

- **Nothing can bind a descriptor to the mesh stage yet.** `ShaderStages::MESH`
  and `TASK` exist and map correctly, but no bind-group layout or push-constant
  range names them, and no backend polices a layout naming a mesh stage on a
  device without the capability. That is why the first mesh shader hardcodes its
  three vertices instead of pulling them from a storage buffer the way
  `triangle.slang` does — pulling needs mesh-stage visibility, which obliges
  every backend to police the flag. **This is the next slice**, and it is the
  prerequisite for a mesh shader that reads real geometry.

  Note the flags are deliberately outside `ShaderStages::GRAPHICS` and `ALL`:
  Vulkan refuses `MESH_BIT_EXT` in a layout on a device without `meshShader`, so
  a composite carrying them would break every existing layout on most devices.

- **Meshlets need a mesh asset system that does not exist.** §3.5 wants clusters
  with bounds and normal cones baked from a mesh; `crcbl-scene` is a stub and
  the only mesh in the tree is a hardcoded cube. The builder, the cluster
  hierarchy and amplification-stage culling are all blocked behind topic 6's
  asset work — building any of them now would be building ahead of a consumer.

- **`crcbl-vk`'s absent-capability refusal is unexercised.** Both drivers here
  report `MESH_SHADER`, so only the null backend takes that arm. The e2e
  falsifies what any device can refuse instead — a mesh pipeline naming a
  fragment entry point as its mesh stage.

- **Metal and D3D12 have the stages and the committed artifacts, and neither
  loads them.** `msl/mesh_shader.metal` and the `ms_6_6`/`as_6_6` DXIL are built
  and validated; what is missing is `MTLMeshRenderPipelineDescriptor` and the
  D3D12 pipeline-state stream. Both refuse the entry points by name today.

### Owed by the capability work (P7)

- **The null backend cannot express several device states, so the engine's
  handling of them is untestable without a GPU.** Found while giving the log
  lines tests. Each is a `crcbl-hal` null limitation rather than an engine one,
  and closing any of them is a small change to `crates/crcbl-hal/src/null/`:
  `SurfaceCaps::current_extent` is hardcoded `None`, so the "surface reports X
  but the shell configured Y" path is unreachable; `NullInstance::adapters`
  returns exactly one adapter, so "no adapter can serve this surface" is
  unreachable; `AcquiredFrame::suboptimal` is hardcoded `false`, so the
  reconfigure-after-present path is unreachable; `wait_until_presented` always
  returns `Ok`, so the lapsed-timeout path is unreachable; and **neither preset
  advertises `PRESENT_FEEDBACK`**, so a device that claims it has to be
  hand-built in the test.

- **The observed half of the pacing line is pinned to `Unknown` in every test.**
  `NullDevice::display_timing` returns `Unknown` unconditionally, and no driver
  in this project has ever answered anything else — so `Fixed`, `Variable` and
  `Stepped` reach `settle_pacing` nowhere. The tests distinguish outcomes by the
  _requested_ and _resulting_ halves instead, which proves the line is not a
  constant but leaves three of four observed arms unexercised end to end. Same
  missing machine as the `VK_EXT_present_timing` entry elsewhere in this file.

- **No local driver reports the mesh/ray capabilities absent, so the degradation
  path is unexercised here.** `crcbl-vk` now reports `MESH_SHADER`,
  `TASK_SHADER`, `RAY_QUERY`, `RAY_TRACING_PIPELINE` and
  `ACCELERATION_STRUCTURE` from the real device — and **lavapipe reports all
  five too** (Mesa 26.1 implements `VK_EXT_mesh_shader` and the whole ray
  tracing set), which was not expected: the software rasteriser was assumed to
  be the negative case and is not. So on this machine and in CI's `vk e2e`,
  `GeometryPath::MeshShader` and `LightingPath::RayTraced` are what gets
  selected, and the fallbacks are compiled and unrun on the Vulkan backend. The
  unit tests cover the mapping; nothing covers a real device that lacks the
  extensions. Same shape as the Tier B indirect-draw arms recorded elsewhere in
  this file — an assumption about which driver is the weak one, that turned out
  to be wrong.

- **`accelerationStructure` is enabled without forcing `bufferDeviceAddress`.**
  Checked against the installed `validusage.json`: no VUID requires them
  co-enabled, and it is validation-clean on radv. Recorded because an
  acceleration-structure _build_ slice will need `BUFFER_DEVICE_ADDRESS`
  regardless — build infos take device addresses — so the pairing question
  returns the moment anything uses the capability rather than reporting it.

- **Only the downgrade line is asserted; the engine's other decision lines are
  not.** `crcbl-core`'s logger can now be captured in a test
  (`crcbl_core::log::capture`), and `crcbl`'s device-open path asserts both that
  it names a downgrade and that it stays silent when nothing was lost. The
  mechanism exists for the rest and nothing uses it yet: the pacing resolution
  (`asked for Auto, pacing Vsync`), the present-feedback capability line, and
  Win32's `exact refresh for …` are each the **only** record that a decision was
  taken, and each could be deleted with every test staying green.

  Two limits of the capture, both documented in the code: it is thread-local, so
  a test cannot see what a worker thread logged; and a `capture()` racing an
  `init_logging()` from outside the API can still lose its probe. Neither occurs
  in the workspace today.

- **"Tier" vocabulary survives in inline comments across the backends.** The
  type is gone and the doc comments are cleaned, but narrative comments,
  `.expect()` strings and test names still say Tier A/B in `crcbl-render`,
  `crcbl-wgpu`, `crcbl-mtl`, `crcbl-vk` and `crcbl-dx12`. Two caveats before
  anyone sweeps it: `crcbl-render`'s references were about the
  `ui.slang`/`ui_tier_b.slang` **shader permutation**, which no longer exists —
  that fork is deleted, so those are now simply dead words; and much of
  `crcbl-dx12`'s is real D3D12 `ResourceBindingTier` vocabulary that must stay.

- **Every path selector value must be executed by something.** A `GeometryPath`,
  `BindingModel` or `LightingPath` value no device in CI selects is compiled and
  unrun. The existing instance is the Tier B arm of the indirect-draw tests:
  lavapipe reports the higher capability, so the fallback has never run
  anywhere. This is the risk most likely to be realised.
- **The downgrade log line must be asserted**, not admired — an e2e that forces
  a feature off has to see the engine say so.
- **`required` must be shown to fail.** A device request naming a feature the
  null backend does not report must produce the named error; a `required` that
  cannot fail is not a gate.

### The two PowerShell harnesses keep their own copy of the nextest summary guard

Every bash e2e harness now sources `tools/nextest-summary.sh` for the one thing
they all have to do — strip the colour, find nextest's summary line, tell a
complete run from the `<ran>/<total>` shape nextest prints for one it cancelled,
fail on zero. That is eight copies collapsed to one, after five of them had
drifted into reading `2/15 tests run` as a healthy fifteen.

`crates/crcbl-vk/tests/run-vk-e2e.ps1` and
`crates/crcbl-shell/tests/run-win32-e2e.ps1` cannot source a bash file, so each
keeps a PowerShell transcription of the same logic. Both are **correct today** —
their `(?:(\d+)/)?(\d+) tests? run` is where the bash fix came from — and both
are now the only place the guard can drift, since nothing compares them against
the shared one and no fix to it reaches them. `run-vk-e2e.ps1` and
`run-vk-e2e.sh` are two harnesses over the same `crcbl-vk` `vk_e2e` suite, which
is the sharpest version of the problem: one suite, two guards, one of them
shared and one of them a copy.

**The option, stated and not taken: make the Windows harnesses bash and delete
the `.ps1` copies.** It is demonstrably possible for at least one of them —
`crates/crcbl-dx12/tests/run-dx12-e2e.sh` runs on `windows-latest` today, and
its "Why bash, when `run-win32-e2e.ps1` argued for PowerShell" section argues
that the Git Bash that image ships has `mktemp`, `tee`, `sed` and `grep`, that
GitHub Actions' `shell: bash` selects it, and that what bash buys is guards
`shellcheck` and a Linux developer can exercise, which matters because nobody on
this team has a Windows machine.

Against it, from those files' own headers:

- `run-win32-e2e.ps1` chose `pwsh` because it starts nothing and needs nothing a
  Windows shell lacks: `windows-latest` boots into a session with a window
  station and a desktop, so unlike the Wayland and X11 harnesses there is no
  compositor to launch, and `mkfifo` and `trap EXIT` — the two things those
  harnesses need bash for — mean nothing on Windows. Porting it would buy the
  shared guard and nothing else.
- `run-vk-e2e.ps1`'s reason is a measurement rather than a preference, and it is
  the strong one. `run-vk-e2e.sh` **was** the Windows harness, for three CI
  runs, and the Vulkan loader never saw its environment. Two real causes were
  found and fixed on the way (the manifest reaching the loader in Git Bash's
  `C:/…` form, and exported variables not reaching a native child), and the
  loader still reported
  `windows_read_data_files_in_registry: Registry lookup failed`. Its conclusion
  is that a native process launching a native process is the only shape with no
  environment translation in it.

**Correction to the premise this entry was raised under:** `run-vk-e2e.ps1` does
_not_ register an ICD in `HKLM`. What it does is resolve `CRCBL_VK_ICD` to a
native path, fill in `VK_DRIVER_FILES`/`VK_ICD_FILENAMES` when nobody else set
them, walk `PATH` for `vulkan-1.dll`, dump those variables one process from the
loader, and run `vulkaninfo`. The `HKLM:\SOFTWARE\Khronos\Vulkan\Drivers` and
`…\ExplicitLayers` writes are in `.github/workflows/ci.yml`, in the job step
that extracts lavapipe — and stay PowerShell whichever shell the harness is
written in.

That correction cuts both ways, which is why the call needs a measurement rather
than a re-read. The workflow's own comment says GitHub's Windows runners are
elevated and that the loader **discards every environment path when the process
is elevated**, deliberately, which is why HKLM is what actually selects lavapipe
there and `CRCBL_VK_EXPECT_ADAPTER` is the only thing that proves which driver
answered. If that holds, the environment-translation argument for `pwsh` is no
longer load-bearing on the runner it was written for: nothing the harness
exports selects the driver either way. Verifying that is a CI run, not a
reading.

What was verified here: the two `.ps1` guards' regexes and their cancelled and
zero branches; that `run-dx12-e2e.sh` is bash on `windows-latest` in `ci.yml`;
that the HKLM writes are in the workflow and not in either harness. What was
not: whether `run-vk-e2e.sh` under Git Bash would pass on that runner today,
which only a CI run can answer.

Related, and also not acted on: `tools/nextest-summary-test.sh` exercises the
shared guard against every summary shape and **nothing runs it**. `ci.yml` has
no shell-lint or script job to hang it on —
`grep -n shellcheck .github/workflows/` matches nothing — and adding one was
outside the paths that slice owned. Until it is wired in, the guard's own test
is a file somebody has to remember to run.

## Test-file names: what the rename slice left, and one rename declined

The naming slice took `docs/plan/12-testing.md`'s "filenames name the subject,
never the taxonomy tier" and applied it to nine files. What it could not close,
and one thing it deliberately did not do:

**`crates/crcbl-shell/tests/appkit_session.rs` is not renamed to `appkit_e2e.rs`
— considered and declined.** By subject it is the macOS member of the family
`wayland_e2e.rs` / `x11_e2e.rs` / `win32_e2e.rs` belong to: a real WindowServer,
a real window, injected input. But in this workspace the `_e2e` suffix carries a
second meaning beyond the subject — every other file wearing it opens with a
crate-level `#![cfg(all(target_os = …, feature = "…-e2e"))]`, carries
`#[ignore]`, and is driven by its own harness script. `appkit_session.rs` has
none of that, and cannot: `.github/workflows/ci.yml`'s AppKit step records that
this target is the AppKit backend's _only_ executable coverage, so putting it
behind a gate would leave the backend with none by default. A name promising a
switch nobody has to throw is the more expensive error — the reader goes looking
for the feature that enables it and concludes it is off. The file's `//!` header
now argues this under "Why it is not called `appkit_e2e`", so the question does
not get re-opened from the filename alone. If the suffix ever stops implying a
gate, the rename becomes correct and the header is where to look.

**Stale path references left behind, all in files that slice did not own.** Each
is prose in a code span, not an intra-doc link, so nothing fails to build and
`cargo doc` stays green — they are simply wrong and will send a reader to a file
that is not there:

- `crates/crcbl-phys/src/broadphase.rs` names `tests/churn.rs` twice (module
  header and the depth-bound doc comment); it is now
  `tests/broadphase_churn.rs`.
- `crates/crcbl-phys/src/forces.rs` names `tests/property.rs`; it is now
  `tests/dynamics.rs`.
- `crates/crcbl/src/engine.rs` names `tests/library_seam.rs` in the doc comment
  about the hand-driven loop; it is now `tests/seam_from_outside.rs`.
- `docs/plan/12-testing.md` names `tests/churn.rs` and `tests/property.rs` in
  its seeded-generator paragraph, and names
  `crates/crcbl-server/tests/integration.rs` as the one file carrying a taxonomy
  tier for a name. That example is now spent — the file is
  `client_server_session.rs` — so the paragraph needs rewriting rather than a
  path substitution, and there is no remaining file in the workspace to point at
  as the counter-example.
- `docs/code-review.md` cites `crates/crcbl-server/tests/integration.rs:15` and
  `crates/crcbl-audio/tests/orbit.rs:191`. That file is a dated record of past
  reviews, so leaving the paths as they were written may be right; the decision
  was not made either way.

**Test _names_ inside these files were not touched.** `docs/plan/12-testing.md`
records six crates as drifted below the prose-sentence-name convention —
`crcbl-ecs`, `crcbl-net`, `crcbl-input`, `crcbl-phys`, `crcbl-audio` and
`crcbl-store`. Two of the renamed files sit in that set and show it:
`crates/crcbl-audio/tests/spatial_chain.rs` still has
`centre_position_is_symmetric` and `right_position_pans_to_right`, and
`orbit_cue_changes_over_time` now names a fixture the file is no longer named
after. Renaming the functions is a separate task and stays unclaimed.

### Stale test-name references left behind by the backend-qualifier rename

`docs/plan/12-testing.md`'s "a test that exists on more than one backend names
the backend or its API" rule has now been applied to `crcbl-vk`, `crcbl-mtl`,
`crcbl-dx12` and `crcbl-wgpu`: 26 verbatim-identical names across two or three
of those crates are gone, along with 12 near-identical pairs whose only
divergence was cosmetic. The rename touched test function names and their doc
comments only — no production code, no signature, no behaviour — plus the three
places a test name is a string outside a test body (`.github/workflows/ci.yml`'s
Metal job comments and `crates/crcbl-mtl/tests/run-mtl-e2e.sh`'s usage example).
`crates/crcbl-dx12/tests/run-dx12-e2e.sh` needed no change:
`the_pinned_adapter_opens_a_device_and_names_itself` and
`a_fresh_device_says_whether_it_is_validated_and_is_not_already_removed` were
never duplicates, and `.github/workflows/ci.yml`'s
`not test(a_layer_swapchain_acquires_a_drawable_and_presents_it)` filter names a
test that is unique to `crcbl-mtl` and was likewise untouched.

Prose in two files still names the old identifiers, and neither was in the
rename's paths:

- `docs/plan/12-testing.md`'s naming section argues the rule by citing
  `a_device_outlives_the_instance_that_made_it` and
  `a_compute_dispatch_writes_the_values_it_was_asked_for` as names existing "in
  three each", and gives a count of twenty-six. Both citations and the count are
  now historical. The paragraph wants rewriting to describe the convention as
  held rather than as owed. **Do not reach for the old three-way examples**: the
  per-backend copies of `device_outlives_the_instance_that_made_it`, the compute
  and indirect dispatch pair, the render-pass clear, the foreign handle, the
  destroyed handle and mtl's swapchain refusals were deleted once
  `crates/crcbl/tests/hal_seam_e2e.rs` covered them on all four backends in CI.
  A behaviour every backend owes now has exactly one name, in that file — which
  is the convention arriving rather than being described.
- `docs/backlog.md` itself names old identifiers in entries that predate the
  rename: `a_render_pass_clear_reads_back_the_exact_texels` (now
  `a_metal_`/`a_d3d12_`-prefixed, and the entry means the Metal one),
  `the_slices_that_have_not_arrived_still_refuse_and_name_themselves`,
  `an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer`,
  `an_indexed_draw_reads_the_bound_index_range`,
  `a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`,
  `a_pulled_triangle_is_drawn_and_read_back_texel_by_texel` and
  `reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it`.
  Each now carries a backend word.

### Two backend test-name pairs deliberately left diverging

Both are cases where renaming would break something outside the rename's paths,
not cases where the convention was judged not to apply:

- `an_indirect_calls_stride_is_only_checked_when_it_is_used` (`crcbl-dx12`,
  `src/draw.rs`) against
  `an_indirect_draws_stride_is_only_checked_when_it_is_used` (`crcbl-mtl`,
  `src/draw.rs`). They differ by one word — `calls` against `draws` — so a grep
  for either misses the other. The Metal name is quoted in
  `docs/plan/12-testing.md` as an exemplar of the prose-sentence rule, so
  renaming it strands that citation.
- `reported_limits_come_from_d3d12_and_agree_with_the_features` against
  `reported_limits_come_from_the_device_and_agree_with_the_features`
  (`crcbl-mtl`). The Metal side never says "Metal". `docs/plan/12-testing.md`
  presents this exact pair as an example of the convention being followed, so it
  is left as the doc describes it; the honest reading is that the D3D12 side
  names its API and the Metal side does not.

### Exact test-name collisions still open between non-backend crates

Measured over every crate under `crates/` with the same detector used for the
backend rename — a name defined under `#[test]` in more than one crate. Eight
remain, all outside the four backend crates and so outside that task's paths:

- `debug_format` in `crcbl-client`, `crcbl-ecs`, `crcbl-phys`, `crcbl-server`
  and `crcbl-ui` — five copies, and the only name in the workspace that is not a
  prose sentence at all.
- `automatic_selection_reports_what_it_tried` (`crcbl`, `crcbl-shell`),
  `messages_name_the_specific_problem` (`crcbl-hal`, `crcbl-shell`),
  `the_seam_is_also_usable_generically` (`crcbl-hal`, `crcbl-shell`),
  `placeholder_compatibility_is_refused` (`crcbl-client`, `crcbl-server`),
  `sweep_removes_dead_entities` (`crcbl-ecs`, `crcbl-phys`),
  `the_entry_points_answer_zero_until_a_source_is_installed` (`crcbl-audio`,
  `crcbl-store`), `the_frames_corners_do_not_grow_with_the_menu`
  (`crcbl-render`, `crcbl-ui`).

`a_device_outlives_the_instance_that_made_it` in
`crates/crcbl-hal/tests/seam_from_outside.rs` is deliberately **not** in that
list any more and should stay unqualified: it is the seam's own obligation
checked on `NullBackend` from outside the crate, which `docs/plan/12-testing.md`
calls the backend-agnostic shape. It was a fifth copy of that name until the
three backend copies took their prefixes; the bare name now belongs to the one
test that is genuinely about no backend.

### The first-triangle milestone is four different claims, not one written four ways

Recorded because the opposite is the obvious guess and unifying the four names
would flatten a real difference. All four were read end to end:

- `crcbl-mtl`'s
  `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` draws a
  hand-written MSL triangle with **no bindings at all** — geometry from
  `[[vertex_id]]`, a fragment shader returning the `INK` literal — and
  `assert_ink_triangle` checks that the centre texel is exactly the ink colour,
  all four corners exactly the clear, and every other texel is one of those two.
  `ink_msl`'s own doc says why it is not the engine's shader: pulling vertices
  needs bind groups.
- `crcbl-dx12`'s
  `a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel` runs the
  engine's `crcbl_shaders::triangle` through an SRV over a storage buffer, and
  `assert_triangle_drawn` asserts three fixed probes are red-, blue- and
  green-dominant and that each probe's channels sum to full scale — the
  barycentric property that catches a wrong element stride.
- `crcbl-vk`'s `a_triangle_pulled_from_a_vulkan_storage_buffer_reaches_memory`
  makes the same pulled-vertex claim but derives its probes from the geometry
  (75% of the way from centroid to each vertex) rather than fixing pixel
  coordinates, and adds a centre-blend assertion for interpolation.
- `crcbl-vk`'s `the_vulkan_triangle_matches_its_golden_image` is the P1
  golden-image gate against `tests/golden/triangle.png` at
  `Tolerance::RASTERISER`.

So the flat-colour coverage check, the two dominance checks and the golden
compare are four distinct assertions; only the backend qualifier was missing and
only that was added. What is genuinely absent is a golden-image gate on Metal
and D3D12 — `crates/crcbl/tests/render_e2e.rs` is the backend-agnostic golden
and covers the cube scene, not the triangle.

## What the mtl/dx12 `#[ignore]` placement slice left open (2026-08-10)

The slice marked every `crcbl-mtl` and `crcbl-dx12` test that opens a real
device, instance or adapter with `#[ignore]`, and narrowed both harnesses from
running the whole crate to `--run-ignored only`, so the count each guards on is
the number of device tests. `docs/plan/12-testing.md`'s placement section
records what landed. What it did not settle:

### The device-test counts are a source reading, not a measured run

`run-mtl-e2e.sh` should select 71 tests and `run-dx12-e2e.sh` 73 (the mtl CI job
filters `a_layer_swapchain_acquires_a_drawable_and_presents_it` out, so it
should report 70). **Those numbers come from classifying the test bodies, not
from watching nextest select them** — nothing on this team's machines executes
either crate, and `cargo nextest list --run-ignored only` for both is empty on
Linux because every device test lives in a `#[cfg(target_os = …)]` module Linux
does not compile. The first `mtl e2e` and `dx12 e2e` runs after this are the
first observation of the real counts; a number well below these means an
`#[ignore]` did not land where it was thought to, and a number above means the
classification missed a device path.

The classification traced `instance::tests::open`, `device::tests::open_device`
and `instance::tests::pinned_adapter` transitively through each module's local
helpers, then every test the trace called pure was read. **A test that reaches a
device by some route none of those three names would have been missed**, and
nothing in the tree would report it: it would simply keep running in the
`--workspace --all-features` sweep on the macOS/Windows runners and fail there
rather than in the harness.

`crates/crcbl-dx12/tests/run-dx12-e2e.sh`'s CI job header still records "the HAL
suite above passed **155/155 on WARP**" from runs on `dc846ff` and `0354eec`.
That is a dated account and correct for those runs; a reader comparing it
against the ~73 the harness will now print should read the drop as the selection
narrowing, not as tests disappearing.

### The workspace sweep deliberately did not gain `--run-ignored all`

The slice brief asked for `--run-ignored all` on both
`cargo nextest run --workspace --all-features --locked --profile ci` lines in
`ci.yml`, so the ordinary sweeps would keep running the newly-ignored tests.
**That was not done, on `docs/plan/12-testing.md`'s authority**, which reserves
that run as the one that deliberately does not execute the ignored set so it
stays green on a machine with no compositor and no GPU. Measured rather than
argued: `cargo nextest list --workspace --all-features --run-ignored only` on
Linux selects 159 tests — `crcbl-vk::vk_e2e` 73, `crcbl-shell::wayland_e2e` 37,
`crcbl-shell::x11_e2e` 32, `crcbl-wgpu::wgpu_e2e` 15, and one each from
`crcbl::render_e2e` and `crcbl-cli::cli_e2e`. `--all-features` compiles the
Vulkan, wgpu and render suites on the macOS and Windows runners too, so the flag
would have those jobs open a Vulkan device on a runner with no loader.

What that leaves is a **pairing nothing enforces**: `crcbl-mtl`'s device
coverage now exists only in the `mtl e2e` job and `crcbl-dx12`'s only in the
`dx12 e2e` job plus `test-cross-platform`'s "DX12 adapter report" step. Delete
or disable any of those and the crate's device tests stop running everywhere,
with every remaining job still green — the harnesses' zero-count guards cannot
fire for a harness nobody invoked. The obvious fix is a required-job list the
workflow checks against itself; it was out of scope here.

### The pure/device split is not checked by anything

Nothing fails when a newly written test opens a device and forgets `#[ignore]`,
or when an existing one stops needing a device and keeps it. The first shape is
caught late (the test runs in the sweep on a runner without the hardware and
fails); the second is never caught at all — the test simply stops being run by
anything except the harness. A lint would have to know which helpers open a
device, which is the same trace this slice did by hand; recorded as a gap rather
than attempted.

### What the non-backend test-name rename left behind

The prose-sentence rule in `docs/plan/12-testing.md` was applied to every test
name of three words or fewer outside the backend crates. Measured with a
`#[test]`/`#[tokio::test]` extractor over the whole tree: 138 such names before,
1 after. The `debug_format` bullet and the `ray_misses_aabb`, `decode_empty`,
`decode_truncated`, `debug_output` and `debug_formatting` copies named in "Exact
test-name collisions still open between non-backend crates" above are resolved;
the rest of that entry's list still stands.

- **`orbit_integration_deterministic` (`crcbl-audio`, `tests/spatial_chain.rs`)
  was deliberately not renamed.** It is cited by name in `docs/code-review.md`,
  which was outside this slice's paths, so renaming it would strand that
  citation. The citation is already stale in two other ways and is worth fixing
  together with the rename: it gives the path as
  `crates/crcbl-audio/tests/orbit.rs:191` (the file is now
  `tests/spatial_chain.rs`), and its finding — that the test XORs per-block
  hashes, which is order-insensitive — no longer holds, because the test feeds
  one hasher in block order and asserts that the reversed event order hashes
  differently.
- **`crcbl-wgpu/src/conv.rs`'s `format_mapping_round_trips` remains**, a
  three-word name naming the function under test rather than a claim. `crcbl-vk`
  and `crcbl-mtl` state the same contract as
  `no_two_formats_share_a_metal_format` and its siblings, so the wgpu name
  should follow that shape. It was outside this slice's paths.
- **The four `the_workgroup_size_matches_the_numthreads_the_shader_declares`
  copies and the three `the_params_block_matches_the_offsets_slangc_emits`
  copies in `crcbl-shaders` now name their shader** — read end to end first, and
  they are one contract instantiated per shader, not one claim written several
  ways: each reads its own `.slang` source, or asserts its own `PARAMS_SIZE` and
  field offsets. That is the same situation `docs/plan/12-testing.md` describes
  for the backend crates, where the fix is to differ by the one word that names
  what is under test. Renaming them meant editing the `PARAMS_SIZE` doc comments
  in `cull.rs`, `clear_counters.rs` and `draw_gen.rs`, which cite the test by
  name; those three doc-comment lines are the only non-test text the rename
  touched.
- **Same-crate duplicate names in `crcbl-render` were left alone**:
  `a_pool_leaks_nothing` and
  `a_pool_error_flattens_into_the_seams_without_losing_its_message` each exist
  in both `instance_pool.rs` and `mesh_pool.rs`. Both are prose sentences
  already, so they were outside the rename's criterion, but a grep for either
  finds two tests over two different pools and nothing in the name says which.
  Naming the pool in each would close it.
- **Not re-examined:** the 4-to-6-word names that already read as claims but are
  thin — `sweep_removes_dead_entities`, `t_values_are_correct`,
  `element_ids_are_preserved` and their neighbours in `crcbl-phys` and
  `crcbl-net`. The rename's cut was at three words, so these were never read;
  whether they state what the body asserts is unmeasured, not judged fine.

### Fixed sleeps left in tests the assert-nothing slice did not own

The slice that gave the assert-nothing tests real assertions removed two fixed
sleeps as part of the work — `jitter_does_not_panic`'s 50 ms in
`crcbl-net/src/condition.rs` and `null_stream_runs_without_error`'s 20 ms in
`crcbl-audio/src/lib.rs`, both now poll-with-deadline. Its brief named those two
tests, so the neighbours that sleep the same way were left alone and are
recorded here rather than left to be re-derived:

- **`source_fill_receives_stereo_buffer`** (`crcbl-audio/src/lib.rs`) sleeps 30
  ms and drops the stream. Its `CheckSource::fill` asserts the rate and the
  buffer shape, so the assertions are real — but they run on the stream's
  polling thread, and a run where the thread never got scheduled inside 30 ms
  executes none of them and still passes. It is the same shape as the loop with
  no count that `the_value_column_starts_past_the_longest_label` had. The fix is
  the one `the_null_stream_fills_its_source_until_it_is_dropped` now uses next
  to it: count the fills in the source, poll for a non-zero count against a
  deadline, and assert the count before dropping the stream.
- **`the_latency_only_constructor_delays_delivery_and_still_delivers`** and
  **`a_message_under_latency_does_not_arrive_until_the_delay_has_passed`**
  (`crcbl-net/src/condition.rs`) sleep past a configured latency and then assert
  delivery. These are the honest use of a sleep — the thing being tested is a
  wall-clock delay — but they still cost their sleep on every run and would fail
  on a machine that stalls past the margin. `ConditionSimulator` schedules
  against `Instant::now()` with no injectable clock, so making them poll would
  mean either a clock seam in the simulator (a production change) or a poll loop
  that spins until the message arrives. Neither was in scope.

Also not touched: `docs/plan/12-testing.md`'s frame-poll rule is prose, and
nothing enforces it. A grep for `thread::sleep` under `crates/*/src` and
`crates/*/tests` is the whole of the available check.

## What the coverage audit found and this session did not fix

Audited 2026-08-10 across five angles: tests that cannot fail, the seam's
documented obligations, backend parity, backend-agnostic coverage, and crates
with thin coverage. The "tests that cannot fail" findings all shipped — see
`git log` for the thirteen. What is below is what did not, each with the
evidence that produced it, so the next session does not re-derive it.

Two results are deliberately recorded as **non**-gaps, because both look like
gaps and re-auditing them costs a day: `crcbl-scene` has zero tests and is
correct — `src/lib.rs` is thirteen lines of doc and no items, and it says the
`Scene` type arrives with its phase. And the "ECS replication roundtrip"
`docs/plan/12-testing.md` asks for exists already, as
`a_lossless_run_leaves_the_clients_state_hash_equal_to_the_servers` in
`crates/crcbl-net/tests/replication.rs`, with loss and reorder variants beside
it.

### Obligations tested on exactly one backend

- **Deferred driver-object destruction** (`crcbl-hal/src/device.rs`, the
  obligation that `destroy_surface` invalidates the handle at once while the
  driver object lives until the last swapchain dies) is properly tested only on
  Vulkan, by `a_surface_with_a_live_swapchain_defers_its_driver_object` and the
  two negatives beside it. `crcbl-dx12` has the survival half
  (`a_dxgi_swapchain_keeps_working_after_its_surface_handle_is_destroyed`) and
  not the deferral half. `crcbl-mtl` has neither:
  `crates/crcbl-mtl/src/swapchain.rs` argues the obligation is discharged more
  simply because Metal has no separate surface object, which is plausible and is
  a claim rather than a test. `crcbl-wgpu` has neither.

- **Clamp-and-report** (a swapchain clamps the shell's requested extent into the
  platform range and reports the result on `AcquiredFrame::extent`) is tested on
  Vulkan and Metal and on neither D3D12 nor wgpu. On D3D12 the platform does pin
  the range on a real `HWND`, so this is a real gap; the fixture
  `a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd` already
  builds the window a test would need.

- **A caller renders at `AcquiredFrame::extent`** is asserted by nothing at all.
  `crates/crcbl-hal/src/swapchain.rs` states it as a caller obligation and says
  using the requested size instead is the bug the field exists to prevent, and
  that it only appears while a window is being dragged. The engine does adopt
  it, but no test drives an acquire whose returned extent differs from the
  requested one and then checks the recorded render area. On the null backend
  the two are always equal, so the bug is structurally invisible there.

### The null backend can be resized and killed, but not clamped

**Both halves of this closed.** `crcbl_hal::null::Recorder` gained
`report_swapchain_out_of_date` and `lose_device` beside the four injectors it
already had, and `crates/crcbl/src/engine.rs` now tests all three of its
out-of-date arms — the acquire, the present and the pacing wait's deliberate
no-op — plus the device-loss policy end to end through `drive`. A
nineteen-strong mutation sweep over the hooks and those arms left no survivor.
So `crcbl-vk` no longer carries the only test of a resize, and "this device is
gone and stays gone" is no longer a state nothing can express.

What is left is the third thing the old entry wanted from one hook and did not
get: **an injector that makes `acquire_next_frame` hand back an extent other
than the one configured.** The seam's obligation 3 says a caller must use the
answer rather than the request, and `NullDevice::acquire_next_frame` says in a
comment that it has no window system to clamp against, so it always answers with
the configured extent. That leaves `GpuContext::acquire`'s
`acquired.extent != self.configured_extent` branch — the one that writes the
compositor's chosen size back into `config` so a later `resize` does not see a
change that is not one — reachable only on a compositor that actually clamps.
This was deliberately not built with the other two: nothing about a clamped
extent is a _failure_, so it does not belong in the fault-injection shape those
two took, and it wants its own decision about whether the recorder holds a clamp
rule or a one-shot override.

### The dedicated cross-backend job still compares two backends of two

**Half of this closed on `41b6e61`**, differently from how it was framed. The
entry used to say MSL and DXIL were compared against nothing, because
`run-cross-backend-e2e.sh` renders every scene on Vulkan and wgpu and on nothing
else. That is no longer the gap: `crates/crcbl/tests/render_e2e.rs` now draws
`Cube`, `Sprite` and `Ui` on whichever backend `CRCBL_GPU` names, so Metal and
D3D12 compare all three against the same lavapipe-blessed references the other
two do. Both matched on their first run, at max channel delta 1.

What is left is narrower and worth stating precisely. The vk-versus-wgpu job
compares two backends' output **to each other**, which catches a divergence
neither has a golden for; the golden path compares each backend to a
_reference_, which catches a backend drifting from what was blessed. Those are
different checks, and only the first is still two-backends-wide. Extending it
would mean running two backends in one process on one machine — which is what
that job does — and no runner has both Metal and D3D12.

So the remaining gap is structural rather than owed: a Metal-versus-D3D12
comparison has nowhere to run. The golden path is the substitute and is already
in place.

### Coverage the testing plan asks for and nothing provides

- **No sample owns a golden frame** — declined rather than owed, and the
  reasoning is under "Declined for now: a golden frame per sample" below rather
  than repeated here.
- **The ECS churn soak with a leak assert does not exist.** The plan asks for it
  by name. Nothing in `crcbl-ecs` spawns and despawns over many ticks and then
  asserts nothing leaked. One seeded loop, no GPU.
- **`crcbl-ui` owes a hit-test grid and has two points.**
  `button_hit_test_inside` and `button_hit_test_outside` exist; the sweep the
  plan names does not.

## Decisions taken 2026-08-10, so they are not re-argued

Each of these was a question the coverage audit raised and left open. They are
answered here rather than carried, with the reasoning, so a later session can
disagree with the argument rather than rediscover the question.

### Decided: `crcbl-wgpu` gets owner tagging, and the wasm build pays for it

The question was whether the browser target should carry the side table the
seam's third obligation requires. **It does.** The cost is one `u64` compare per
handle resolve, against a hash lookup that already happens; that is not a cost
the wasm build needs protecting from, and a seam obligation honoured by three of
four backends is not honoured. Cross-device handle misuse being undefined on
exactly the backend a browser runs is the worst place to have it, not the most
defensible.

Follow whichever existing backend's shape is closest rather than inventing a
fourth spelling.

**Noted while deciding, not acted on:** owner tagging will then exist in four
crates as four hand-written copies of one idea. That is duplicated knowledge and
it will drift. Extracting it into `crcbl-hal` is the obvious move and is
deliberately _not_ being done now — three of the copies work, and rewriting
working backends to host a fourth is scope the task does not need. Revisit if a
fifth backend ever appears, which is the point at which the duplication stops
being tolerable.

### Decided: device loss surfaces, it does not self-heal

The engine will not recreate the device. `HalError::DeviceLost` propagates and
the loop stops with an error naming it.

Recreation means rebuilding every resource the frame graph, the pools and the
renderers hold, on a code path that by construction almost never runs — the
classic shape of a recovery path that is broken when it is finally needed.
Surfacing it is honest, testable in one assertion, and leaves the harder policy
available later for whoever has a real reason to want it. A game that wants to
survive a lost device can restart the engine; nothing in the samples does.

**Implemented and pinned.** `Recorder::lose_device` reports a device as gone and
keeps it gone, and `a_lost_device_stops_the_driven_loop_with_an_error_naming_it`
in `crates/crcbl/src/engine.rs` drives `drive` over a real `GpuContext` on it:
the run ends on the frame that hit the loss, with the driver's own message, with
its frame budget unspent and with no rebuild attempted. The last of those is
asserted off the `hal: reconfiguring the swapchain to ` log line rather than off
the recorder, because a rebuild that failed records no event — so an engine that
never tried and one that tried and was refused look identical in the stream, and
those are exactly the two policies this entry chose between.

### Decided: the four-backend compare is more scenes in `render_e2e`, not a new job

The audit's framing was a cross-platform image compare — bless a shared
reference and have the macOS and Windows jobs compare against it. **That already
exists**: `crates/crcbl/tests/render_e2e.rs` compares against a checked-in
golden blessed on lavapipe, and CI runs it on all four backends. What it does
not do is cover more than one scene.

So the gap is one line of scope, not a new job: `Scene` has `Cube`, `Sprite` and
`Ui`, `render_e2e` draws only `Cube`, and `sprite.slang` and `ui.slang` are the
two shaders that have _actually_ diverged per target in this repo's history. The
fix is to draw all three scenes there and bless two more goldens, which gives
Metal and D3D12 the coverage `run-cross-backend-e2e.sh` gives Vulkan and wgpu.

Cheaper than a new job, reuses the anti-vacuity colour floor and the tolerance
that are already calibrated, and it puts the coverage in the file whose whole
purpose is being backend-agnostic.

### Declined: minimum-count floors on the e2e harnesses

Both backend harnesses now select `--run-ignored only`, so the number they guard
on is the device-test count — 70 on the Metal runner and 73 on the D3D12 one,
measured. The zero-count guard still passes a selection that collapsed from 73
to 3.

A floor would catch that, and it is **not** being added: any threshold below the
real number is arbitrary, and a threshold equal to it fails CI every time a
device test is added, which trains people to bump it without reading. The counts
are printed by both harnesses and visible in the run log, and the classification
that produces them is now documented in `docs/plan/12-testing.md`. Revisit if a
collapse ever actually happens — at that point the floor has evidence behind it
instead of a guess.

### Declined for now: a golden frame per sample

`docs/plan/12-testing.md` asks every sample for a determinism check _and_ a
golden frame. The determinism half is met everywhere.

**The argument that used to close this entry was wrong, and a shipped bug is the
proof.** It said the samples "are already pinned by replay hashes, which catch
the same regressions a golden would catch". They do not catch _rendering_
regressions: there is no `replay_hash` in the tree, and what exists — tuple
comparison of simulation state, horde's `state_hash` over kills/HP/positions,
and `crcbl-server::sim_hash::hash_world` over tick id, system names and
component bytes — contains **no pixel at all**. All of them pass unchanged if
the frame is black, the tonemap is wrong or a shader is swapped. Every browser
demo rendered a transfer function too dark for several commits and every gate
stayed green; a user reported it.

**And the cost is lower than this entry assumed, because the pattern already
ships.** `apps/lumen/tests/golden.rs` with `golden/room.png` and
`run-lumen-golden.sh` renders lumen's own scene through
`OffscreenSetup::open_forward_with` and diffs it with `crcbl-golden`, wired into
CI. Replicating that per demo needs each sample to expose its scene construction
— no engine feature. The `--screenshot` route is the alternative: one field on
`Common` in `crates/crcbl/src/args.rs` plus one arm in `Common::consume`,
reaching all seven apps. Either would have caught this bug at zero tolerance.

### The seam does not describe what `wait_semaphores` does with an impossible wait

Found while writing `crcbl-wgpu`'s timeline-semaphore test, 2026-08-10, and
verified against all three implementations.

`crates/crcbl-hal/src/device.rs` documents `wait_semaphores` as returning
`Ok(true)` when the waits are satisfied and `Ok(false)` on timeout — "a timeout
is a normal outcome for a frame-pacing poll, not an error" — with
`InvalidHandle` and `DeviceLost` as the only errors. It says nothing about a
wait on a value **nothing submitted will ever signal**, which is neither
satisfied nor a timeout.

The backends disagree, and both answers are defensible:

- `crates/crcbl-wgpu/src/device.rs` returns `HalError::Unsupported` naming the
  cause, because wgpu has no standalone semaphore that could signal the value
  later, so waiting would hang until the deadline and then lie by calling it a
  timeout. That error is **not in the seam's documented set**.
- `crcbl-mtl` treats the same case as an ordinary unsatisfied wait.

**Decided: the seam grows the third outcome rather than wgpu losing it.**
Failing fast with a reason beats blocking for the full timeout to report a
timeout that was never going to be anything else — a frame-pacing poll wants to
know it asked for something impossible, not to pay the deadline first. So
`wait_semaphores`' docs should name `Unsupported` for an unsatisfiable wait, and
the other backends should adopt it when next touched.

Not done in the slice that found it: changing a seam contract means changing
every implementation of it plus the tests that assert the current behaviour, and
that slice was adding coverage rather than moving a contract. The tests as
written assert each backend's _current_ answer, so whoever takes this will see
exactly which ones move.

### `crcbl-dx12` has no timeline-semaphore test because it has no timeline semaphore

Recorded so it is not mistaken for a coverage gap. `crcbl-vk`, `crcbl-mtl` and
now `crcbl-wgpu` each have
`a_<backend>_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it`;
D3D12 has none because the feature is unimplemented there, not because the test
was forgotten.

## `crcbl-wgpu` owner tagging: what the tests do not reach

Obligation 3 is now implemented in `crcbl-wgpu` —
`crates/crcbl-wgpu/src/handle.rs` holds the tag/id pair, and every pool entry is
an `Owned<T>` — so the "Decided: `crcbl-wgpu` gets owner tagging" entry above is
answered. Two things about that work are worth carrying rather than
rediscovering.

**The slot's `u64` cannot separate owners whose tags collide.** Not a defect in
this backend, and true of `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` for the same
reason: every pool holds exactly one owner's rows, so a foreign handle that gets
past the tag lands on a row the _looking-up_ owner filled, the id agrees, and
the lookup succeeds. The id half is what catches a shared pool and what stops
`handle::remove` from taking a row this owner does not own — that much is
asserted by
`a_wgpu_slot_belongs_to_the_owner_that_filled_it_even_when_two_tags_collide` —
but it is not a second line of defence against a colliding tag, and this session
briefly wrote a test claiming it was before the test failed and said otherwise.
The hole opens only after `OWNER_TAG_COUNT` owners in one process.

**The windowed swapchain path's owner checks are untested.**
`WgpuDevice::swapchain` and `WgpuDevice::surface` funnel through
`dead_or_foreign`, which keeps a dead handle reporting `SurfaceError::Lost`
(what callers retry on) and reports a foreign one as
`SurfaceError::Hal(ForeignObject)`. **Nothing asserts that split.**
`crates/crcbl-wgpu/tests/wgpu_e2e.rs` is offscreen-only by construction and the
harness's Xvfb half runs `apps/sandbox` and `apps/breakout` rather than checking
errors, so no test hands `acquire_next_frame`, `present` or
`reconfigure_swapchain` a swapchain from another device or a surface from
another instance. The three new e2e tests cover buffers, the queue handle and
surfaces at `Instance` level only. Closing it needs either a windowed test
target or an offscreen swapchain crossed between two devices — the latter is
cheap and was simply not in this task's scope.

## What the three-scene `render_e2e` does and does not prove

`docs/backlog.md`'s "Decided: the four-backend compare is more scenes in
`render_e2e`, not a new job" is implemented: `crates/crcbl/tests/render_e2e.rs`
now has one `#[test]` per `Scene` — cube, sprite and UI — each with its own
golden under `crates/crcbl/tests/golden/`. What follows is what that run did not
settle.

**Metal and D3D12 remain unverified.** Both goldens were blessed on lavapipe and
both hold bit-identically on `CRCBL_GPU=vk` and `CRCBL_GPU=wgpu` against the
same ICD, which is the only cross-target evidence obtainable on a Linux machine.
The `mtl-e2e` and `dx12-e2e` jobs are the first time `sprite.slang` and
`ui.slang` will be compared against anything on MSL or DXIL, and nobody has seen
those frames. A large pixel delta there is a tolerance question; a structural
mismatch or a failed slot assertion is a finding about the backend. The
comparison prints enough to tell them apart, and
`every_sprite_slot_is_painted_and_the_gaps_are_not` is the assertion that names
the `SV_InstanceID` failure mode directly rather than as a summary number.

**`Tolerance::RASTERISER::max_failing_ratio` has been tightened from 2% to 0.1%,
and CI is the only thing that can confirm it for Metal and D3D12.** The old
figure was sized against "how many pixels differ at all" while gating "pixels
differing by more than `max_channel_delta`", and the gap was wide enough to pass
a plainly visible recolour of a quarter of one sprite — measured, at 0.7345%.
The new value is derived from the quantity actually gated: every `crcbl-vk`
golden and every `render_e2e` scene reports **0 over tolerance** on vk and wgpu
(re-run locally against lavapipe after the change, 74/74 and 3/3 twice), dx12 on
WARP and metal on a paravirtual device report 0 on all but one, and the
exception is **metal's cube at 2 pixels — 0.0041%**, which the new bound clears
by about 24x. The derivation now lives in `Tolerance::RASTERISER`'s doc comment
and in `crcbl-golden`'s crate docs, and both ends are pinned by
`a_localised_recolour_that_the_old_two_percent_ratio_passed_now_fails` and
`the_worst_measured_cross_backend_frame_still_passes_with_room_to_spare` in
`crates/crcbl-golden/src/compare.rs`.

What is **not** verified: nothing on Metal or D3D12 was re-run, because neither
can run here. Metal's cube is the closest measurement to the new limit of
anything ever taken, and its 2 pixels are wrong by a channel delta of 207, so
`max_failing_ratio` is the only knob that can absorb them — no
`max_channel_delta` worth having would. If that scene ever drifts to 50 failing
pixels at 256x192 it goes red where it used to pass, and that is the intended
behaviour rather than a regression. The `mtl-e2e` and `dx12-e2e` jobs are the
verdict.

**`max_channel_delta` was left at 2, and the local run says it cannot go
lower.** `crcbl-vk`'s `sprite_rotation` golden reports
`125 pixel(s) differ at all (0.2543%), max channel delta 2, 0 over tolerance`
against lavapipe. Dropping the delta to 1 turns those 125 drifting pixels into
125 failing ones — 0.2543%, two and a half times the new ratio — so the two
knobs cannot both be tightened. Considered and declined for that reason, with
the measurement rather than a guess behind it.

**The per-scene anti-vacuity checks are the part with real headroom, and the
colour floors are not.** The three floors are the cross-backend harness's own
`CRCBL_CROSS_MIN_COLORS_*` numbers; measured on the blessed frames at 256x192,
the sprite scene has 17 distinct colours against a floor of 16 and the UI scene
has 7 against 6. One colour of headroom each is deliberate — that is how those
floors were calibrated — but it means a scene edit that removes a colour trips
the floor before it reaches the golden, which is what happened while proving the
checks can fail.

**The four CI steps are still named "Draw a frame through ForwardRenderer".**
Three frames now, through three renderers, and only the cube's goes through
`ForwardRenderer`. The step comments were corrected; the names were left alone
because they are the labels a run's history is read by and nothing greps them.
Rename them if the churn is ever worth it.

## The material table has both halves; what is still missing from a material

`crcbl_render::MaterialTable` is `docs/plan/03-gpu-driven-rendering.md` §3.2's
material table SSBO, `crcbl_shaders::mesh::GpuMaterial` is a row, `mesh.slang`
binding 6 is where the fragment stage reads one, and binding 7 is the
`Texture2DArray` page a row's `base_color_texture` selects a layer of. The
texture-indices half of this entry is done and has been deleted. What is still
deliberately not there, and what it would take:

**One base-colour texture, and no other slot.** A row has a factor and one page
layer. Normal, metallic-roughness and emissive maps are each another `u32` in
`GpuMaterial`, another sample in `fragmentMain` and — for the first one that is
not colour data — another _page_, because an `ArrayPages` page is one image and
one format, and a normal map is linear where a base colour is sRGB. That is the
first thing that makes the single-page shape insufficient, and it should be
where the second page is introduced rather than a generic page manager arriving
ahead of a second caller.

**A page is one image, which is the limit `Bindless` exists to lift.** Every
layer shares an extent, a format and a mip count, so two textures of different
sizes cannot share a page. The extent is the caller's now — `PageDesc::extent`,
whatever `PageDesc::opaque_white` was given, asserted against the recorded
copies by `forward`'s `an_app_page_and_table_reach_the_device_whole` — but it is
still _one_ extent for every layer. Real content does not look like that. See
the wgpu entry below for what stands between here and the bindless form.

**No mip chain, and the sampler is nearest because of it.** §3.2 makes mip
generation a compute pass of its own and it is not written, so
`upload_texture_layers` uploads `mip_levels: 1` and `forward`'s base-colour
sampler filters nearest — a filtered read of a page with no mips buys a shimmer
rather than a smoother picture. The first minified material texture is what
makes the compute pass worth writing.

**Nothing imports a texture.** `crcbl-scene` parses glTF materials and this
slice did not wire it. An application can now author layers — `PageDesc` at its
own extent, `push_layer` per layer — but it has to produce the texels itself:
the demo's are `crcbl_render::scene`'s `CHECKER_TEXELS` and whatever
`PageDesc::opaque_white` writes into layer 0. Wiring an importer needs a
decoder, a page allocator and a lifetime story for a layer, which is P9's.

**A material is a start-up write.** `MaterialTable` is one host-visible buffer
with no ring — the mesh table's shape, not `InstancePool`'s — because nothing
rewrites a row between frames. `MaterialTable::set` therefore carries the same
caveat `MeshPool::upload` does, stated in its docs: called while a frame is in
flight it is a read-after-write hazard across submissions. **The first animated
material is what makes this a ring**, and it is the moment
`instance_pool::DirtyRanges` becomes worth sharing, because there would then be
two callers that coalesce runs rather than one that writes single rows.

## The debug-overlay retrofit: what was rejected, and one engine doc that is now stale

Breakout, flappy and asteroids now contribute `DebugModule` sections
(`BoardStats`, `CourseStats`, `FieldStats`, plus `DebugModule for Audio` on
flappy and asteroids), wired through `HostedGame::debug_sections` the way
horde's `SceneStats` already was. What was considered and left out:

- **Breakout has no audio section.** `breakout::audio::Audio` keeps no counter
  at all — no `plays`, no `dropped` — so a row would have meant adding state to
  the game for the panel's benefit. Rejected on those grounds. If breakout ever
  grows a `plays` vector the way flappy's did, the section is three lines.
- **Ball speed was the only invisible breakout number.** `GameLogic::ball_speed`
  is the difficulty ramp and nothing displayed it; everything else breakout
  knows (score, lives, state, high score) is already in `HudStrings`. A
  `paddle`/`ball x,y` row was considered and dropped as a number the player can
  see.
- **Asteroids does not repeat the wave.** `HudStrings::refresh` already draws
  `Wave: {wave + 1}`, and two numbers on screen under the same word that differ
  by one is worse than one.
- **No entity count for breakout.** It would have meant a new
  `Game::entity_count` accessor, and breakout does not churn: it spawns the grid
  once and despawns bricks until a restart respawns them. Flappy and asteroids
  both already had the accessor because both are churn samples.
- **Four audio modules, four different facts — deliberately not shared.** Horde
  reports `dropped` (it is the only sample with a `MAX_VOICES` cap), flappy two
  cue counts plus live voices, asteroids three cue counts plus whether the held
  engine loop is sounding, breakout nothing. The `label: value` shape is common;
  the knowledge is not, and the samples are separate binaries, so extracting one
  would mean a new crate or a change to `crcbl-ui`.

Two findings that are **not** fixed:

- **`crcbl::engine::HostedGame::debug_sections`'s doc comment is now wrong.** It
  says the empty default "is exactly what four of the five samples want" and
  that "`apps/horde` is the one that does not". Four of the five override it now
  (breakout, flappy, asteroids, horde); sandbox is the only one that does not.
  Left alone because the retrofit was scoped to `apps/**` and the comment is in
  `crates/crcbl/src/engine.rs`.
- **Debug row labels share one namespace across modules, and one collided.**
  `crcbl-render`'s `FrameTimings` renders `timings: pending` while a timestamp
  report is in flight, and asteroids' first draft used `pending` as the label
  for its deferred-despawn count. Nothing detects that — the panel is a flat
  list of `label: value` rows and a reader tells two `pending`s apart by which
  section they are under. The asteroids row was renamed to `despawns`; the
  general problem stands, and the place it bites first is a test that searches
  the draw list by label text (`row_value` in each sample's `app.rs`), which
  silently reads the wrong row rather than failing.

## `apps/hud` milestone 1: what was deliberately left out

`docs/plan/sample/04-hud.md` names milestone 1 as "P4 skeleton: HUD page with
the slice-1 primitives", and that is all that was built. Everything below was in
scope for the sample overall and is not in the tree.

**Waiting on the styling system, not on this sample.** The CSS subset and its
`.css` files, the ≥2 themes and their runtime switcher, the widget gallery page,
the UI inspector, the live-restyle hot-reload showcase, and the per-theme golden
frames in CI. All of them are P10 in the sample's own doc and all of them rest
on a layout/styling engine that does not exist: `crcbl-ui` today has `DrawList`,
`FontAtlas`, `Label`, `Button`, `Style`, `Hud`/`HudPanel`/`Anchor`, `Menu` and
the `debug` panel, and no stylesheet, cascade, selector or box model anywhere.
Building any of it now would be machinery with a single speculative consumer.
The one thing worth recording for whoever starts P10: `page::draw` is one
function that positions everything from named constants at the top of
`apps/hud/src/page.rs`, so the styling work replaces that function's body rather
than restructuring the sample.

**Sample rule 7 is met now** — hud has a `web.rs`, a `cdylib` lib named
`crcbl_hud`, polled `PolledGpu`/`PendingLoop` bring-up, and an entry at every
registration site. It is the smallest wasm artifact of any sample at **2 720 934
bytes**, against breakout 2 947 252, flappy 2 937 308, asteroids 2 970 845 and
horde 3 028 644, which is the measurement this entry said was worth having.

Four things that slice found in the shared web tooling and did not fix, none of
them hud's:

- **Settled: Chrome 151 broke the browser gate, and it was a device mismatch,
  not a readback quirk.** This entry predicted the failure and it arrived
  exactly as described — GitHub's runner moved from Chrome 150.0.7871.128 to
  151.0.7922.108 between `4eb0d65` (Pages green) and `77fa401` (Pages red),
  group A went red for all five demos at once, and the deploy was **skipped**
  rather than failed, which is the shape that hides a broken publish.

  The cause: a WebGPU canvas is handed between two devices — Dawn renders into
  it and Chromium's compositor reads it back for `toDataURL` — and those must be
  the same Vulkan implementation. `--use-webgpu-adapter=swiftshader` moves
  **only Dawn**; the shared-image device stayed on whatever the machine had, and
  on 151 the hand-off fails. The snapshot was therefore **uninitialised memory
  rather than black** — decoding the raw PNG outside the browser gave 2427
  distinct colours, almost all at alpha 0, which is why it decoded as
  `rgb(0,0,0)`. Chromium said so in its own stderr:
  `ReadPixels: Source shared image is not accessible` and
  `CopyTextureForBrowser from [Invalid Texture]`.

  The fix is `--enable-features=Vulkan --use-vulkan=swiftshader` in
  `browserFlags`, pointing the shared-image device at SwiftShader too. Neither
  flag works alone, and `--use-angle=swiftshader` does **not** substitute — it
  is specifically Chromium's shared-image Vulkan device that has to match
  Dawn's. Nothing about how pixels are read changed: `toDataURL` was never the
  problem, and the control and every render check still read through the same
  path.

  **It was never confined to the control.** With group A bypassed, the real
  breakout demo failed identically at its own canvas size with 16 device errors.
  The control was faithfully representing group D, which is its whole purpose.

  **No browser pin.** The gate passes on 151, and the control is what turned a
  silent regression into a loud one — a pin would have hidden this rather than
  fixed it.

- **An unexplained workaround, found and deliberately not used.** Creating a 2D
  canvas in the page _before_ the WebGPU context and reading it back with
  `toDataURL` also makes the SwiftShader readback work, with the old flags.
  Priming it after the fact does not work, the mechanism is unexplained, and it
  would have to be injected into every demo page. Recorded only in case the flag
  fix stops working.

- **Xvfb + `--hardware` reads transparent black on this machine** and loses the
  WebGPU device mid-run, on Chromium 151 with an RX 7900 XTX under RADV. It is
  harmless because `auto` falls through to SwiftShader — which is also what CI
  does, since the runner has no GPU at all — but a developer who passes
  `--hardware` under Xvfb gets a confusing failure. Not investigated.

- **A browser that hands out no adapter is guarded twice, and there is a race
  between the guards.** `WgpuInstance::new_async` probes with wgpu's
  `is_browser_webgpu_supported` before enumerating, and `web/engine/demo.js`
  probes before downloading the wasm. If `requestAdapter()` succeeds for a probe
  and returns null for wgpu's own enumeration a moment later, `new_async` still
  traps — proven reachable with a call-counting double that lets the first _n_
  requests through. Closing it properly needs an upstream fix: `wgpu::Adapter`
  exposes no accessor for its inner `GPUAdapter` (`api/adapter.rs` has only
  `as_custom`), and every other reader — `features()`, `limits()` — is the same
  structural getter with the same uncatchable failure.

- **Worth filing upstream against wgpu**: the vendored
  `Gpu::request_adapter{,_with_options}` bindings type a nullable WebIDL return
  (`Promise<GPUAdapter?>`) as `js_sys::JsOption`, which is undefined-only by
  documented design — `wasm-bindgen`'s own `sys.rs` says "JavaScript `null` is a
  distinct present value". Either the binding needs a null-aware wrapper or
  `future_request_adapter` should test `is_null_or_undefined`.

- **No standing regression guard for the null adapter.** The double that
  reproduces it lives in a scratchpad, not the repo. Making it permanent means a
  group-A sub-check that patches `requestAdapter` and asserts the named message,
  which is a real change to a harness with a documented one-demo-per-run shape —
  left out rather than decided unilaterally.

- **Neither WebGPU-refusal branch in `web/engine/demo.js` calls `settle()`**, so
  the Stop button stays enabled and does nothing. Pre-existing on the sibling
  branch; the new branch matched it rather than fixing half.

- **`web/engine/demo.js` has no way for a demo to say it saves nothing.** On
  `STOPPED` it prints `` `${savedLabel} saved.` `` unconditionally, so hud
  passes `savedLabel: 'Nothing'` and its status bar reads "Nothing saved." —
  true, and a workaround. A falsy branch in `demo.js` is the honest fix.
- **`web/templates/demo-window.html` is one copy for every demo**, so hud's
  canvas is labelled `aria-label="HUD game"` and the page carries a note about
  browsers not starting audio until you interact. hud is neither a game nor
  audible.
- **CI's shellcheck step covers `tools/*.sh` and `crates/*/tests/*.sh`, not
  `web/*.sh`.** Both web scripts touched here were checked by hand and are
  clean, but nothing in CI would have caught it.

**Sample rule 8 (spatial audio through `crcbl-audio`) is not met, and this may
be an honest exemption rather than a gap.** The rule is about _positional game
events_, and hud has none — no world, no listener, no position. UI click sounds
would be an audio system this sample invented for the rule's sake rather than
because anything needs them, and the sample's own scope ("fake data only — no
server simulation beyond a trivial ticker") does not reach for them. Recorded as
a decision to confirm rather than one taken: if the answer is that hud should be
silent, rule 8 should say so the way rule 11 already names hud's exemption.

**No game input, considered and declined.** Binding number keys to fire the
ability slots early was designed and dropped: the ticker already drives every
slot through ready and cooling states, the doc's scope says the page is driven
by "a scripted loop", and a second way to fire an ability would be a second
thing for the determinism script to have to cover. The consequence is that
`HostedGame::key_event` is empty for this sample. If a later milestone wants a
pointer-driven gallery, that is where input arrives.

**The samples' `build.rs` bake half is still copied per sample**, as recorded
above — hud does not add a fourth copy only because it is rule-11 exempt and has
no `build.rs` at all. Nothing changed about that finding.

**Not verified:** the sample has never been run in a real window. Every check
was headless — the null backend, and lavapipe through `--backend vk` — so the
page's colours, spacing and legibility at a real size are unreviewed, and
nothing in CI would catch a page that is correct in the draw list and ugly on
screen. The per-theme golden frames the exit criteria call for are what would
close that, and they are P10.

### `crcbl-assets` after stage 6 task 2

`AssetId`, `AssetHandle`, the `Loading | Ready | Failed` state machine,
`AssetSource` and `DirSource` landed in `crates/crcbl-assets`. What did not, and
what was decided along the way.

**`AssetId` is still hash-of-path, which the plan's own correction says is
wrong.** `AssetId::from_path` derives the id from the canonical key, so renaming
`props/crate.glb` gives it a different id and orphans every reference. The
corrected model in `docs/plan/06-assets-scenes.md` is a sidecar
`crate.glb.meta.ron` carrying a random 128-bit GUID, created on first import.
Nothing can create one here — first import is the importer, which is task 3 — so
the type was made 128 bits wide and given `AssetId::from_bits` so a sidecar GUID
drops in without the type changing. What is missing is the sidecar reader, a
writer that mints a GUID, and a registry path that prefers the sidecar's id over
the path's. Do it in the same slice as the importer, before any content exists
to be renamed.

**No `FetchSource` asset source, deliberately.** Stage 10 owns the browser asset
path. `crates/crcbl-assets/src/source.rs` shows the whole implementation in its
module docs — a newtype over `crcbl_store::web::FetchSource` delegating `read` —
because that type already canonicalises the key, already enqueues on a miss and
already answers `StorageError::Pending`. It is not written because a wrapper
with no consumer is a wrapper nobody has exercised. The claim that it needs no
caller changes is a design argument, **not** something a test proves: no
`AssetSource` other than `DirSource` and the crate's own scripted test source
exists.

**A blanket `impl<S: StorageSource> AssetSource for S` was considered and
declined.** It would have made every storage backend an asset source for free,
and it is recorded here because it will look obvious to the next reader. Two
reasons against: an asset source must not be writable, and coherence — a blanket
impl claims the trait for every present and future `StorageSource`, so
`PackSource` (a baked blob, not directory-shaped storage) could not implement
`AssetSource` on its own terms.

**Asset keys are restricted to `[A-Za-z0-9._-]` and `/`, on native too.**
`DirSource` runs `crcbl_store::web::canonical_key` before touching the
filesystem, so `my asset.png` and `café.png` are refused even though the
filesystem would serve them. Deliberate: those load natively and 404 over HTTP,
and the failure would surface at the point it is hardest to fix. The cost is
that an artist cannot name a file with a space. If that becomes a real
complaint, the fix is percent-encoding in the fetch backend, not a second key
rule here — two rules is how the two backends drift apart.

**No `Unloaded` state and no GPU retire.** The plan lists
`Unloaded → Loading → Ready | Failed`; only the last three are built, because
nothing can produce `Unloaded` — an unrequested asset has no entry and a
released one is removed. It comes back with hot reload (task 5), which turns a
`Ready` entry back into one with no bytes, and with the refcounted release's
other half: the retire calls into the stage 2 deletion queue, which need a
GPU-resident asset to retire and therefore the importer.

**`Ready` and `Failed` are terminal, with no retry.** A failed asset stays
failed until a caller releases and re-requests it. No backoff, no retry budget,
no distinction between a 404 and a transient network error — the last of those
would matter for a browser source and does not exist yet.

**Nothing depends on `crcbl-assets`.** Like `crcbl-scene`, it is a workspace
member every `cargo build --workspace` compiles for nothing until task 3 gives
it a consumer. Same trade-off, same argument as that crate's header.

**Not reviewed or built:** the exit criterion "no synchronous IO anywhere in
engine crates (CI: deny `std::fs` outside `DirSource` + tooling)". There is no
such CI gate, and `crcbl-assets` did not add one — `DirSource` reaches the
filesystem through `crcbl_store::NativeStorage` rather than calling `std::fs`
itself, so a lint written literally against `std::fs` would not name the crate
the criterion is about. Whoever writes that gate has to decide what it actually
forbids.

**Not reviewed:** thread-safety. `AssetSource` requires only `Debug`, where
`crcbl_store::StorageSource` requires `Send`. Nothing loads assets off the frame
thread today and `crcbl-jobs` has not been pointed at this seam, so the bound
was left off rather than guessed at. Adding it later is a breaking change for
any implementor that is not `Send`.

**Not reviewed:** budgets. The registry has no size cap, no eviction and no
limit on how many loads can be outstanding; `poll` walks every `Loading` entry
every call, which is fine for tens and unmeasured for thousands.

### Accepted: CI will not have a real Metal GPU, and that is not a task

Recorded as a decision so it stops reading as work somebody could pick up.

GitHub's hosted macOS images expose an `Apple Paravirtual device`. Real GPU
passthrough is an open feature request on their side with no date, so no amount
of work in this repository changes it. The options that would are a self-hosted
runner or a Mac in somebody's office, and both are a standing cost for a gap
that is narrower than it first looks.

**What the paravirtual device does cover**, and this was itself a correction —
it was long assumed to run no shaders at all, generalised from macos-14, the one
image whose `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and macos-26
run compute dispatches and triangle draws correctly, `macos-latest` resolves to
macos-26, and the Metal suite's device tests pass there. The render e2e draws
every scene on it and matches goldens blessed on lavapipe.

**What it does not cover**, stated so nothing implies otherwise: a discrete or
unvirtualised Apple GPU, and anything a real driver does that a paravirtual one
does not. `crates/crcbl-mtl/tests/run-mtl-e2e.sh`'s header already says this and
should keep saying it. Metal has no software rasteriser, so unlike Vulkan
(lavapipe) and D3D12 (WARP) there is no second implementation to cross-check
against — the cross-backend comparison is the substitute, and it is weaker
because it compares Metal against a _different API_ rather than against a second
Metal.

The mitigation is the one already in place: a person on a real Mac can run
`run-mtl-e2e.sh` unchanged, and that remains the only thing that covers a
non-virtual GPU. Nothing else is owed here.

### The split comparator: what CI has to confirm, and what was declined

The scoring split landed — `Tolerance` carries `gross_channel_delta: 24` and
`max_gross_ratio: 0.001` beside a `max_failing_ratio` relaxed to 0.01, and
`compare` counts each pixel against both thresholds on its one existing visit.
What is left is verification nobody here can run, plus the alternatives that
were tried on paper and rejected, so they are not re-proposed.

**Not verified locally, and CI is the only verdict: the two frames the bound is
sized against.** Neither backend runs on this machine.

- **D3D12 / WARP's sprite scene** — 76 pixels of 49 152 over the delta at up
  to 13. It now clears the drift budget by 6.5× where it used to clear one ratio
  by 3.2×, and its 13 is under `gross_channel_delta`, so it scores nothing at
  all on the gross budget. The exposure is the second of those: if a future
  sprite scene puts WARP's edge disagreements past delta 24 on more than 0.1% of
  the frame, D3D12 goes red where the old ratio passed it. Delta 13 on an edge
  texel is a function of the contrast across that edge, not of driver quality,
  so a higher-contrast sprite could plausibly reach it. Nothing measured has.
- **Metal's cube on a paravirtual device** — 2 pixels at delta 207, 0.0041%.
  This is the one legitimate frame that scores on the gross budget at all, and
  it sits 24× under it. At 97×61, the smallest size the gate runs at, that
  budget is five pixels.

Both are pinned by fixtures in `compare.rs`'s tests
(`warps_sprite_edges_pass_and_are_what_the_ratio_is_sized_against`,
`the_worst_measured_cross_backend_frame_still_passes_with_room_to_spare`) that
reproduce the reported per-pixel numbers, so a future tightening argues with a
test. A fixture is not the frame, though: it reproduces the counts and deltas,
not the pixels.

**The one place this is looser than what it replaced.** A frame with between
0.5% and 1% of its pixels off by 3 to 24 levels was refused by the 0.005 ratio
and passes now. That band is empty in every measurement across vk, wgpu, dx12
and metal, and the alternative is leaving WARP 3.2× from a false alarm on the
backend nobody here can debug. Recorded because it is a real trade, not a free
win: the criterion the split had to meet was more room on **both** sides, and
more room on the legitimate side necessarily means a looser drift budget.

**Declined: a budget on `mean_abs_error`**, which is the shape this entry used
to propose and which the data refutes. P1.3's HDR frame is legitimate at 0.2284
mean abs error — 91% of the frame off by one level, a quantisation boundary the
whole background lands on — and the sprite recolour that must fail is 0.0734.
Any total-error budget loose enough for the first passes the second by a factor
of three, whichever way it is normalised, because total error cannot tell a
level spread over the frame from a patch that is badly wrong. Separation has to
be on **per-pixel magnitude**, which is what a second delta threshold does.
Restricting the sum to pixels already over `max_channel_delta` does work — it
separates WARP from the recolour by 16× — but only 4× on each side, which is
under the bar the split had to clear, and it costs a metric nothing else reads.

**Declined: scoring how _localised_ the differing pixels are.** It is the real
physical difference — WARP's 76 are scattered along quad edges, the recolour's
361 are a 19×19 block — and `differing_bounds` already computes a box. It was
not built because metal's legitimate 2 pixels are adjacent, so density does not
separate that pair; because a real bug need not be contiguous; and because it
needs a second traversal or a running per-region accumulator where the two delta
thresholds cost one comparison inside the existing loop. If the gross budget
ever proves too blunt, this is the next idea, not a new ratio.

**Worth keeping from the entry this replaces:** the original derivation was
built from a table of per-backend figures that did not include D3D12's sprite
scene, because that number had never appeared in a log anybody had read. It was
not wrong about the data it had. That is the ordinary shape of a bound
calibrated on the backends that are easy to measure, and the reason the Metal
and D3D12 jobs upload their diffs.

### Re-affirmed: no Vulkan on macOS, and two facts the original decision lacked

`docs/plan/09-backends-metal-dx12.md`'s 2026-08-05 correction made Apple
platforms Metal-only and cancelled the MoltenVK spike. It was reconsidered on
2026-08-11 and **kept**. The plan doc still carries the reasoning; this records
the reconsideration so the question is not opened a third time, and two things
found while costing it that the original argument did not use.

**`crcbl-vk` cannot enumerate a portability driver at all.** There is no
`VK_KHR_portability_enumeration`, no `ENUMERATE_PORTABILITY_BIT_KHR` on the
instance create flags, and no `VK_KHR_portability_subset` handling anywhere in
`crates/crcbl-vk/src/`. Without those, `vkEnumeratePhysicalDevices` returns zero
devices on macOS whether or not MoltenVK is installed. So "install MoltenVK and
it works" was never true — it is a code change first, and a small one, but it
means no macOS Vulkan support exists to accidentally regress.

**MoltenVK runs on Metal, so a macOS Vulkan CI job adds no GPU coverage.** It
would exercise `crcbl-vk`'s portability against the same paravirtual device
`crcbl-mtl` already uses, not a second driver. That is worth something — it
would have tested the capability model's degradation, since MoltenVK has neither
`DRAW_INDIRECT_COUNT` nor `VK_EXT_mesh_shader` — but it is not the independent
coverage a second backend usually buys, and the original decision's cost (two
GPU paths on the platform with the least CI capacity) stands unchanged against
it.

**If it is ever revisited, the tooling question has a trap in it.** `ash-molten`
statically links MoltenVK and would make a bare `cargo build` sufficient, but it
bypasses the Vulkan loader, and with no loader there are no validation layers —
which `crcbl-vk`'s harness asserts the presence of by design, because a suite
that passes for want of a layer proves nothing. The configuration that keeps
that guarantee is the LunarG SDK, which ships MoltenVK, the loader and the
layers together. Downloading any of it from a `build.rs` was considered and is
the wrong mechanism regardless: it breaks `--offline` and sandboxed builds, runs
in every job including the ones that need nothing, and is invisible to the
`cargo deny` gate this workspace already has.

### glTF import: what the first half left, and what it found upstream

`crcbl_scene::import_gltf` parses; nothing uploads. Written down here because
each item below is a decision or a gap rather than a line of work someone can
pick up from the code alone.

**`gltf` 1.4.1 panics on malformed input, in two places, and both were
reproduced rather than inferred.** `gltf_json::mesh::primitive_validate_hook`
indexes `root.accessors` with the primitive's `POSITION` index directly, after
the derive has already _reported_ that index as out of bounds — so
`gltf::Gltf::from_slice` aborts on a file it was called to reject
(`index out of bounds`). `gltf::binary::Glb::from_slice` computes
`header.length as usize - 12` before checking anything, so a `.glb` declaring a
total length below its own header subtracts with overflow. Both are debug-build
panics and release-build silent wrongness. The consequence is structural:
`crcbl_scene::gltf_check` exists because upstream validation cannot be trusted
to return, and `Gltf::from_slice_without_validation` is what the importer calls.
If either is fixed upstream, the argument in that module's header is what to
re-read before deleting anything — and the checks would still be needed, because
several of them (buffer views inside their buffers, accessor spans inside their
views, indices inside their own vertex array) are things `gltf` never checked at
all. Not reported upstream yet; that is the open action.

**A scaled glTF node produces a non-rigid `GltfInstance::transform`, and
`GpuInstance::transform` requires rigid.** The shader transforms normals with
the matrix's 3×3 part and no inverse-transpose, so a node with non-uniform scale
would light wrongly once uploaded. The importer preserves the scale deliberately
— dropping it here would take the choice away.

**Picked, and it was a fourth option: pass the matrix through and report
non-uniform scale loudly.** `build_render_scene` emits a `Skip` naming the node
and logs it; the object still draws, and only its lighting is wrong. Uniform
scale is exact, because the mesh shader normalises the interpolated normal, so
the defect is confined to genuinely non-uniform nodes. Baking at import was
rejected for the reason above — it loses instancing of one mesh at several
scales — and refusing the node outright would fail files that are otherwise
fine, which is the opposite of what the viewer needs.

**Still owed if a real file needs it:** a per-instance normal matrix (a wider
`GpuInstance`) is the only way to make non-uniform scale actually correct.
Nothing has asked yet; the skip makes it visible when something does.

**Malformed files are `StorageError::Other(String)`.** That is deliberate reuse
— a second error enum beside `StorageError` would make every caller of the
importer match twice — but it means a caller cannot tell "this file is corrupt"
from "the disk is on fire" except by reading the message. The smallest addition
that would change it is one variant, `StorageError::Malformed { key, reason }`,
in `crcbl-store`; it is not there because no caller branches on the difference.
Revisit when one does (a hot-reload path that wants to keep the last good
version is the likely first).

**`data:` URI buffers and sparse accessors are refused, not implemented.** The
first needs a base64 decoder; the `gltf` crate's is only reachable through its
`import` feature, which is the feature that also does blocking file IO and pulls
in `image`, so enabling it is not an option and the decode would be ours.
Blender exports "glTF Embedded" this way, so a real asset will eventually hit
it. Sparse accessors are refused partly on YAGNI and partly because `gltf`'s
sparse iterator has the same `count - 1` underflow the dense one has, in three
more places.

**Not covered by anything yet:** a real-world glTF. Every fixture is
hand-assembled in `crates/crcbl-scene/src/gltf_fixture.rs` — one triangle, one
material, two nodes — which is what makes the malformed cases readable in a diff
but means no Khronos sample, no exporter output and no large file has ever been
through this code. [12-testing.md](12-testing.md)'s anchor list wants a vendored
Khronos subset at P9; that is where the "does it load Sponza" question gets
answered, and until then "it parses glTF" means "it parses the subset the
fixtures cover".

**`deny.toml`'s `multiple-versions` skip list has three stale entries.**
`cargo deny check` and `cargo deny --all-features check` both warn
`unmatched-skip` for `toml_edit@0.22.27`, `toml_datetime@0.6.11` and
`winnow@0.7.15` — the "toml 0.8 via crcbl-store" stack, which no longer exists
now that the workspace pins `toml = "1.1"`. The gate still passes (the check is
`deny` and the unmatched skips are warnings), so this is tidying, not a failure.
Noticed while adding `gltf`; not fixed because `deny.toml` was outside that
task's paths.

### What the backend validation gates do not cover

`crcbl-dx12` and `crcbl-mtl` now assert a clean validation report at every
device test's teardown, the line `crcbl-vk` has always held. What that does and
does not buy, per backend, because the three are not equivalent and a reader
should not assume they are.

**Metal's is genuinely weaker, and not parity.** Metal has no queryable
validation channel. `MTL_DEBUG_LAYER` is read when the framework loads, before
any of this code runs, so nothing in `crcbl-mtl` can turn it on for itself; an
API misuse is printed to stderr and then handled per
`MTL_DEBUG_LAYER_ERROR_MODE`, with no message list, no callback and no count. So
`assert_clean` there asserts two things only: that Metal interposed the layer on
this device, and that no command buffer it submitted ended in
`MTLCommandBufferStatus::Error`. **An API misuse never reaches the second**, and
the first is read from a private detail — the layer replaces the device object's
class with `MTLDebugDevice`, and `layer_wrapped_device` reads that name. If a
macOS release renames the wrapper the assertion fails naming the class it saw,
which is diagnosable, but it is the one fragile assumption in that crate.

**There is no Metal deliberate-violation gate**, unlike the other two, and that
is a finding rather than an omission: a violation aborts the process, so there
is nothing to assert against. The fault half of the teardown guard is therefore
exercised by nothing today — only a real GPU fault would prove it fires.

**`MTL_SHADER_VALIDATION` is asked for and reported, never asserted.** Whether
it took is not knowable in-process. `MTLShaderValidation::Enabled` exists
per-pipeline-descriptor in `objc2-metal` 0.3.2 and is the programmatic
alternative, but it still cannot be read back.

**Metal's extra validations are off**: `MTL_DEBUG_LAYER_VALIDATE_LOAD_ACTIONS`
and `_STORE_ACTIONS` catch reading an attachment nothing wrote and are the
reasonable next step once the base layer is known green. `_UNRETAINED_RESOURCES`
is irrelevant here — the command buffers are retained.

**D3D12's info queue has a message-count limit (1024 by default) and nothing
clears it any more**, now that `diagnosis` reads rather than drains — which is
what stops a validation error quoted in a `HalError` from consuming the one that
should fail teardown. A healthy run stores zero messages, so this is
theoretical; a device producing more than the limit would start dropping _new_
ones. `attach` could raise it with `SetMessageCountLimit`. Left alone as
premature.

**Never executed anywhere:** whether the 73 D3D12 and 71 Metal device tests are
actually clean under their layers, whether the D3D12 gate's message really names
`CreateCommittedResource`, whether the Metal suite survives `abort` on warnings,
and whether the paravirtual device supports shader validation at all. The layer
itself is confirmed present — a `main` run reports `debug layer=true` on
`windows-latest` — but none of these crates executes on this machine.

### Metal's debug layer is on `nslog`, and `assert` is the follow-up

The first run of the Metal suite under `MTL_DEBUG_LAYER` set both mode variables
to `abort`, and all 71 tests died with

```
Assertion failed: (0), function MTLGetEnvCase, file MTLUtils_Internal.h, line 100.
```

**That was not the layer finding 71 problems.** `abort` is not a value Metal
accepts, and Metal does not ignore a value it does not recognise —
`MTLGetEnvCase` asserts, so every device creation aborted before any test ran.
The accepted set is `ignore`, `assert`, `nslog`.

Both are `nslog` now, which reports each finding to stderr and lets the process
continue. That is what a first run needs — the suite has never executed under
this layer and the job is to read what it says. **It also means an API misuse
does not fail anything today**: it is a line in a log nobody's assertion reads.
`assert` is where this should end up once the log is clean, and moving it is the
follow-up. Until then the enforced half on Metal is what the backend can observe
in-process — that the layer interposed, and that no command buffer ended in
error — which is already recorded as weaker than Vulkan's and D3D12's.

### `a_copy_d3d12_cannot_place_is_refused_by_name` provokes a real layer error

Recorded because it is the one D3D12 test whose validation report is dirty on
purpose, and a future reader will otherwise try to "fix" it.

The refusal it asserts is **D3D12's own**: the seam does not reject a 252-byte
row pitch before the call, so the copy reaches the driver and the debug layer
says so. It calls `defuse()`, exactly as `crcbl-vk`'s gate tests decline to call
`Headless::finish`. The first run with teardown assertions enabled found this
and nothing else across all 75 tests — one deliberate provocation, correctly
flagged.

### Metal GPU validation changes the UI frame, which points at an out-of-bounds read

Found on the first run of `MTL_SHADER_VALIDATION=1` against the render e2e, and
it is the most interesting thing the validation work turned up.

With shader validation on — Metal logs it as `Metal GPU Validation Enabled` —
the `Ui` scene came back with **five distinct colours against a floor of six**,
so something that draws stopped drawing. The floor is not arbitrary: it is the
measured count `run-cross-backend-e2e.sh` records for that scene, and the same
frame passes on vk, wgpu and D3D12.

**Why this reads as a real defect rather than a quirk of the switch.** GPU
validation traps an out-of-bounds shader access instead of letting it return
whatever happened to be in memory. A frame that changes when the trap is turned
on is a frame that was relying on the untrapped read. The obvious shapes are an
index past a bound array or a sampler reading outside its texture in the UI
path.

**What narrows it usefully:** the Metal HAL suite runs _with_ shader validation
and is clean at 71 tests and zero failed submissions. So whatever this is lives
in the renderer's path — `ui_pass`, `ui.slang`, or what the pass binds — and not
in `crcbl-mtl` itself. The cube and sprite scenes fail the same step, but their
failure may be a consequence of the UI test's rather than independent; nobody
has separated them.

**Not diagnosed here**, because it needs a Mac: nothing in this repository can
run Metal, and the CI job's output is a colour count rather than a message —
Metal names no offending access in `nslog` mode for this. The step therefore
runs with API validation and **without** shader validation, so it keeps gating
the picture; turning `MTL_SHADER_VALIDATION` back on in
`.github/workflows/ci.yml` is how the investigation starts, and the first thing
worth trying is `MTL_DEBUG_LAYER_ERROR_MODE=assert` to see whether Metal will
name it.

## The material lookup moved to the fragment stage, and what that probe learned

`mesh.slang` reads binding 6 in `fragmentMain` now. `VertexOutput` carries
`nointerpolation uint material : TEXCOORD0` and the vertex stage writes
`vertex.color` untinted. The move was made on its own, with no texture beside
it, because a flat integer varying is the third integer this file hands across
the stage boundary and the other two — `SV_InstanceID`, `SV_VertexID` — both
lowered differently per target and were both caught by rendering rather than by
reading the emitted code.

**One edit this needs is outside the slice's paths and was not made.** The
material table's `BindGroupLayoutEntry` is `ShaderStages::VERTEX`, which is now
the one stage that does not read it:

- `ForwardRenderer::mesh_layout`'s binding 6 in
  `crates/crcbl-render/src/forward.rs`.
- The same binding in the layout `crcbl-vk`'s `vk_e2e/depth_probe.rs` builds for
  itself.

Both must become `ShaderStages::VERTEX.union(ShaderStages::FRAGMENT)`. **The
union, not `FRAGMENT` alone**, and that is a Metal constraint rather than
symmetry: Slang's Metal backend materialises every global in every entry point
(see "Slang's Metal backend materialises every global shader parameter…"), so
`vertexMain` in `msl/mesh.metal` still takes `materials [[buffer(6)]]` whether
it reads it or not. Verified with the change applied in a scratch worktree —
`vk` and `wgpu` are green and bit-identical with the union.

**Until it lands, `wgpu` cannot draw the cube at all.** Not a validation
warning: `Device::create_render_pipeline, label = 'forward mesh'` fails with
"Shader global ResourceBinding { group: 0, binding: 6 } is not available in the
pipeline layout / Visibility flags don't include the shader stage", and
`crates/crcbl/tests/run-render-e2e.sh` on `CRCBL_GPU=wgpu` reports
`3 tests run: 2 passed, 1 failed`.

**Vulkan is looser, and where it is loose depends on who is listening.** The
pipeline is created and draws the correct frame either way, but the layer emits
`VUID-VkGraphicsPipelineCreateInfo-layout-07988` — which `run-render-e2e.sh`
only logs, and which `crcbl_vk::debug` escalates to a panic. So
`crates/crcbl-vk/tests/run-vk-e2e.sh` reports
`12/74 tests run: 11 passed, 1 failed`, failing
`depth_probe::reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not`
and cancelling the rest — the depth probe's layout, not the renderer's.

**A workspace `cargo nextest run` catches neither** — it was
`2980 tests run: 2980 passed, 168 skipped` with the shader changed and the
layouts not. Nothing below the GPU seam checks a bind-group layout's visibility
against the module bound to it.

### What the probe found, which is not what it was pointed at

**Every one of the four targets emits the flat qualifier**, read out of this
crate's own regenerated artifacts with slangc 2026.14: SPIR-V decorates both
sides `Flat`, WGSL writes `@interpolate(flat) @location(3)`, MSL puts `[[flat]]`
on the fragment's `[[stage_in]]` struct — which is where Metal reads it, not the
vertex output struct — and DXIL's input signature lists `TEXCOORD 0` as
`nointerpolation`. No divergence to report.

**Dropping `nointerpolation` does not make a golden go red, and cannot.** Tried
it, on both backends that run here:

- **SPIR-V repairs it.** Slang drops `Flat` from the vertex _output_ but keeps
  it on the fragment _input_, which is the decoration that decides
  interpolation, so `vk` draws a bit-identical frame:
  `golden cube on vulkan — 256x192: 0 pixel(s) differ at all (0.0000%)`.
- **WGSL refuses it**, and does so before any frame — naga rejects the module
  with "`@interpolate(flat)` must be explicitly specified for integer I/O". That
  is caught by `crcbl-shaders`' own
  `wgsl_validation::every_committed_wgsl_artifact_validates` on a machine with
  no GPU, which is a better gate than a golden anyway.

**And the cube scene could not detect a wrong interpolation _mode_ even if one
existed**, which is worth knowing before trusting it for the next varying. The
material id is constant across every primitive — all three vertices of a
triangle belong to one instance — so flat and linear interpolation of it agree
by construction, and there is no "fragment between two vertices" that could
resolve a third row. What `nointerpolation` actually buys here is what
`sprite.slang`'s `sheet.z` note says it buys: an exact integer instead of one
that arrived through a float unit and truncates a row early.

**What the golden does detect is a fragment resolving the wrong row**, which is
the failure a texture fetch would produce and the reason the scene's two
pyramids are in unlike colour families. Pinned by making the fragment stage read
a fixed `materials[0]` and rendering:
`256x192: 4105 pixel(s) differ at all (8.3516%), max channel delta 105, 4105 over tolerance (8.3516%), mean abs error 2.0736, rmse 11.1112, ssim 0.991305 — failed: TooManyDifferingPixels`,
the same line on `vk` and on `wgpu`.

**`msl` and `dxil` were not rendered.** Nothing here runs Metal or D3D12, and
they are the two whose lowering this probe least exercises. Their artifacts were
read and carry the right qualifier; CI is the only thing that can say the frame
does too.

### D3D12 allow-list: two entries, and what retires each

`crcbl_dx12::debug::ALLOWED` is a table of message ids the validation gate
passes over, each with the argument for it, consulted only for
`Severity::Warning` — the same id arriving as an error or corruption fails as
before, and allowed messages are counted and named so "silent" and "answered
for" stay distinct.

**Id 820, `CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE`.** The layer files two
different things under this number: "you passed no optimized clear value"
(advisory) and "the value you promised is not the one you cleared to" (a real
defect). This backend passes `None` for `pOptimizedClearValue`, so today only
the first can occur. **The entry is safe only while that stays true** — the
moment `Device::create_image` passes a value, this allowance hides the defect.
Removing it needs a clear-value field on `crcbl_hal::ImageDesc` and a decision
about what a pass that clears to a different colour should do, since the promise
is per-resource and the colour arrives per-pass at `begin_render_pass`.

**Id 1361, `CREATE_SAMPLER_COMPARISON_FUNC_IGNORED`.** `create_sampler` writes
`D3D12_COMPARISON_FUNC_ALWAYS` as filler when `SamplerDesc::compare` is `None`.
The right value is `D3D12_COMPARISON_FUNC_NONE` — zero, the enumerant that says
exactly what the seam means, and the old comment claiming zero is "a sampler
feedback value" was wrong and is corrected. **Not switched**, because this
backend asks for `D3D_FEATURE_LEVEL_11_0` and it is not established which
runtimes accept a zero in a classic `D3D12_SAMPLER_DESC` rather than filing
`CREATE_SAMPLER_INVALID`; that would trade an advisory for an error on machines
nothing here can test, and the CI runner being Windows Server 2025 would prove
nothing about older ones. Settle that and this entry goes.

### Verified, not a problem: the D3D12 info queue does not leak across tests

Recorded so it is not re-investigated. `debug::read_queue` reads from index 0
and never clears, which looks like it would let one test's messages fail
another's teardown. It does not: every device test opens its own `ID3D12Device`
through `device::tests::open_device`, and `debug::attach` clears _that device's_
queue at creation, so a report means "since this device was created" by
construction. Evidence: in run 31454155654, message 597 — the gate's own
deliberate violation — appears exactly once, inside the expected panic of the
test that raises it, and that test passed.

## The base-colour page is still `ArrayPages`; the wgpu blocker under it is gone

`docs/plan/03-gpu-driven-rendering.md` §3.2's texture half is implemented as one
`Texture2DArray` page — `crcbl_render::forward`'s `base_color_page`, bound at
`mesh.slang` binding 7, with `GpuMaterial::base_color_texture` selecting a
layer. `BindingModel::Bindless` — one runtime-sized array _of descriptors_,
indexed per fragment — is still not implemented, but the reason has changed and
the old reason is worth not re-deriving.

**What was blocking it is fixed.** `crcbl-wgpu` could not fill an array binding
at all: `create_bind_group` keyed every entry on `binding` alone and
`BindGroupEntry::array_index` appeared nowhere in the crate, so a bindless slice
would have selected the bindless path on wgpu (it reports `DESCRIPTOR_INDEXING`)
and then failed to build the group. `crates/crcbl-wgpu/src/binding.rs` now does
the bucketing, and
`a_wgpu_shader_reads_the_array_element_the_bind_group_put_in_each_slot` reads
both elements out of a two-texture array on lavapipe. All four backends honour
`array_index`.

**What is left is above the seam, and it is a real slice.** Nothing selects
`BindingModel::Bindless` — `crcbl_render::forward` builds one page
unconditionally. Going bindless means a descriptor array whose length is a
runtime bound, a per-material index into it that is a descriptor slot rather
than a layer, `BindingFlags::VARIABLE_COUNT` and `BindGroupDesc::variable_count`
actually being used (see the wgpu entry below — that backend ignores the second
one), and a `mesh.slang` that declares the array. The two paths then have to
render the same frame, which is the observable.

**What bindless buys**, so the case stays on the record: a page is one image, so
its layers share an extent, a format and a mip count. A descriptor array lifts
all three, which is what real imported content needs. Until then the engine has
one page of one size, which is enough for the observable and not enough for a
game.

## What `crcbl-wgpu`'s binding work still leaves, after the refusals landed

The three silent drops found while writing `crates/crcbl-wgpu/src/binding.rs`
are fixed — `BindingFlags` and the `VARIABLE_COUNT` ordering rule are checked at
layout creation, `variable_count` is checked against the layout's variable
binding and the entries supplied, `count: 0` is refused, and
`create_bind_group_layout` is error-scoped. What is left:

- **`update_bind_group` is still `Unsupported` on wgpu** — WebGPU bind groups
  are immutable and there is no update-after-bind path, so the seam's streaming
  bindless write is create-only here. It is the other half of what `array_index`
  exists for: a page of descriptors that grows as content loads has to be
  rebuilt rather than written into.

**Not verified: the browser.** Binding arrays are a native-only wgpu feature and
`DESCRIPTOR_INDEXING` will be absent under WebGPU, so the array-shaped tests
take their skip branch there. No browser run was made, and the skip branch means
a wasm regression in this code would not be observed by anything. The refusals
that do not need an array layout — the flags gate, `count: 0`, the in-band
layout error — run on every adapter, so that half is not skip-shaped.

## What the shadow LOD bias left, and two stale docs

- **`FrameUniforms::lod_params` is now dead in the uniform block on both
  shaders.** `mesh.slang`'s doc says "that file's amplification stage is the one
  that reads it" while `mesh_cluster.slang` says "read by nothing here since
  hysteresis landed". Neither reads it. Removing it is a uniform-block change
  and therefore an artifact regeneration; the docs should stop disagreeing
  either way.
- **The shadow atlas's contents were never compared for a DAG mesh.**
  `the_shadow_atlas_is_written_rather_than_left_at_its_clear_value` uses the box
  scene, which has no DAG, so the bias's effect on actual atlas depths is
  inferred from the cut readback plus bit-identical colour frames rather than
  measured on the atlas itself.
- **`SHADOW_LOD_BIAS` is one constant for every cascade.** Topic 18 suggests
  +1/+2 stepping by cascade, and a per-cascade factor would be sound — the
  monotonicity argument only needs one constant per _pass_, and each cascade is
  its own pass with its own `DrawGen` and its own history. Not done because
  nothing yet shows the near cascade wants a different figure from the far one.

## Mobile input: what it decided, and the bugs it found

Flappy taps to flap and breakout's paddle follows a finger. **Horde's half has
since shipped too** (corrected 2026-08-15): `apps/horde/src/controls.rs` gives
it a floating `crcbl::ui::TouchStick` and the engine's `PauseControl`, wired
through `Binding::Virtual`, so the heading this entry carried — "horde does not"
— and its paragraph calling horde "still keyboard-only" were both out of date,
as was the claim that on-screen controls were topic 19's post-MVP row. What
survives is everything below: the decisions, and the bugs the survey did not
predict.

**Asteroids is deliberately excluded**: rotate, thrust and fire are three
concurrent controls with no room on a phone screen, and every layout for that
shape is worse than the keyboard one; it would want a redesigned control scheme,
not buttons bolted on.

**The survey that preceded this work was wrong about the cheap half.** It said
single touch already reached the engine and that only bindings and page
ergonomics were missing. Touch reached the _shell_, but the loop swallowed every
pointer event — `Pending::observe` returned `Handled::Loop` for
`ShellEvent::Button` and `HostedGame` had only `key_event`, so no binding in any
sample could ever have fired. `HostedGame::pointer_event` and the routing in
`Loop::frame_body` are new. Worth remembering as a shape: "the events arrive"
was true and told us nothing about whether anything consumed them.

### Decisions taken

- **`Binding::PointerPosition { axis }` feeding an `Axis1`**, normalised to the
  surface at −1…+1 with +X right and +Y up. Not an `Axis2`, which would put a
  _place_ in the same value shape as `Binding::Wasd`'s _direction_ — handed
  `(0.5, 0.0)` a consumer cannot tell "half way right" from "moving right at
  half speed". The pixel→normalised step happens once in the engine loop; the
  surface→world step stays in the game, because the play field is not the
  canvas.
- **An absolute binding replaces the relative ones within one `Axis1`** rather
  than summing: a place plus a rate is neither.
- **The pointer wins on the tick it moves; the keyboard owns every other tick.**
  A resting mouse is not a command, so arrow keys still work on a desktop with a
  cursor over the field, and a lifted finger has not asked for anything. That is
  what `Axis1Action::pointer_moved` exists for — the edge an absolute source has
  and a relative one does not.
- **A pointer that leaves keeps its last position.** A leave carries no
  coordinate so nothing is fed to the map at all, which is why the paddle stays
  put instead of walking to the middle on every tap — a touch pointer is
  destroyed on `pointerup`, so a lift is a leave.
- **Breakout's launch is bound to the pointer too.** A lost life returns to
  `WaitingForLaunch` with no menu on screen, so without it a phone could move
  the paddle and never serve again.
- **The viewport meta is unchanged and zoom is not suppressed.** `layout.html`
  is shared with every prose page, iOS Safari has ignored `user-scalable=no`
  since iOS 10, and `touch-action: none` on the canvas already kills double-tap
  zoom — which is the actual complaint. Suppressing it would be an accessibility
  regression that does nothing on the platform it targets.

### Three touch bugs the survey did not predict

All were real, all are fixed, and all three are invisible to a mouse:

- **`pointercancel` was unhandled.** The OS taking over a gesture leaves the
  button down forever, and a held button raises no _edge_ — so the tap silently
  stops working.
- **Non-primary contacts were forwarded** into a seam with no contact ids, so a
  second finger read as the first one teleporting.
- **A tap that opened and closed inside one pump dropped its release** — which
  on a phone is every tap. The first tap worked and the second did nothing.
  Found by writing the test first and watching it fail.

### What is left

- **The gate now drives touch, and closing that gap found the feature did not
  work.** Group F in `web/tools/browser-e2e.mjs` uses `Input.dispatchTouchEvent`
  and `Emulation.setTouchEmulationEnabled`. It caught three defects that shipped
  green, and the first two are worth remembering as a class:
  - **A tap on a menu button did nothing, so no demo could be started at all on
    a phone.** For a touch pointer the browser fires `pointerleave` in the same
    pump as `pointerup`; the shim reported it as a focus loss,
    `Pending::pointer` became `None`, and `PointerCapture::resolve` hit-tested
    the release against a position that was already gone. The identical click
    with a mouse worked.
  - **`pointercancel` handling was inert.** The spec gives it `button: -1`,
    which became a `PointerButton::Other` the engine ignores, so the release was
    dropped and the game stayed holding the button — exactly the failure the
    handler existed to prevent. The shim remembers the button that went down.
  - **The coarse-pointer copy swap did nothing**: `.key-row { display: flex }`
    ties on specificity with `.touch-only`/`.pointer-only` and won on source
    order, so every desktop saw the keyboard row and every phone saw `Esc`,
    `F11` and `F3`. Both blocks only hide now, with the class doubled.

- **Both pointer defects were fixed in the web shim, and both are really the
  engine's seam.** A release that arrives with a leave, and a cancel that names
  no button, are what any touch platform does — an Android or iOS backend would
  hit the same two. The durable fix is in `PointerCapture::resolve` and
  `Pending::observe`, not in `shell.js`. **Decision wanted** before a native
  mobile backend is attempted.
- **`touch-action` coverage is Chromium-under-emulation, not a phone.** The
  check is behavioural rather than a stylesheet read — with the declaration
  overridden to `auto` the same drag delivers 1 move of 8, raises a
  `pointercancel` and scrolls the page 233px, and all three are asserted. But it
  proves Chromium's gesture recogniser honours the rule, not a real phone's.
  **Nothing here has run on a phone.**
- **`web/tools/browser-e2e.mjs` is 2107 lines.** Group F is self-contained apart
  from `check`/`evaluate`/`until`/`hud`, so the seam is obvious; splitting it
  means handing a driver's closures to a module. Worth doing, not done.
- **The paddle is read out of the rendered frame, not the HUD.** Breakout's HUD
  carries `Ball x` and `reset_ball` pins the ball while it is unlaunched, so no
  logged value moves with the paddle; the check counts blue pixels in the bottom
  band instead, measured 1:1 against the touch x. Worth knowing before someone
  looks for a HUD line that does not exist.
- **Every demo with touch controls has a PAUSE button**; asteroids and hud stay
  out on purpose. It is `crcbl::engine::PauseControl`, shared rather than three
  copies: size, margin, corner, palette, the appear-condition, the hit-test and
  the tap-take are one piece of knowledge, and moving the corner would otherwise
  be three edits. It lives in `crcbl` rather than `crcbl-ui` because it needs
  `TouchUpdate`/`PointerUpdate` and owns the extent, so a sample needs no extent
  field and no pixel conversion of its own.
- **The finger pressing the pause button is also the emulated pointer**, so
  without a guard it flapped in flappy and served in breakout on the way to
  pausing. `PauseControl::takes_pointer` answers that, and it forced the loop to
  deliver **contacts before the pointer** — a sample cannot say "that pointer
  press was my control's" until it has heard about the finger, and the first tap
  of a run always arrived pointer-first.
- **A mouse click on the drawn pause button is swallowed but does not pause.**
  The keyboard has Escape, so this is cosmetic rather than a lockout, but it is
  an inconsistency someone will notice.
- **The seam carries contacts now.**
  `ShellEvent::Touch { contact, phase, position }` with `ContactId` and
  `TouchPhase`, routed to `HostedGame::touch_event`. Decisions taken:
  - **A touchscreen produces both streams**: every contact as `Touch`, and the
    _primary_ contact additionally as the emulated pointer. That is the
    browser's own compatibility rule, and it is now an obligation on any backend
    that sets `ShellCaps::TOUCH`. The engine deliberately does **not**
    synthesize the pointer itself — the browser already does it correctly,
    including "a second finger does not move the mouse", and doing it again a
    layer up gives two answers to "where is the pointer" that agree until they
    do not. A game bound only to `Binding::MouseButton` sees exactly what it saw
    before.
  - **A contact id is unique among contacts that are down together and reused
    afterwards.** State keyed on one must be dropped when the contact ends or
    the next finger inherits it — said on `ContactId`. The numbering is the
    platform's, passed through rather than renumbered.
  - **`Cancelled` is not `Ended`.** The system took the gesture; the position is
    the last one the platform knew rather than a place anyone chose, so a
    consumer undoes rather than commits. All they share is that the contact is
    over, which is `ends_contact()`.
  - **A menu does not claim contacts.** A menu is hit-tested against a position
    the loop knows; an on-screen stick is the game's own widget, and handing the
    game fewer contacts than the screen has takes the decision away from the
    only code that can make it.
  - **`Pending` lost `Copy`** (contacts are a `Vec`, appended rather than
    merged, because a tap is a `Began` and an `Ended` in one pump). No app
    needed a change.
  - **Contacts carry no pressure, radius or tilt.** No consumer, and the
    platforms disagree about what they mean.

- **No desktop backend implements touch, and none claims to.**
  `ShellCaps::TOUCH` is clear on all of them, and `caps.rs` names the path each
  would have to write — `XI_TouchBegin`, `wl_touch`, `WM_POINTERDOWN`, `NSTouch`
  — and says the bit is clear because the code is not written, not because the
  platform lacks it. `HeadlessShell::touch` returns `Unsupported` without the
  cap, so a test cannot script a finger on a shell modelling a touchless
  backend.
- **The browser gate reads contacts out of the engine's Debug log**, because no
  demo draws an on-screen control yet and the pointer is the only other place a
  contact surfaces. That is a deliberate coupling to `ShellEvent`'s `Debug`
  shape: it will fail loudly if the variant is reshaped, which is the intended
  behaviour, but it is not a contract anyone declared.
- **Horde's on-screen controls landed**: `crcbl_ui::touch`'s `TouchStick` and
  `TouchButton` are widgets acting as a virtual device, and `Binding::Virtual`
  is what `ActionMap` binds them through — topic 19's design, built. Decisions:
  - **The stick floats**, appearing where the thumb lands. Held two-handed every
    fixed position is wrong for some grip, and a floating origin reads exactly
    `(0, 0)` on the frame the finger lands, so the widget needs no dead zone.
  - **A stick's deflection lands in the same accumulator `Wasd` uses**, so a key
    and a stick sum inside the unit disc rather than to twice the speed.
    `Binding::Virtual` on an `Axis1` is inert and a test pins that: a stick has
    no way to choose one axis.
  - **First come, first served, and a second finger on a held control is
    refused** and passed to the next control. The button is offered first
    because it has a rectangle and can decline; the stick claims whatever is
    left, so the whole field is somewhere to put a thumb.
  - **Controls appear once a contact has arrived**, not on `ShellCaps::TOUCH` —
    a desktop with a touchscreen sets that too. A desktop player sees nothing
    change, and no golden can move because a headless golden never touches
    glass.
  - **`Cancelled` and `Ended` agree for a stick** and the code says so in one
    arm: a stick has nothing latched, its value _is_ the command, and it is zero
    the moment the contact goes. The distinction is real for the button, which
    is where the code branches.
  - **Horde got a PAUSE button as well as a stick**, because pause is the loop's
    and not a game action, so a phone could start a run and never stop it. It
    needed `HostedGame::take_pending_pause`, modelled on
    `take_pending_frame_limit`.

- **Both fixed: a second finger can work a menu while the first holds a control,
  and a refused control re-grabs.** The lockout fix landed in the _menu's_
  hit-testing, driven from the loop's contact routing — contacts are a second
  device driving the same widgets, exactly as `MENU_ACTIVATE_KEY` is. Not in the
  shim, which would fix one platform and contradict the browser's own "a second
  finger does not move the mouse"; and not in the pointer routing, which is
  `7ce0f2b`'s decision and stayed untouched. A contact is the menu's only if its
  `Began` latched a button, one at a time, and the contact carrying the emulated
  pointer is skipped — without that skip a one-finger tap fires twice.
- **That primacy is re-derived in the loop rather than carried by the seam**:
  the contact down while no other is, which is the Pointer Events rule the shim
  already documents. **The durable alternative is a `primary` flag on
  `ShellEvent::Touch`**, which touches `crcbl-shell` and `shell.js`. Worth a
  decision.
- **Horde's in-frame HUD still says "WASD to move" on a phone.**
- **`web/engine/shell.js` is the one web file prettier would rewrite**, and it
  was already so before any of this work — `demo.js`, `style.css` and the
  templates all pass. Reformatting it is a whole-file diff unrelated to whatever
  change happens to touch it, so it is hand-matched to the existing style
  instead, and the drift grows with every edit. Worth deciding once,
  deliberately; measure it with `prettier` rather than trusting a figure written
  here, because it moves. Note the local wrapper prints
  `Prettier: All files formatted correctly` **while exiting 1** — read the exit
  code, not the line.

## What `crcbl lod` left owed

- **`preview` needs a scene that can draw an imported mesh, and none exists.**
  `crcbl::screenshot::Scene` is a closed enum of three built-in scenes and
  `OffscreenSetup::open` takes one of them, so "render this glTF at this level"
  is a new capability rather than a delta on `crcbl screenshot`. That is the
  prerequisite, and it is also what a golden frame per LOD level would need.
- **`tools/cook-clusters.rs` duplicates `ClusterDag::cook()`.** The
  transcription now lives on the type in `crcbl-scene`, where both callers can
  reach it; the example still has its own private copy and should call the
  method instead. One file, mechanical.
- **`stats` only reports nodes the scene draws.** A mesh no node instantiates is
  invisible to it; `--node` reaches any node in the table.
- **The importer's skip warnings are invisible from the CLI**, which installs no
  logger — a primitive dropped for not being a triangle list is silent, and only
  total emptiness is caught.
- **`gen` writes one DAG per file**, so a multi-primitive mesh takes one run per
  primitive.
- **The asset-key rule bites on the command line.**
  `crcbl lod stats "my model.gltf"` is refused because a file name must match
  `[A-Za-z0-9._-]`. The message explains it, but for a bake tool pointed at
  artist output that is real friction and worth revisiting.
- **The stall fixture is empirical**: `grid(56)` stalls twice on this decimator
  where `grid(48)` and `grid(64)` do not. The test says so and says to pick
  another fixture rather than delete the check if the decimator changes.
- **`crates/crcbl-cli/src/args.rs` is 1592 lines** and its own docs say to take
  `clap` and delete it "when this file passes roughly two hundred lines of
  `match`" — a line it was well past before this added 285 more.

## What the light list left owed

The list, the froxel grid and the sun-as-a-row landed; the decision below it is
recorded in `docs/plan/18-render-features.md`. What is left:

- **Both closed**: `Scene::Spot` draws a cone and asserts its shape by pixels,
  and the froxel bound is a cone as well as a sphere (144 froxels to 91 on a
  narrow spot, every golden unmoved).
- **The CLI's `--scene` knows only `cube`, `sprite` and `ui`**, so `Dunes`,
  `Lights` and `Spot` are reachable only from the test suite, and
  `run-cross-backend-e2e.sh` therefore does not cover them.
- **`spot_cone` is a linear ramp in cosine space, not a smoothstep** — worth
  knowing before someone "fixes" the falloff to match a description that was
  never in the code.
- **Spot shadows landed; point-light shadows are what is left.** The atlas is a
  fixed grid and the rule for which lights get maps is `shadow::Selection`. See
  "What punctual-light shadows left owed" below.
- **`mesh.png`, `mesh_ortho.png` and `mesh_second.png` differ from what this
  machine's lavapipe produces** by exactly one channel level across 80–96 % of
  pixels, comfortably inside tolerance. **Pre-existing** — identical before and
  after the light work — so those three were presumably blessed on a slightly
  different driver. `mesh_clusters.png` matches exactly.
- **`mesh.png` is written by two different tests in one run**, so a bless is
  last-writer-wins and the file is not a stable baseline. That is why the
  bit-identical check above compared captured output rather than re-blessing.

## `GpuInstance::transform` claims to be rigid and is not

Its doc says "Rigid (rotation + translation), so its upper-left 3×3 transforms
normals correctly without an inverse-transpose", declared
character-for-character in every `.slang` file that reads a transform — `mesh`,
`cull` and `draw_gen` — and held there by an equality test. **It is false** for
two shipped scenes: the `Scene::Ao` trough is `scale(6, 2, 1.6)` and
`Scene::Spot`'s floor is a cube scaled `6 × 0.2 × 6`.

The culling half of that lie is fixed (see the cluster-radius entry). The normal
half is not, and it is a different failure: a non-uniformly scaled instance
shades with a normal that is not perpendicular to its surface, which reads as
lighting that is subtly wrong rather than geometry that vanishes.

Two ways out, and it is a decision rather than a fix:

- **Make the claim true** — forbid non-uniform scale in an instance transform
  and push it into the mesh. Cheap to state, expensive for callers, and the two
  scenes above would have to change.
- **Make the code true** — carry an inverse-transpose, or renormalise after
  transforming. Costs bytes in `GpuInstance` or work in the shader, and touches
  all five declarations plus three shaders' artifacts.

Nothing observable is known to be wrong today, which is why this is a decision
and not an incident. But it is a documented contract nothing enforces, and the
culling bug is what happens when one of those goes unexamined for long enough.

## An audio test fails intermittently under a loaded full-suite run

`apps/asteroids`'
`audio::tests::the_engine_is_one_looping_voice_that_outlives_its_buffer` fails
inside a full `cargo test` with "the engine's release block was cut", and passes
in isolation and on most full-suite runs. This entry previously said it had
happened once and asked for a second sighting to make it a pattern. **The second
sighting has arrived**: it fired again during a `cargo test --all-features`
sweep, with the same assertion, and the very next run of the same command was
green (`3402 passed`). Nothing near it had changed either time.

Both sightings were under a **full workspace** run rather than a targeted one,
which is the only correlation there is so far. That points at a timing
assumption rather than at the audio maths: the assertion is about a release
block surviving, and a suite that is loading every core is where a voice that
depends on wall-clock progress would get cut. Nobody has read the test with that
hypothesis in hand, and it is a cheap thing to do — the alternative is that it
keeps costing a random CI run.

Not diagnosed, and deliberately not "fixed" by rerunning: this is now the second
intermittent failure in the tree whose entry says "recorded so a second sighting
is a pattern", and both have since repeated.

## One primitive shades black on Windows lavapipe, intermittently

`depth_probe::reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not`
fails on `vk e2e (lavapipe, windows)` with `[0, 0, 0, 255]` at the centre pixel,
while the same test passes on Linux lavapipe and radv.

**Five occurrences**: `e8d3dab`, `0b10832`, `e875e44`, `c22f7d5`, and `bfc270c`
**twice in a row** — the first time a re-run has not cleared it on the first
retry. A third attempt on that commit was green, and the change under it cannot
reach this test (`depth_probe` never constructs a `ForwardRenderer` and its only
mentions of the scene API are comments), so it is still the flake and not a
regression. But it now costs multiple CI runs per push, where it used to cost
one.

**The diagnostics have narrowed it to one primitive, and the arithmetic is
exact.** On both `bfc270c` attempts the frame-wide count read **9776 pixels
differ from the corner**. The far quad reaches 60 pixels from the centre and the
near one 34, so their footprints are 14400 and 4624 — and 14400 − 4624 is 9776
exactly. A correct frame measures 14400, confirmed locally by forcing the
assertion.

So the failing picture is the far quad drawn normally with a hole punched in it
exactly the near quad's size. **The near quad wins the depth test** — which is
why the blue does not show through — **and then shades to the clear colour.**
That is a shading failure on one primitive. It is not a lost submission, not a
mis-projection, not a readback race, and not an empty frame, which is what this
entry claimed for its first four occurrences.

What the earlier diagnostics ruled out, each from the job log alone:

- **No `0xa5` anywhere**, so the copy landed and the bytes are a real frame's.
- **`wait_idle: Ok`**, so the device was never lost.
- **`validation: enabled=true, 0 error(s), 0 warning(s)`** — and that silence is
  evidence rather than an absent channel, because the same log carries a VUID
  from `validation_gate`'s deliberate violation.

**What would settle it**, and none of it has been done: read the depth
attachment back to confirm the near quad wrote depth (it needs `TRANSFER_SRC` on
the shared `scene_depth` descriptor, which is a production change for a test's
benefit — see the assertion's own note); or dump the near quad's fragment
inputs; or bisect lavapipe's version on that runner. The Linux legs have never
reproduced it, so this is a Windows-lavapipe-only investigation.

The rest of this entry records how the diagnosis got here, and stands:

- every readback destination is filled with `harness::POISON` before it is
  polled, so a copy that never landed reads back as `0xa5` instead of as the
  zeroes a legitimately black frame also produces. `depth_probe::render_probe`
  asserts on that case by name, ahead of the colour assertions — `POISON` is
  above the threshold the first of those uses, so left to them it would have
  been diagnosed as a projection failure.
- `Headless` has a `Drop` that fires only when the thread is panicking and
  prints what `finish` would have asserted on: `wait_idle`'s answer, naming
  `VK_ERROR_DEVICE_LOST` when that is what comes back, and the validation
  report's counts and summary. `finish` is the last line of a test, so before
  this the device's verdict and the layer's were discarded on exactly the runs
  that went red.
- `harness::instance` installs `crcbl_core::log::init_logging`, so the debug
  messenger's `log::error!`/`log::warn!` reach stderr when the layer emits them.

**Two claims in the previous version of this entry were wrong.** Both were ours
and neither survived checking:

- The Linux leg does **not** take "single-digit seconds". Read out of the two
  jobs' own logs, the same 95 tests take 142.05s on `vk e2e (lavapipe, windows)`
  against 32.35s on `vk e2e (lavapipe)` — 4.39x, not the order of magnitude the
  entry implied. The single-digit figure was the `run-render-e2e.sh` step, which
  is a different suite.
- "An empty frame is what a readback that outran the render looks like" is
  contradicted by those timings: the failing frames were not slow, and the two
  failing runs' totals bracket the green one. The theory is dropped rather than
  carried forward; it was never more than a shape.

**Not a regression from the cluster-radius fix**, and that is checkable rather
than assumed: `depth_probe` draws through `mesh.slang`'s vertex and fragment
stages with no amplification stage, so `mesh_cluster.slang`'s `cluster_survives`
is not compiled into the path at all. Every instance it draws is at identity, so
the radius scaling is `1.0` besides.

## The vk e2e suites report nothing the validation layer says

Fixed for `crcbl-vk`, still open everywhere else, and worth stating on its own
because it is a hole in a suite whose module doc opens with "Every test asserts
a clean validation report".

`crcbl_vk::debug::messenger_body` routes every message the layer emits into
`log::error!`/`log::warn!` as well as into the counting sink. Nothing in the vk
e2e test tree installed a `log::Log`, so the facade dropped all of it: the whole
66,836-line Windows job log contained no VUID text anywhere, **including from
`validation_gate::a_deliberate_violation_is_caught_by_the_layer`, whose entire
job is to provoke one**. "The layer was silent" was therefore never evidence —
the channel was closed. `harness::instance` now installs the engine's own
`crcbl_core::log` sink, and the counting sink was always independent of this, so
no assertion changes; what changes is that a message is readable at the moment
it is emitted rather than only as a count at teardown.

Still open, and not touched here:

- **`crcbl-wgpu`, `crcbl-dx12` and `crcbl-mtl`'s e2e trees have the same hole.**
  None of them calls `init_logging` anywhere under `tests/`. `crcbl-shell`'s
  suites and `crcbl/tests/render_e2e.rs` do. Grep for `init_logging` under a
  crate's `tests/` to check; the fix is the one line `harness::instance` now
  carries.
- **The messenger asks the layer for `ERROR` and `WARNING` only**
  (`debug::messenger_create_info`), so `INFO`/`VERBOSE` never arrive whatever
  `CRCBL_LOG` says. Deliberate — the comment there explains that `INFO` is where
  the loader narrates its manifest search — but it means `CRCBL_LOG=trace` buys
  nothing from the layer itself.

## Where the Windows vk e2e leg's time goes, and the one measurement nobody has taken

The leg is 4.39x the Linux one (142.05s against 32.35s over the same 95 tests),
read out of the two jobs' logs. Inside that:

- **GPU time is at parity.** The same four-frame test measures 8.856 ms of GPU
  work on Windows against 7.629 ms on Linux while wall time is 7.4x apart, so
  four frames of GPU work is 1.7% of that test's runtime and ~98% of a rendering
  test's cost on Windows is host-side, on every recorded command. The
  inter-frame gaps are flat (1.927, 1.904, 1.656, 1.643 s), which rules out
  one-time shader JIT.
- **The loader's debug output is not the cost.** 2035 occurrences of the
  package-scan line, 19 per test; a 421-line test completes in 0.075s on Windows
  against 0.086s on Linux. `VK_LOADER_DEBUG: all` is deliberate and the reason
  is written above it in `ci.yml` — leave it on.
- **Serial execution was a large part of it, and is fixed.** Both harnesses
  passed `--no-capture`, which hands the test binary the real stdio and so
  silently forces one thread: nextest printed
  `warning: ignoring --test-threads because --no-capture is specified` on every
  run of both legs, making the `--test-threads 1` beside it dead. Both now pass
  `--success-output immediate` instead. Measured locally on this workstation,
  from the suite's own summary line: lavapipe 9.786s -> 1.439s, radv 7.753s ->
  1.193s.
- **`crcbl-wgpu/tests/run-wgpu-e2e.sh` still carries the same dead pair** —
  `--test-threads 1` immediately followed by `--no-capture`, so nextest warns
  and serialises that suite too. Left alone deliberately: it is a different
  suite on a different job, and turning its parallelism on wants its own
  several-runs-per-driver check, which is exactly what turned up the
  use-after-free in `crcbl-vk`'s deletion queue. `run-cli-e2e.sh`,
  `run-wayland-e2e.sh`, `run-x11-e2e.sh` and `run-win32-e2e.ps1` pass
  `--test-threads 1` _without_ `--no-capture`, so theirs is effective and
  deliberate — `run-win32-e2e.ps1`'s header says why.

### The suspicion that is still unmeasured: synchronisation validation

`CRCBL_VK_SYNC_VALIDATION: '1'` is set on both legs and sync validation is
expensive. **Nobody has measured what it costs on the Windows leg**, and turning
it off is a coverage decision that is not ours to take —
`docs/plan/ 02-vulkan-backend.md` names sync bugs as this stage's headline risk
and this as the mitigation.

What makes it worth measuring rather than arguing about: that leg's own probe
reports
`sync-validation reach: record-time=yes one-submission=yes cross-submission=no`,
where the Linux leg reports `cross-submission=yes`. So the Windows leg is paying
for sync validation and getting **less** from it than the leg beside it — the
class of hazard that only submit-time validation can see is out of its scope,
and `crcbl-render`'s graph-compile suite is what actually gates that class.

The measurement, to be run as a one-off and then reverted:

1. On `vk e2e (lavapipe, windows)`, in the `Run the suite against lavapipe`
   step's `env:` block, change `CRCBL_VK_SYNC_VALIDATION: '1'` to `'0'`. That
   variable is read by `crcbl_vk::debug::sync_validation_wanted` and by nothing
   else; leave `CRCBL_VK_VALIDATION` alone, or the whole suite fails on
   `ValidationReport::assert_clean`'s "validation was not enabled".
2. Read the `Summary [...] 95 tests run` line the harness prints, against the
   142.05s the same line reports today. Read the `vk e2e: gpu` timings in the
   same log too: they should not move at all, because sync validation is host
   work.
3. What each outcome means:
   - **Most of the 142s goes away** — sync validation is the cost. The decision
     in front of the user is then whether the Windows leg keeps paying for a
     layer that already reports `cross-submission=no`, given the Linux leg runs
     the same suite with the fuller reach. Note that
     `validation_gate::synchronisation_validation_catches_a_missing_barrier`
     skips itself when the variable is not `1`, so switching it off silently
     removes that test's teeth — it would need a matching `if:` on the job.
   - **Little changes** — sync validation is not it, and the next suspect is
     ordinary validation plus Windows' per-call overhead, which the same
     procedure measures by flipping `CRCBL_VK_VALIDATION` instead. That one is a
     bigger coverage loss and would only ever be a diagnostic run.

## A recording that names a swapchain view is still not protected

`crcbl_vk::device`'s `retire_swapchain` deliberately does not go through the
deletion queue: a present is queue work no timeline semaphore signals, so it
drains pending acquires, idles the device, and destroys the swapchain's views,
images and sync objects on the spot. The hold `poll_retire` now honours for
command buffers that are recorded and not yet submitted therefore does not reach
them, because nothing about them is ever parked.

So a caller that acquires a frame, records against `AcquiredFrame::view`, and
then calls `reconfigure_swapchain` (a resize) or `destroy_swapchain` before
submitting is left holding a command buffer that names a destroyed
`VkImageView`; submitting it is the same
`VUID-vkQueueSubmit2-commandBuffer-03874` the deletion-queue bug produced.

**Reasoned from the code, not reproduced, and not fixed.** No test records
against an acquired view without submitting, and `crcbl-render`'s frame loop
acquires, records and submits within one frame, so it is not known to be
reachable in practice. A fix would mean `retire_swapchain` consulting the same
recorded-but-unsubmitted set `poll_retire` builds — and then deciding what to do
when one is found, since it cannot park what it is destroying: either wait, or
refuse, or invalidate the recording loudly. That decision is the reason this is
an entry rather than a change.

## `CARGO_NET_OFFLINE` on the vk e2e steps: looked at, not applied

`Updating crates.io index` costs about 7.4s per cargo invocation on the Windows
leg despite `--locked`, and `CARGO_NET_OFFLINE=true` would remove it. **Not
applied, because the workflow cannot be read as making it safe.**
`Swatinem/rust-cache@v2` restore is best-effort — a cold key, an evicted entry
or a changed lockfile hash all leave the registry index absent — and no step in
either vk e2e job runs `cargo fetch` first, so an offline cargo would fail the
job outright rather than fetch. Making it safe means an explicit
`cargo fetch --locked` step before the suite, and only then the variable; that
is a workflow change with its own failure mode and wants the user's call.

The other cost in the same job, also untouched: the pinned LunarG SDK install is
about 46s and `Swatinem/rust-cache` does not cover it, since it caches cargo
directories and not `C:\VulkanSDK`. An `actions/cache` keyed on
`VULKAN_SDK_VERSION` would, at the cost of a second cache to reason about.

## A timing test started its clock after the thread it was timing

`crcbl-mtl`'s `a_wait_sleeps_until_the_presented_handler_reports` asserts the
wait blocked for at least `HANDLER_DELAY` (40 ms), and took `Instant::now()`
**after** spawning the handler thread. The handler's sleep begins the moment the
thread runs, which is somewhere inside `spawn`, so the clock was already behind
it by however long the spawn took. CI caught it at **39.932915 ms** —
sixty-seven microseconds short — after months of passing.

Fixed by starting the clock before the spawn. Worth knowing for its shape rather
than its size:

- **It made the assertion sound, not tighter.** Moving the clock earlier can
  only increase the measured elapsed, so the lower bound got easier to pass.
  What it removed was the window in which a correct implementation could fail
  it. The check still has teeth against what it exists for — a
  `wait_until_shown` that returns immediately elapses in microseconds —
  confirmed by mutating the wait to return at once and watching it go red.
- **The local suite could not have found it.** It is a race whose window is a
  thread spawn, and it fired once in CI and never in twenty consecutive local
  runs of the same test.

## What screen-space AO left owed

`docs/plan/18-render-features.md`'s AO section holds the decisions and the
reasons. What the first slice deferred or turned up:

- **The forward pass still clears and re-writes depth**, so the prepass buys AO
  its depth and nothing else — the overdraw win is deliberately deferred because
  it needs `GreaterOrEqual` and depth invariance across four rasterisers, which
  only CI can settle. The engine now has per-pass GPU timers and frame counters,
  so that change can be **measured** rather than assumed when it is made.
- **AO still runs at full resolution.** Half-resolution AO with an upsample was
  the slice after the bilateral blur and has not been done. It is the one of the
  two that costs quality for speed, so it wants the per-pass GPU timers pointed
  at `ssao` and `ssao-blur` first — nobody has measured what the pair actually
  costs.
- **`SSAO_RADIUS` is 0.5 and the kernel reaches ~⅞ of it laterally**, both tuned
  against `Scene::Ao` alone. The first, normal-hugging kernel gave a ratio of
  1.03 — visually nothing. Neither has been retuned; both have now been
  **measured** against a real room — see "The AO constants against lumen's
  room", which finds the radius sane at room scale.
- **`AO_RATIO` is 1.10 rather than a shadow-like 1.5**, and that is not
  slackness: bands are read after the sRGB encode and AO scales ambient alone,
  so a wall closing half a hemisphere cannot approach halving a pixel. Measured
  1.162 with about ten LSB of separation, far past the one-level driver drift
  the tolerance already absorbs.
- **`crcbl_hal::SampleType::Depth`'s doc is now narrower than the type.** It
  says the variant means "read through a comparison sampler" and that the paired
  sampler must set `comparison: true`; the SSAO layout uses it with **no sampler
  at all** and works on every backend.
- **`prepass_stats` is a wrapping counter nobody clears.** Documented in code
  and harmless, but a smell.
- **Metal and D3D12 compiled the depth `Load` and never ran it.** SPIR-V and
  WGSL both executed — `OpTypeImage Depth = 1` on radv and lavapipe,
  `texture_depth_2d` on wgpu — so the `DepthTexture2D` vs `Texture2D<float>`
  trap this engine hit once before did not recur, on the two targets that can
  say so.
- **Five goldens were re-blessed and one deliberately was not.** `cube`,
  `lights`, `dunes`, `spot_shadow` and `point_shadow` moved; `ui`, `sprite` and
  **`spot`** did not. `spot` staying bit-identical is the evidence the term is
  contact AO rather than a global scale — it is the one 3D scene with nothing
  near anything else. The depth-weighted blur moved the same five plus `ao`
  itself and left the same three byte-identical, `spot` included, which is that
  evidence a second time.

### What the depth-weighted blur left owed

`ssao_blur.slang` now weights its 4×4 kernel by view-space depth. What that
slice deferred or turned up:

- **The determinism margin is weaker at a silhouette, and nothing has measured
  the new one.** The box divided an isolated driver disagreement by sixteen
  everywhere; the weighted kernel divides by sixteen on a flat surface and by as
  little as one where every neighbour is rejected. The header says so, and the
  frames drawn on radv, lavapipe and wgpu still agree to one channel level — but
  that is three rasterisers agreeing, not a bound. What would settle it is the
  CI matrix's Metal and D3D12 legs.
- **A depth-only weight cannot separate two surfaces that meet.** `Scene::Ao`'s
  floor still brightens by a level or two in the row where it meets a wall — the
  wall and the floor have the _same_ view-space depth at that crease and differ
  only in gradient, so every tap keeps nearly full weight. Fixing it means a
  normal term in the weight, which means either reconstructing the normal a
  second time in the blur or the normal attachment the AO section refuses. Not
  done, and not obviously worth it: it is a two-level artifact on one row.
- **`DEPTH_TOLERANCE_RADII` is 2.0 and has been tuned against nothing.** It puts
  half weight at exactly one `SSAO_RADIUS` and zero at two, which is derived
  rather than fitted. `apps/lumen`'s room is the first scene with two surfaces a
  metre apart in view depth inside one kernel footprint, and it shows no halo at
  2.0 — see "The AO constants against lumen's room", including why that is an
  upper bound rather than a tuning result.
- **The halo check lives in `Scene::Cube`, not `Scene::Ao`.**
  `the_clear_does_not_brighten_the_silhouette_in_front_of_it` reads the plain
  pyramid's underside, because the AO scene's camera looks into a closed trough
  and its frame contains no far plane at all — nothing there can show a halo.
  The AO scene is still the one that says occlusion happens; it cannot say
  anything about this kernel. If `Scene::Ao` ever grows a silhouette against the
  clear, the check belongs there instead.
- **The check depends on where a pyramid's base lands in the frame.**
  `PYRAMID_UNDERSIDE_RIM_AT` and its neighbour are pixel coordinates on a
  six-row-tall surface; a change to that camera or that mesh moves them, and the
  failure would read as a halo rather than as a moved band. The anti-vacuity
  floor beside it catches the worst version — both bands landing on the clear —
  and nothing catches the band sliding onto the teal side face.

## What punctual-light shadows left owed

The atlas is a fixed 4×2 tile grid, `shadow::Selection` decides who gets tiles,
`shadow::spot_matrix` and `shadow::point_matrix` build reversed-Z perspective
projections, and `mesh.slang`'s `spot_visibility` and `point_visibility` sample
them through the same 3×3 PCF kernel the cascades use. Decisions taken, so they
are not re-argued:

- **Two budgets, because they buy different things.** `LIGHT_TILES` is atlas
  space — six of them, the minimum a point light can exist in — and
  `LIGHT_SLOTS` is cull space, two of them, because a slot costs a `DrawGen`. A
  `DrawGen` is ~5.0 MiB measured on the spot scene, 3.7 MiB of it per-instance
  LOD hysteresis state that is device-local and permanent; a tile is 4 MiB of
  `D32Float`. So the atlas is 32 MiB and the four generators are ~20 MiB,
  allocated whether a scene has a shadowed light or not. That is deliberate:
  building a `DrawGen` inside `begin_frame` means a frame that cannot allocate
  is a frame that cannot draw. **Raising either budget is the memory
  conversation, not the atlas one.**
- **The reachable states are one point light or two spots**, and nothing in
  between. Six light tiles is the minimum a point light needs; two of them are
  what a spot each takes. A frame with more shadow-worthy lights than fit ranks
  them by projected influence and hands out runs first-fit, and a point light
  that cannot fit six consecutive tiles is **skipped without taking the budget
  down with it** —
  `a_point_light_that_cannot_fit_leaves_the_lights_around_it_alone` is the
  assertion.
- **A cone at or past `MAX_SPOT_HALF_ANGLE` (80°) is refused a tile** rather
  than given a map narrower than the cone. `tan` runs to infinity at 90° and the
  matrix stops being one. Such a light lights without occluding, which is the
  same honest degradation as running out of tiles.
- **A punctual light biases in world units before projecting**, denominated in
  tile texels at the receiver, where the cascades bias in shadow-clip depth. A
  perspective map's depth precision piles up at the near plane under reversed-Z,
  so the cascades' constants do not transfer. The shipped pair
  (`PUNCTUAL_DEPTH_BIAS_TEXELS` 2.0, `PUNCTUAL_SLOPE_BIAS_TEXELS` 4.0) is double
  the smallest that made the two geometry paths agree on lavapipe. **A point
  light's 90° face reuses them unchanged**, and that is not laziness: the
  constant counts texels and the world footprint of a texel is computed per
  receiver, so the same count means the same thing at any cone angle. Verified,
  not assumed — the two geometry paths draw `Scene::PointShadow`
  byte-identically.
- **No texel snap on a spot's or a face's matrix, and none is owed.** The
  cascades snap because their box follows the camera; a punctual light's matrix
  is a pure function of the light, so a still light already produces a
  byte-identical matrix. `a_still_spot_produces_the_same_matrix` asserts it.
- **Face order is the cube-map convention**, `+X -X +Y -Y +Z -Z`, in
  `shadow::face_axis` and `mesh.slang`'s `point_face`. The test that pins it
  writes the six directions out **literally** rather than deriving them from
  `face_axis` — the derived version passed cleanly with faces 4 and 5 swapped,
  which is exactly the check-that-cannot-fail this project keeps finding.

What is left:

- **Fixed: `cluster_survives` carried a mesh-space radius into a world-space
  test.** This and the "large open box stops drawing" entry were **one bug seen
  from two ends** — a cluster's bounding radius was not scaled by the instance
  transform, so a scaled instance's clusters were rejected while the instance
  itself was correctly kept. Both files documented it as safe because
  `GpuInstance::transform` "is rigid", and that claim was **already false in two
  shipped scenes**: the open-box floor's true world radius is 3.10 against a
  local 0.71, and `Scene::Spot`'s floor 4.24 against 0.87.

  The radius is now multiplied by `max_stretch`, the square root of the largest
  absolute row sum of `BᵀB` — an upper bound on the basis's largest singular
  value that is **exact** for any rotation-then-scale and needs no contract
  about what callers may pass, which is the point: the old code needed one and
  did not have it. It is `1.0` for a rigid transform, so nothing previously
  correct moved and no golden changed. The transformed cone axis is normalised
  in the same function for the same reason — unnormalised, `dot(axis, d)` scaled
  with the transform while the radius term did not, so the same shape at two
  sizes got two answers.

  **Only the amplification path was affected** — `IndirectCount` drew the box at
  every offset — which is why the earlier note said "both paths" and was wrong.

- **The cube's face seams are unpadded, and the plan's mitigation is not
  built.** `tile_pcf` clamps taps half a texel inside the tile, so a receiver
  within one texel of a face boundary re-samples its own edge texel instead of
  the neighbour's: the shadow edge is **under-filtered** along the twelve cube
  edges rather than wrong. One texel at distance `d` covers `2d/1024` world
  units, so at a metre from the light that is about two millimetres. Topic 18
  names a border of padding per tile as the fix; build it if a seam ever shows.
- **A shadowed spot idles five of the six light tiles.** The region is sized for
  a point light, so a scene of spots wastes 20 MiB of atlas. A packing policy is
  what would fix it and topic 18 puts packing post-MVP.
- **Nothing exercises two shadowed lights at once.** `Scene::SpotShadow` has one
  and `Scene::PointShadow` has one, so the second cull slot is covered only by
  the host-side `Selection` tests and by the vk_e2e assertion that a free tile
  stays at the clear.
- **`Scene::Lights` changed shape without changing pixels.** Its three point
  lights are now shadow-eligible, so the most influential takes the region and
  the frame records one more cull triple. `lights.png` is byte-identical and
  `every_scene_records_the_passes_it_names…` expects the extra triple — worth
  knowing before reading that scene as unshadowed.

## P7B could not start as written: the engine has one light

Surveying P7B before delegating it found that `docs/plan/18-render-features.md`
names "CSM for sun, single map for spot, cube for point" while **the engine has
exactly one light** — a single `DirectionalLight` carrying a direction and a
colour. No light list, no point or spot lights, no light culling, and **no
specification anywhere** of how many lights there are or how they are gathered:
grepping topic 18 and topic 3 for clustered, tiled, Forward+, light list or
light culling returns nothing. So "spot and point shadows" would have meant
shadowing lights that have no representation.

**Decided and written into topic 18: clustered forward.** Lights are an SSBO of
rows like instances and materials, and a compute pass assigns them to a froxel
grid the fragment stage indexes. The reasoning is on the record there; the short
form is that tiled/Forward+ degrades badly over depth range and the samples that
motivate lighting are exactly that shape, deferred conflicts with two rules
already locked in that topic (one BRDF shared with the ray-traced twin, one post
stack after either path) and would make the raster path structurally unlike its
twin, and clustered forward needs nothing of a device — a compute pass and two
storage buffers — so it is the same code on all four backends, which is the
constraint every other path here is held to.

Two things that follow and are worth holding to:

- **A directional light becomes a row too**, flagged as affecting every cluster,
  so the sun stops being a special case in the shader.
- **Cluster overflow is counted, not dropped silently** — it surfaces through
  topic 40's counters, because a scene that overflows should be visible in the
  panel rather than mysteriously dark.

**Left undecided on purpose**, each wanting the list to exist first:
shadow-atlas allocation across light types, the rule choosing which lights get
maps (nearest, brightest, largest screen influence), and whether point lights
use a cube map or six atlas tiles.

**So the P7B order is: the light list first, then shadows for the types it
introduces**, not the other way round.

## Decision needed: `taiki-e/install-action` is a single point of failure 17 times over

All three hit on 2026-08-12 within an hour, on commits that could not have
caused any of them, and **every one left its real step skipped**. None is a
defect in the tree; each cost a red build and a re-run.

- **`decoder fuzz (libFuzzer)`** — `taiki-e/install-action` could not download a
  prebuilt `cargo-fuzz 0.13.1` from either GitHub releases or QuickInstall, fell
  back to building it from source, and **that build cannot succeed on current
  Rust**: it pulls `proc-macro-error 1.0.4`, which fails with "attributes
  starting with `rustc` are reserved for use by the `rustc` compiler". So the
  fallback is not a slow path, it is a broken one, and every download flake is a
  hard failure. Pinning a `cargo-fuzz` whose source still builds, or caching the
  binary, would make this self-healing. **Every meaningful step was skipped** —
  the fuzzer did not run at all.
- **`vk e2e (lavapipe, windows)`** — `Install the pinned lavapipe` failed and
  the suite was skipped.
- **`shaders (committed artifacts match their sources)`** —
  `Install the pinned dxc` failed with curl exit 56 (failure receiving network
  data) and the comparison was skipped. Nothing wrong with the artifacts;
  verified locally with the pinned compilers before re-running, and the re-run
  passed.

**It happened a fourth time within the hour** — `vk e2e (lavapipe, windows)`
again, `Run taiki-e/install-action@v2: failure`, every subsequent step skipped.
Three of the four were that action.

**`.github/workflows/ci.yml` uses it 17 times**, and each use is a step that can
fail the whole job while testing nothing. The action already has an internal
fallback chain (GitHub releases → QuickInstall → binstall → `cargo install`),
and today every link failed for network reasons, so the fix is not "add another
fallback" — it is to survive a transient outage or to stop downloading per run.
The options, none of which I took because each is a real trade-off:

1. **A step-level retry.** GitHub Actions cannot retry a `uses:` step natively,
   so this means a third-party action such as `nick-fields/retry` — **a new
   dependency, and therefore the user's call**. Cheapest to write, and it does
   address the observed failure, which is transient rather than persistent.
2. **Cache the installed binaries** with `actions/cache` keyed on tool and
   version. They are already pinned, so the key is stable and a warm cache never
   touches the network. The first run per key still downloads, so it narrows the
   window rather than closing it.
3. **Replace the action with `run:` steps that retry.** No new dependency, but
   it discards the action's own fallback chain and means rewriting 17 sites —
   the most work and the most to get wrong.

**Recommendation: 2, then 1 if it still bites.** Caching removes most of the
exposure without adding a dependency, and the pinning that makes CI reproducible
is exactly what makes the cache key sound.

**The shape worth naming**, because it recurs: a failed install leaves the real
check **skipped**, and a skipped check is not a passed one. The Pages deploy did
the same thing earlier in the session — the build failed and the deploy was
_skipped_, which reads as a green-looking push with nothing shipped. When
reading a red run, check whether the meaningful step ran at all before
diagnosing the code.

## Profiling and benchmarking: decisions taken 2026-08-13, before any code

`docs/plan/40-profiling.md` is new and specifies the whole thing; it is a
cross-cutting track in the roadmap alongside CLI, testing, audio, persistence,
debug tools and pixel art. Nothing is implemented. The decisions, so they are
not re-argued when a slice starts:

- **Trace export is Chrome Trace Event JSON**, which Perfetto and
  `chrome://tracing` both read. Text, no dependency, and `crcbl-cli` already has
  JSON machinery. **Tracy was considered and declined for now**: it is a client
  library, therefore a new dependency and the user's call, and its wire protocol
  is not something to hand-roll. If it is wanted later it is an optional feature
  over the same span data rather than a second instrumentation pass.
- **Spans are always compiled and gated at runtime by an atomic**, not compiled
  out behind a feature. A profiler you have to rebuild to use is one nobody
  turns on mid-investigation, and a build that changes what it measures is the
  classic way to measure the wrong thing. A compile-time off switch exists for
  shipping builds. The cost of this decision — one relaxed atomic load per span
  when disabled — should be measured rather than asserted, by benchmarking the
  profiler itself.
- **Benchmarks report p50/p95/p99/max, not means.** Frame time is a tail problem
  and a mean hides the stutter a player notices. This session already produced a
  case where a within-arm spread was wider than the between-arm difference being
  claimed, which is the same failure in miniature.
- **CI publishes benchmark numbers and does not gate on them.** A shared runner
  is far slower and noisier than a dev box — the roadmap says so already — so CI
  proves the benchmark _runs_ and stores the output as an artifact; comparison
  happens against a baseline from a known machine. A perf gate that fails for
  reasons unrelated to the commit is a gate people learn to ignore.
- **A benchmark's output carries its environment or it is not comparable**:
  adapter, driver, backend, the three capability selectors, build profile,
  commit. A comparison against a baseline from different hardware should be
  refused rather than printed.
- **The GPU report stays frames-latent.** No benchmark mode "reads it properly"
  by stalling, because a stall changes what is being measured.

**What the survey found already built**, so no slice rebuilds it: per-pass GPU
timestamps (`crcbl_render::timing`) wired into `CompiledGraph::execute`, frames
latent by design, a pass's span deliberately including its barriers, degrading
to an empty report without `Features::TIMESTAMP_QUERY`, and feeding a
`DebugModule`. That half is good. What is absent is everything else: no
benchmark harness beyond horde's ad-hoc flags, no baseline or comparison, no
trace export, no memory or pool-occupancy accounting, no `crcbl-jobs`
instrumentation, and counters scattered across `SceneStats`, `visible_count` and
each sample's own rows rather than one place.

**`crcbl_core::trace` landed, and `Loop::frame` and the panel's budget row are
its first callers.** Decisions taken there, so they are not re-argued:

- **CPU frame time is the frame span less `pace` and `present-wait`.** Under
  vsync the loop blocks inside the present, so an unsubtracted frame span reads
  as the display's period on every machine, always exceeds the GPU total, and
  answers "CPU-bound" without having looked. Verified on a real horde run: the
  loop's wall-clock line reported 1.053 ms at a 1000 fps cap while the row
  reported 0.41 ms of work, the difference being the `pace` sleep.
- **The plan's `schedule`, `physics`, `upload` and `record` phases do not
  exist** in the loop — the first two live inside a game's `tick` closure and
  there is no asset upload in the frame. `perf.rs` records that rather than
  faking them. They arrive with whichever slice gives `tick` its own structure.
- **`shell.wait_events` is outside the frame span**, deliberately: it is the
  loop idling, and a frame span containing the compositor's idle timeout would
  report it as CPU cost on every still frame.
- **`drain` is called once per frame while the gate is on, and not at all while
  it is off.** Calling it unconditionally is two mutex acquisitions per frame to
  move nothing, which is the disabled-cost claim the module makes about itself.
- **The two windows are distributions, not a pair.** The GPU report is frames
  latent by design; over 120 frames a two-frame lag cannot move a percentile,
  whereas a per-frame pairing would be wrong by exactly that offset. The row
  carries the frame number its newest GPU sample came from rather than hiding
  it.
- **`MIN_PERCENTILE_SAMPLES` is 20 because it is derived, not picked**:
  nearest-rank p95 is `ceil(0.95n)`, which is just the maximum for every `n`
  under 20. Below it the row says `filling 7/20`.

What is left:

- **`apps/bare` drives `GpuContext` with its own loop and never drains**, so
  with the trace on its `present-wait` spans fill the per-thread buffer and are
  then refused and counted. Bounded and reported, never a leak — but a trace of
  a bare-loop run is one buffer's worth and then a `dropped` count. It also
  never calls `init_from_env`, so the gate has to be set by hand there.
- **`CRCBL_TRACE` is silent on a malformed value**, matching `CRCBL_LOG`'s
  precedent exactly. A typo therefore leaves the panel with no budget row and no
  explanation. Diverging from `log` to warn is a one-line change and was not
  taken.
- **`trace::ThreadTrack::new` is public with only test callers today.** It is
  `get`'s inverse and the thing an out-of-crate consumer needs to build a
  `Snapshot`, which `crcbl::perf`'s tests are the first of — but it is the shape
  that turns into dead public API if nothing else ever wants it.
- **Nothing is instrumented outside the loop.** No spans in `crcbl-ecs`,
  `crcbl-phys` or `crcbl-jobs`, and none of the panel's other rows: the pass
  list, the CPU breakdown, memory, jobs, the freeze toggle.
- **The culling-stats ring reads the camera's cull only.** Each shadow `DrawGen`
  has a stats buffer and none is read: a cascade's survivors answer a different
  question about a different frustum, and summing them would produce a number
  larger than the instance count. If a shadow cull's cost ever needs measuring
  it wants a row of its own, not a place in this sum.
- **The ring's request happens at the next `begin_frame`, not where the copy is
  scheduled** — a copy is recorded, not submitted, so the readback cannot be
  requested until the frame that contains it has gone. That puts a contract on
  callers: submit the graph you executed before starting the next frame. It is
  documented on `CullStatsRing::begin_frame` and **cannot be enforced from
  inside the ring**; a caller that breaks it gets stats attributed to the wrong
  frame.
- **`triangles drawn` is still `indirect` on the forward path.** The ring gives
  instances and clusters; triangles would need instances-drawn multiplied by
  per-mesh index counts, and the nominal-per-cluster shortcut is the
  authoritative-looking lie this slice refused.
- **The cluster survivor word is written by the amplification stage**, so it is
  `unknown` on both indirect tails and on `MESH_SHADER` without `TASK_SHADER` —
  a claim now rather than a blank, but still a counter that exists on one of the
  four ways the engine draws.
- **`Scene::Cube` culls nothing**, so the cross-backend readback test can only
  assert `drawn == submitted`; the actually-culled claim is vk-only. A scene
  with something off-screen would let wgpu, Metal and D3D12 assert it too.
- **`crcbl-wgpu`'s e2e suite has no `crcbl-render` dev-dependency**, so wgpu's
  coverage of the ring lives in `crcbl`'s `render_e2e` instead. Adding the
  dev-dep, as `crcbl-vk` has, was not taken.
- **Metal and D3D12 have never run the readback.** Cross-target type-checks
  only, as ever.
- **`crcbl-wgpu` enumerates `Backends::all()`, which includes OpenGL — a
  platform the matrix declined.** The right expression is
  `wgpu::Backends::PRIMARY`, which is `VULKAN | METAL | DX12 | BROWSER_WEBGPU`
  on every target with no `cfg`, and `SECONDARY` is GL alone; the browser would
  lose only the WebGL2 fallback, also declined. **Not taken, because the trade
  is real**: measured with `VK_LOADER_DRIVERS_DISABLE='*'`, a Vulkan-less Linux
  box currently gets the GL adapter and passes all 28 wgpu e2e tests, honestly
  declining `DESCRIPTOR_INDEXING` and `DRAW_INDIRECT_COUNT` rather than claiming
  them. Narrowing to `PRIMARY` turns that into no adapter at all.
  `create_native`'s own doc already claims Vulkan/Metal/DX12 while the code says
  otherwise, so whichever way this goes, one of the two is wrong today.
  **Decision wanted.**
- **The whole counters section lags the frame by one, uniformly.** The panel is
  gathered in `draw_debug_overlay`, which runs before `GameGpu::frame` records
  anything. Stated in the module docs and asserted by
  `the_counters_row_moves_with_the_frame_and_trails_it_by_one`; `apps/horde`'s
  `scene_stats` already had the same lag.
- **`SceneStats::batches` re-runs the batching that
  `SpriteRenderer::begin_frame` runs a moment later.** Sourcing it from
  `SpriteRenderer::counters()` instead would need `Gpu` to fill it after
  `begin_frame`, which breaks horde's two device-free batching tests. A horde
  change rather than a counters one, and not taken.
- **`ForwardRenderer`'s docs link `[ForwardRenderer::bucket_count]`, which does
  not exist** — only `DrawGen::bucket_count` does. It sits on a private const so
  rustdoc does not flag it, which is why it survived the doc gate.
- **Enabling the trace in a browser build panics on the first span.**
  `std::time::Instant::now` compiles on `wasm32-unknown-unknown` and panics at
  runtime — `std`'s unsupported-platform stub. Left loud rather than papered
  over with a zero clock, and the gate starts off so a browser build that never
  enables it never reads the clock. A real browser clock is `performance.now()`
  through a dependency `crcbl-core` does not have, which is **a decision for the
  user** when the browser wants a trace. `trace::init_from_env` returns `false`
  on `wasm32` before touching the environment, so the only thing that can turn
  the gate on cannot do so in a browser build; `set_enabled` deliberately still
  can, and reversing that is the user's call.
- **`crcbl-jobs` has not gained the `crcbl-core` edge.** Correct today: CI runs
  `cargo machete`, which fails on an unused dependency, so the edge arrives with
  the first span opened in a worker.
- **The compile-time kill switch was built and then deleted.** It was a Cargo
  feature, and CI's only two workspace test runs both pass `--all-features`, so
  it would have been _on_ in CI and every test would have asserted the
  compiled-out arm — green on code CI never ran. When it earns its place it is
  `--cfg crcbl_trace_off` with a `build.rs`, not a feature;
  `docs/plan/40-profiling.md` records the argument. Nothing ships from this repo
  yet, so it has no caller either way.

**`crcbl_render::MAX_TIMED_PASSES` bounds this crate's renderers, not the
caller's own passes** — a deliberate call, taken 2026-08-13. Every renderer here
carries a `MAX_PASSES` and the constant is their sum, so a pass added anywhere
below moves it; but a sample that records a pass of its own — the 2D samples
each have a clear — is that much over. Today none of them is close (they record
four against a bound of 22), and the once-per-`PassTimers` warning is the
backstop if one ever is. The alternative was every sample writing
`MAX_TIMED_PASSES + 1`, which is the guessing this constant exists to end.

## Sidecar meta RON: three items now want it, and the workspace has no RON reader

`docs/plan/06-assets-scenes.md` and topic 25 both assume a `crate.glb.meta.ron`
sidecar, and **nothing in the workspace reads or writes RON**. There is no `ron`
crate in `[workspace.dependencies]`; what exists is the plan's
RON-for-entity-data rule and `crcbl-shell`'s `application/x-crcbl+ron` clipboard
mime, which names the format and moves opaque bytes without parsing them. Adding
the crate is a new dependency and therefore the user's call.

Three things now want that one file, and they should land together rather than
as competing conventions: the `AssetId` GUID this backlog already owes,
per-asset LOD overrides, and whatever the importer's report should persist.

**But the LOD half needs a decision first, not just a reader.** The chain era's
"per-asset ratio override" no longer describes anything: the DAG halves
structurally — each group simplified to about half its triangles and re-split —
so a ratio is not a parameter of `build_cluster_dag`. Reinstating one is a
change to the generator's signature. `docs/plan/25-lod.md` now says so; the
delivery table's "sidecar overrides" line is still chain-era.

Also recorded from the hand-authored import slice:

- **`MSFT_lod` on materials is deliberately not read** — it declares a material
  chain for a mesh that keeps its geometry, and nothing here shades at two
  levels.
- **Multi-primitive meshes resolve but are untested**: one DAG per primitive
  with the chain depth taken as the shallowest, and no fixture exercises more
  than one primitive.
- **Nothing consumes `resolve_lod`.** The geometry it resolves never reaches a
  GPU pool; wiring it to `crcbl-render` is the slice that would make hand LODs
  visible.

## The host-visible-write rule has now cost two devices, and the seam could enforce it

`crcbl-dx12` refuses a shader-written binding that names a host-visible buffer —
D3D12's upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at creation,
so there is no UAV of one. The refusal is correct and its message is excellent.
It has now caught the same mistake **twice**: the draw-generation counters, and
the LOD hysteresis state.

**Done.** `NullDevice` refuses a `read_only: false` entry naming a mappable
buffer at `create_bind_group` and `update_bind_group`, and `MemoryLocation`
carries the rule with its D3D12 mechanism. Nothing in the tree violated it, so
there was no third latent instance. Read-only bindings of host-visible buffers
stay legal — removing that exemption fails 28 tests, which is the measure of how
load-bearing it is.

**The image half is closed too**, and it turned out stronger than the buffer
rule: `ImageDesc::memory` has exactly one legal value, because the seam has no
way to touch an image's bytes from the CPU — no `write_image`, no mapping, no
subresource layout — so the field buys nothing observable on any backend while
removing a D3D12 device. **Done — the field is deleted.** Taken as a sane
default under the standing instruction, on `CLAUDE.md`'s rule that a contract is
enforced rather than documented: a field with one legal value is one every
caller must fill and can still fill wrongly, and removing it makes the state
unrepresentable instead of refused. 36 sites, 20 files, no golden moved, and the
run-time refusal added one commit earlier is deleted with it — a guard against
the unconstructable is noise.

**What blocks writing it from outside `crcbl-hal`:** the null backend does not
record bind-group _contents_. `Detail::BindGroup` keeps the layout handle and an
update-after-bind flag, and `Recorder` exposes no accessor for a group's entries
or for a buffer's `MemoryLocation`. So a general "no shader-written binding
names a host-visible buffer" assertion cannot be written by a consumer today.

The narrow version that _can_ be, and now is:
`nothing_the_draw_generation_lets_a_shader_write_is_host_visible` in
`crcbl-render/tests/graph_compile.rs`, which builds a real `DrawGen` on the null
backend and checks each buffer it hands out. Its observable is exact rather than
a proxy — `NullDevice::create_buffer` materialises bytes only for a mappable
buffer, so non-empty `Recorder::buffer_bytes` **is** host visibility. It runs
with no ICD, so it covers the WARP leg from a Linux dev box.

**Also worth an entry:** each shadow cascade now costs its own start-up submit
and `wait_idle` — one per cascade plus the camera's — to zero its hysteresis
state. Correct and blocking, and coarser than it needs to be, on the same terms
`mesh_pool`'s timeline-less upload path already accepts.

## Topic 25's MVP is closed; what remains, and one coverage hole it found

Per-cluster selection runs on `MeshShader` and the uniform cut on both indirect
tails, so the dunes patch draws on every geometry path and the two agree. What
is left of topic 25:

- **Hysteresis.** A camera drifting across a threshold switches level every
  frame at the boundary. It needs one persistent per-instance word on the
  indirect paths and one per cluster on the mesh path; neither changes a data
  layout that has landed, so it was deferrable and was deferred.
- **Hand-authored LOD precedence and `MSFT_lod` import**, and QEM auto-gen for
  arbitrary meshes — the only DAG in the tree is the cooked dunes artifact.
- Shadow LOD bias, the bake cache, the debug tint overlay and shadow-map
  inspector, `crcbl lod` CLI, HLOD, impostors, dithered crossfade.

**The coverage hole worth knowing about:** `OffscreenSetup::OPTIONAL_FEATURES`
never asked for `TASK_SHADER`, so every `render_e2e` run on a mesh-capable
adapter had been drawing through the un-amplified `meshMain` while reporting
`GeometryPath::MeshShader`. The goldens were real frames of a real path — just
not the path the device advertised, and not the one carrying per-cluster
culling. Fixed here. The knock-on is worth remembering: **Vulkan enables
`meshShader` when `taskShader` is requested**, so a test that wants a lesser
path must subtract both flags, and subtracting one leaves both arms on the same
path with the self-comparison quietly passing.

Still open from earlier slices and unchanged by this one:

- **Every `ForwardRenderer` uploads the dunes DAG** whether or not `set_dunes`
  is called — a build-time cost every app pays for geometry most never draw.
- **Settled: the cascades select from the camera's eye, not the light's.** What
  looked like light-as-eye was `camera.eye + light_direction * cascade_far` —
  the camera's own eye pushed along the sun's direction, stepping per cascade,
  so two cascades asked two different detail questions about one caster.
  Replaced by the camera's eye at the camera's pixels-per-unit with both budgets
  scaled, because the artifact is a shadow edge displaced by the group's error
  and that displacement is seen by the camera. The light is still the eye for
  the amplification stage's cone test, where a shadow map's viewer genuinely is
  the light.
- **Group radii compound faster than a model grows**, so levels 2–6 have little
  spatial discrimination; a tighter enclosing sphere is the lever, not a bigger
  model.
- **The DAG's top three levels report the same error**, so they are never
  separately selected by a budget.
- **`projected_error` has three implementations** — two compared for bit
  equality by the cooker, the third held to the rule by comparing the decision.
- **The cooked artifact is only ever generated on x86-64 Linux**; no CI leg
  regenerates it on macOS or Windows.

## What `crcbl_scene::simplify` owes, and one workspace-wide trap it found

Topic 25's QEM simplifier exists host-side with no consumer. What is left:

- **`glam`'s `DMat3: Default` is the identity, not zero**, so a derived
  `Default` on a quadric seeds every vertex with the three coordinate planes. It
  was caught only by the hand-derived quadric tests — the structural ones
  (closed mesh, border kept, deterministic) all passed with the bug in place.
  `Quadric::ZERO` is spelled out with that reason. **This applies anywhere in
  the workspace that derives `Default` through a glam matrix**, which is why it
  is here rather than only in that file.
- **Flip rejection is per-collapse, not a global invariant.** A face can rotate
  a little under each of several individually-accepted collapses until it has
  come all the way round; demonstrated by popping in descending cost order,
  where a height field ends up with a face pointing at `-Z` and every single
  collapse having passed the check. The cheapest-first order is what keeps the
  local test a workable stand-in. A global orientation check is the fix.
- **A rejected candidate is dropped, not deferred** — an edge refused now is
  only reconsidered if an endpoint is later merged into. Cheap, terminates,
  leaves some collapses unmade.
- **`max_error` is not a certified Hausdorff bound**, so the plan's "reported
  error ≥ sampled Hausdorff" property test does not exist. Runtime selection
  will lean on this number, so that test is owed before it does.
- **Never measured on a real asset.** Every fixture is synthetic — torus, height
  field, tetrahedron — there is no glTF corpus case and no benchmark, and the
  cost is O(E) in candidates with re-pushes per collapse.
- **Position welding is absent**: vertices compare by index, so duplicated
  coincident vertices read as disjoint surfaces whose every shared edge is a
  border and therefore locked.
- **One mutation is provably unobservable rather than untested.** Making
  `max_error` record the last collapse instead of the largest stays green,
  because the heap pops cheapest-first and a collapse only ever adds a positive
  semi-definite quadric, so popped costs are non-decreasing and max equals last.
  That ordering is asserted directly by
  `the_costs_of_the_collapses_performed_never_decrease` rather than left as
  prose, and `.max()` is kept because it is what the metric is defined as.

## The shared layout validator: two Metal decisions, and what only CI can prove

`BindGroupLayoutDesc::check_entries` and `BindGroupLayoutEntry::resolved_count`
replaced four drifted copies. Decisions taken while doing it, so they are not
re-argued:

- **`crcbl-mtl` refuses the `u32::MAX` count sentinel where the other four clamp
  it.** Metal reports `max_bindless_descriptors: 0` — flat argument tables have
  no runtime-sized array — so clamping would hand back a **one**-element array
  on a backend that cannot do bindless at all, which is exactly the quiet
  downgrade the seam exists to forbid. `plan_set`'s table-capacity `checked_add`
  refuses it by name instead. Reversible in about a line plus a `limits`
  parameter on `plan_set` if this ever looks wrong.
- **`crcbl-mtl`'s own flags refusal is now unreachable through
  `create_bind_group_layout`.** The seam's check fires first, so a caller asking
  for bindless on Metal gets the generic "descriptor-indexing flags on a device
  without DESCRIPTOR_INDEXING" rather than `plan_set`'s Metal-specific "flat
  argument tables have no runtime-sized array". Kept that way because one
  message per mistake across all backends is the point of the extraction, and
  the generic one names the actionable fact. `plan_set` keeps the refusal and
  its own tests exercise it directly; flipping the order is the fix if the
  specific wording turns out to matter more.
- **`crcbl-vk`'s
  `a_bindless_capable_layout_is_accepted_or_refused_according_to_the_tier` only
  `eprintln!`s the misplaced-`VARIABLE_COUNT` error** rather than asserting on
  it. It does fail if the refusal stops happening — that is how the vk call site
  was proven — but it would not notice the message becoming useless.

**Coverage, stated as a gap rather than implied.** The validator is proven
_called_ on three backends by a real run: the seam and null backend on the host,
`crcbl-vk` on an RX 7900 XTX, `crcbl-wgpu` on lavapipe — neutering
`check_entries` to return `Ok(())` reddens ten seam tests plus one test in each
of those two device suites. **`crcbl-dx12` and `crcbl-mtl` are type-checked only
here.** `crcbl-dx12` is entirely `#[cfg(target_os = "windows")]`, so a Linux
`cargo test -p crcbl-dx12` never compiles `binding::tests` at all, and
`--target x86_64-pc-windows-msvc` has no linker on this box; the `--target`
clippy runs are a type-check and nothing more. Those two backends' new tests
first _execute_ on CI's Windows and macOS runners, which is where their evidence
comes from.

## `BindingKind::StorageImage` has no way to say "reads _and_ writes"

`BindingKind::StorageImage` now carries `read_only`, `view_type` and `format`,
and `web/engine/gpu-replay.js` builds a real `GPUStorageTextureBindingLayout`
out of the three. What is left is the one distinction the `bool` cannot make.

The variant calls itself "a read/write storage image", so `read_only: false`
permits a shader that reads as well as writes. `STORAGE_TEXTURE_ACCESS` in
`gpu-replay.js` maps it to WebGPU's `'write-only'` and never to `'read-write'` —
**deliberately**, because WebGPU allows `'read-write'` on a much shorter format
list than the storage list itself (`r32uint`, `r32sint`, `r32float` in core), so
mapping `false` to it would refuse the `rgba8unorm` and `rgba16float` layouts
the seam actually asks for.

The narrowing fails loudly rather than silently: a WGSL module that reads
through a `write` binding is rejected at pipeline creation naming the binding.
So this is recorded rather than fixed. **Fixing it wants a decision**, and the
two options are:

- a third state on the seam — `StorageAccess { Read, Write, ReadWrite }` in
  place of the `bool` — with `crcbl-webgpu` refusing `ReadWrite` on a format
  WebGPU does not allow it for; or
- leaving the `bool` and documenting that a WebGPU target must declare the
  binding `write` in WGSL.

Nothing in `crcbl-render` or the committed shaders declares a storage image at
all, so neither option has a caller to prove itself against yet. Revisit when a
compute pass first wants one — a mip-generation pass is the likely first,
`docs/plan/03-gpu-driven-rendering.md` §3.2.

**`crcbl-wgpu` deliberately did not grow the arm.** It could:
`wgpu::BindingType::StorageTexture` takes exactly the three values the seam now
carries. It is scheduled for deletion once the other four backends reach parity,
so `map_binding_kind` still answers `HalError::Unsupported` and
`crcbl_hal::DIVERGENCES` classifies the row `Unwritten` — off the parity
blockers because `BackendKind::is_parity_target` answers `false` for it. Writing
the arm is maybe fifteen lines if anybody ever wants it before the crate goes.

## What the sun shadow pass owes

Topic 18's sun CSM landed at two cascades, GPU-driven, on every `GeometryPath`.
What is left, and what it taught:

- **Going to three cascades is the constant** — the atlas, the uniform block,
  the cull loop, the viewport loop and the tests are all parametric off
  `SHADOW_CASCADES` (ceiling 4, because `cascade_far` is one `float4`). It needs
  a re-bless of `cube.png` and a fresh look at the frame, which is why it was
  not taken in the same slice.
- **Single-sided geometry casts no shadow.** The shadow pass rasterises
  `CullMode::Back` and, with an amplification stage, cone-culls clusters facing
  away from the light — both correct for a closed caster, both discarding a
  one-sided wall. The open box's inward-facing faces therefore cast nothing. A
  two-sided caster mode needs `CullMode::None` on the shadow pipeline **and** a
  way to tell the amplification stage not to cone-cull for a light.
- **Each cascade's `DrawGen` duplicates pipelines it does not need**, building
  its own clear/cull/draw-argument compute pipelines and full argument buffers
  when only the cull half is used. Sharing pipelines across instances, or a
  cull-only constructor, is the follow-up.
- **The `shadow_placeholder` 1x1 depth image is forced from both sides.**
  Slang's Metal backend materialises every global into every entry point —
  `msl/mesh.metal`'s `vertexMain` really does take `shadow_atlas [[texture(1)]]`
  — so the depth-only pipeline's bind group cannot drop the slot; and WebGPU
  refuses a texture that is both an attachment and a bind-group resource in one
  pass, so the slot cannot be filled with the atlas it is writing.
- **The cascade fit is deliberately coarse.** A sphere centred on the eye also
  covers the half behind the camera, so a frustum-fitted box would be about
  twice as dense. Taken on purpose: a tight fit has to branch on `Projection`,
  and an orthographic camera has no field of view to build corners from.
- **Slang trap worth keeping:** `Texture2D<float>` lowers to WGSL
  `texture_2d<f32>`, which `textureSampleCompareLevel` does not accept — the
  artifact compiles and can never be made into a pipeline. `DepthTexture2D` is
  the spelling that lowers correctly on all four targets. Written into the
  shader.
- **Not verified:** Metal and D3D12 run no draw here, so a comparison sampler on
  either is type-checked only and CI is the first thing that will exercise it.

## What a sampled binding still cannot say

`map_binding_kind` assumes every sampled image is float-filterable and
single-sampled: `wgpu::TextureSampleType::Float { filterable: true }` and
`multisampled: false`, both constants. That is what every sampled binding in the
engine is. A shadow-comparison sampler (`TextureSampleType::Depth`), an integer
texture (`Uint`/`Sint`) or an MSAA source would each need another field on
`BindingKind::SampledImage`, and each would fail on wgpu the way the array did —
at pipeline creation, loudly. The other three backends would not notice, so
**the wgpu suite is the only local gate on it**:
`CRCBL_GPU=wgpu crates/crcbl/tests/run-render-e2e.sh`.

## Settled: the `D2Array` page samples on Metal and D3D12

Was an open coverage gap — `SampledImage { view_type }` is dropped by
`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` (each takes the dimension off the bound
view, and each says so at the arm that drops it), and neither Metal nor D3D12
runs a draw on this machine, so both were type-checked only.

CI confirmed it on `7c4042b`: `golden cube on metal` and `golden cube on dx12`
each came back **max channel delta 1, 0 over tolerance, 0 grossly wrong**
against the lavapipe-blessed golden. Metal is the one that mattered — it is the
only `ArrayPages` device, because it withdraws `Features::DESCRIPTOR_INDEXING` —
and its cube previously carried `max channel delta 207` with 2 pixels grossly
wrong, so agreement went strictly up. Kept only as the record that this was
checked and how; there is nothing owed.

## `crcbl_scene::meshlet`: decisions taken, and what it does not do

The §3.5 bake step exists as `build_meshlets` and has no producer and no
consumer. Decisions, so they are not re-argued:

- **It lives in `crcbl-scene`, not a crate of its own.** That crate's `lib.rs`
  already says its job ends at host memory — vertex arrays, index arrays — and a
  cluster builder is host-side geometry over exactly those. `GltfPrimitive` is
  its first producer. A crate would have been a fourth name for one
  responsibility.
- **It takes `&[[f32; 3]]` and `&[u32]`, not a `GltfPrimitive`.** Keeps it
  testable from literals and keeps the importer's private struct out of it.
  Deliberately **no** `GltfPrimitive::meshlets()` — that is a second caller that
  does not exist yet.
- **A dedicated `MeshletError`, against the crate's stated
  `StorageError`-for-everything convention** (argued in
  `crates/crcbl-scene/Cargo.toml`). The convention is about the IO seam; the
  builder reads no bytes, so every `StorageError` variant but `Other(String)` is
  unreachable and `Other` would erase which of the two caller bugs was hit.
  Reason is recorded in the manifest beside the dependency. Revisit only if a
  third error enum shows up in this crate.
- **Greedy sequential clustering**, no dependency. `meshoptimizer` would be a
  new dependency and that is the user's call; the simple form is deterministic
  by construction, which is what §3.5 actually requires.
- **Offsets are `usize`.** Narrowing them for the GPU is the later slice's call,
  and `u32` here would have needed a third error variant for overflow.

What it does not do, in the order it would be wanted:

- **No spatial pre-sort.** The walk follows index-buffer order and nothing else,
  so an incoherent mesh gets loose spheres and wide cones that cull almost
  nothing. This is the single biggest quality gap and it is a pass ahead of
  `build_meshlets`, not a change to it.
- **No per-cluster padding of `MeshletBuild::triangles`.**
  `MAX_CLUSTER_TRIANGLES` keeps a _full_ cluster's corner run a whole number of
  four-byte words, but a cluster the vertex bound closed early ends anywhere, so
  the next run is not aligned. A GPU slice that wants to read corners as `u32`
  has to add the padding; it was left out as machinery for a consumer that does
  not exist.
- **Determinism is verified same-process only.**
  `the_same_mesh_built_twice_gives_identical_clusters` cannot catch
  cross-machine float drift. A golden artifact is what would, and that belongs
  to the bake-cache slice.
- **The bounds are compile-time constants with no runtime configuration.** A
  device reporting a lower `maxMeshOutputPrimitives` than the ecosystem figure
  cannot be honoured; that is the capability slice's decision to make.
- **The bounding sphere is the AABB midpoint and the furthest vertex**, which is
  valid and not minimal. Ritter's or Welzl's would be tighter and neither was
  worth transcribing for a first cut.

### What the mesh-shader path owes, now that it draws

The layering question is settled the way it was recorded: the `Meshlet` record
lives in `crcbl_shaders::meshlet`, the builder stayed in `crcbl-scene` and
re-exports it, and `crcbl-render` gained no dependency on `crcbl-scene`. What is
left:

- **Only `apps/sandbox` constructs a `ForwardRenderer`, so only sandbox has a
  mesh to draw.** Worth knowing before reading too much into which samples
  select which `GeometryPath`: `EmitTail::from_caps` is the sole reader of
  `geometry_path()` in `crcbl-render`, the sprite pass records a plain
  `encoder.draw` and the UI pass a plain `encoder.draw_indexed`, and neither
  branches on a selector. The other samples ask for `MESH_SHADER` to satisfy
  sample rule 12 and to make the downgrade line name it — not for speed.
  Measured on horde at 10 000 instances: no difference, with the between-arm gap
  smaller than the within-arm spread. §3.5's exit criterion is about meshlets
  and cluster LOD, which none of these samples have.
- **`apps/hud` reports `IndirectPerBatch` / `ArrayPages` on an RX 7900 XTX**,
  which sample rule 12 arguably forbids. Its `desc()` omits `GPU_DRIVEN`
  deliberately — nothing in it issues an indirect draw, and it builds neither
  renderer — so the flag would have no consumer. Whether rule 12 outranks that
  reasoning is a decision, not an oversight.
- **`PRESENT_TIMING` is granted and still reports nothing.** radv grants both
  present flags — there is no downgrade line for them on vk at all — and every
  run still logs `hal: display timing Unknown; asked for Auto, pacing Vsync`. So
  the timing half of present support is negotiated and inert on this machine:
  the extension is there and the query answers `Unknown`. The feedback half is
  live and proven (`vkWaitForPresentKHR on present 1; the loop is closed`). Not
  investigated; it is `crcbl-vk`/engine territory.
- **Nothing automated asserts that a _game_ closes the present loop.**
  `crates/crcbl-shell/tests/run-wayland-e2e.sh` makes that assertion for
  `apps/sandbox` only. The four samples' new tests are **drift guards** — they
  assert `optional_features` equals the engine's — which is not the same as
  asserting pacing happened; that was verified by hand against a private
  headless sway session. Extending the wayland harness to cover a game is the
  fix.
- **`--headless --hardware` is the browser-gate flag pair that works here, and
  it is the silent-pass pair on a machine without a GPU.**
  `web/run-browser-e2e.sh`'s own header argues for Xvfb over `--headless` for
  exactly that reason: headless plus SwiftShader returns transparent black from
  the canvas readback rather than failing. It is safe on this box because there
  is a real GPU (`"hardware" adapter — amd rdna-3`), and it is being used
  because Chromium 151 broke the Xvfb path. Worth knowing before that flag pair
  is copied anywhere it would run without a GPU.

- **Settled: `Features::GPU_DRIVEN`'s doc was wrong, not its callers.** The doc
  said "never as a requirement" while nine call sites across five files pass it
  as `required_features` — `crcbl-render/src/{ui_pass,sprite_pass,texture}.rs`,
  `tests/graph_compile.rs` and `tests/ui_pass_stream.rs`. Every one is test code
  opening `NullInstance::gpu_driven()`, a preset that holds the bundle by
  construction, so there is no hardware to refuse and the requirement is a
  precondition assert — one with teeth, shown by degrading a preset and watching
  ten tests fail on `UnsupportedFeatures`. It is also load-bearing: the null
  backend grants `adapter.features ∩ (required ∪ optional)`, so naming a subset
  would change the device's selected path and quietly retarget those tests. The
  rule the doc defends binds a caller that must run on whatever device it finds,
  and no shipping caller violates it; the doc now says that, and the callers are
  unchanged.
- **`crates/crcbl-vk/tests/run-cross-backend-e2e.sh` does not echo its ICD pin**
  the way `run-render-e2e.sh` does, so which adapter it used is not observable
  from its output. It passed 6/6, but with `CRCBL_VK_ICD` set it still drew vk
  on the discrete card here.
- **Lavapipe reports `VK_EXT_mesh_shader`** (Mesa 23.2 and later), so CI's Linux
  and Windows vk jobs take the mesh path too — verified locally with
  `VK_DRIVER_FILES=…/lvp_icd.json vulkaninfo` and by a full local run on
  lavapipe at zero differing pixels. wgpu, WARP and Metal do not: each reports
  no `MESH_SHADER`, so those jobs keep drawing through an indirect tail and are
  the coverage that the fallback still works.
- **Settled: the cone cull rule needed a radius term, and it now has one.** The
  documented form was the point-sized one — it treats every triangle as sharing
  the centre's view direction, so a cluster with a real radius close to the
  camera could hold a front-facing triangle and be rejected anyway. The
  conservative form adds `+ radius`, and the derivation on
  `ClusterBounds::cone_cutoff` now carries it. Measured over 400 000 random
  samples: the corrected form dropped a front-facing cluster **0** times, the
  old one **11 225**. The `cone_cutoff > 0` guard is **not** subsumed by it —
  `sqrt(1 - cutoff²)` is even in `cutoff` and cannot tell a narrow cone from one
  wider than a hemisphere.

  **On shipped geometry the term only ever adds slack**, which is why it needs a
  constructed test rather than a picture: no mesh in the engine has a
  `cone_cutoff` strictly between 0 and 1 — a flat face gets `1.0`, a closed
  shape gets `OMNIDIRECTIONAL_CUTOFF` — and at `cutoff == 1` the two forms
  differ only where the camera has crossed the face's plane, i.e. for a
  genuinely back-facing cluster.
  `a_cluster_the_point_form_would_reject_survives_the_conservative_one` is the
  host-side case that actually pins the correction.

- **`CRCBL_BLESS` is suite-wide and there is no way to scope it to one golden.**
  Setting it re-blesses every golden the run reaches, so it cannot be used to
  regenerate a single image. The safe way to regenerate one golden is to delete
  that file and run **only** its test (`run-vk-e2e.sh -E 'test(name)'`), because
  a missing reference is created by `Golden::check` and reported as
  `Blessed { created: true }`, which the harness turns into a failure saying the
  run proved nothing. Worth knowing before someone reaches for `CRCBL_BLESS=1`
  to fix one image.

  **This entry used to claim that a suite-wide bless "fails fast on the first
  test that objects", and that is wrong — corrected 2026-08-15.** Adding the
  `EXTENT_ODD` goldens to `render_e2e` began with an unscoped
  `CRCBL_BLESS=1 run-render-e2e.sh`, and it rewrote most of the existing
  references in `crates/crcbl/tests/golden/` before the run ended. Fail-fast
  cannot protect anything here: nextest runs a process per test, so the other
  tests have already written their files by the time any one of them reports.
  The rewritten images still passed `Tolerance::RASTERISER` against the
  originals — the drift is real but within budget — which is precisely why this
  is dangerous: nothing goes red, and the blessed-on-lavapipe references
  silently become blessed-on-whatever-ran-it.

  Scoping the filter is therefore **required, not merely preferable**:
  `CRCBL_GPU=vk CRCBL_BLESS=1 crates/crcbl/tests/run-render-e2e.sh -E 'test(<name>)'`
  writes only the goldens those tests reach. A guard in the harness — refusing
  an unscoped bless, or refusing to overwrite a reference that already matches
  within tolerance — would be better than a note here, and has not been written.

- **The open box's golden is blessed on lavapipe**, like every other vk golden,
  so CI compares it at zero differing pixels and a local radv run drifts instead
  — 94.55 % of pixels differ at `max channel delta 1`, `0 over tolerance`. That
  is the split comparator working as designed, but it means this golden sits on
  the drift budget rather than near it: if `Tolerance::RASTERISER`'s
  `max_channel_delta` is ever tightened to 1, this is the first golden that
  fails, and it will fail only on the discrete card.
- **`group_is_live` in `mesh_cluster.slang` can no longer return 0** for any
  group the driver launches, now that the extents are the culled count. It is
  kept as a range check on buffer-sourced input, and its docs say so rather than
  claiming to be the filter. Removing it would also drop binding 12
  (`draw_args`) from that shader, renumber 13 and 14, shrink the forward pass's
  bind-group layout, and let the mesh path stop declaring `read_buffer(args_id)`
  — a binding renumber under Metal's declaration-order rule, on a path only
  Vulkan runs. Deliberately out of scope.
- **`crates/crcbl-vk/tests/vk_e2e/draw_gen.rs` poisons three of the clearing
  pass's buffers with a sentinel, and `mesh_args` is not among them.** The
  extent test's second lap covers the zeroing indirectly — an accumulating
  extent would double — but the sentinel path does not reach it.
- **`mesh_args` is imported with `final_state: IndirectArgument` on every
  geometry path**, so the two indirect tails pay one end-of-frame transition for
  a buffer they never read. That matches what `counts` already does on the mesh
  path; not changed.
- **Frustum rejection of these clusters is inherently marginal**, and the counts
  should be read with that in mind. A flat 1x1 face's AABB-midpoint bounding
  sphere has radius 0.707 — comparable to the whole mesh — so cluster spheres
  always straddle a side plane, and the _instance_ AABB cull is tighter than the
  cluster sphere cull. The only decisive rejections available are below or
  behind a camera inside the box, which is what the third test camera uses
  (margins of roughly 0.12–0.24 world units, about five orders of magnitude
  above f32 noise). A tighter bounding sphere — Ritter's or Welzl's, already
  flagged as not done on `ClusterBounds::center` — is what would change this.
- **The cluster count is per-frame-total, not per-bucket.** A per-bucket
  breakdown needs a wider stats buffer; the tests work around it by measuring
  each camera twice, with the open box in the scene and out of it, and
  attributing the difference.
- **`cargo clippy --all-targets` does not compile `crcbl-vk`'s e2e target** — it
  needs `--features vk-e2e`, so the bare command `CLAUDE.md` documents will not
  see a borrow error in `crates/crcbl-vk/tests/vk_e2e/**`. One got through both
  documented clippy gates in this session and surfaced only inside
  `run-vk-e2e.sh`. `cargo clippy --all-targets --all-features` **does** cover
  it, since `vk-e2e` is a real feature in that crate's manifest; the habit worth
  keeping is the `--all-features` form.
- **The cluster buffers are `HostUpload`, written once at build.** Device-local
  storage is what a bake cache that streams clusters would want.
- **No bake cache, no input hashing, no cluster LOD/QEM.**
- **Still undecided, and inherited from the builder:** whether meshlets are a
  bake artifact the renderer receives prebuilt (what §3.5 describes, and what
  keeps the dependency direction clean) or something built at `MeshPool::upload`
  time. Today neither — `crcbl-render`'s clusters come from constants in
  `crcbl_shaders::meshlet` (`cube_clusters`, `pyramid_clusters`), pinned against
  the real builder by `crcbl-scene`'s
  `the_hardcoded_meshes_cluster_the_way_the_shaders_crate_says`. That pinning is
  what stops the two drifting; it is not a substitute for deciding.

## The GGX slice: what it left owed (2026-08-13)

`GpuMaterial` gained `metallic` and `roughness` into the row's own padding
(`MATERIAL_STRIDE` is still 32), `mesh.slang` shades with one Cook-Torrance GGX
lobe driven by them, and `SPECULAR_POWER`/`SPECULAR_STRENGTH` are gone. The
decision and its two consequences are written up in
`docs/plan/18-render-features.md`. What this session did not finish, decided
against, or found on the way:

- **A metal is black until something reflects in it, and that is the model.**
  Ambient scales the diffuse albedo and a conductor's is zero, so a fully
  metallic surface out of every light's reach shades black. Nothing regresses —
  `GpuMaterial::UNTINTED` is `metallic 0.0` and no scene in the tree sets one
  higher — and the row that closes it is **screen-space reflections**, with
  irradiance probes behind it, both P7B in that file's delivery table. Until one
  of them lands, an author who reaches for `metallic 1.0` gets a black object
  and is not wrong about the shader.
- **A zeroed material row is no longer exactly black.** It is `metallic 0.0`, so
  its `F0` is the dielectric 0.04 and it carries a mirror-sharp four-per-cent
  highlight where a light happens to reflect off it. Still nothing anyone would
  mistake for an authored material, and `GpuMaterial`'s docs say so — but the "a
  row nobody wrote shades black" contract is now "shades black apart from a
  glint".
- **The importer reads neither `metallicRoughnessTexture` nor
  `baseColorTexture`.** `crcbl_scene::gltf_import` takes all three _factors_ off
  the one `pbr_metallic_roughness()` accessor and leaves both images alone, for
  the reason the base-colour one was already left: nothing here decodes an
  image, uploads one, or owns a layer of the renderer's page. The gloss map is
  the one whose absence is now _visible_ — a document that varies roughness over
  a surface arrives with the factor applied flat across it.
- **An imported default material is no longer `GpuMaterial::UNTINTED`**, and
  that is deliberate. glTF defaults a material to `metallic 1.0, roughness 1.0`;
  the engine's neutral row is a dielectric at half roughness. The importer
  reports the document. `gltf_import`'s module docs and its
  `GLTF_DEFAULT_MATERIAL` test constant are where that is written down, and the
  `assert_ne!` in
  `a_material_with_no_factors_takes_the_gltf_defaults_and_a_primitive_may_name_none`
  is what stops the two quietly becoming equal again.
- **Considered and declined: coupling the diffuse to `1 - F`.** Energy
  conservation would scale the Lambert term by one minus the Fresnel
  reflectance. It is a four-per-cent effect on every dielectric in the tree, it
  moves every 3D golden a second time, and it buys nothing until a material with
  a coloured `F0` exists. The lobe is plain Lambert plus GGX.
- **Considered and declined: keeping the `1 / pi` in `D`.** The engine's diffuse
  is a bare `albedo * N·L`, so the textbook normal distribution would sit a
  factor of `pi` under it. Folding the `pi` out of `ggx_lobe` is what puts the
  two lobes in one convention — argued in the shader and in
  `docs/plan/18-render-features.md`. It was measured, not assumed: with the `pi`
  in, `Scene::Lights`' green quadrant lost so much specular that the frame's
  brightest pixel there stopped leading its own channel.

### Two scene constants the new lobe invalidated

Both were calibrated against a Blinn lobe at a fixed strength of 0.35, which is
several times brighter than a four-per-cent dielectric GGX lobe at the same
angles. Neither assertion was weakened; the scenes were recalibrated.

- **`crcbl::screenshot`'s green point light.** `scene_lights`' own rule is that
  each light's colour is chosen against the material under it — and "the
  material" turns out to be the mesh's vertex colour as much as the row's
  factor. Every pyramid shows the same purple `+Z` face
  (`PYRAMID_SIDE_COLORS[2]`, whose blue is nearly three times its green), so the
  green light was the one fighting its own surface, and the Blinn highlight was
  what carried it. Its blue is now 0.1, the same as the red light's weakest
  channel. **This file is outside the slice's brief and the edit is one
  constant**; the alternative was to change what
  `each_point_light_pools_where_it_was_put_and_nowhere_else` asserts, which
  would have been weakening a test to fit the code.
- **`render_e2e`'s two-geometry-path comparison is no longer an exact byte
  compare.** It is now "at most `PATH_LSB_CHANNELS` channels differ, and never
  by more than 1". The mesh arm and the indirect arm transform a vertex through
  two different shaders (`mesh_cluster.slang`'s mesh stage, `mesh.slang`'s
  vertex stage) and are not obliged to contract their multiply-adds alike; a
  sharper highlight turns that last bit into a pixel where a broad one absorbed
  it. Measured: llvmpipe disagrees on **one** channel of the dunes frame, by
  one, out of 196608; radv and wgpu are still byte-for- byte identical on every
  scene. The budget is 16, two orders of magnitude under anything a level that
  failed to draw would produce.

### The render-e2e observable, and what it does not say

`the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one` in
`crates/crcbl/tests/render_e2e.rs` is the check that the lobe actually responds
to the column: `Scene::Cube`'s two top pyramids are the same mesh at the same
orientation under the same sun, and `crcbl_render::forward`'s
`PYRAMID_ROUGHNESS` is the only shading difference between their rows. It
measures the **falloff across one face** — inner block over outer block — on
both, and requires the smooth one's to exceed the rough one's by
`HIGHLIGHT_FALLOFF_RATIO`. Proven red: with both rows at one roughness it
measures 1.057 against 1.003 and fails; as the renderer writes them, 1.357
against 1.003.

- **It is a falloff and not a width, and the brief that asked for it wanted a
  width.** "Brighter at its centre and narrower across it" needs the lobe's
  centre to sit inside the measured surface with room either side. Under a
  _directional_ light on a _flat_ face the half-vector sweeps monotonically
  across the face, so the highlight's centre is at the face's inner edge and the
  frame shows one flank of the lobe. The falloff across that flank is the same
  claim by the only statistic this geometry supports. A scene with a curved
  surface, or `Scene::Spot`'s floor with two materials on it, is what would let
  the width be measured directly — and neither exists.
- **The right-hand pyramid is the only surface in the frame at the mirror
  direction.** `DirectionalLight::default`'s sun comes from `+X` and that
  pyramid stands at `+X`; the left-hand one's face never reaches the reflection
  angle, which is why the same pair of roughnesses leaves it flat and why it
  works as the control. The consequence is that the roughness edit had to go on
  the _tinted_ row, so `material_rows`' "one row and two single-column edits"
  invariant is now "two edits, neither of which can be mistaken for the other".
- **Not covered: a metal.** No scene sets `metallic` above zero, so the `F0`
  interpolation and the `1 - metallic` on the diffuse albedo are compiled,
  type-checked and never exercised by a rendered pixel. The first scene with a
  conductor in it is what would test them, and it wants SSR to look like
  anything.
- **Not covered locally: Metal, D3D12 and wasm.** The lobe was run on lavapipe,
  radv and wgpu-on-radv. `msl/mesh.metal` and `dxil/mesh.fragmentMain.dxil` were
  regenerated and compile, and CI is the only thing that can say the frame they
  draw matches.

## Re-affirmed: shader artifacts stay committed (2026-08-13)

Asked directly whether the shaders should be built during `cargo build` so no
binaries live in the repo, and whether committing them is standard practice.
Answered no on both counts, and recorded here so it is not re-argued from
scratch.

**Committing prebuilt shaders is a minority pattern**, not an industry standard.
The common camps are: compile at build time (Khronos' Vulkan samples, most CMake
projects calling `glslc`); ship text and compile at load (WGSL in wgpu, MSL,
HLSL through `D3DCompile`); a cook step into a derived-data cache (Unreal,
Unity); and committing binaries, which is what this repo does.

What makes the choice narrower than it first looks:

- **Two of the four columns are already text.** `wgsl/` and `msl/` carry
  `text eol=lf` and are source in every meaningful sense — `crcbl-mtl` compiles
  the `.metal` at device init, which is the load-time camp exactly. Only SPIR-V
  and DXIL are binary, and that is intrinsic: Vulkan consumes only SPIR-V, D3D12
  only DXIL.
- **The size cost is nil.** Every SPIR-V and DXIL blob across the whole history
  is 186 objects and 0.8 MiB, against a 167 MiB `.git`. Repo weight is not the
  argument either way. The real cost is review noise — a shader change shows as
  `Bin 24516 -> 25524 bytes`, which no reviewer can read.
- **`dxc` is the actual obstacle.** `pinned_dxc` has no `PATH` fallback because
  distributions ship Shader Model 6.10 preview builds that abort on this source,
  so there is no package-manager path to a working one; Slang is a GitHub
  release tarball for the same reason. Building at compile time therefore means
  every contributor's first `cargo build` and every macOS, Windows and wasm CI
  leg acquiring two pinned toolchains no package manager provides — in practice
  a download inside `build.rs`, which puts the network in the build, or a
  vendored compiler far larger than the artifacts it replaced.
- **The pin is needed either way, and asymmetrically.** Committed artifacts need
  the pinned toolchain in one CI job, to verify. Build-time compilation needs it
  in every build on every platform. Build-time is the more demanding position,
  not the cheaper one.

**What would change the answer:** topic 6's runtime recompilation for shader hot
reload at P9. That makes a `slangc`-shaped compiler a dependency anyway, and if
it is present for hot reload the argument for committing SPIR-V weakens a lot.
Revisit then, not before.

**One real gap the question surfaced, now fixed:** `.gitattributes` marked
`*.spv binary` and never `*.dxil`, so DXIL was covered only by git's
NUL-sniffing heuristic under the file's own `* text=auto`. Nothing was being
corrupted — git does call it binary today — but the block's stated rule is that
an artifact whose bytes are a checked invariant should not rely on a heuristic,
and DXIL is hashed in `spirv/manifest.txt` like everything beside it.

**Coverage gap this leaves standing:** the committed bytes are only ever
_verified_ by a machine that has the toolchain, which is the one `shaders` CI
job. If that job were skipped or broken, drift would reach `main` and every
other leg would build the stale artifact and pass. The manifest hash catches a
source edited without regenerating; it cannot catch a manifest regenerated
against a source that was then not committed, which is what the recompile step
exists for and which only that job always runs.

**Editing a comment in a `.slang` rewrites `msl/` and nothing else** —
surprising but not a bug, and worth knowing before it is diagnosed a second
time. Slang's MSL backend emits `#line` directives pointing back into the
`.slang`, so adding or removing a comment line shifts them and changes the
`.metal` bytes and its `msl-sha256`. Measured on the comment above
`mesh.slang`'s `float3 lit = …`, which grew by four lines: the entire
`msl/mesh.metal` diff was three `#line` directives moving by exactly four, and
`spirv-sha256`, `wgsl-sha256` and both `dxil` hashes were unchanged. So the
other three backends are comment-invariant and MSL is not. Two consequences: a
comment-only shader edit still has to be regenerated like any other (the
manifest hashes the **source**), and the `msl/` churn in that diff is noise
rather than codegen — a reviewer should read the `#line` numbers and stop, not
go looking for what moved.

## `apps/lumen` is at milestone 1a: what it owes next (2026-08-14)

The entry this replaces said lumen could not be built because the engine had no
way for an app to describe a scene. That is gone: the six scene-API slices
landed and `apps/lumen` renders the charter's room from a `SceneDesc` of its
own. What follows is what the sample still owes, and the findings the first real
room produced. `docs/plan/sample/13-lumen.md` carries the status.

### Owed, in the order a slice would take them

- **Ray tracing** — acceleration structures, `LightingPath::RayTraced`, and the
  side-by-side and A/B-flip modes the charter's milestones 2 and 3 want. The
  selector already exists and every device in the tree resolves to `Rasterised`
  because nothing builds a structure; lumen's panel says so on a row rather than
  implying a choice was made.
- **The render-to-texture monitor camera**, and with it a consumer for the
  camera-stack layer of the toggle resolution order. One camera per frame today.
- **The rest of milestone 4's matrix.** The three effect rows are on the pause
  menu now, doing read-modify-write on the programmatic layer. What the charter
  asks for beyond them — the side-by-side and A/B-flip comparison modes — needs
  the monitor camera above, and the toggles' `--help` text still describes only
  the flags, with no line pointing at the rows the way `--camera`'s does.
- **`UNAVAILABLE` has never run on hardware.** The effect rows report a
  device-clamped effect as unavailable rather than off, and a press on such a
  row is a deliberate no-op. Both are covered by a constructed device set only,
  because no device in this tree clamps any of the three effects. It is the same
  shape as the shadow-atlas rule considered and declined below: the arm exists
  and nothing can reach it here.
- **The Pages web demo shipped**, and what is left of it is one thing: the
  browser gate cannot run on a software adapter. See "lumen's browser gate needs
  a real GPU" below.
- **Sound.** Rule 8 says no sample ships silent after P4A. lumen has no audio at
  all, and it is not obvious it should: it is an acceptance fixture with no
  events, and `hud` — the other fixture — is the precedent for a sample with no
  cue grammar. **Left as a decision rather than as work**: either lumen claims
  the exemption in its own doc the way rule 11's is claimed, or it gets a hum
  positioned at the lamp, which would at least give the moving light an audible
  correlate.

### Findings the first real room produced

- **The sun's shadow peter-pans at contacts.** A lit strip along the foot of
  every wall the sun should be shadowing, and a sawtoothed band at the head of
  the back wall where the ceiling should be. **Diagnosed and largely fixed** —
  see "the sun's shadow still peter-pans" below. It was one defect rather than
  the two this bullet used to claim, and the strip was 0.60 m rather than the
  "metre wide" it said, a figure that had reached `room.rs`'s `SHADED_FLOOR` doc
  comment and is corrected in both places.
- **A single-quad wall casts no shadow at all.** Back faces are culled in the
  shadow pass as well as the colour one, so an inward-facing quad is invisible
  to the sun. lumen's first frame was an evenly lit floor with a window that did
  nothing; the room is built of slabs for that reason and `room::SHELL` records
  it. Worth knowing before the next scene is authored: it is not a bug, it is
  what `CullMode::Back` means on an open surface, and nothing warns about it.
- **A gap in a shell leaks light and reads as an artefact.** Stopping lumen's
  ceiling at the room's own footprint left a slot over the top of every wall;
  the sun came through the one above the window wall and laid a band along the
  back wall that looked exactly like a shadow-map failure. The ceiling caps the
  walls now. Same class as the row above: authoring hazard, not an engine
  defect.
- **`crcbl::screenshot::Scene` is not where lumen belongs, and that is
  decided.** Considered: adding a `Scene::Lumen` variant so
  `crcbl screenshot --scene lumen` would work. **Declined** — the room is an
  _application's_ scene description and putting it in `crates/crcbl` would make
  the engine own sample content, which is the exact thing this sample exists to
  prove is no longer necessary; and the enum's stated job is one variant per
  engine shader pair that has pixels of its own, which lumen adds none of. What
  it needed instead was a way in: `OffscreenSetup::open_forward` takes a
  caller-built `ForwardScene` and reuses the surface, adapter pin, ring,
  readback barriers and row unpadding. That is rule 1 working as designed — a
  sample needing a backdoor is an engine API gap, filed and fixed in the engine.
- **`crcbl new` scaffolds a shape lumen would have had to undo.** The template
  is one `src/main.rs` with a bin target and a `Game` with a simulation in it.
  lumen needs a lib target — an integration test cannot reach a bin crate's room
  — and has no simulation. Not a defect in the template, which is aimed at
  games; recorded so the next fixture does not start from it either.
- **`OffscreenSetup` leaked a swapchain and a surface when a scene refused.**
  `Scene::Dunes`' "no amplification stage" arm destroyed its own renderer and
  returned, leaving both behind. Fixed in the same change as `open_forward`,
  because the new entry point made the refusal path reachable from an
  application.
- **Coverage gap: nothing in the tree asserts a debug row's _text_.**
  `apps/lumen/src/app.rs`'s `f3_shows_the_path_report_and_the_unbuilt_notice`
  checks that the section titles exist and that the row _labels_ — `geometry`,
  `lighting`, `metal`, `mode` — reached the draw list. The value each row
  carries is asserted nowhere: `apps/lumen/src/gpu.rs`'s `row_str("metal", …)`
  changed from calling the metals black to calling them reflection-lit and no
  test went red. Verified by grepping the whole tree for both strings; only the
  `gpu.rs` call site has either. **A test that closed it** would read the row's
  value back off the panel — the section's rows rather than `ui_text`'s labels —
  and assert the `metal` row names a reflection, so a row that goes stale
  against the shader fails instead of drifting. **Not written, because it is a
  judgement rather than a correction:** pinning the panel's prose in a test
  makes every wording pass a test edit too, which is friction on exactly the
  copy that should stay current; against that, the panel is lumen's user-visible
  claim about what the renderer did, and it has now been wrong once. Whether the
  assertion should be on the exact string or on a keyword the wording must
  contain is the same call.

## The AO tuning constants, measured against a real frame (2026-08-13)

Two entries above — under "What screen-space AO left owed" and "What the
depth-weighted blur left owed" — say `SSAO_RADIUS`, the kernel's lateral reach
and `DEPTH_TOLERANCE_RADII` were tuned against `Scene::Ao` alone and that
`lumen` is what would tune them. This is what `Scene::Ao` could say on its own;
the section after it is the same three questions asked of lumen's room, which
now exists. **Nothing has been retuned in either**; the numbers are here so the
retune has a starting point.

Measured on this box — AMD RX 7900 XTX, radv, Mesa 26.1.6,
`MeshShader / Bindless / Rasterised` — from
`crcbl screenshot --scene ao --size 1280x960`, and from the render-e2e suite's
own printed numbers at its 256×192.

- **The trough is already room-scale in one axis, which the entries do not
  say.** `AO_RUN` is 6.0 and `AO_WALL` is 2.0 world units: a six-metre run
  between two-metre walls. What `Scene::Ao` is missing is not scale, it is a
  camera at eye height inside the box, a corner where three surfaces meet, and
  anything to cast a silhouette. The straight-down camera is load-bearing for
  the measurement it exists for and should not be changed to get those.
- **The occlusion reaches 0.38–0.40 world units from the wall**, against a
  kernel whose stated lateral reach is `7/8 × SSAO_RADIUS` = 0.4375. Floor luma
  down the middle of the run, averaged over a 3-unit-wide strip: 74.66 on open
  floor, falling to 63.31 at 0.10 from the wall, back within one percent of open
  floor by 0.38. So the term is **bounded by the kernel and not by the
  geometry** — `SSAO_RADIUS` is doing exactly what it says, and the trough is
  wide enough not to clip it.
- **So it reads as a broad ambient wash, not as contact occlusion.** A 0.4-unit
  gradient against a 2-unit wall is a fifth of the wall's height. Contact
  occlusion in a room wants a tighter band; 0.5 is at the top of the usual
  range. **This is the finding a retune would act on**, and it is a judgement
  about looks, so it wants goldens and a human, which is what the deferral said.
- **The gradient terraces.** 63.31 → 74.66 is about eleven sRGB levels spread
  over roughly 150 pixels at 1280×960, so a band every fourteen pixels or so.
  Invisible at the goldens' 256×192, where the whole gradient is ten pixels
  wide, and visible in a contrast-stretched crop of the full-resolution frame.
  In a room lit mostly by ambient — which is what a lighting fixture is — that
  banding is what a reviewer would notice first, ahead of the radius.
- **The bilateral blur holds at a silhouette, with about a fortieth left.** The
  render-e2e's own reading on this GPU:
  `cube — the pyramid's underside measures 67.3 along its silhouette and 66.0 two rows in, against a clear of 37.0`
  — a residual halo of 2.0%, matching what the blur entry claimed. Against a
  _real_ silhouette rather than a pyramid's underside, nothing has been
  measured, because no scene in the tree has one.
- **`DEPTH_TOLERANCE_RADII` remains unmeasured and that is not fixable here.**
  It weights the blur by view-space depth difference, so what exercises it is
  two surfaces at different depths sharing a kernel footprint. `Scene::Ao` has
  one flat floor and two walls at the same depth as it, and `Scene::Cube` has
  one silhouette against the clear. Neither separates the tolerance from the
  far-plane test beside it. A room with furniture is still what would.
- **Where these came from**, so they can be reproduced: `SSAO_RADIUS` is a
  private `const` in `crates/crcbl-render/src/forward.rs`, the lateral reach is
  the sample table in `crates/crcbl-shaders/shaders/ssao.slang`, and
  `DEPTH_TOLERANCE_RADII` is a `static const` in `ssao_blur.slang`. None is
  reachable from an app, which is its own small finding: a retune is an engine
  edit and a re-bless, not a sample knob.

## The AO constants against lumen's room (2026-08-14)

The measurement the entry above could not make, now that there is an eye-height
camera inside a real room. Read off `apps/lumen/tests/golden.rs`'s own
projection at 1280×960 on AMD RX 7900 XTX, radv, Mesa 26.1.6,
`MeshShader / Bindless / Rasterised`, averaging a 21×21 block about each world
point. **Nothing was retuned.**

- **AO reads as contact occlusion in a real room, and its reach is the kernel's
  rather than the geometry's.** Floor luma approaching the back wall, in the
  ambient-only part of the room: flat at 77–80 from 1.4 m out down to 0.5 m,
  then 71.5 at 0.35 m, 60.5 at 0.20 m, 61.5 at 0.05 m. A 25% darkening confined
  to the last 0.35 m. The same shape on a wall going up from the floor — 53.8 at
  0.12 m, recovering to 63.7 by 0.6 m — and at the metal block's contact with a
  _sunlit_ floor, where AO touches the ambient alone and still cuts 172 to 130
  over the last 0.3 m.
- **So `SSAO_RADIUS = 0.5` is sane at room scale**, and the "broad ambient wash"
  reading the `Scene::Ao` entry above reports is a property of that scene rather
  than of the constant: 0.4 units against a 2-metre trough wall is a fifth of
  it, and the same 0.4 units against a 3-metre room wall with an eye-height
  camera reads as the band under a skirting board. **The finding the earlier
  entry proposed a retune on does not survive the room it asked for.**
- **The three-surface corner is measurably darker and is the weakest of the
  three.** Down the diagonal into the floor–back-wall–coloured-wall corner: 99.5
  at 0.40 m, 95.6 at 0.30 m, 83.4 at 0.20 m. A 16% cut where two walls close
  most of the hemisphere, against the 25% a single wall gives. Not chased: it is
  in the sunlit part of the floor, so ambient is a smaller share of the pixel
  there, and separating the two needs the AO-off frame nothing can produce yet.
- **`DEPTH_TOLERANCE_RADII` finally has something to be measured against, and
  shows no halo at it.** The room puts two surfaces one to two metres apart in
  view depth inside one kernel footprint at three places: the metal block's
  vertical silhouette edge, the plinth's, and the mirror panel's against the
  back wall. Row profiles across each: the block's edge goes 132.3 to 93.2 in
  **one** pixel with no gradient on either side, and the panel's goes 0.0 to
  57.0 in two. So the depth-aware blur is not smearing a near surface's
  occlusion onto a far one at 2.0 radii.
- **That is an upper bound on the artefact, not a tuning measurement, and the
  reason is the missing toggle.** Every number here is AO folded into a shaded
  pixel. Isolating the term needs the same frame with the occlusion pass off,
  which is the first row of the lumen entry above. Until that exists,
  `DEPTH_TOLERANCE_RADII` can be said not to be _visibly_ wrong and cannot be
  said to be right.
- **A finding worth more than any of them: the sun's shadow bias contaminates
  the floor's occlusion profile near a wall.** The first profile taken —
  approaching the `-x` wall — read 49 at 0.8 m and 138 at 0.25 m, which is
  backwards. That is the peter-panning recorded in the lumen entry above, not
  AO. Anyone repeating this measurement must take it on a wall whose foot the
  sun does not reach, which is why the numbers above are from the back wall.

## The sun's shadow peter-pans on a facet seam, at 0.26 m (2026-08-14)

Two slices have run at this. The bias was re-denominated from cascade clip depth
into cascade texels (`DEPTH_BIAS_TEXELS`, `sun_visibility`), then its slope term
was moved from the interpolated shading normal onto the rasterised facet
(`geometric_normal_of`, `shadow_slope`). `apps/lumen`'s wall-foot strip went
0.601 m → 0.382 → 0.256, and the band down the back wall's left edge 0.579 →
0.373 → 0.244. Both shipped. What is left is below.

### What the second slice found, against what this entry predicted

**This entry predicted the constant would fall to about half a texel once the
slope read the geometric normal. It did not — it fell from 6.0 to 3.0.** The
prediction was wrong in an informative way, and the reason is a second artefact
the first diagnosis did not see.

The geometric normal removes the _broad cross-hatch_ over the dunes' valley
floor, which is real and is what six texels were paying for. Underneath it is a
**facet seam**: adjacent triangles of a tessellated surface climb at different
rates, each is biased by its own slope, and the texel their shared edge falls in
stores the steeper one's depth. No slope read off either facet predicts the
other's, so a constant still covers it — a smaller one. Measured, slope
coefficient held at 2.0:

| `DEPTH_BIAS_TEXELS` | lumen's strip | dunes, shading `N` | dunes, facet `Ng`   |
| ------------------- | ------------- | ------------------ | ------------------- |
| 0                   | 0.128 m       | heavy cross-hatch  | seam on most edges  |
| 1                   | 0.170 m       | —                  | seam on some edges  |
| 2                   | 0.213 m       | faint cross-hatch  | a few isolated dots |
| 3 (shipped)         | 0.256 m       | —                  | clean               |
| 6 (was)             | 0.382 m       | clean              | clean               |

Shipped at 3.0 with **no margin above it**, deliberately unlike the 6-over-5 it
replaced: six covered an unexplained shortfall, three covers a bounded and
understood one, so margin here is lumen's strip bought back for nothing. 4.0
would cost about 0.29 m if that judgement is ever revisited.

**The facet-seam mechanism is inference from the pictures, not instrumented.**
What is measured is that the broad hatch goes and a dotted hairline on triangle
edges appears in its place; the account of why fits and was not confirmed by
reading the shadow map.

### What would take the strip below 0.26 m

Nothing cheap. The remaining constant is covering a real quantity, so lowering
it alone brings the seam back — the table is the evidence. Removing the seam
means biasing per-edge rather than per-facet, which nothing in the tree does and
which is a research task rather than a slice. **Recorded as the floor of this
approach** rather than as work: 0.256 m on a 0.15 m shell is a shadow that still
detaches, and the next real gain is more likely to come from the shadow map's
resolution at the contact than from the bias.

### Still open

- **The sunlit shaft over-reaches its sill edge by 0.185 m**, unexplained by the
  bias at any denomination — the sill silhouette is 2.99 m from the receiver.
  Suspected sub-kernel occluder, the sill's top face being narrower than the PCF
  footprint. **Not measured**, and untouched by either slice.
- **The cornice metric never reconciled between the two slices.** The first
  reported the band at the back wall's head going 112 luma → 5.7; the second,
  measuring peak luma in the band over the wall below it, read 61 on the
  _unmodified_ tree and 21 after. The second slice's pair is matched and
  reproducible; the first's statistic was not recorded and could not be
  recovered, and the figure has been dropped from `CHANGELOG.md`. **Only the
  second pair should be quoted.**
- **Cascade 0 is still unmeasured** across all of this. Every artefact measured
  is in cascade 1 — the nearest floor point the fixed camera reaches is 4.74 m
  against a 4.699 m split.
- **The geometric normal's sign was measured on SPIR-V/radv only.** It is
  derived from the shading normal rather than hard-coded, which makes the other
  three targets correct by construction; none of them was run.
- **One backend throughout.** vk on radv, cross-checked on lavapipe, which
  improved slightly on both slices. wgpu, mtl and dx12 were not run;
  `run-cross-backend-e2e.sh` was not run for either change.
- **The dunes acne grading is visual**, backed by amplified difference maps. A
  radius-4 high-pass misses the cross-hatch entirely — its wavelength is tens of
  pixels — and a deficit-against-clean-reference measure conflates acne with
  legitimate shadow tightening at low bias. No numeric acne metric in this tree
  is trustworthy on its own.

## Run the local `cargo doc` gate with `--all-features`, as CI does

**Corrected 2026-08-15.** The entry this replaces said `crcbl-sprite` had six
unresolved `crate::bake` doc links and concluded "a `bake` module was removed or
renamed and its referrers were not followed". **That diagnosis was wrong.**
`crates/crcbl-sprite/src/bake.rs` exists and is declared in `lib.rs` behind
`#[cfg(feature = "bake")]`, and the `bake` feature is off by default because it
pulls in the PNG encoder a runtime consumer would never call.

So the six warnings are an artefact of **how the gate was invoked**, not a
defect in the crate. A bare `cargo doc --workspace --no-deps` unifies the `load`
feature on — something else in the workspace asks for it — while nothing turns
on `bake`, so `load.rs` compiles and its links into `bake` dangle. CI's `docs`
job in `.github/workflows/ci.yml` runs `--all-features`, under which every one
of them resolves; `cargo doc --all-features --no-deps -p crcbl-sprite` was run
to confirm and emits **zero** warnings.

**Re-derived independently on 2026-08-18, and it is the same answer.** A later
pass found the six links again and wrote them up as a live defect with three
unattractive fixes — merge the features, blanket-allow the lint, or duplicate a
sentence per link. That entry has been deleted rather than kept beside this one,
because two entries disagreeing about whether something is a defect is worse
than either.

What the second pass did add is a measurement the first left open: **`bake` and
`load` gate the identical dependency set** — both are `["dep:png"]` — so merging
them costs nothing in dependencies and only widens what a `load`-only consumer
compiles. That is a public-surface decision rather than a fix, and nothing
forces it: `cargo doc --all-features -p crcbl-sprite` was re-run today and still
emits zero warnings.

**The lesson worth keeping is about the gate, not the crate.** A feature-gated
module is normal Rust and an intra-doc link into one is not a defect, but a doc
run whose feature set differs from CI's produces warnings CI never sees — and
standing phantom warnings are exactly the noise that hides a real one. That
happened twice in one session here: two separate readings took the six at face
value, and the second wrote this entry's wrong diagnosis. Match CI's invocation
when running `cargo doc` locally.

**Considered and not taken:** `[package.metadata.docs.rs] all-features = true`
would make published docs complete regardless. No crate in this workspace sets
that metadata today, so adding it to one is an inconsistency and adding it to
all is a convention nobody has asked for. Worth doing as a deliberate sweep if
these crates are ever published in earnest.

## Irradiance probes: the slice plan (designed 2026-08-14)

The design is `docs/plan/18-render-features.md`'s "Irradiance probes: the
design" — a static grid of L1 spherical-harmonic probes in a read-only storage
buffer, adding no render pass, added to `frame.ambient` for diffuse and returned
by an SSR miss for specular. Read it first; this is only the order to build it
in and the decisions still open.

All slices are built. The seam still permits a read-only storage binding of a
host-visible buffer, and appending the mesh binding after
`AMBIENT_OCCLUSION_BINDING` needed no `mesh_cluster.slang` mirror for the same
reason occlusion did not.

### The slices

1. ~~The probe table, additive and zero.~~ **Shipped** as `ce253ad`.
2. ~~`Scene::Probes` and its golden.~~ **Shipped in this slice:** the
   replacement clamps most of the floor to either endpoint probe and confines
   interpolation to a narrow central band. The e2e fixture compares both
   endpoint colours and the centre against the Rust mirror, asserts the outer
   regions stay flat, and runs both geometry paths. See "the probes fixture is a
   full-frame gradient" below for why the first version was reverted.
3. ~~lumen's room gets a volume.~~ **Shipped in this slice:** `bounce::probes()`
   computes the sun's analytic first bounce from the room's own constants. The
   coloured wall measurably tints the plaster beside it, and the golden moves
   for the reason its module docs name.
4. ~~The specular fallback.~~ **Shipped in this slice:** `SsrParams` carries
   `inv_view` and the probe volume, the SSR pass binds the existing read-only
   probe table, and misses return its approximate L1 radiance through the same
   Fresnel term as hits. The stored diffuse-irradiance rows have their per-band
   clamped-cosine transfer removed before evaluation. The arithmetic keeps the
   old hit multiplication order, so a zero volume adds exact zero rather than
   changing half-float rounding. lumen's authored-versus-zero control moves the
   SSR miss from 20.3 to 0.0 while its real hit moves 51.6 to 49.0, within the
   measured 6% budget.
5. ~~The roughness cutoff decision.~~ **Shipped without raising it:**
   `ROUGHNESS_CUTOFF` gates only screen-space marching. Rough surfaces still
   evaluate probe environment specular and return it with exact-zero sharpness,
   so the blur composites that centre fallback directly. Positive sharpness
   blends continuously through a square-root filter share; unlike the first
   linear share, it keeps the existing SSR fixture below its fixed-stride
   stepping limit. The reflectivity attachment stores sharpness rather than
   quantised roughness so the cutoff endpoint survives `Rgba8Unorm` exactly;
   Vulkan readback asserts alpha zero. lumen's fully metallic brass at roughness
   0.55 measures 97.4 with authored probes and 89.7 with zeroed rows.
   `Scene::Probes` disables reflections so its diffuse Rust-mirror contract
   remains absolute.

**The cheapest useful version is not a separate slice.** One probe and a 1×1×1
grid is slice 1 with `probes.len() == 1` — the ambient becomes directional,
which is most of what a probe buys on a picture, and lumen's panel goes
non-black across its whole face. Stopping after slice 4 with a single probe
requires rewriting nothing to add the grid later. The grid is recommended anyway
because a room needs light that differs between the window and the far corner.

### Decisions

- **Q1: does the probe half of the environment specular evaluate above
  `ROUGHNESS_CUTOFF`?** **Resolved yes.** A wide lobe is where the low-frequency
  probe is more honest than one screen-space ray. The cutoff therefore gates the
  march only; rough surfaces return probe environment with zero sharpness, and
  the blur composites that centre value without filtering. This keeps
  `UNTINTED`'s exact-zero march endpoint without leaving lumen's brass black.
- **Q2: does this get a `RenderEffects` bit?** **Resolved no.** The off-switch
  is the scene, and a zero volume is bit-identical, so there is nothing to
  resolve through four layers. `effects.rs`'s own rule is that an effect which
  is off is a frame with fewer passes and never a shader branch — a probe bit
  removes no pass. If lumen's milestone-4 matrix wants a row anyway, it should
  swap the bound table for the zero one, which is still data and still no
  branch. It is public API shape, so it is yours.

### Named limits, so they are not rediscovered

- **Light leaking is the grid's real weakness** — a probe inside a wall lights
  the room beyond it. lumen's room is a single box so it will not show there; a
  scene with two rooms will. The literature's answers are per-probe visibility
  or DDGI's depth moments, neither in scope.
- **An L1 probe in a mirror is a gradient, not a room.** Fixing that is
  prefiltered radiance cubemaps, which need the filtered read `ssr.slang`
  refuses. Trigger: when somebody looks at lumen's panel and objects.
- **With `REFLECTIONS` off, metals go black again**, because the reflection pair
  is what draws the environment specular. Coherent rather than a defect, and
  `--no-reflections` showing it is honest.
- **The bake tool is deferred on a hard prerequisite**, not on taste: a gather
  bake needs a ray-triangle intersector and a BVH, and `crcbl-phys` has neither
  — only ray-vs-sphere, ray-vs-AABB and ray-vs-capsule.

### Not verified

The design was produced without running anything: every number in it is a
constant read out of a source file. No frame, no benchmark, no test.

## The probes fixture is a full-frame gradient, and WARP will not have it (2026-08-14)

`a5f0e29` added `Scene::Probes` and `a88d671` reverted it. **The probe maths was
not what broke** — on the WARP run that failed, the shader and
`crcbl_shaders::probe`'s Rust mirror agreed to 0.07 levels and the two geometry
paths were bit-identical, so the evaluation is right on that device. What failed
was the golden: max channel delta 8, **1212 pixels (2.47%) over tolerance
against `Tolerance::RASTERISER`'s 1% budget**.

### Why, and it is a lesson about fixtures rather than about probes

The fixture was designed so that **every pixel is the probe term and nothing
else** — ambient exactly zero, the sun parallel to the floor so Lambert and the
specular lobe vanish, the measured bands twice the occlusion radius from any
wall. That makes the anti-vacuity argument airtight and it is why the shader
could be compared against the mirror absolutely rather than only as a ratio.

It also makes the whole frame one smooth gradient, which removes the margin an
8-bit golden lives on. Every other scene's cross-driver drift is confined to
edges, so a handful of pixels exceed tolerance and the _ratio_ stays tiny —
`point_shadow` on the very same WARP run has max channel delta **34** and
passes, because only 0.057% of its pixels are affected. A gradient spanning the
frame has no such confinement.

**The two properties are in tension and that was not seen when the design was
written.** "Every pixel is the effect" and "an 8-bit golden survives four
rasterisers" pull against each other, and this is the first fixture in the tree
where the effect covers the whole frame rather than a shape inside it.

### Rebuilding it: two options, and the numbers needed to choose

- **A scene-scoped budget**, the way `path_lsb_channels` in
  `crates/crcbl/tests/render_e2e.rs` already scopes an allowance to
  `Scene::Dunes`. Honest if the argument is written down — the 1% ratio was
  derived for localised edges, not for content that is gradient everywhere.
  Dishonest if it is picked to be whatever makes WARP pass, which is the trap.
- **A fixture that is not gradient-dominated**: keep the probe term as the only
  term but give the frame flat regions — facing quads at distinct normals rather
  than one floor across the interpolation. The ratio assertion survives; the
  golden regains its margin.

**Neither can be chosen from this machine.** The failure only appears on dx12
under WARP, which needs Windows, so any fix validated locally is a guess pushed
to CI. Get the WARP numbers for a candidate fixture before blessing anything —
the previous attempt passed radv, lavapipe, both geometry paths and a four-way
negative control, and still broke `main`.

### Not at issue

`ce253ad` (slice 1) was never implicated and stays. The design's determinism
argument — that probe evaluation has no comparison between fetched values to
diverge on — was _supported_ by this run, not contradicted: radv against
lavapipe is max delta 2, and WARP agrees with the host mirror to 0.07 levels.
The 8-bit golden is the fragile part, not the arithmetic.

**The first replacement preserved the semantics but not WARP's budget.** With a
`0.4`-unit interval, WARP still reported 1,216 pixels over tolerance — 2.4740%
of the frame, effectively the reverted fixture's result — even though every
semantic check passed and the shader agreed with the Rust mirror to 0.20 levels.
The gradient had been confined, but not enough to fit the global 1% budget.

Reducing the interval to `0.1` units disproved that diagnosis: WARP again
reported exactly 1,216 over-tolerance pixels. The uploaded actual/diff artifact
located every one on the thin oblique `±X` wall strips — 656 on the left and 560
on the right. The floor had 8,825 differing pixels but none over tolerance and a
maximum channel delta of 2. The wall strips reached 7 and 8 respectively.

The room is now wide enough to crop those `±X` strips while retaining the `±Z`
walls as context. That changes no measured floor point, probe row, tolerance, or
semantic assertion. The centre is still compared against both endpoints, and the
widened-to-room negative control still fails on an 11.60-level endpoint-region
change against its 0.5-level flatness budget. Only the next WARP run can confirm
the crop.

## The Pages browser gate fails on the runner's GPU stack, not on the code

`ce253ad`'s Pages run failed in `build the demo site` at "Render breakout in a
real browser", with the runner's GPU process dying during initialisation:
`VerifyExtensionsPresent: Extension not supported: VK_KHR_surface`, then
`eglInitialize Vulkan failed with error EGL_NOT_INITIALIZED`, then
`Exiting GPU process due to errors during initialization`. The next commit's
Pages run passed with no change to anything the gate touches, and `breakout` was
not modified in that commit or any near it.

So it is the ANGLE/Vulkan stack on the ubuntu runner image, not the demo.
Recorded because a red Pages run on a commit that did not touch wasm is going to
look like a real break to whoever sees it next, and because a gate that fails
this way occasionally is a gate people learn to re-run rather than read. **Not
fixed, and not obviously ours to fix** — if it recurs, the question to answer is
whether the gate should demand a software GL/Vulkan fallback explicitly rather
than taking whatever the image offers.

## The draw-argument pass exceeds WebGPU's per-stage storage-buffer budget (2026-08-15)

`apps/lumen` shipped as the sixth Pages demo and it is the only one
`.github/workflows/pages.yml` does **not** render in a browser. The step is
missing on purpose, and a comment where it would go says so.

**The measurement.** `crcbl-render`'s draw-argument pass — `draw_gen.rs`'s
`gen_layout`, labelled `draw args` — binds one uniform and **fourteen** storage
buffers in a single compute stage. Chrome's SwiftShader adapter reports a
per-stage ceiling of ten, so Dawn refuses the bind group layout and every
pipeline, bind group and submit built on it:

```text
The number of storage buffers (14) in the Compute stage exceeds the maximum
per-stage limit (10).
 - While validating [BindGroupLayoutDescriptor ""draw args""]
 - While calling [Device "lumen"].CreateBindGroupLayout(…)
```

The demo reaches `STATUS_RUNNING`, draws nothing, and the run ends in
`STATUS_FAILED`. Measured on Chromium under Xvfb with
`--use-webgpu-adapter=swiftshader`, which is the configuration a GitHub runner
with no GPU takes: 21 of 33 checks passed. The same artifact on the same machine
against the real adapter (`--headless --hardware`, amd rdna-3) passes **33 of
33**, including both of the `EXPECTATIONS` claims that are lumen's own.

**Whose problem it is.** Not lumen's — nothing in `apps/lumen` chooses the
binding count, and every geometry path (`MeshShader`, `IndirectCount`,
`IndirectPerBatch`) goes through the same pass. `ForwardRenderer` builds a
`DrawGen` unconditionally, so this is the first time any consumer of it has run
on the wgpu backend against an adapter with WebGPU's ordinary ceilings. The
engine does not check the limit either: `crcbl_wgpu::hal_limits_for` maps no
per-stage storage-buffer count into `DeviceCaps::limits`, so nothing above the
seam can refuse cleanly or select a smaller pass — the failure arrives as a Dawn
error on the device callback, which `wgpu` does not turn into a `Result`.

**What would fix it**, in increasing order of cost:

- Pack the pass's buffers so the stage binds ten or fewer. Four of the fourteen
  are `docs/plan/25-lod.md`'s selection tables and the hysteresis state, which a
  `Geometry::Flat` scene never uses; merging pairs, or splitting the LOD half
  into its own dispatch, are the two shapes. Either touches `draw_gen.slang` and
  therefore the SHA-pinned SPIR-V, plus `crcbl-vk`'s draw-gen end-to-end.
- Surface the limit through `DeviceCaps::limits` and fail at
  `ForwardRenderer::new` with a message naming it, so a device that cannot host
  the pass says so instead of drawing black. Cheap, and worth doing whichever
  way the first bullet goes.

**Then add the step back** — one line, and the `EXPECTATIONS` row it needs is
already in `web/tools/browser-e2e.mjs`:

```yaml
- name: Render lumen in a real browser
  run: CRCBL_WEB_E2E_DEMO=lumen ./web/run-browser-e2e.sh
```

Two pieces of prose also claim the exception and want deleting with it:
`web/pages/lumen.html`'s "This one wants a real GPU" note, and the clause on
`web/pages/index.html` that excuses lumen from "every build is loaded in a real
headless browser".

**Not measured:** how slow lumen is under SwiftShader. The pipeline never
builds, so there is no frame time to compare against the other five demos, and
the timing question the CI step would have answered is still open.

**Do not read this as "it needs a real GPU", which is the heading this entry
first carried.** The limit has now been looked up, and it makes SwiftShader the
generous one rather than the stingy one.

**WebGPU's guaranteed minimum `maxStorageBuffersPerShaderStage` is 8** — the
default in the specification's supported-limits table. SwiftShader's ten is
already _above_ the floor, so no conforming WebGPU implementation is obliged to
run this pass at all and an adapter that refuses it is not defective.

What the field looks like, from web3dsurvey's survey of that limit (cumulative
share of surveyed devices reporting **at least** each value):

| value | share |
| ----- | ----- |
| 8     | 100%  |
| 9     | 98%   |
| 16    | 78%   |
| 31    | 17%   |
| 48    | 4%    |

Fourteen falls between published buckets, so the share that can host the pass is
**between 78% and 98%** and the survey cannot narrow it further. The pessimistic
end is roughly one device in five. This is not a CI-runner problem wearing a
browser costume; it is a portability ceiling on the GPU-driven path, invisible
until now only because lumen is the first 3D demo to reach a browser.

That settles the question this entry used to leave open: packing the pass is
**not a nicety**, it is the browser tier's only path to 3D on the hardware that
tier runs on. The cheap fix — surfacing the limit so `ForwardRenderer::new`
refuses by name instead of drawing black — is still worth doing first and
independently, because a device in that tail currently gets a black canvas and
no diagnosis.

Sources:
[WebGPU specification, supported limits](https://www.w3.org/TR/webgpu/#limits) ·
[web3dsurvey](https://web3dsurvey.com/webgpu/limits/maxStorageBuffersPerShaderStage)

## The pinned shader compilers ARE installed here (2026-08-14)

**A previous entry in this file said they were not, and that claim blocked a
real improvement for a day.** It is worth its own heading because the next
reader has to be able to trust the rest of this file: an entry claiming a tool
is missing is one nobody re-checks, and this one was never true. Both pinned
compilers are on the development machine, at the versions
`crates/crcbl-shaders/tools/compile-shaders.sh` names:

- `~/.local/slang/bin/slangc` — `2026.14`, the script's `SLANG_VERSION`
- `~/.local/dxc/bin/dxc` — `1.9(1-0d3ee6b5)(1.9.0.1)`, the script's
  `DXC_VERSION`

So editing a `.slang` is ordinary work here, not something to defer.
`crates/crcbl-shaders/build.rs` verifies each source by SHA-256 against
`spirv/manifest.txt`, so _any_ edit — a comment included — fails the build until
the artifacts are regenerated, and the regeneration is one command:

```
CRCBL_SLANGC=~/.local/slang/bin/slangc CRCBL_DXC=~/.local/dxc/bin/dxc \
  crates/crcbl-shaders/tools/compile-shaders.sh
```

then the same script with `--check`. Note that `CRCBL_DXC` has no PATH fallback
by design, and Arch's `directx-shader-compiler` is a preview build the script
refuses — so the path above is not interchangeable with whatever `which dxc`
finds.

## The effect toggles landed, and two things about them are owed (2026-08-14)

`crcbl_render::effects` is topic 39's resolution point: `RenderEffects` is the
effect set, `EffectRequest` carries the three requested layers,
`EffectRequest::resolve` applies the order, and `ForwardRenderer::begin_frame`
resolves once per frame and freezes the answer. What follows is what that left.

### Two of the four toggle layers have no source in the tree

`EffectRequest::camera` and `EffectRequest::video` are fields nothing but a test
writes. They are present because the _order_ is what was built — a resolution
point missing two of its inputs cannot be shown to apply them in the right order
— and `crcbl_render::effects`' module docs carry a table saying which are wired.

- **Camera stack.** There is no render-stack RON. Nothing in the workspace reads
  or writes RON at all, there is no `ron` dependency and no `.ron` file, and
  `crcbl-render` has one camera per frame. The consumer topic 18 names is the
  render-to-texture monitor camera, which is on lumen's owed list.
- **`[engine.video]`.** Closer, and the gap is smaller than it looks.
  `crcbl_store::settings::SettingsStack` implements the whole four-layer TOML
  resolution and `get_section::<T>("engine.video")` is one call. What is missing
  is a _schema_ (no `VideoSettings` type exists; the namespace appears only in
  doc comments and unit-test strings) and a _startup_ — nothing in `crates/` or
  `apps/` constructs a `SettingsStack`, opens a `StorageSource` or names a
  `settings.toml`. Wiring it is a decision about where the file lives and which
  binaries load one, which is topic 14's P10 settings-screen slice rather than a
  renderer change. Deliberately left rather than invented for one sample.

### The device-capability clamp is real and its rule set is empty

`ForwardRenderer::device_effects` is `RenderEffects::all()`, and that is a
statement about these three effects rather than an unfinished clamp:

- AO has no device fact to gate on, which topic 18 says in as many words —
  "inventing a capability that is really a performance opinion is what topic 39
  exists to prevent".
- The reflection pair says the same of itself in `crcbl_render::ssr`'s module
  docs: every backend has a full-screen draw, a sampled `D32Float` and a sampled
  `Rgba8Unorm`.
- Shadows are a `D32Float` image and a depth-only pass. **Considered and
  declined:** a rule requiring `max_image_2d >= shadow::atlas_extent()`. It is
  true and it is unreachable — a device that fails it cannot create the atlas at
  `build`, so the renderer never exists to be clamped, and writing the rule
  would imply a degradation path that is actually a build failure.

The clamp _step_ is exercised:
`the_layers_resolve_in_the_order_topic_39_specifies` passes a reduced device set
to `EffectRequest::resolve` and checks it wins over an override forcing an
effect on. The first rule that fires arrives with the ray-traced variants, which
`LightingPath` already selects.

### Coverage gaps this slice leaves

- **The toggle frames do run in CI**, and this entry said they did not. The
  `Draw lumen's room on lavapipe` step in `vk e2e (lavapipe)` runs
  `apps/lumen/tests/run-lumen-golden.sh`, which drives the whole suite —
  `every_effect_toggles_and_the_frame_says_so` included, three tests, all
  passing there. What is still missing is rule 12's second half: the run
  exercises the runner's own path and not one below it, which
  `--force-geometry indirect-per-batch --force-binding array-pages` now makes
  possible and no job does.
- **The `[engine.video]` and camera layers are untested against a real source**,
  because they have none. What is tested is the order, with values a test wrote.
- **`EffectOverride::force(.., None)` — releasing an override — has no caller
  outside its unit test.** It is there because a settings row returning to
  "auto" is the obvious consumer and leaving the tri-state out would have made
  that a breaking change; nothing exercises it end to end.

## The scene API: the slice plan (decided 2026-08-13)

`apps/lumen` could not be built because an application cannot describe a scene:
`ForwardRenderer::begin_frame` takes the cube's transform as an argument, five
`set_*` methods place instances of meshes the renderer holds the ids of, and
there is no material call on the type. The roadmap already put P9's scene work
before S4B while P7B's deliverable named lumen. **Resolved by pulling the scene
work forward**, rather than by moving lumen.

The resident set is a description now — `crcbl_render::scene` and
`ForwardRenderer::with_scene`, with `new` as `with_scene(&scene::demo())`;
instances are a runtime API: `ForwardRenderer::add_instance` / `set_instance` /
`remove_instance` over a `scene::InstanceDesc`, and the five `set_*` demo
wrappers are gone; `begin_frame` no longer takes the cube's transform — the cube
is an ordinary instance every caller places for itself, by `scene::DEMO_CUBE`
and the other public demo indices. Materials and page layers are the caller's
too: `PageDesc` at the caller's own extent, `push_layer` per layer,
`SceneDesc::materials` row by row, refused at build when a row names a layer the
page has not got. The pools are sized by `Capacities`, and a description that
outgrows one of the four is refused up front rather than part way through
filling it. `apps/lumen` is the application that consumed it, and what still
binds a future caller is everything below.

### The shape

The resident set becomes a description the app hands to `new`; instances become
a runtime API. That split is where the seam already is: pools, the cluster pool,
the bucket table and the page are fixed at build and never grow, while
`MaterialTable::insert` and `InstancePool::insert`/`set`/`remove` are already
per-frame paths. A runtime `add_mesh` would mean recreating the camera's
`DrawGen` and the four shadow ones plus every bind group naming their buffers
mid-life, which is the streaming path `crcbl-render`'s own `mesh_pool` docs
already assign to P9.

### The slices left, in dependency order

None. `apps/lumen` shipped on top of this API at milestone 1a, and it is the
first and only consumer of it — everything else in the tree still draws
`scene::demo`. What the sample still owes is its own entry, "`apps/lumen` is at
milestone 1a"; what the API still owes is "Deliberately left at P9-proper"
below.

### Flat meshes only, and why that is not negotiable yet

`build_meshlets` needs positions alone and emits vertex runs indexing the
original array, so attributes survive exactly — a flat app mesh is fine. **A
cluster DAG is not.** `crcbl_scene::simplify` is position-only and says so in
its own module docs: a coarse level has no normals and no UVs. The engine's one
DAG works because the dunes patch is analytic — `residents` synthesises each
coarse vertex through `crcbl_shaders::dunes::vertex_at`. An app-supplied DAG
needs attribute-aware simplification or nearest-source attribute transfer, which
is unbuilt topic 25 work listed in that plan's own risks. `Geometry::Dag`
carries that constraint in its own documentation, so the limitation is stated at
the type rather than discovered.

### Capacity: a documented cap the caller chooses, never growth

The `POOL_*` constants are fields of `Capacities` now, whose `Default` is the
numbers the engine shipped. Growth is out for the reason `mesh_pool` already
argues — every bind group names those buffers. A description that outgrows one
is refused by `ForwardRenderer::check_scene`, before the first device object
exists, naming the pool, the capacity and what the description needs; the plan's
"`MeshPoolError::PoolExhausted` must reach the caller un-flattened" turned out
to be the wrong answer to that, for the reason the `SceneError` entry below now
records. Worth knowing while sizing: raising the instance cap is not linear,
since the LOD hysteresis buffer is per instance per `DrawGen` and there are
five. `Capacities::instances` is the one number no description can be measured
against — objects are placed while the renderer runs — so filling it is
`InstancePoolError::PoolFull` from `add_instance` and nothing earlier.

### Deliberately left at P9-proper

`AssetSource`/`AssetRegistry` and refcounting (nothing depends on it, and
`SceneDesc` borrows byte slices so the registry wires into the same seam later);
glTF import into the pools (the importer exists, but glTF instances carry
non-rigid transforms which `GpuInstance::transform` forbids, and that is a
decision rather than a wiring job); RON scene files and the deterministic
writer; hot reload; material templates; the glTF corpus; runtime `add_mesh`;
texture slots beyond the page.

### Where the refactor could silently change a frame

Ordered by how quietly each would fail. Row 0 of the material table is what
`GpuInstance::default` names, so a reordered description swaps the pyramids'
materials. Mesh table ids come from upload order and the cull pass reads a
bounding box out of the entry the instance names, which for a DAG is level 0's.
Page layer 0 must stay opaque white or every untextured material is scaled by a
texel that is not 1.0 — a global albedo change that reads as a lighting
difference. `draw_gen`'s scatter takes the first bucket whose mesh id matches,
so two buckets naming one mesh means the second never draws. Instance index is
the LOD hysteresis key, inert with one DAG instance and not inert with two. And
the rollback path gains new early-failure points that must sit on the same side
of the self-cleaning handover, or a rejected description leaks two device-local
buffers.

**Four of these are invisible to `cargo test`**: `crcbl-render`'s unit tests run
on the null backend and cannot tell a right frame from a wrong one. Every
remaining slice is verified by `run-render-e2e.sh` and `run-vk-e2e.sh` on a real
device or it is not verified.

What the landed slices did about each, so the next one does not re-derive it:
row order is `SceneDesc::materials` order and `material_rows` inserts in it,
asserted by `scene`'s
`the_demo_scene_shades_by_omission_through_an_untinted_row`; ids are description
order, asserted by `forward`'s
`the_description_resolves_to_the_ids_it_was_written_in`; layer 0 is
`PageDesc::opaque_white`'s to write and `PageDesc::check`'s to verify; buckets
are built by walking the mesh list, so a duplicate is not refused but
unspellable; and every description check runs from the top of
`ForwardRenderer::check_scene`, before the first device object exists, which
`a_refused_description_creates_nothing_at_all` reads off the recorder's live
object count — with one arm deliberately refused _after_ the pool exists, so
that count is evidence about `build_geometry`'s rollback and not only about
`check_scene`.

### The materials-and-layers slice was almost entirely already done

Recorded because the next reader will otherwise re-derive it. Of the three
things that slice was scoped as, the first description slice had already
delivered all three:

- The constants a caller reads the pattern off — `PYRAMID_TINT`,
  `PYRAMID_ROUGHNESS`, `CHECKER_TEXELS`, `CHECKER_LAYER`, `PAGE_EXTENT` — are
  public on `crcbl_render::scene`, and `scene::demo` builds its page through
  `PageDesc::opaque_white` and `push_layer` like any other caller.
  `UNTEXTURED_TEXELS` does not exist any more: layer 0's texels are
  `opaque_white`'s to write, which is what makes the mistake unspellable.
- A row naming a layer the page has not got is already refused by
  `ForwardRenderer::check_scene`, naming the row, the layer and the page's layer
  count, before any device object exists — and
  `a_refused_description_creates_nothing_at_all` already has an arm for it. Not
  duplicated.
- `PageDesc` already lets a caller append layers and gives it no way to spell a
  bad layer 0, its fields being private and `opaque_white` its only constructor.

What was actually missing was **evidence**, not mechanism: every scene built
anywhere in the tree was `scene::demo()` — three rows, two layers, one extent —
so a `with_scene` that uploaded the first two layers and stopped, or inserted
the first three rows and stopped, would have left all eleven goldens
byte-identical and passed everything else. `forward`'s
`an_app_page_and_table_reach_the_device_whole` is what closes that: a four-layer
page at an extent that is not `PAGE_EXTENT` and six material rows, checked
against the recorded `CopyBufferToImage` per layer and against the material
buffer's bytes per row. Shown red three ways — the page upload truncated, the
row insert truncated, and the row insert reversed.

**Considered and declined: a `PageDesc::layer_bytes` accessor.** `extent² × 4`
is computed in `opaque_white`, in `check` and by any app producing texels for
`push_layer`, so there is a real second caller for it. Left out anyway: it is a
convenience rather than a sufficiency gap — an app has `extent()` and the RGBA8
layout is documented on `push_layer` — and this slice's whole obligation was not
to manufacture work. Revisit if a second page format arrives, at which point the
arithmetic stops being a constant an app can safely transcribe.

The instance-index risk is now **documented rather than removed**, on
`ForwardRenderer::add_instance`: the index is the LOD hysteresis key and the
pool reuses slots, so a slot freed by `remove_instance` hands the next object
the previous occupant's expanded-group state. `group_state` is never cleared per
instance — it is zeroed once at build by `DrawGen`'s start-up staging copy,
which is where `draw_gen.slang`'s monotonicity induction starts. What the reuse
costs is one frame selected against a history that is not this object's; every
group is judged afresh, so it is never a wrong cut. **Not fixed, deliberately.**
Fixing it is not a host write: the buffer is device-local because a shader
writes it, so clearing one instance's `group_stride` words means work recorded
inside a frame, and the existing per-frame clearing dispatch is explicitly the
thing that must not touch this buffer. Unmeasured whether the pop is visible at
all.

### Instance order is the caller's now, and it is what keeps a golden still

`ForwardRenderer::new` used to insert the cube itself, so it was always instance
0 and every `set_*` object landed above it. Nothing is inserted at build any
more, so the pool's slot order is the order a caller places objects in — and the
eleven goldens stayed byte-identical across the `begin_frame` slice and again
across the setters' retirement because every caller places the cube **first**:
`screenshot.rs`'s `place_cube`, `vk_e2e`'s `mesh::place_cube` / `place_cube_at`,
and `forward`'s own test helper, each of which says so at the call site.

Retiring the setters made this the whole risk of that slice, and it is why every
converted call site is a straight `add_instance` in the setters' own order, why
the toggling ones hold a handle and `remove_instance` before placing again
rather than inserting twice, and why the three helpers that grew out of it —
`screenshot.rs`'s `place`, `vk_e2e::mesh::place` and `forward`'s `place_demo` —
each say the order is load-bearing where a reader will find it.

Whether a different order would actually move a frame is **not measured**. The
visible list is filled by an atomic, so the draw order is not the pool's order
to begin with; but the instance index is `docs/plan/25-lod.md`'s hysteresis key
(see the entry above), and a slice whose whole obligation was that no golden
moves was not the place to find out.

### A renderer nobody placed anything in records fewer dispatches

Not a defect, and newly reachable. With an empty instance pool the cull dispatch
covers no workgroups, and `DrawGen::add_passes` records **no dispatch at all**
rather than one of zero — Metal rejects the empty dispatch, and the comment
there says so. Before the cube became a caller's instance the pool was never
empty, so this could not be reached from `ForwardRenderer` at all.

`forward`'s
`the_frame_records_one_indirect_call_per_bucket_whatever_the_scene_holds` is
what found it: its no-pyramid half recorded 7 dispatches against the other
half's 10. It places the cube in both halves now, so the two differ in the
pyramid alone, which is what the test was always about. Nothing else in the tree
draws a frame with an empty pool.

### Declined twice: a `SceneError`, and the second reason retires the condition

The description slice declined one as indirection with a single implementation,
and set a condition for revisiting it: `MeshPoolError::PoolExhausted` reaching
the caller un-flattened, because its `largest_free`-versus-`total_free` pair
tells fragmentation from a genuinely full pool and `HalError` cannot carry that
distinction. The capacity slice revisited it and **declined again, because the
condition is not reachable at `with_scene`**.

`build_geometry` creates the `MeshPool` and then fills it; nothing is ever freed
in between, and `FreeList::alloc` is first-fit over a list that starts as one
block, so every allocation comes off the front of a single trailing block and
`largest_free == total_free` at every failure. The refusal a build can actually
produce says so out loud — breaking the new check and letting the pool refuse
instead prints
`the largest free block holds 1 and 1 are free in total, out of a capacity of 1`.
So the only thing exhaustion can mean here is "too small", the only answer is
"raise the capacity", and both are known from the description before a device
object exists. `check_scene` says it there instead, naming the pool, the
capacity and the need.

The condition becomes real when meshes can be freed and re-uploaded during a
renderer's life — P9's streaming `add_mesh`, deliberately deferred — and that is
the slice where the type earns itself. Not before: today it would still be one
implementation, and it would be carrying a distinction that cannot arise.

### `add_instance` still cannot refuse a DAG the device cannot draw

A mesh stage with no amplification stage — `Features::MESH_SHADER` without
`Features::TASK_SHADER`, which is a real and supported device state — emits
every cluster of a bucket, and for a DAG that is every level at once.
`add_instance` cannot refuse there, because its only error is
`InstancePoolError` and "this device cannot choose a level" is not a full pool.

What the cook slice changed is where a caller reads the condition. `set_dunes`'s
`bool` was the only place it was spelled out, and deleting that method would
have left the two-term device test
(`geometry_path() == MeshShader && !culls_clusters()`) to be re-derived at each
of the seven call sites that ask it. It is `ForwardRenderer::selects_levels` now
— one predicate, documented at the type, asked by `screenshot.rs`'s
`Scene::Dunes` and by `vk_e2e::mesh::place_dunes`' callers before either places
the patch.

**Still not a refusal**, and nothing checks that a caller obeys: turning it into
one wants an error type on `add_instance` that says something other than "full
pool", which is the type the entry above has now declined twice. No test covers
a `Geometry::Dag` instance placed on that device shape.

### The demo setters are gone, and what indexed the description besides them

`set_pyramid`, `set_tinted_pyramid`, `set_textured_pyramid`, `set_open_box` and
`set_dunes` are deleted, with `ForwardRenderer::place` — the body they shared,
whose swallowed `InstancePoolError::PoolFull` this backlog kept as "it goes when
they go" — and the five `Option<InstanceHandle>` fields, and the
`REQUIRED_MESHES` / `REQUIRED_MATERIALS` floor `check_scene` enforced.

**The floor was not held up by the setters alone**, which is what the plan for
this slice assumed. `ForwardRenderer::build` also indexed the description at
`DEMO_DUNES` in two places — the cluster range it published as `dunes_clusters`
and the per-level bucket list it published as `dunes_level_buckets` — so with
the check simply deleted, a one-mesh description **panicked** out of `build`
rather than being refused. Both are per-description-mesh now: the fields are
`mesh_clusters` / `mesh_level_buckets` and the accessors are
`ForwardRenderer::cluster_range(mesh)` and
`ForwardRenderer::level_buckets(mesh)`, whose only callers are
`vk_e2e/mesh.rs`'s `read_cut` and `selected_dunes_level`. `forward`'s
`a_description_smaller_than_the_demo_is_a_scene` is the test, and it was shown
red both ways — against a restored floor, and against the positional indexing,
where it fails with `index out of bounds: the len is 1 but the index is 3`.

Nothing in `crcbl-render`'s non-test code names a `DEMO_*` constant any more.

### Where the capacity slice drew the line, and what it left to the pool

`check_scene` owns what only the whole description knows — the four totals
(vertices, indices, mesh table entries, material rows) against `Capacities`, and
the cross-references between page, rows and DAG levels. What one mesh's _bytes_
say stays the pool's: `MeshPoolError::VertexStrideMismatch` and `EmptyMesh` are
still raised from inside `build_geometry`, mesh by mesh, and arrive as
`HalError::Backend` carrying their numbers.

**Considered and declined: hoisting those two into `check_scene` as well.** It
would make every description refusal free, and it would also make the
self-cleaning branch of `build_geometry` unreachable from any description — dead
code with a test that could no longer drive it. Left where it is deliberately,
so `a_refused_description_creates_nothing_at_all`'s last arm is a real path: it
appends one byte to the open box's vertices, which is refused on the third of
four meshes with the pool created, its buffers live and two meshes already
staged into them, and asserts both that something _was_ created (or the arm
proves nothing about the rollback) and that the live-object count came back.
Shown red by removing `pool.destroy(device)` from `build_geometry` — 8 objects
leaked, and that arm was the **only** failure in the whole `crcbl-render` suite.

The four capacity refusals are `HalError::InvalidDescriptor` like every other
`check_scene` answer, each asserted against a fragment of its own message so an
arm cannot pass on another check's refusal, and each shown red by deleting its
row from the table — every one of them then reached the pool and came back as
`Backend`. The opposite mistake has its own test, because nothing else in the
tree could fail on it: every other scene reserves far more than it holds, so a
comparison written `>=` would pass the entire suite and refuse only the
application that had sized its pools exactly right.
`a_description_that_exactly_fits_its_capacities_is_built` is what fails there.

Not covered, and not attempted: `Capacities::lights` has no description to be
measured against — a caller sets lights per frame — and no test drives its
overflow; and no non-default `Capacities` value has ever reached a real device,
since every end-to-end run is still `scene::demo()`.

### Coverage gap: no device has drawn a description with two DAGs

Half closed. `forward`'s
`a_second_dag_reaches_its_own_groups_and_not_the_first_s` builds a five-mesh
description with **two** DAGs on the null backend's mesh path, places an
instance of each with `add_instance`, and asserts the three things a second DAG
is the only thing that exercises: one mesh id per description mesh with the
second DAG's level 0 a whole hierarchy past the first's, `DrawGen::group_stride`
summing both DAGs' groups rather than taking the first's, and every
`ClusterSelect` record of the second DAG naming a group in the second half of
the concatenated `level_groups` — the `first_group` offset handed to
`ClusterDag::selection_records`, which is zero for one DAG and so invisible with
one. Both halves were shown red by dropping the offset and by suppressing the
concatenation.

**A real device now draws one non-`demo` description, and this entry said none
did.** `crates/crcbl/tests/gltf_e2e.rs` imports a `.glb` through the real
`DirSource`, converts it with `crcbl_scene::gltf_render::build_render_scene`,
hands the result to `ForwardRenderer::with_scene` and draws — verified on an RX
7900 XTX on 2026-08-19 via `crates/crcbl/tests/run-gltf-e2e.sh`: _"an imported
glTF drew its own texture on vk"_. So "every run is still `scene::demo()`" is no
longer true, and anyone closing the rest of this gap has a working precedent to
copy rather than a blank page.

**What that test does not cover is the shape this entry is really about.** Its
document is a single textured quad — one mesh, one primitive, one material — so
it exercises none of **two DAGs, a fifth mesh, or non-default `Capacities`**,
and the null backend cannot tell a right frame from a wrong one. Closing that
still means an end-to-end test building such a description and reading something
back: `ForwardRenderer::cluster_selection` is the observable that already
exists, and asserting both DAGs' runs have clusters chosen in them needs no new
golden image.

The cook slice added one description that is not `scene::demo()` and does reach
a device object: `crcbl`'s
`an_application_bakes_its_own_mesh_and_the_renderer_makes_it_resident` bakes a
mesh's clusters through `crcbl::scene` and builds a one-mesh, one-row renderer —
but on the **null** backend, and behind the `scene` feature, so a plain
`cargo test` does not run it at all and only `--all-features` does. Nothing
about the above is closed by it.

### The dependency call was made, and the split it declined

`crcbl` takes `crcbl-scene` behind the non-default `scene` feature, and
`crcbl::scene` is the re-export. `cargo tree -p crcbl -e normal` finds no `gltf`
and no `crcbl-scene` by default and finds both with `--features scene`, and
`crcbl` builds for `wasm32-unknown-unknown` both ways.

**Declined, and still open as its own change:** splitting the meshlet and DAG
builders out of `crcbl-scene` into a crate that does not carry `gltf`. That is
the cleaner shape — a game linking the bakes would then not link a parser at
all, feature or no feature — and it is a crate move rather than a manifest line.
The feature gate is what the cook slice did because it is reversible and touches
nothing else.

Also unmeasured: the cook's load-time cost — time the `cook-clusters` example
rather than quoting a guess.

### What the cook slice actually had to move, and what was already there

`ClusterDag::cook` was **already** in `crcbl-scene` (landed with
`crcbl lod gen`), so the `cook`/`sphere` pair in
`crates/crcbl-shaders/tools/cook-clusters.rs` was a second copy of the same
transcription with nothing between them. The example calls `built.cook()` now
and its own copy is gone;
`cargo run -p crcbl-shaders --example cook-clusters -- --check` is what says the
move changed no byte, and it was shown red by perturbing one cooked vertex index
(`they first differ at byte 4972`).

New is `MeshletBuild::into_clusters`, which is the flat-mesh half an application
needs and had no spelling at all: `build_meshlets` produces three private `Vec`s
and `crcbl_render::scene::Geometry::Flat` takes a
`crcbl_shaders::meshlet::MeshClusters`. `ClusterDag::cook` goes through it now
too (`level.clusters.clone().into_clusters()`, the same three allocations the
three `to_vec`s cost), so the mapping lives in one place.

## Screen-space reflections: the slice plan (decided 2026-08-14)

The design and its refusals are in `docs/plan/18-render-features.md`'s SSR
section. This is the slice order and what each one's observable is.

**The attachment, march, blur, probe fallback and rough-surface integration have
landed.** The cutoff remains at 0.5 because it gates marching rather than probe
environment specular; the measured cutoff raise below remains a declined
alternative, not pending work.

What those slices found on the way, and what a reader of the design should know
before writing the next one:

- **The reach had to become a share of the frame.** The design said a fixed
  pixel stride and a fixed loop bound, which a first cut read as a fixed pixel
  _reach_ — and a reflection that shrinks as the window grows is the same defect
  the design refuses one level down. `ssr.slang`'s `REACH_FRACTION` is the fix
  and `docs/plan/18-render-features.md` carries the amendment.
- **The forward pass stores its depth now.** `PassBuilder::clear_depth` is
  `StoreOp::Discard`, and a discarded attachment is undefined rather than
  "whatever was written": radv and llvmpipe handed the values back and wgpu
  handed back the clear, so the same build reflected on one backend and not the
  other with no error anywhere. Anything else that wants to read the depth
  _after_ the forward pass inherits this.
- **`Scene::PointShadow` earned a geometry-path budget.** Its caster carries the
  tinted row, the only demo material under the cutoff, so it is the first scene
  whose pixels come from a march rather than from shading the fragment the
  rasteriser handed over — which makes the depth buffer's last bits visible in
  the picture. One channel, off by one, on llvmpipe alone, stable across runs.
  See `path_lsb_channels` in `crates/crcbl/tests/render_e2e.rs`.
- **The cross-driver evidence, which the design asked for and had none of.**
  `ssr.png` blessed on llvmpipe compares on radv and on wgpu at **max channel
  delta 1, zero pixels over `Tolerance::RASTERISER`** — a _less_ divergent frame
  than `cube.png`, which has no reflection in it and differs on 60% of its
  pixels at the same delta. The structural ratio reads 92.8 against 64.0 on all
  three, to the decimal. `lumen`'s room is where the exposure is visible: of the
  nine pixels over tolerance between llvmpipe and radv, five are in the panel's
  reflecting band and two of those are gross (deltas 66 and 33). That is one
  fixture's worth of evidence, not a general argument, and the design's recorded
  resolution — flatten the reflected content or drop the golden and keep the
  ratio — has not had to be used.
- **The stepping is gone, and it is measured rather than eyeballed.**
  `the_reflection_does_not_step_down_the_band` in
  `crates/crcbl/tests/render_e2e.rs` takes the **second** difference of the
  reflection down single rows of `Scene::Ssr`, so a reflection that merely fades
  down the band scores zero and only the alternation counts: 17.7 levels per row
  with `ssr_blur.slang`'s kernel cut down to its centre tap, 2.8 with the real
  one, limit 8. A block average hides it, which is why every other claim on that
  scene cannot see it and why the review PNG was the only evidence before.
- **The blur reduced cross-driver divergence where it mattered.** On the 192
  pixels of `lumen`'s room the blur changed, llvmpipe and radv disagree by at
  most **8** and 27 are over `Tolerance::RASTERISER`; the unfiltered march's
  worst inside the panel's band was 66. The pixels over tolerance that remain
  gross (worst 134) are **bit-identical to the pre-blur frame** on llvmpipe, so
  they are triangle-edge divergence and not the reflection's. A sixteen-tap
  denominator turning one whole-pixel disagreement into a spread of small ones
  is exactly what the AO pair's design predicts.

1. ~~**The roughness cutoff.**~~ **Resolved without raising it.** lumen's
   `ROUGH_METAL` receives probe environment specular above the cutoff; one
   screen-space ray remains disabled because it cannot represent that broad
   lobe. The authored-versus-zero control is the observable.
2. ~~**What a miss returns.**~~ **Shipped as probe radiance.** The pass removes
   the diffuse transfer from the stored L1 rows, evaluates the reflection
   direction, and blends fallback against hits by confidence. A zero table
   preserves the old hit arithmetic exactly.

### lumen's mirror panel is close to SSR's worst case

Worked out by hand before the slice and **measured** by it since. The panel
faces `+Z` at the camera, so its rays point back past the viewer and its centre
reflects a point on the front wall behind the camera — off screen, a miss. Only
where the panel point is below eye height do rays go downward, and the band that
reaches the floor _while still inside the frame_ is narrower than the hand
estimate: `y = 0.45` up to about `0.607`, an eighth of the face rather than two
thirds, because the vertical frustum edge binds before the geometry does.
`room.rs`'s `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head` bisects
for that height rather than writing it down.

So the observable is a block hung on the panel's **bottom edge** against one
further up the same face — same material row, same normal, same `F0`, same
roughness, same absence of direct light, differing only in whether the ray finds
anything. It reads about 22/255 against exactly 0 at 256×192 and 18 against 0 at
1280×960.

**`METAL_DARKNESS` was replaced by that gradient rather than deleted**, and the
constant itself was kept: it is read at `MIRROR_MISSES` now, which is above
everything that reflects, so the claim it makes about a conductor's absent
ambient is still exactly true and its number has not moved. `MIRROR_GRADIENT` is
the new central claim beside it, and `reflecting > LIT_FLOOR` is the floor that
stops a ratio against zero from being a check that cannot fail.

**Considered, and for the sample's owner rather than the SSR slice:** if lumen
wants a mirror showing the room, the panel wants angling or moving to a side
wall. That is a change to the sample's content and should not be done on the way
past.

### The cutoff raise: what it costs, measured before it was declined

The blur slice was written with `ROUGHNESS_CUTOFF` at **0.75** first, run
end-to-end, and then split back out — so the cost below is measured on this tree
rather than estimated, and slice 1 above starts from an answer instead of a
guess.

**Two questions to settle before writing it.**

- **Is 0.75 the smallest cutoff that gets `ROUGH_METAL` reflecting?** It was not
  derived; it was placed between `lumen`'s brass at 0.55 and its plaster at 0.9.
  The ramp is `1 - roughness/cutoff`, so brass weighs 0.083 at a cutoff of 0.6,
  0.214 at 0.7 and 0.267 at 0.75 — and the falloff comparison needs the brass
  reflection to clear `LIT_FLOOR` and to turn that face's own downward gradient
  around, which at 0.083 it may not. **Unmeasured below 0.75**, and worth
  measuring: a lower cutoff buys nothing in blast radius (any value over 0.5
  takes `UNTINTED` in) but it does keep more of the frame's arithmetic near
  zero.
- **Does any cutoff over 0.5 cost the determinism claim?** Yes, and it should go
  on the record as a decision rather than arrive as a side effect.
  `GpuMaterial::UNTINTED`'s roughness is exactly 0.5, no monotone ramp passes
  0.55 and stops at 0.5, and the design's one _unconditional_ determinism
  statement is that a pixel shaded through that row weighs exactly zero on four
  rasterisers. Raising the cutoff at all trades that for "the rough end —
  plaster, a fully rough conductor, `crcbl_scene`'s imported glTF default —
  weighs exactly zero", which is a real claim and a narrower one.

**What it moved, at 0.75, on llvmpipe.** Nine goldens: `ao` 41.3% of its pixels
at delta 3, `dunes` 6.6% at 9, `spot_shadow` 5.5% at 16, `cube` 0.07% at 1,
`lights` 0.04% at 1, `crcbl-vk/mesh_clusters` 12% at 5 — all of those purely
because `UNTINTED` entered the ramp — plus `ssr` and `point_shadow` moving
further than the blur alone moved them (0.25 weighs 0.667 where it weighed 0.5)
and `lumen/room` gaining the brass block's reflection. `ui`, `sprite`, `spot`
and every sprite and UI golden in `crcbl-vk` stayed byte-identical.

**What it broke.** Two byte-exact geometry-path comparisons: `Scene::Ao` in
`render_e2e` (four channels, off by one, llvmpipe only, stable over three runs)
and `crcbl-vk`'s open box in
`a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths` (one
channel, off by one, llvmpipe only). Both for `Scene::PointShadow`'s recorded
reason — a marching pass makes the depth buffer's last bits visible in the
picture, and the two paths compute the same world position through different
arithmetic. **If that slice re-adds a budget to `crcbl-vk`, it should be the
measured value per comparison and not `render_e2e`'s 16**: that constant is
per-scene there and zero everywhere it holds, and handing a second suite a
blanket sixteen is slack nobody measured.

**What the observable would be.** A sixth claim in `apps/lumen/tests/golden.rs`,
in the shape of `render_e2e`'s
`the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one`: two blocks up
each conductor's face, hung off its bottom edge five half-extents apart, and the
brass block's falloff asserted above one while the panel's is not. It reads
1.211 at 256×192 and 1.156 at 1280×960 with the cutoff at 0.75, against 1.007
and 0.897 with no reflection on that row — so a threshold near 1.08 has about a
fifteenth of margin either side. **The block's own shading runs the other way**,
which is what makes the measurement a claim about the reflection: the sun
reaches that face at a glancing angle and it darkens towards the floor, so a
build with no reflection on it reads _under_ one. It needs `BLOCK_FOOT` in
`room.rs` — the middle of the block's bottom edge — and a no-GPU bisection
beside `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head` showing that
the block reflects across most of its face where the panel reflects across an
eighth of its own.

### The roughness weight is unmeasured, and no assertion was invented for it

`ssr_blur.slang`'s second weight is the design's stated reason for this kernel
rather than a box, and **nothing in the tree separates it**. No fixture puts a
mirror-sharp surface next to a rough one at the same depth, which is the case it
exists for. Dropping the factor and re-running at a cutoff of 0.75: `lumen`'s
panel foot moves 26.6 → 24.3 and the brass block's 110.7 → 108.9, and the
falloff comparison, `MIRROR_GRADIENT`, `Scene::Ssr`'s ratio and the stepping
number all stay where they were.

It is kept on the construction argument — a tap on a surface the march had
nothing to say about weighs exactly nothing, which is what a matt floor under a
metal block is. What would give it a check is a fixture with the adjacency: a
mirror-grade strip laid into the brass block's face, or the panel standing on
the floor rather than on a plinth. Both are changes to a sample's content, which
the SSR slices have kept declining to make on the way past.

### `crcbl-vk`'s mesh goldens differ from a render on this machine, and always did

`mesh.png`, `mesh_ortho.png` and `mesh_second.png` differ on **100%** of their
pixels at max channel delta 2, and `mesh_shader_triangle.png` and `triangle.png`
on a few percent at delta 1 — on llvmpipe, with zero pixels over tolerance, so
every one of them passes. `crcbl`'s own goldens are exact on the same machine,
so those files were blessed against a different driver or Mesa version.

Nothing was re-blessed for it. Recorded because a blanket `CRCBL_BLESS=1`
absorbs that drift into whatever commit runs it — the blur slice did exactly
that and had to restore four files by name. **Bless the golden that failed, by
name.**

### One shared-code hazard

`ssr.slang` re-declares `depth_at`, `view_position` and `normal_at` verbatim and
`ssr_blur.slang` re-declares `depth_at` and `view_z`, because this repo has no
include mechanism by design — the manifest hashes one source per artifact.
`crcbl_shaders::ssr`'s `the_shared_screen_space_helpers_have_not_drifted`
compares the bodies as text and holds all of them: four copies of `depth_at`,
two each of `view_z`, `view_position` and `normal_at`. (The plan said three
copies of `normal_at`; `ssao_blur.slang` carries `depth_at` and a `view_z` cut
down from `view_position`, not a normal.)

Two shader **constants** are copied as well, and each has a guard beside that
one: `DEPTH_FAR`, which every screen-space source declares and
`the_far_plane_matches_the_constant_the_reflection_pair_declares` checks against
`crcbl_shaders::ssao::DEPTH_FAR`; and `THICKNESS_FLOOR`, which the march and its
blur both declare and `the_thickness_floor_matches_the_one_the_march_declares`
holds together. The blur has no ray, so that floor is the only length the march
owns which it can still evaluate — which is why it is a copy rather than a
uniform field.

Making that an equality rather than a substitution cost one rename: all three
files bind the projection block as `camera` rather than as `ssao`, because
`view_position`'s body names it. No compiled instruction moved and no golden
did.

## The browser entry point is shared; what the move left behind (2026-08-15)

S1B finding 2 is closed: `crcbl::web_exports!` writes the ten
`#[unsafe(no_mangle)]` symbols and the page state, and `apps/asteroids`,
`apps/breakout`, `apps/flappy`, `apps/horde` and `apps/hud` each invoke it. It
was landed as a move, and these are the things it deliberately did not fix.

### `asset_source` has no caller in any sample

All four samples that define it — asteroids, breakout, flappy, horde — export
`pub fn asset_source() -> Option<Rc<FetchSource>>` and nothing in the workspace
calls it. `opfs_store` is genuinely used (`crate::best` in three of them,
`crate::high_score` in breakout). `asset_source` is the speculative half.

Left alone because the task was a move and deleting it is a public-API change to
four sample crates in the same commit as the migration. It is **not** a wasm
export — it has no `#[unsafe(no_mangle)]`, so removing it cannot change what the
shim resolves. Deleting it is a two-line-per-sample change whenever someone
wants it gone.

### The unit test cannot observe `prepare`'s log line

`web::tests::the_generated_exports_drive_the_page` invokes the macro over the
`FakePending` fixture and drives nine of the ten symbols. It **cannot** assert
that `prepare` logs, because `log::set_logger` is process-global and
`args::tests::the_front_end_returns_the_contract_exit_codes` calls
`crcbl::core::log::init_logging` in the same test binary — whichever runs first
wins, and the assertion passed alone and failed in the suite. It was observed
failing both ways before being rewritten to push onto `LOG` directly.

What covers the line instead is `web/tools/smoke.mjs`, which `web/build.sh` runs
per demo against the real artifact and which asserts "the log queue delivers the
line prepare wrote". The exact rendered text was also read out of all five
browser-gate page logs: `<name>: prepared; assets from assets/`, unchanged from
the literal each sample used to carry, because the macro reaches it through
`HostedGame::NAME`.

`boot` is the tenth symbol and is not driven by the unit test at all: it opens a
`Web` shell, which exists only on `wasm32`. The browser gate is its only cover.

### `crates/crcbl/src/web.rs` is now the size that was the reason for the move

It holds the status codes, the log queue, `App`/`Stage`/`WebLoop`/`WebPending`
and `web_exports!`. The seam if it needs splitting is that last one — the macro
and its expansion are a separate responsibility from the state machine they
drive — but it would have to become `crates/crcbl/src/web/` with `mod.rs` and
`exports.rs`, which was outside this task's file set.

### The macro is reachable by two paths

`#[macro_export]` puts it at `crcbl::web_exports!`, and a
`#[doc(inline)] pub use` in the module makes `crcbl::web::web_exports!` work
too. The samples all call it as `crcbl::web_exports!`. Nothing enforces that; a
future sample writing the longer path is not wrong, just inconsistent.

## The command stream's contract, read from the other side

`crates/crcbl-webgpu` encodes the browser command stream and
`web/engine/gpu-stream.js` decodes it. Writing the second implementation against
the first surfaced six things about the contract. Three were fixed in the same
change and are gone from here; these are the three that were not.

### A bad presence byte and a bad enum code report the same error

`read_opt_string` passes its own field name down to `read_present`, so a
malformed presence byte on `BufferDesc::label` surfaces as
`InvalidEnum { field: "BufferDesc::label", code: 2 }` — the same shape a bad
`MemoryLocation` code produces. The two are different defects: one is a
structural framing error, the other a value the far side does not recognise.
Nothing in the docs says which a reader is looking at, and a JS implementer has
to trace two calls to find out.

A distinct `DecodeError` variant for a non-canonical presence byte would say it
plainly. Not done because it widens a public error enum for a case that has not
bitten anyone, and the JS half matched the current behaviour deliberately so the
two agree.

### `MEMORY_*` is the only enum code table not named for its enum

`LOAD_OP_*` and `STORE_OP_*` carry their enum's name; `MemoryLocation`'s codes
are `MEMORY_DEVICE_LOCAL` and siblings. Trivial, and the only cost is that a
reader has to grep to be sure there is no separate `Memory` enum. Renaming
touches both halves of the format and the fixture stays valid, since names are
not on the wire.

### The JS decoder is hand-written against the Rust tag table

The fixture check catches drift, which is why this is not urgent — but it
catches it rather than preventing it. Generating `gpu-stream.js`'s constants
from `tag.rs` would make a whole class of disagreement impossible instead of
merely detected. Declined for now: it adds a codegen step to a build that has
none, and the failure it prevents already fails loudly in the Pages job on every
pull request. Worth revisiting if the tag table grows to the full surface and
the two tables start being edited in separate sessions.

## The command stream is driven, but only the probe puts anything in it

`web/engine/demo.js`'s frame loop drains the stream, replays it and delivers
replies, on every frame of every demo. The only thing that ever encodes into it
is `crcbl-webgpu`'s probe, which the browser gate drives; in an ordinary page
`len` answers 0 and the loop costs one integer call. That is the intended state
rather than an unfinished edge, but it is worth writing down so nobody reads the
wiring as a working backend.

- **No HAL implementation writes through `StreamChannel::encode`.** The probe's
  shim calls `crcbl_webgpu::web::install` and nothing else does. This ends when
  the WebGPU backend lands.
- **"Encoding moves the buffer" is documented, not tested.** The stable half —
  that `release` does not move it — is asserted. Forcing a `Vec` realloc
  deterministically is allocator-dependent, so the moving half rests on the
  documented rule that a JS view must not outlive the frame it decoded.

### The umbrella's re-export is not what keeps the symbols

`crates/crcbl/src/lib.rs` re-exports `crcbl_webgpu as webgpu` on wasm32, and
that re-export is **not** required for the exports to reach the artifact.
Measured: a build with the dependency alone and one with the dependency plus the
re-export produce a byte-identical `crcbl_breakout.wasm` — compared with `cmp`,
not a diff wrapper — and `cargo machete` stays green either way.

It is kept so the crate is genuinely named by something rather than the ABI
resting on rustc keeping an rlib's `#[unsafe(no_mangle)]` symbols when nothing
references it, which `web/tools/check-exports.mjs`'s own header calls
unguaranteed. Deleting it is safe today, and `check-exports` is what would
notice the day that stops being true.

### Prettier rewrites unrelated lines in workflow YAML

Running `prettier --write` on `.github/workflows/pages.yml` reflows unrelated
double-quoted scalars to single quotes. The repo has no prettier config and no
prettier step in CI, so on YAML it is a formatter that only ever produces
drive-by diffs. Either adopt it for YAML with a config that settles quote style,
or keep it to markdown, where the project rule already puts it. Left alone for
now; the rewrites were reverted by hand.

### Not verified

- The stream exports on any target other than `wasm32-unknown-unknown` release.
  They are `cfg`-gated off elsewhere by design, so there is nothing to run.

## What the reply channel still owes

Both directions now run under the real browser gate: `web/engine/demo.js`'s loop
calls `putReplyStream` on every frame it has replies, and groups G and H of the
gate turn on answers that crossed that way. What is left is coverage, not
wiring.

- **`__crcbl_web_gpu_reply_pending` is exported and nothing calls it.**
  `check-exports.mjs` lists it as informational and does not fail on it. It is
  the diagnostic for "the engine stopped draining"; keep it when a shim reads
  it, drop it if the HAL work arrives without one.
- **`crates/crcbl/src/lib.rs`'s `pub use` doc undercounts the exports.** It says
  "the three `__crcbl_web_gpu_stream_*` exports", which is right for the stream
  family and reads as if it covers the module — the module exports more than
  that now. `demo.js` carried the same sentence and no longer does. The
  byte-identical-artifact measurement recorded beside the `pub use` was taken
  before the reply exports existed and has not been re-run.
- **`poll_readback`'s exact-length contract cannot be enforced at decode time.**
  Nothing in a reply buffer says what size the descriptor asked for, so the
  payload's own length prefix is all the decoder has. Whoever implements the HAL
  call owes the comparison against the descriptor it kept — the plan doc used to
  claim the reply buffer was sized from the descriptor, which is not how a reply
  stream works, and has been corrected.
- **Not covered by any reply shape yet:** the device-request poll, the rest of
  `AdapterInfo` and `DeviceCaps`, surface capabilities, and any reply carrying a
  `HalError`. The set that exists is deliberately representative, not complete,
  and the crate docs say so.

## `webgpu` is a name and a refusal, not yet a backend

`crcbl::backend::REGISTRY`'s `GpuBackend::WebGpu` entry returns
`WEBGPU_NOT_IMPLEMENTED` unconditionally. Replacing it means giving
`crcbl-webgpu` an `Instance`, adapter enumeration and a device, then deciding
whether it displaces `GpuBackend::Wgpu`'s automatic selection on wasm32 — wgpu
is still the browser's only automatic backend, and
`exactly_one_backend_is_auto_selectable_and_it_depends_on_the_target` is what
pins that, so the swap is a deliberate edit rather than something that can
drift.

It is registered rather than left out on purpose: an unregistered name yields
`UnknownBackend`, which reads as a typo, where the registered refusal reads as
work not yet done.

### The e2e scripts' backend hints omit `webgpu` deliberately

`crates/crcbl/tests/run-render-e2e.sh` and
`apps/lumen/tests/run-lumen-golden.sh` carry a "Name one:" usage hint listing
the backends that can draw a golden. `webgpu` is not among them and should not
be until it can render. **No script validates backend names against a
whitelist** — every one passes `CRCBL_GPU` and `--backend` straight through and
lets the Rust reject them — so these hints are documentation, not gates, and
nothing fails if they lag.

### `apps/sandbox`'s backend rejection message is stale

Its `Invocation::BadUsage` arm suggests `vk`, `mtl`, `dx12` or `null`. It
already omitted `wgpu` before this work and now also omits `webgpu`, while the
`USAGE` text directly above it lists both. Pre-existing drift, left alone as
outside the task. The other apps do not have the problem: they share
`crcbl::args::COMMON_OPTIONS_HELP`, which has one rejection message, and a
per-app test compares each copy against the engine's verbatim.

### The env-var path is not covered automatically

The refusal is tested through `request_open_backend`, which `open()` funnels
through, and was confirmed by hand against the `sandbox` binary under both
`CRCBL_GPU=webgpu` and `--backend webgpu` (exit 1, no fallback). Nothing sets
`BACKEND_ENV_VAR` in a test: it is `unsafe` in edition 2024 and would race the
other tests sharing the process. Stated as the gap it is.

### Not verified

The macOS and Windows **runtime** behaviour of the new registry entry. `crcbl`,
`crcbl-mtl` and `crcbl-dx12` all type-check clean against `aarch64-apple-darwin`
and `x86_64-pc-windows-msvc`, but no test ran on either platform; that verdict
only comes from CI.

## The stream carries adapters, a device and a surface; the backend does not exist

A browser enumerates adapters, opens a device and resolves a real canvas into a
`GPUCanvasContext`, all through the stream and all proven by groups G and H of
the browser gate. What stands between that and `crcbl::backend` accepting
`webgpu`:

- **Something that owns a frame loop.** `open` must resolve after at least one
  round trip. `web/engine/demo.js`'s loop now drains, replays and delivers, so
  the page's half exists — but it is the demo's loop, and a backend that only
  works inside `bootDemo` is not a backend. What is owed is the same three steps
  driven by something the backend constructs.
- **`impl Instance`, `impl PendingDevice`, `impl Device`.** Each is deliberately
  absent rather than stubbed, and `instance.rs` and `device.rs` argue why in
  their module docs: an impl written before the commands exist is a wall of
  methods that compile and answer nothing, and a `PendingDevice::poll` with no
  `Device` behind it can only ever answer `Pending`, which passes any test that
  polls a few times and gives up.
- **The feature intersection**, which has its own entry below.

### The browser golden gate is wired, and two scenes are excused by name

`web/run-render-harness-e2e.sh` now runs in `pages.yml` on Linux, with
`--expect-fail ssr,ui`. It is the only check in the tree comparing the browser's
_pixels_ against the same references the native suites use — the gap the sRGB
bug went through, since the goldens covered the offscreen path that already
worked and the browser gate read only text.

**The comparison is deterministic across machines, which was not assumed.** The
first green CI run reported the same pixel counts as the local one to the digit
— `ssr` 25611 differing and 459 over tolerance, `ui` 3872 and 506. So hosted
SwiftShader and local Chromium agree exactly, the expected-fail list is not
machine-specific noise, and a future change to either scene will move the
numbers rather than drift them.

**The two exclusions are not a suppression.** A listed scene that starts
_passing_ fails the run and says the list is stale, so the moment a driver
update or a fix makes either match, CI forces the list to be updated. Both print
their full numbers on every green run: `ssr` 58 gross pixels and ssim 0.988822,
`ui` 506 and ssim 0.983257.

**What is still owed, and it is a measurement rather than a decision:**

- **macOS and Windows need one run each to write their lists from.** The
  mechanism is exact in both directions, so a guessed list fails either as a
  regression or as a stale entry — correct behaviour, but not a way to learn.
  Note the Windows probe's four expected failures do **not** transfer: those are
  a canvas readback stranding the queue behind it, and this harness never
  touches a canvas.
- ~~`ui` has never been checked on real hardware~~ — **measured, and both scenes
  pass.** Driving the same harness against this machine's discrete adapter
  instead of SwiftShader gives **11 of 11**: `ui` at max channel delta **1**
  with **zero** pixels over tolerance, and `ssr` at max delta 9 with zero
  grossly wrong. So neither is a backend defect — **the WebGPU backend draws
  every one of the eleven goldens correctly on real hardware**, and both
  exclusions are properties of the software rasteriser CI has to use.

  Two things follow. The `--expect-fail ssr,ui` list is a statement about
  SwiftShader, not about `crcbl-webgpu`, and it should empty the day a hardware
  runner exists — which the mechanism will force, since a listed scene that
  passes fails the run. And the harness pins `swiftshader` deliberately: a
  golden comparison must not leave the rasteriser to the machine, so this was
  measured by patching a copy and restoring it, not by changing the gate.

### REVERSED IN FACT — the probe was going to be deleted and became the gate

This entry used to say: `crcbl-webgpu`'s probe, its `__crcbl_web_gpu_probe_*`
exports and `web/engine/gpu-probe.js` exist only so the round trip is observable
before a backend exists, and **delete all three when the backend installs its
own** — because a second way to drive the stream survives by accident and then
has to be kept working.

**The opposite happened, and deliberately.** The probe page now carries 25
groups (`G` through `AE`), runs in three CI jobs across Linux, macOS and
Windows, and is the **only** gate on this backend's seam — the native suite is a
native binary and cannot reach a browser at all. Group AB opens a real
`WebGpuDevice` and holds its whole `supports()` matrix against `DIVERGENCES`,
which is the browser counterpart of the native parity report and the only thing
checking those declarations anywhere.

**So the original reasoning was right about the risk and wrong about the
remedy.** A second way to drive the stream did survive — and it turned out to be
the one that could reach the platforms the engine actually ships on. The demo
gates assert log lines and HUD text; the probe asserts bytes.

**What is still true from the old entry**, and worth keeping: the probe refuses
when anything else has installed a channel, so the two ways of driving the
stream cannot both be live at once. That is what has kept it from being a second
maintenance burden rather than a second gate.

### One command is answered exactly once

`drain_replies` refuses a second reply for a sequence, in the same buffer or a
later one, so an enumeration yields one adapter. That fits WebGPU, where
`requestAdapter()` grants one or none. A backend that must report several needs
a counted-list reply shape rather than several replies. `Reply::Adapter`'s doc
still says "one entry of an enumeration", which implies the forbidden thing —
wording, not behaviour.

### Not verified

The round trip on hardware Chromium, on Firefox, or on a browser whose
`adapter.info` is populated differently. The gate ran under SwiftShader in Xvfb.
The name check compares against the same browser, so it holds wherever it runs —
but nowhere else has run it.

## The adapter reply reports features the stream cannot serve

`web/engine/gpu-replay.js` maps a browser's `adapter.features` onto
`crcbl_hal::Features` and reports what the browser said. That is right while
there is no device — but it means `TIMESTAMP_QUERY` is reported on a browser
that has it, while `crcbl-webgpu` has no `create_query_set` command and could
not serve a query set at all.

**When `impl Instance` lands it must intersect the mapped set with what the
stream can actually encode**, which is what `crcbl-wgpu`'s own feature mapping
already does for query sets. Noted in `gpu-replay.js` and in
`crcbl-webgpu/src/instance.rs`; it is the sort of thing that reads as correct
right up until a caller believes it.

### Considered and declined

- **`GPUAdapter.isFallbackAdapter` as `DeviceType::Cpu`.** It grades
  _performance_, not device class; a fallback adapter is not necessarily a CPU
  one, and the mapping would put a guess where the honest answer is "declined to
  say".
- **Reporting `max_sampler_anisotropy: 16` and granting `SAMPLER_ANISOTROPY`**,
  the way `crcbl-wgpu` does. WebGPU accepts `maxAnisotropy` above 1 but reports
  no queryable ceiling, and `Limits` is what the backend _guarantees_ — 16 would
  be a number nothing told us.

### Coverage gap in what the browser corroborates

Group G corroborates one of nineteen limits through wasm (`max_image_2d`). The
other eighteen are checked in-page against the live adapter, and field by field
against a stub in `gpu-replay.mjs` — a weaker claim, since neither goes through
the wire. `vendor_id`, `device_id`, `device_type` and `driver` are never
corroborated at all, because a browser has nothing to disagree with.

### Not reviewed

Whether `Features::DEBUG_MARKERS` genuinely reaches a capture tool in every
browser. It is granted unconditionally on the grounds that `pushDebugGroup` is
core WebGPU, matching what `crcbl-wgpu` does, but nothing was measured.

## The device request round-trips; `PendingDevice` is still not implemented

`PendingDevice::poll` returns `DeviceRequestState`, whose `Ready` arm carries a
`Box<dyn Device>`. With no `Device` impl, `poll` could only ever answer
`Pending` — a device request that is never ready, on a trait whose contract is
that it eventually is, and it would pass any test that polls a few times and
gives up. So the state machine is exposed as `crcbl_webgpu::DeviceProbe`
instead, and `poll` becomes `absorb` plus a match when `Device` lands.

- **Device loss has no reply.** `GPUDevice.lost` and `uncapturederror` belong to
  `Device::take_error`; nothing listens for either and there is no tag for them.
  Deliberate — nothing holds a device long enough to lose one — and a different
  event from a request that failed.
- **One device is implied, not enforced.** No command carries a device id. The
  owner side-table that answers `HalError::ForeignObject` is owed the moment a
  second device exists, which the HAL's own docs warn about because two pools
  genuinely issue identical handle bits.
- **`compatible_surface` crosses and is refused**, loudly, because the replayer
  has no surface table. `create_surface`, `destroy_surface` and `surface_caps`
  are still unencoded — the last piece before `impl Instance`.
- **The "not a copy of its adapter" check degrades on a floor-level adapter**
  and says so in its detail rather than failing. Forcing a machine-independent
  difference would need requestable limits on the wire, which `DeviceDesc` does
  not carry.
- **The refusal message names bit indices, not flag names** (`bit 9`), because a
  copy of `Features`'s names in JS would be a second table to keep in step. The
  names reach a log through the `Features` word the reply carries.
- **Coverage:** no test opens two devices, and the gate ran only under
  SwiftShader here — hardware failed the readback control on this machine, which
  is the documented Xvfb-plus-hardware row rather than a regression.
- **Pre-existing drift, untouched:** `web/tools/browser-e2e.mjs`'s header still
  says "Five groups" while A through G exist.

## The WebGPU parity gate passes — wire it into CI

`./web/run-render-harness-e2e.sh` drives all eleven golden `Scene`s through
`crcbl-webgpu` in headless Chromium, reads each frame back, and compares it
against `crcbl/tests/golden/<name>.png` with `crcbl-golden`'s comparator at
`Tolerance::RASTERISER`. **All eleven match** on this machine's GPU; `dunes` and
`probes` are pixel-identical, and the three with any residual (`spot_shadow`,
`point_shadow`, `ssr`) differ only on shadow-comparison and march-crossing
pixels, inside tolerance. The gate exits 0.

**What is owed: a CI step.** The gate is still not wired in, and the reason is
no longer that it fails — it is that a CI runner has no GPU, and SwiftShader's
storage-buffer ceiling stops nine of the eleven scenes from drawing at all (see
the ceiling entry below). Two ways forward, and they are not exclusive:

- **Wire it now for the two scenes SwiftShader can draw** (`sprite`, `ui`). That
  needs a scene filter on the harness — it drives `GreyboxSprite::ALL`-style
  whole-set today — and would catch a regression in the 2D path, the offscreen
  surface, the readback and the comparator, which is most of the machinery.
- **Split the draw-args bind group** so the forward path fits the guaranteed
  floor, after which all eleven run on a software adapter and the gate covers
  the whole set in CI. That is the durable answer.

Until one of them lands, the eleven-scene result is a thing someone ran by hand
on hardware, not a thing CI defends — so it can silently regress.

## Left over from packing draw args into eight storage buffers

The draw-args pass is at exactly 8 storage bindings now and all eleven golden
scenes render on a software adapter. Three things that slice surfaced and did
not fix:

- **`ssr` does not match its golden on SwiftShader** — 58 gross pixels (0.118%
  against a 0.1% budget) and ssim 0.988822 against a 0.99 floor. Marginal on
  both thresholds, and **newly observable rather than newly caused**: that scene
  could not create a pipeline on a software adapter before, so it has never been
  compared there. On real hardware it matches with max channel delta 9 and zero
  gross pixels, and the diff is banding confined to the reflection ray-march and
  the horizon edges — geometry and instancing are right, which is what a
  regrouping error would have broken. **Decide**: widen the rasteriser tolerance
  for this scene, or record it as a known software-rasteriser difference beside
  `ui` (506 pixels, ssim 0.983). Until then a CI gate on the software adapter
  can only claim 9 of 11.
- **The new seam check covers 4 of the 12 `create_pipeline_layout` sites** in
  `crcbl-render` — draw_gen's three and `forward.rs`'s `mesh frame`. Unchecked:
  `light_grid`, `sprite_pass`, `ssao` (x2), `ssr` (x2), `ui_pass`, and
  `forward`'s `tonemap`. None is near the limit today, so this is prevention
  rather than a live defect, and extending it is mechanical.
- **`crcbl-dx12`'s register case list claims "every shader is listed" and omits
  `clear_counters`.** Found while updating `draw_gen`'s row; pre-existing. The
  table transcribes each shader's binding classes so the register assignment can
  be asserted, so a missing shader is an unasserted one.

## Two swapchain holes the seam does not close on any backend

Found while fixing dx12's missing acquire tracking; neither is a dx12 bug, and
neither should be fixed on one backend alone.

- **Acquiring twice without presenting is accepted** by dx12, vk and mtl — each
  simply overwrites the outstanding acquire. Only wgpu refuses it
  (`"acquire_next_frame with a frame already acquired; present it first"`).
  Making the other three refuse it would turn dx12's own e2e job red today: the
  windowed loop in `crcbl-dx12/src/swapchain.rs` acquires and then calls
  `draw_and_present`, which acquires again before presenting. So the question is
  which behaviour the seam means — if wgpu is right, three backends and a caller
  need fixing; if it is wrong, wgpu's refusal should go. Decide before adding it
  to `hal_seam_e2e.rs`.
- **`reconfigure_swapchain` does not clear the outstanding acquire** on dx12, vk
  or mtl, so `acquire` → `reconfigure` → `present` is accepted even though the
  reconfigure already destroyed that image and its view. Exotic — the engine
  only reconfigures from the `OutOfDate` arms, never between an acquire and its
  present — but it is a real hole, identical on three backends, and cheap to
  close once someone decides the first question.

The suite that would hold all four to an answer already exists
(`crates/crcbl/tests/hal_seam_e2e.rs`, run by CI on WARP, lavapipe, Metal and
wgpu), so these are decisions rather than infrastructure.
