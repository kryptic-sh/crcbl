# Topic 54 — Android: the shell backend the platform is missing

Written 2026-09-07, from a survey of what an Android build would actually hit.
Its place in the set is [15-windowing.md](15-windowing.md)'s backend table,
which this adds a row to and nothing else; the precedent for how a platform port
is shaped here is the wasm track, whose record in the
[browser notes](../notes/browser.md) (_What the deleted 10-wasm-webgpu plan left
behind_) is the honest account of what such a track costs. The decision that
this is a windowing topic at all is `docs/notes/backends.md`'s, under
"Considered and declined: an OpenGL / GLES backend": _"The Android gap is a
`crcbl-shell` surface backend, not a HAL backend — `crcbl-vk` already exists and
is the best-tested path in the workspace."_ The same note ranks it "the largest
coverage win available", below finishing Metal.

That framing is the whole reason this document is short where a new backend
would be long. The [platform matrix](../notes/backends.md) already puts Vulkan
on Android, and `crcbl::backend`'s registry already registers
`GpuBackend::Vulkan` on every target that is not `wasm32` — so on an Android
build the GPU half is present, auto-selectable and untouched. What is absent is
a window to give it.

## Where this is

**An Android build compiles and opens nothing.** `crcbl_shell::backend`'s
`REGISTRY` gates its Wayland and X11 entries on `target_os = "linux"`, and
Android is not Linux to `cfg`: the table on that target holds only the headless
entry and the web entry, neither of which is `auto`.
`crcbl_shell::backend::open` says so in its own "Today" section — _"Every other
target has only `HeadlessShell`, which is not auto-selected, so this returns
`ShellError::NoBackend` there unless `CRCBL_SHELL=headless` is set — the honest
answer for a build with no window-system backend in it."_ That is the gap,
stated by the code before anyone asked.

Nothing else is Android-shaped and broken. Read from the tree on 2026-09-07:

- **Nothing in the tree names the target.** `git grep -il aarch64-linux-android`
  and the same for `ANativeActivity`, `AAssetManager` and `cargo-ndk` each
  return nothing. The word "android" appears in tracked files only as a browser
  user-agent in `crates/crcbl-shell/src/web/mod.rs` and `web/engine/shell.js`,
  and once in `.github/workflows/ci.yml` noting that Khronos ships the
  validation layers for Android and no other platform.
- **The dependency graph is already clean.** Everything platform-bound is
  `cfg`-gated at the manifest — `crcbl-vk` and `crcbl-golden` on `not(wasm32)`,
  `crcbl-mtl` on macOS, `crcbl-dx12` on Windows, `cpal` on `not(wasm32)` — so
  `cfg(target_os = "android")` inherits the native set, which is the right set.
  `deny.toml` already carries a skip for `jni-sys` reading "transitive; the
  android backends of cpal/ndk", and `Cargo.lock` already resolves `jni`, `ndk`,
  `ndk-context` and `ndk-sys`.
- **`crcbl-vk` already loads the right library.** The survey that preceded this
  document listed "whether `ash`'s `Entry::load` opens `libvulkan.so` (no
  `.so.1`) on Android" as undeterminable from the repository. It is determinable
  from the pinned dependency: `ash` 0.38.0+1.3.281's `Entry::load` selects
  `LIB_PATH` by `cfg`, and the
  `any(target_os = "android", target_os = "fuchsia")` arm is `"libvulkan.so"`.
  `VkInstance::open`'s single `Entry::load` call therefore needs no change.
- **The device floor is Vulkan 1.3 and nothing above it.** `VkInstance::open`
  refuses a loader reporting below 1.3; `crates/crcbl-vk/src/adapter.rs`'s
  `Core13Support` requires `dynamicRendering`, `synchronization2` and
  `maintenance4`; `crates/crcbl-vk/src/device.rs` asks for `timelineSemaphore`
  and `shaderDrawParameters` unconditionally; and
  `crcbl_hal::DeviceDesc::for_adapter` requires `Features::COMPUTE` and
  `Features::TIMELINE_SEMAPHORE`, described there as "the two nothing in the
  engine can work without". **The survey that preceded this document said
  `Features::GPU_DRIVEN` is hard-required by the forward, grid and sprite
  passes; it is not.** Those `required_features` lines are in those modules' own
  `#[cfg(test)]` helpers. The production path is
  `crcbl::engine::GpuContextDesc`'s `Default`, whose `required_features` is
  `Features::empty()` and whose comment says why: "demanding `GPU_DRIVEN` would
  refuse to run on the lesser devices `docs/plan/39-capabilities.md` requires
  the engine to degrade onto". A device without `drawIndirectCount` selects
  `GeometryPath::IndirectPerBatch` — the variant documented as "the floor, and
  what WebGPU gets" — and `crates/crcbl-vk/src/adapter.rs`'s own tests assert
  exactly that degradation.
- **Touch is already the seam's vocabulary.** `ShellEvent::Touch` carries a
  `ContactId`, a `TouchPhase` and a position; `ShellCaps::TOUCH` gates it;
  `Pending::observe` folds it into `TouchContact`; `crcbl_ui::touch` has the
  on-screen controls; `HeadlessShell::touch` scripts it. Only the web backend
  sets the capability today. [15-windowing.md](15-windowing.md)'s "Explicitly
  out (post-MVP or never)" list still ends with the word "touch", and that line
  was overtaken by the browser backend — it is wrong as written and this
  document is the correction.
- **The lifecycle vocabulary is the real hole.** `ShellEvent`'s window-lifetime
  variants are `Resized`, `ScaleFactorChanged`, `CloseRequested`,
  `WindowDestroyed`, `Focus`, `PointerFocus` and `MonitorsChanged`, and none of
  them says "the surface went away and the window did not" — which is what an
  Android app does every time it goes to the background. Underneath it,
  `crcbl_hal::SurfaceError::Lost` is documented as _"Not recoverable by
  reconfiguring"_, and `crcbl::engine`'s acquire path recovers `OutOfDate` and
  returns everything else as a frame error.

## The decisions

Eight, taken 2026-09-07. Each names the alternative it refuses, because each of
them is a road somebody will propose again.

### 1. Android is a shell backend. The HAL is not touched

`crcbl-vk` gains a surface variant and an extension; nothing else in the GPU
stack changes, and `crcbl_hal::BackendKind::Vulkan` is what an Android device
reports, so the capability parity tables in `crcbl-hal` need no new column.

**Refused: a lower renderer tier for older phones.** That is the GLES argument
`docs/notes/backends.md` already answered — _"The blocker is above the seam, not
at it… A Tier C is a renderer change with a third draw-emission path and a third
set of golden images, which is far more expensive than the backend crate it
would sit under."_ The same sentence applies whatever API the tier is spelled in
— and it is not needed here anyway: decision 5 says where the real line is, and
the engine already degrades above it.

### 2. The FFI is hand-written, and the framework crates are out

The Android backend declares the NDK entry points it uses — the activity
callbacks, `ALooper`, `AInputQueue`, `ANativeWindow`, `AConfiguration`,
`AAssetManager`, `AMotionEvent`, `AKeyEvent` — as `extern "C"` in this
workspace, the way `crates/crcbl-shell/src/win32/` and
`crates/crcbl-shell/src/appkit/` already do for their platforms.

**Refused: `winit` and `android-activity`.** By the rule
[15-windowing.md](15-windowing.md) states and has held to on every other
platform: _"Rejected: frameworks that own policy — winit, SDL, GLFW"_, against
_"Accepted: thin bindings to APIs the OS or driver requires by ABI"_. An
activity glue crate owns the loop, the lifecycle state machine and the input
translation — which is the policy, not the ABI. The note in that document about
Windows and macOS is the measure to apply: _"FFI declarations are code we write
and own — dozens of functions, not thousands; audited by use."_

### 3. The glue thread owns `loop {}`; the seam is not inverted

`crcbl::engine::drive` is a `loop { engine.frame() }` and stays one. The Android
backend runs on the thread the activity glue gives it, `Shell::pump` drains what
the looper delivered, and `Shell::wait_events` blocks on the looper — which is
exactly what that method's contract already allows for and what
`ShellCaps::EVENT_WAIT` announces.

**Refused: the browser's inversion.** The web backend is the one place the
platform owns the loop: `crcbl::web`'s `__crcbl_web_frame` is called from
`requestAnimationFrame`, and every browser demo is built around that. Android
does _not_ require it — an activity's native thread may block — and taking the
inversion anyway would mean a second driver for a platform that does not need
one. This is the decision most likely to be revisited under pressure, because
the ownership of the main thread versus the glue thread is where an Android
backend usually goes wrong; the Risks section keeps it open as an implementation
hazard rather than an unsettled design question.

### 4. A destroyed surface gets its own event, and `SurfaceError::Lost` becomes recoverable

Two changes, and they are one decision because either alone is useless.

- **`ShellEvent` gains a surface-lifetime pair** — the surface behind a live
  window went away, and a new one arrived — sitting beside `Resized` rather than
  inside it. `Shell::surface_target`'s documentation already has the vocabulary
  for the second half: _"Re-query it after a `set_mode`: some backends recreate
  the underlying surface on a mode switch, and a cached target then names a dead
  object. A resize is not such an event."_ Android adds a second reason to
  re-query, not a new concept.
- **`crcbl_hal::SurfaceError::Lost` stops meaning "fatal".** Its doc comment
  says "Not recoverable by reconfiguring", which is true on a desktop and false
  on a phone; `crcbl::engine`'s acquire and present arms gain a `Lost` path that
  drops the swapchain, waits for the new surface and rebuilds — the shape the
  existing `OutOfDate` arm has, one step longer.

**Refused: reusing `WindowDestroyed` or a zero-sized `Resized`.** The first is a
lie the engine would act on by tearing the window down, and the app would then
have nothing to come back to. The second is worse: it is indistinguishable from
a minimised desktop window, so every consumer that already handles a zero extent
would silently do the wrong thing. `SurfaceTarget`'s own module documentation
makes the argument for spelling a new platform out rather than folding it into
an existing variant — _"a silently-ignored new platform is exactly the failure
this type exists to prevent"_.

### 5. The floor is Vulkan 1.3, and a lesser phone lands on the browser's path

The floor Android inherits is the one the "Where this is" section reads off the
code: a 1.3 loader, the three 1.3 core features, timeline semaphores and
`shaderDrawParameters`. Nothing is added to it for this platform and nothing is
taken off it.

Above that floor, a phone that lacks bindless or `drawIndirectCount` is **not
refused**. It selects the reduced `GeometryPath` and `BindingModel` that
[39-capabilities.md](39-capabilities.md) owns and that the browser-boundary
table in the [browser notes](../notes/browser.md) already describes:
`GeometryPath::IndirectPerBatch` draws and `BindingModel::ArrayPages` textures.
That path is built, shipped, and gated in a real browser on every CI runner — so
Android's weak-device story is a path this workspace already tests every push,
not a new one.

**Refused: raising the floor to `GPU_DRIVEN` so the Android path is the good
one.** It would refuse mid-range hardware outright to avoid testing a path that
already exists, and `GpuContextDesc`'s default comment refuses it in as many
words. **Also refused: lowering the floor below Vulkan 1.3.** Dynamic rendering
and synchronization2 are not features the renderer branches on — they are how
every pass is recorded — so a 1.2 path is decision 1's refused Tier C reached
from the other side.

The consequence is stated rather than hidden: **the Vulkan 1.3 requirement, not
the feature set, is what decides which phones run this**, and which phones those
are is an open question in Risks.

### 6. The two touch fixes land in the seam before the backend, not with it

`docs/backlog.md` records the decision, dated 2026-09-06: a release that arrives
with a leave, and a `pointercancel` naming no button, are _"what any touch
platform does — an Android or iOS backend would hit the same two"_, and the fix
is to move both out of `web/engine/shell.js` into `crcbl-shell`'s pointer
handling — `PointerCapture::resolve` treating a release-with-leave as a release,
and `Pending::observe` remembering the button that went down.

They are listed here as a **prerequisite rung** rather than as Android work.

**Refused: an Android-local workaround.** That is the shape the backlog entry
already rejected: _"a per-backend workaround would be written once per
backend"_. Landing them first also means the Android backend is written against
a seam that already behaves, so a touch defect found on a phone is a phone
defect.

### 7. Assets come from the platform, and the compile-time root does not travel

An `AssetSource` over `AAssetManager` is a direct mirror of
`crcbl_assets::DirSource`: both key through `crcbl_store::web::canonical_key`,
which that trait requires an implementation to enforce rather than assume, and
both can answer at once — an asset-manager read does not block, so it satisfies
`AssetSource::read`'s "never blocks, on any implementation" without ever
answering `StorageError::Pending`. Storage roots come from the activity's own
paths through `crcbl_store::NativeStorage::at`, not from `dirs`.

**Refused: unpacking assets to a directory at first run so `DirSource` works
unchanged.** It doubles the install size, it needs a version check nobody would
maintain, and it converts a read that cannot fail into one that can. The reason
this needs a decision at all is a mechanism `apps/viewer`'s `shelf::root`
documents plainly: a native sample finds its assets from an environment variable
with a `CARGO_MANIFEST_DIR` fallback, and _"there is no lookup beside the
executable. Nothing in this workspace packages a binary with an asset directory
today"_. An APK is the first thing that does, and the fallback is a path that
cannot exist on the device.

### 8. The build is cargo plus one script

Android's build tooling is one script, on `web/build.sh`'s pattern — its own
header states the standard: _"the same script the Pages workflow runs, so 'it
works in CI' and 'it works on my machine' are the same claim."_ The script
produces the APK around a native activity; the samples are already `cdylib` plus
`rlib`, which is the shape a native activity needs and the shape the browser
already consumes.

**Refused: Gradle.** It is a second build system with its own dependency
resolution, its own cache and its own opinions about where output goes, adopted
to wrap a library cargo already built. The wasm track is the precedent for
declining exactly this: the [browser notes](../notes/browser.md) record
`wasm-bindgen` going from mandatory tool to no tool at all, leaving `cargo` and
`node` as the whole list. If the APK assembly genuinely cannot be done without
it, that is a finding for this document, not a silent adoption.

## What changes, crate by crate

Every symbol named here exists today unless the line says it is new.

### `crcbl-core`

- **`SurfaceTarget::Android`** — new: a non-null `ANativeWindow*`, on the shape
  the `AppKit` variant already has for a `CAMetalLayer*`. The variant table in
  that type's module documentation gains its row, naming
  `vkCreateAndroidSurfaceKHR` as the Vulkan entry point.
- **The log sink** stays where it is and gains a client. `crcbl_core::log`
  already separates the process's filter from whichever sink took the slot —
  `register_sink` and `sink_permits` exist because `crcbl::web`'s queueing
  logger needed them in a browser _"where there is no stderr to write to"_. An
  Android sink writing through `__android_log_write` is that same shape with a
  different destination, so `log warn,crcbl_vk=trace` at the debug console means
  the same thing in logcat as in a terminal. No new mechanism.

### `crcbl-shell`

- **A new `android` module under `crates/crcbl-shell/src/`**, gated on
  `target_os = "android"`, holding the hand-written FFI of decision 2 and the
  `Shell` implementation over it. The template is the web backend rather than a
  desktop one: callbacks append to one queue that `pump` drains, the window has
  no size until the platform reports one, and the primary contact is mirrored as
  a pointer — `crates/crcbl-shell/src/web/mod.rs`'s module docs argue each of
  those and the arguments carry over unchanged, except that Android's
  contact-to-pointer synthesis is ours to do rather than the browser's.
- **`ShellBackend::Android`** and its `REGISTRY` entry, `auto: true`, `#[cfg]`
  on the element the way every other platform's is.
- **`ShellCaps`** for that backend: `TOUCH`, `EVENT_WAIT` and `FRACTIONAL_SCALE`
  set — density comes from `AConfiguration` and is fractional. Clear:
  `TEXT_IME`, because the soft keyboard's composition is not something this
  backend reads; `POINTER_LOCK`, `POINTER_CONFINE`, `POINTER_WARP` and
  `RAW_POINTER_MOTION`, which have no touch meaning; `MULTI_WINDOW`,
  `WINDOW_POSITION`, `SERVER_DECORATIONS` and `DRAG_DROP`. Each is a behaviour
  that is absent, which is the rule that module states.
- **`crates/crcbl-shell/src/linux/keymap.rs`'s `#[cfg]` widens.** That table
  maps kernel evdev codes to `KeyCode`, and its own documentation says why it is
  a fixed table rather than a layout database: _"`KEY_A` is 30 whether the user
  types QWERTY, AZERTY or Dvorak"_, and the numbers are
  `linux/input-event-codes.h`, _"kernel UAPI and therefore frozen"_.
  `AKeyEvent_getScanCode` returns exactly those codes, so the mapping is reused
  rather than rewritten. The `xkb` sibling is not: there is no keysym source
  here.
- **The surface-lifetime events of decision 4**, in `ShellEvent`, with
  `HeadlessShell` able to script both — otherwise the engine's new recovery path
  is testable only on a phone, which is decision 4 built and unverified.

### `crcbl-vk`

Small, and the whole of it is known:

- `khr::android_surface::NAME` joins the instance-extension loop in
  `VkInstance::open`, which already asks only for what is present.
- `InstanceInner` gains an `android_ext: Option<khr::android_surface::Instance>`
  beside the Wayland and Xcb loaders.
- `create_surface` gains the `SurfaceTarget::Android` arm. The `Win32` and
  `AppKit` arms next to it still return `Unsupported`; this one will not.

**Pre-rotation is not in this list, deliberately.**
`crates/crcbl-vk/src/swapchain.rs` picks `IDENTITY` when the surface offers it
and the surface's current transform otherwise, and its comment already names the
case: _"Anything else means the driver rotates the image for us, which is a
phone concern the engine does not have yet — but passing an unsupported
transform is a hard error, so this is a check, not an assumption."_ So a
landscape phone works and pays the compositor's rotate. Making the renderer
rotate instead is a rung of its own in the delivery table.

### `crcbl-hal`

`SurfaceError::Lost`'s documentation changes with decision 4 — it is no longer
"not recoverable by reconfiguring", and the doc has to say what a caller is now
expected to do. No new variant: "the surface is gone" is the right name for what
happened, and the recovery belongs to the caller either way.

### `crcbl`

- **`crcbl::engine`'s acquire and present arms** gain the `Lost` recovery. This
  is the change with the widest blast radius in the list, because that path is
  every backend's, and it is the reason `HeadlessShell` must be able to script
  the loss.
- **An Android front end**, doing for an activity what
  `crcbl::args::run_front_end` does for `argv`: there is no command line, so the
  options a game boots with come from the activity's intent extras or from a
  default. This is the native twin of the `web_exports!` split that `crcbl::web`
  owns, and the same division applies — what stays in the sample is the part
  that is the game's own.

### `crcbl-assets`, `crcbl-store`, `crcbl-audio`

- **`crcbl-assets`** gains the `AAssetManager` source of decision 7, beside
  `DirSource` and `MemorySource`.
- **`crcbl-store`** gains nothing. `NativeStorage::at` already takes an explicit
  root, which is what the activity's internal data path is.
- **`crcbl-audio`** needs `ndk-context` initialised from the activity before
  `cpal` opens a device; `Cargo.lock` already resolves that chain and
  `deny.toml` already allows it. Nothing in the crate's own code changes.

## Delivery

Every day figure below is an **estimate** taken as a range from the survey that
preceded this document, not a measurement. They are sequential engineer-days,
and the scheduled rungs together are that survey's own 22–35 total — rung 2 is
drawn from its tests-and-parity line rather than added to it, and rung 9 is
outside the total because it is unscheduled.

| Rung                                                  | What it buys                                                                                                                        | What it costs                                                                                                                                                              |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **1. The target type-checks in CI**                   | The cheapest possible early warning, and the first honest answer about whether anything unconditional blocks the build              | 0.5–1 d. `clippy` and `rustdoc` on `aarch64-linux-android`, riding the job that already does this for Metal and D3D12 in `.github/workflows/ci.yml`                        |
| **2. The touch fixes into the seam** _(prerequisite)_ | Decision 6: a seam that behaves before a second backend is written against it, and two shipped browser defects fixed once           | Part of the 3–5 d the survey prices for tests and parity. `PointerCapture::resolve` and `Pending::observe`, with `web/engine/shell.js` losing both                         |
| **3. The lifecycle seam**                             | Decision 4: the vocabulary an Android app's backgrounding needs, scriptable through `HeadlessShell` and therefore testable on Linux | 2–3 d. New `ShellEvent` variants, `SurfaceError::Lost`'s contract, `crcbl::engine`'s recovery arm — the widest-reaching change in this document                            |
| **4. `crcbl-vk`'s Android surface**                   | The GPU half, complete                                                                                                              | 1–2 d, and the survey's own note is that the code risk here is nil; what is uncertain is decision 5's floor, not this code                                                 |
| **5. The shell backend**                              | A window. Everything else in this document is inert without it                                                                      | 8–12 d — the bulk of the whole topic. Hand-written NDK FFI, the looper pump, lifecycle, density, touch, key scan codes, and the registry entry                             |
| **6. Platform services**                              | Assets out of the APK, saves in the app's own directory, logs in logcat, audio through AAudio                                       | 3–4 d. Decision 7's `AssetSource`, `NativeStorage::at` wiring, the `register_sink` logcat sink, `ndk-context` for `cpal`                                                   |
| **7. Build tooling**                                  | An APK anyone can install, from one command                                                                                         | 3–5 d. Decision 8's script and the linker configuration; the risk is holding the "cargo plus one script" line                                                              |
| **8. A gate that runs on a device**                   | The only thing that turns any of the above from "compiles" into "works". Screenshot goldens need no window, so `adb` can drive them | 2–4 d, and the survey marks this one **uncertain** — see Risks on emulator Vulkan 1.3                                                                                      |
| **9. Renderer-side pre-rotation** _(unscheduled)_     | The compositor's rotate on a landscape phone, removed                                                                               | 2–3 d, and it is a separate row because nothing above it needs it: `crates/crcbl-vk/src/swapchain.rs` already picks a supported transform and the picture is already right |

The ordering is the table's own: rungs 1 through 3 are all doable and testable
on this workspace's existing hardware, and rung 8 is where the topic stops being
a claim.

## Risks

These first ones are **open questions this document deliberately does not
answer**, because no evidence in the repository settles any of them and guessing
would make a plan that reads as verified.

- **Which real devices clear the floor.** Decision 5 keeps the Vulkan 1.3 loader
  check and the 1.3 core features. Nothing in this repository can say what
  fraction of shipped Android hardware reports a 1.3 loader, and that answer —
  not the feature set, which degrades — decides whether this topic is a platform
  or a demo. It is also the one question here that needs no engine work at all
  to start answering — a device reports its loader version to anything that asks
  — so it should be asked before rung 5 is committed to. The second half of the
  same question is a **quality** one that no device list answers: a phone that
  clears the floor and lands on `GeometryPath::IndirectPerBatch` runs the
  browser's draw path at a phone's power budget, and nothing has measured what
  that looks like.
- **Whether CI can host an emulator with Vulkan 1.3.** Every GPU job in
  `.github/workflows/ci.yml` today runs on a software rasteriser. Rung 8 is
  priced as uncertain for this reason, and if the answer is no, the on-device
  gate is a physical device somebody plugs in — which is not a CI job.
- **The main-thread and looper ownership.** Decision 3 says the glue thread owns
  the loop and blocks in `wait_events`. That is the standard shape and it is
  also where an Android backend usually gets it wrong; it is listed here so the
  first surprise is cheap.

The rest are known, with what is actually known about each:

- **No compressed-texture format Android hardware prefers.** `crcbl_hal::Format`
  carries the BC family and nothing else, and `Features::TEXTURE_COMPRESSION_BC`
  is the only compression capability. ETC2 is the format every Vulkan Android
  device supports and ASTC is what a shipping title would use. **Measured
  2026-09-07:** `git ls-files` matching `.ktx`, `.ktx2`, `.dds` and `.basis`
  returns nothing, so no shipped asset in this workspace is compressed at all
  and nothing is blocked today. It becomes real the moment a phone has to hold a
  real texture budget.
- **The shell end-to-end pattern does not transfer.** Every desktop backend is
  tested by an out-of-process suite that injects events into a real compositor
  or window server. There is no compositor to inject into on Android. What does
  transfer is the offscreen half: `HeadlessShell` plus
  `SurfaceTarget::Offscreen` means the screenshot goldens need no window, so
  they can run on a device over `adb` — which is rung 8's shape and why the
  goldens are the gate rather than an event injector.
- **Nothing here has run on a phone, and neither has the touch path that
  exists.** `docs/backlog.md` says so of the browser's touch coverage in as many
  words: the `touch-action` check is Chromium under emulation, and _"nothing
  here has run on a phone"_. Every touch behaviour this topic inherits is in
  that state.
- **`dirs` answers wrongly on Android rather than not at all**, which is why
  decision 7 routes storage through `NativeStorage::at`. Read from the resolved
  version on disk: `dirs` 6.0.0 compiles its Linux XDG module for Android, so
  `config_dir` is `$XDG_CONFIG_HOME` or `$HOME/.config`, and `dirs-sys` 0.5.0's
  `home_dir` has an Android arm whose fallback returns `None` — the `getpwuid_r`
  path is compiled out there. So the answer is either `None`, and
  `NativeStorage::config` fails at run time with "no config directory found", or
  a `$HOME`-derived path that is not the app's own sandbox. **The second is the
  dangerous one**, because it is a plausible path that silently is not where the
  platform expects a save. Nothing enforces the routing today; a lint or a
  `#[cfg]` refusal on the platform is worth considering when rung 6 lands.
- **The pre-rotation cost is unmeasured.** The compositor rotate on a landscape
  phone is real and is being accepted, not measured; rung 9 exists to remove it
  and nothing has priced what it currently costs.
