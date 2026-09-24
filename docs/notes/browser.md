# Browser — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

## What the atmosphere shipped without (2026-09-05)

Decision record; the decision is in `docs/backlog.md`.

- **Neither mirror scene is on the browser gate's excuse list, and that is a
  measurement.** `./web/run-render-harness-e2e.sh --expect-fail ssr,ui` was run
  locally on 2026-09-06 — headless Chromium on SwiftShader, which is CI's linux
  leg — and reported `atmosphere_mirror  pass` and `gradient_mirror  pass`, both
  at a max channel delta of 1, in a run where 18 of 20 scenes matched and only
  `ssr` and `ui` were excused. They behave unlike `ssr`, which is excused,
  because every ray in these fixtures _misses_: there is no crossing for two
  rasterisers to land on different taps at, only a smooth environment term. If
  either ever starts failing there, the excuse belongs in
  `.github/workflows/pages.yml`'s `render-harness` matrix beside `ssr` and `ui`,
  not in a wider tolerance.

## sundial's page knobs: what the browser gate presses (2026-09-04)

`/demos/sundial/` is the second page on the site whose controls are HTML rather
than keys — `apps/sundial/src/web.rs` exports one call per knob and
`web/demos/sundial/main.js` binds them. `web/tools/browser-e2e.mjs`'s `sundial`
row presses **every one of them** and reads the effect off the demo's own
heartbeat: the seam button, the seam slider, the filter button, the sun's
stop/start button, the tick slider, the atlas-viewer button and `reset`. It also
holds the tick slider's `max` against the sweep the heartbeat names —
`sun::Sky::row` prints `tick N of SWEEP_TICKS`, so the arc is the engine's own
answer rather than a copy of the constant kept here. Each was watched to fail
with the export behind it made a no-op, and the `reset` sabotage additionally
reddened group D's changed-frame check — which is the evidence that this row is
right to carry no `still`. That is where it differs from alcove's row below,
where four controls are driven by nothing but a person.

## sundial's page: a filter cycle button rather than a select (2026-09-04)

Considered and declined. A `<select>` would need the page to enumerate the set
`crcbl::render::shadow` declares, which means a name-at-index export pair on top
of `__crcbl_sundial_filter` / `_filter_ptr`. The cycle button is alcove's shape
and carries alcove's argument: the set is the engine's, and a page spelling its
members is a copy that goes stale the day a fourth rung lands. Worth revisiting
when the set grows past three, where cycling stops being a reasonable way to
reach a member.

## sundial's sun is adopted a fixed step late (2026-09-04)

Not a bug, and worth knowing before it is reported as one. The filter and the
seam are console cells and move the frame at once, paused or not. The sun's tick
and its run flag live on `crate::app::Sundial`, so `crate::sun`'s `ask_tick`,
`ask_running` and `ask_reset` leave a request that `Clock::advance` adopts on
the next fixed step — and a page whose canvas lost focus is a paused loop that
runs no fixed step. So on a paused page the sun's controls show the request and
the picture catches up when it ticks again; `web/pages/sundial.html` says so.

The channel exists because there is no other route: `crcbl::web::App` keeps the
running `Loop` in a private `Stage` and exposes no accessor for it, so an export
cannot reach the hosted game. The alternative was an engine change
(`App::with_game`, or similar), which was out of this slice's scope. If one
lands for another reason, `crate::sun`'s channel should be deleted in favour of
it.

One consequence nothing clears: a request still outstanding when a run stops is
adopted by the **next** run's first fixed step, because `PAGE` outlives the
loop. For `ask_reset` that is a no-op; for a placed tick it would open the next
run on the sun the page last placed. Neither `crcbl::web_exports!`'s `shutdown`
nor `crate::app` offers a hook to empty it from inside `apps/sundial`.

## Reaching alcove's page knobs costs the pointer (2026-09-04)

**Reaching a control on `/demos/alcove/` at all costs the pointer, and that
surprised us.** The fixture asks for Pointer Lock while it is running,
`web/engine/shell.js` takes it on the first mouse press inside the canvas, and
under a lock every mouse event goes to the canvas — so a click on a page control
never arrives. `Esc` is the way out and the page says so; letting the pointer go
also pauses the fixture, and focus coming back does not resume it. None of that
is a bug — each half is behaviour a check in this tree asserts on purpose — but
together they make a desktop visitor press `Esc` before the knobs answer, where
a finger never does. If the seam ever wants to be reachable while the court is
being flown, the decision to revisit is `Alcove::pointer_mode`, which locks
whenever the run is not paused even though the page opens on the fixed camera.

Nothing else on that page is owed: `web/tools/browser-e2e.mjs`'s `alcove` row
drives every control `web/pages/alcove.html` offers and reads each one off
`Alcove::log_heartbeat`, and `releasePointer` in that file is the release above
made a step of the gate.

### shard runs at 98x in a browser and nothing has profiled it (2026-08-28)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**The harness half of this is shipped and green.** `until()` defaulted to an
unscaled 90-second ceiling while every other budget in
`web/tools/browser-e2e.mjs` was scaled by a measured `slowdown`, and the
measurement itself lived in group E, after the groups that needed it. The
measurement moved to group B, `slowdown`/`budget` moved to module scope, and
`until` now defaults to `pollCeiling()` —
`min(budget(TIMEOUT_MS), POLL_WALL_CAP_MS)`, five minutes. Pages run `4bf0375`
is the confirmation: **all fourteen demo jobs passed and the site deployed**,
where the two runs before it had shard and puppet red on three checks between
them.

The sweep the fix was sized against, off that run's group B lines — the first
time the site's pace has been measured demo by demo:

| slowdown            | what a poll buys | ceiling  |
| ------------------- | ---------------- | -------- |
| 1.0–1.6x (9 demos)  | 90 s             | 90–146 s |
| 9.0x                | 33.4 s           | capped   |
| 11.9x, 20.6x, 26.6x | 25.3–11.3 s      | capped   |
| 28.4x (puppet)      | 10.6 s           | capped   |
| 37.3x               | 8.1 s            | capped   |
| 98.4x (shard)       | 3.1 s            | capped   |

shard passed on 3.05 simulated seconds, so the cap has margin at today's pace
and no check needs re-denominating in beats yet. A demo slower again would hit
the cap first, and the group B line is what says so.

### The browser gate drives the touch keyboard by arithmetic (2026-08-31)

`CONSOLE_BUTTON_CENTRE`, `KEYBOARD_LETTER_ROWS`, `KEYBOARD_HEIGHT_FRACTION`,
`SPACE_BAR_CENTRE` and `RETURN_KEY_CENTRE` in `web/tools/browser-e2e.mjs` are
copies of constants in `crates/crcbl-ui/src/console/keyboard.rs` and
`crates/crcbl/src/engine/console_button.rs`. The same trade `PAUSE_INSET`
already makes, and it fails loudly rather than quietly when either side moves —
the taps land between keys and the echo never appears — but it is a duplication
and worth knowing about before the layout is changed.

The touch console block also runs on `breakout` only. Every demo's console is
the same engine code, so a second copy would only cost the gate taps; if
breakout's group F block is ever removed, the guard in `web/run-browser-e2e.sh`
moves with it.

### Losing the pointer lock is what pauses a demo in a browser (2026-09-06)

`crcbl::engine::PAUSE_KEY` is `KeyCode::Escape` for every sample, and **a
browser reserves Escape while a page holds Pointer Lock**: it spends the key on
releasing the lock and delivers no `keydown` anywhere. Measured on Chromium 151
with a standalone probe — with the lock held, a CDP-dispatched Escape produced
`keys=[]` on a `document` keydown listener and `pointerlockchange` fired
`unlocked`; the same dispatch unlocked produced `keys=["Escape"]`. So a visitor
who clicked to aim and then pressed Escape to pause got the pointer released and
no pause, while the page's own hint said Escape pauses.

**What shipped: the release _is_ the pause.** `__crcbl_web_pointer_lock` in
`crates/crcbl-shell/src/web/mod.rs` queues a `ShellEvent::Focus` with `focused`
clear when the lock goes away while the shell still wants it — an exit the
engine did not ask for — and `crcbl::engine::lose_focus` does the rest, which is
the same rule a blur already takes. Nothing in the engine or in any sample
learned a second rule, and native shells are untouched: this is the browser's
key, not the shell's.

The consequences worth knowing:

- **More demos hold the pointer than the three this was first written about**:
  `apps/alcove`, `apps/breach`, `apps/lantern`, `apps/quarry` and `apps/sundial`
  all answer `PointerMode::Locked` while they run. `locks` in
  `web/tools/browser-e2e.mjs`'s `EXPECTATIONS` names them, and group E holds
  that list against the engine's own `__crcbl_web_pointer_lock_wanted` on every
  demo — so a sixth cannot join by skipping the check.
- **The two exits that queue nothing** are a release the engine asked for
  (`set_pointer_mode` clears the request before the shim's poll calls
  `exitPointerLock`) and a `pointerlockerror` on a lock that was never granted.
  Both are in the unit test, because either one would pause a run nobody stepped
  out of.
- **The canvas keeps `document.activeElement` across such an exit**, so
  `WindowState::focused` reads false on the web backend while the DOM still has
  the focus, until the page's next real `focus` or `blur`. Nothing in the engine
  reads that field; a consumer that tracks focus from the events sees exactly
  what `window_state` reports.
- **Anything that drops the lock by hand now pauses the demo**, which is what
  `releasePointer` in the knobs block and the mouse-look control in group C of
  `web/tools/browser-e2e.mjs` had to be taught: both put the run back in play
  before measuring a heartbeat.

The two options declined, for the reason each was declined:

- **Bind a second pause key in the browser** and say so in the hint. Cheapest,
  but it makes the demo's controls differ per target, which is the divergence
  "the same build runs in both" exists to avoid.
- **Leave it**, and change each demo's hint to say Escape twice. Honest, and
  worse for a visitor.

### A browser that declines `unadjustedMovement` gives adjusted deltas (2026-08-26)

`crcbl-shell`'s web backend sets `ShellCaps::RAW_POINTER_MOTION`, and the thing
behind it is `requestPointerLock({ unadjustedMovement: true })` in `takeLock` in
`web/engine/shell.js`. That option is the OS acceleration bypass the capability
names, and where it is unavailable the shim retries the plain
`requestPointerLock()` and the `movementX`/`movementY` the engine reads are the
**OS-adjusted** ones — the same acceleration curve as the desktop cursor, so aim
speed changes with how fast the hand moves.

**Who is affected is decided by the OS, not by the browser**, which is the half
this entry originally got wrong. Chromium rejects the option with
`NotSupportedError` on **Linux and Android whatever its version**, and grants it
on Windows and macOS — the platform, not the release, is the gate. Measured here
on Chromium 151.0.7922.173 against four configurations, all rejecting: a `data:`
page and an `http://localhost` one (`isSecureContext` true), headless and
headed, with and without `--enable-blink-features=PointerLockOptions`. The
request is made from inside a real `pointerdown`, so transient activation is
satisfied. Safari on iOS and Firefox for Android also lack it.

The consequence worth naming: **every Linux desktop visitor, and CI's own Linux
job, take the fallback**, so `ShellCaps::RAW_POINTER_MOTION`'s "unaccelerated"
half is not honoured on the platform this project is developed on. It is
honoured natively on the same machine — X11 reads XI2's `axisvalues_raw` and
Wayland reads `relative-pointer`'s `dx_unaccel`, both deliberately.

Which path a run took is no longer merely asserted: group `AM` in
`web/tools/probe-e2e.mjs` asserts it per platform — Windows and macOS must be
granted the option, Linux must be refused it and reach the lock through the
fallback. The deltas themselves are still not measured; only the path is.

What it costs: a competitive shooter cannot trust aim on those browsers, which
is one of the reasons `docs/plan/sample/11-breach.md` gives for breach being
native-first. What would close it is nothing on our side — it is a browser
feature — so the honest options are to leave the caveat stated, where it is now
(`ShellCaps::RAW_POINTER_MOTION`'s docs and the `web` backend's module docs), or
to add a _third_ capability bit separating "relative motion" from "unaccelerated
relative motion". The bit was not added: nothing in the engine would branch on
it today, and `ShellCaps::has_mouselook` would still be the check a camera runs.

### `apps/breach`'s practice map is gated through a page navigation, not a second run

`web/tools/browser-e2e.mjs` reaches the practice map by navigating the _same_
browser to `?map=practice` in the middle of group C and then navigating back to
the range, so the groups after it judge the demo they always have. Two full page
boots is about seven seconds of the breach gate's runtime.

The alternative considered and declined: a second gate target, so
`CRCBL_WEB_E2E_DEMO=breach-practice` would be its own CI step. It would need a
second page, a second `web/build.sh` DEMOS row and two more `pages.yml` steps
(`tools/check-browser-gate-demos.sh` enforces both), all to run a second copy of
groups A, B, D, E, F, H and I against the same wasm — which is far more CI
minutes than the two navigations cost. Worth revisiting only if breach grows a
third map.

### `apps/breach`'s browser gate leans on one moving plate for liveness

An indoor range with a ceiling has no sun, no sky and nothing else that moves,
so the only thing that changes on a breach canvas with nobody touching it is the
far lane's travelling plate — `map::MOVER_LANE`, driven by `map::plate_x` off
the simulated clock. Two of the browser gate's generic claims rest on it: the
`moving` probe in group C reads the `mover:` field off the `[HUD]` line, and
group D's "the canvas changes between frames" needs the plate to be **in shot**.

Two consequences worth knowing before touching either:

- Freeze or remove the travelling plate and two checks go red, one of them in a
  group that has nothing to do with breach. Verified by sabotage: making
  `plate_x` ignore its `seconds` argument fails
  `the travelling target keeps crossing its lane under its own steam` _and_
  `the canvas changes between frames while the simulation runs`.
- The `range` block in `web/tools/browser-e2e.mjs` therefore puts the view back
  down the range after its own checks, measuring the turn rate off the look
  check rather than carrying a copy of it. Before that existed the block left
  the camera pitched at the ceiling and group D failed on a demo that was
  running perfectly well.

A second moving fixture — a swinging lamp, a fan — would take the weight off one
plate. Not built: it is scenery for a map that has none yet.

The **practice map does not have this problem**: three bots walk their patrols
whatever the player does, and the gate's block for that map reads a bot's own
feet rather than a plate. But the range is what group D judges, because that is
the map the page opens on and the map the block navigates back to.

### The sRGB gate was reading whichever menu the run ended on

**A red Pages run on a docs-only commit, 2026-08-20**, and the interesting part
is how nearly it was misread. Group G reported
`expected rgb(107,173,229) … the dominant colour is rgb(63,105,141) at 32.5%`
under the name "the clear reaches the canvas sRGB-encoded" — the message for the
bug that shipped to users once already.

**The numbers said it was not that.** The observed colour is a **uniform 0.61
multiply** of the expected one (ratios 0.589, 0.607, 0.616); a transfer-function
error is a power curve, and `rgb(107,173,229)` sRGB-decoded is
`rgb(37,107,200)`, nothing like what arrived. A uniform multiply is an overlay.
Confirmed by printing the demo's own HUD line beside each sample: six
consecutive samples read `[HUD] Dead score: 0` — flappy's death screen dims the
whole sky, and by group G the bird had died.

**Why it passed here every time and failed there.** Nothing about the runner:
the state at group G depends on how group F's taps went, so it is a race, and
this desktop happened to land on the winning side five runs out of five. That is
what a race looks like from the machine that never loses it.

**Fixed by establishing the state, not by moving or widening anything.** Group G
now presses the demo's own start key until its own `started` line appears — the
same state group C establishes — and checks that it got there, so the sample is
never taken in an unknown state. It cannot hide a broken encode: a live frame
with no encode shows the linear colour, which is what the row's `unencoded` is
compared against. Red-checked by making flappy's `started` predicate match
nothing: the new check fails with "the demo never reported its started state in
3 presses", and the sRGB check fails _beside_ it, so the misdiagnosis cannot
repeat.

**Two things tried first and rejected, recorded so they are not retried.**
Rebooting the page before sampling is worse — `crcbl.status()` reaches RUNNING
before the first frame is presented, so flappy sampled an all-black canvas, and
breakout's clear is only uncovered once its start menu has been dismissed.
Moving the group before E and F is not enough either: the bird is already dead
by then, which is how the `Dead` line was found.

**One thing worth keeping from the detour:** the eight backdrop samples were
taken back to back, spanning a few milliseconds, so they were eight looks at one
frame rather than eight frames. They are spaced now, and the spacing is scaled
by the measured slowdown like every other budget.

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

### What the three browser gates still keep to themselves

The launch-and-poll loop, the CDP client, `openPage`, `evaluate`, `until`, the
browser registry and the exit hooks are one copy each in
`web/tools/browser-launch.mjs`. What is left is deliberate and worth not
"fixing":

- **`fail` stays per gate.** The prefix differs and so does the meaning of the
  exit code — `render-harness-e2e.mjs` documents 0/1/2 as a contract in its
  usage header. A `makeFail(prefix)` factory would be a helper whose whole body
  is its parameter. The part that _was_ shared knowledge, "kill the browsers
  before you go", is now the exit hook rather than something each copy must
  remember.
- **`render-harness-e2e.mjs`'s own poll stays.** It must hard-fail on its
  deadline rather than answer `null`, must let a throwing `evaluate` through as
  its exit 2, and polls at 250 ms against `until`'s 16. Merging it needs a flag
  per difference, which is the shape that argues against merging.
- **`check` and `group` are still two copies**, shared between `browser-e2e.mjs`
  and `probe-e2e.mjs`. `check` is byte-identical; `group` differs only in the
  gate name it prints. Not moved because they are the checks-and-verdict layer
  rather than the browser layer, and `render-harness-e2e.mjs` has no counterpart
  — so a third caller does not exist and may never.

**One thing found by doing it, and it was a real defect rather than a tidiness
question:** `render-harness-e2e.mjs` leaked its whole browser process tree on
every error it diagnosed. It stopped the browser from a `finally` in `main`, but
`fail` calls `process.exit`, which does not unwind one, and it had no signal
handlers where the other two did. Interrupted mid-run it left **12 chromium
processes and a profile directory**; it leaves none now. Worth keeping because
it is the argument for the sharing: the registry and the hooks live with the
`launch` that registers every browser, so a fourth gate cannot be written
without them.

### DECIDED — the browser has no WebGL2 fallback, and the deletion closed the alternative

Un-linking `crcbl-wgpu` from the wasm dropped a capability: **a browser without
WebGPU has no fallback at all.** The WebGL2 path came from `wgpu`, which stopped
being linkable there and was deleted outright on 2026-08-21, so on such a
browser the engine has no backend to open rather than a slower one.

**Detect-and-message shipped**, which was the recommendation. `demo.js`'s `main`
answers a missing `navigator.gpu` with "This browser has no WebGPU" and the
browsers that have it, and — the case that actually happens — answers a
`requestAdapter()` that resolves to `null` with a sentence aimed at a person,
before the megabytes of wasm load. A blank canvas is now an explanation.

**The fallback itself was the open half, and the deletion settled it by
foreclosure.** Both options are kept so neither is re-argued:

- **Accept it.** WebGPU shipped in Chrome, Edge and Firefox; Safari has it
  from 26. The floor rises on its own, and the engine now refuses gracefully.
  This is what happened.
- **A second artifact.** Build a wgpu/WebGL2 wasm alongside and pick at load
  time — the fallback and the small default both, at the cost of two builds, two
  toolchains (that build needs `wasm-bindgen` back) and a loader that chooses.
  **This one is gone**: with `crcbl-wgpu` deleted there is no WebGL2 path left
  to build, so choosing it now means reviving a deleted backend rather than
  re-enabling a flag.

**Revisit only if someone reports a browser that needs it.** Nothing in the
samples requires WebGL2 and no telemetry says anyone is on such a browser — and
reopening it is now a revival rather than a flag, which is the thing to know
before promising anyone a fallback.

### The debug markers have no browser probe, and probably never can

`begin_debug_label`, `end_debug_label` and `insert_debug_marker` are wired and
covered by the node replayer's stubs, but **nothing drives them against a real
browser**, and this is not a gap waiting on effort. WebGPU gives a page no way
to observe them: the three calls return nothing, change no resource, and are
readable only inside a native capture tool attached to the browser process. A
probe group would therefore encode them and then assert the frame still
submitted — a check that passes identically whether they were replayed or
dropped.

**The reasoning now lives in the code**, in a block comment above
`Replayer#debugScope` in `web/engine/gpu-replay.js`, which is where a reader
asking "why is there no group for these?" arrives. It also names what _is_
checked without a browser: `web/tools/gpu-replay.mjs` replays all three against
a stub device whose encoder, render pass and compute pass objects each record
their own debug calls, so a push landing on the wrong object — the unbalanced
group that costs a real `finish()` — fails there by name. Delete that comment,
and this entry, if a WebGPU extension ever reports recorded markers back.

The `dispatch_indirect` half of this entry **shipped** as probe group AG, which
reads the three workgroup counts back per axis rather than asserting a dispatch
happened.

### The Windows probe gate has no adapter; macOS is proven

Record; the two things still open are in docs/backlog.md under the same heading.
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

**~~Coverage gap in what landed~~ — both halves are spent, 2026-08-22.** This
said `render-harness-e2e.mjs`'s _launch_ path had never been executed and that
it was "wired into no workflow at all". Neither holds: `pages.yml` runs
`./web/run-render-harness-e2e.sh` and two later steps consume its readbacks, and
the launch path has since been run here repeatedly — which is how the browser
leak it was hiding got found.

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

**DECIDED 2026-09-06 —** confirmed as it stands: the limit is reported as 1 and
an ask above it is passed through to `createSampler` for the device to clamp.
Precedent: wgpu's WebGPU backend does the same, because WebGPU exposes no query
for the maximum a device supports, so a reported 1 means "no ceiling this
backend can guarantee" rather than "more than one is refused". Nothing owed.
`halLimitsFor` reports `max_sampler_anisotropy: 1` and withholds
`Features::SAMPLER_ANISOTROPY`, while `webgpuMaxAnisotropyFor` passes an ask
above 1 straight to `createSampler` and lets the device clamp. Both halves are
argued where they are written and neither is a bug, but together they mean a
caller who respects the reported limit never exercises the pass-through, and one
who ignores it gets whatever the device does.

The alternative is refusing everything above 1, which would make the seam's
anisotropic filtering permanently unreachable on WebGPU. That is why it was not
done, and it remains **a decision worth confirming** rather than one that sits
implicit. WebGPU has no query for the maximum a device supports, which is why
the reported limit is 1: it is "no ceiling this backend can guarantee", not
"more than one is refused".

**Corrected 2026-08-24:** this entry said the two halves live "in two files that
do not reference each other". They are in one file, `web/engine/gpu-replay.js`,
and `webgpuMaxAnisotropyFor` already linked to `halLimitsFor`; the reference was
one-way, and `halLimitsFor` now links back. So what is left here is only the
decision, not the implicitness.

## The browser leak check watches occupancy, not just totals (2026-08-24)

Record; the decision it still needs is in docs/backlog.md under the same
heading. Check I in `web/tools/browser-e2e.mjs` ("a steady-state frame gives
back everything it takes") took one `liveObjects()` sample, waited 60 frames,
took another, and failed on any kind that had risen. It failed twice on CI and
neither was a leak: lantern `readbacks 2 -> 5` (commit `fa7fc11`) and quarry
`readbacks 1 -> 2` (commit `68e278c`), neither reproducible on this machine.

**`readbacks` is not a monotone tally — it is the occupancy of a fixed-size
ring.** `CullStatsRing` (`crates/crcbl-render/src/cull_stats.rs`) holds
`FRAMES_IN_FLIGHT + 1` = 3 slots per `ForwardRenderer`, and `take_slot` matches
only `Recording` or `Idle`, never `Polling` — so a slot with a live readback is
never reused and the count is hard-bounded at three per renderer. quarry has one
renderer (ceiling 3, observed 2); lantern has two, `renderer` and `monitor` in
`apps/lantern/src/gpu.rs` (ceiling 6, observed 5). Both failures were strictly
inside the bound. Occupancy rises with the readback round trip measured _in
frames_, which is exactly what a loaded CI runner stretches.

The check now asks whether a kind climbs across **every** one of four windows
rather than one: a ring saturates and stops, a leak does not. Verified both ways
— the acquire-path leak it was written for (a fresh image and view per frame,
never retired) still fails it, at `imageViews 520 -> 580 -> 641 -> 701`.

**Per-kind ceilings were the alternative and were declined**: they would put the
engine's ring depths in a JS test file, to be silently wrong the day one
changed. If a future leak turns out to be slow enough to saturate within three
windows, the answer is more windows, not a table of engine constants.

**It happened a third time on 2026-09-02, and this one survived the three-window
hardening.** `render lantern in a real browser` failed the Pages run for
`d0bc715` with `readbacks 2 -> 4 -> 5 -> 6`. That is the same false positive
again, and the arithmetic says so rather than the resemblance:
`FRAMES_IN_FLIGHT` is 2 and `CullStatsRing::new` takes `frames_in_flight + 1`
slots, so three per renderer; lantern's `gpu.rs` holds two, `renderer` and
`monitor`; the ceiling is six, and the run stopped **at exactly six**. The
increments decelerate — `+2`, `+1`, `+1` — which is a ring filling, and an
unbounded leak does not stop at the bound.

So a loaded runner can now stretch the round trip far enough that occupancy
takes three whole windows to saturate, and three windows was no longer enough to
see it stop. The cheap remedy this entry already named was taken — a fourth
window, so the flat step after saturation is inside the sample — and the comment
above the constants labels it a stopgap.

**Not caused by the autoexec that landed in the same commit**, though the timing
invites the reading: an autoexec run on a page with no `autoexec.cfg` reads the
resident map once and allocates nothing, and `readbacks` counts `CullStatsRing`
slots, which nothing on that path touches. The evidence is weaker than it looks
in one respect worth stating — the three Pages runs between this commit and the
last green lantern were **cancelled by my own pushes**, so there are no
observations in between and "it first failed here" is not evidence it started
here.

## What the deleted 10-wasm-webgpu plan left behind (2026-09-24)

Record; the built part of the plan is `crcbl-webgpu`, the `wasm32` build with no
`wasm-bindgen`, the WGSL artifacts, `crcbl::web`, the audio worklet feed, OPFS
storage, `web/run-browser-e2e.sh`, `web/run-cross-backend-e2e.sh` and the demos
under `web/demos`. What it left open is in `docs/backlog.md`: the browser budget
under _Horde has no browser budget of its own_ and _Quarry's two human
judgements and its browser budget are untaken_, Firefox and WebKit in the P5B
threads bullet under _P5B — the job system, and the two decisions in front of
it_, the fetch wrapper under _`crcbl-assets` after stage 6 task 2_, and the
editor under _Task 6 of stage 10 — editor-in-browser — was never examined_. It
specified stage 10: games in the browser on WebGPU and `wasm32`, a track of its
own that taxes every earlier stage with design constraints.

Code cites the plan as "stage 10", "topic 10's risk list" or "the 2026-07-27
correction". Its tasks were numbered 4 to 6, and those numbers are what the
backlog still uses:

| Citation                  | What it specified                                                                                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The constraint table      | What wasm imposes on earlier stages — the table below                                                                                                  |
| Task 4                    | WebTransport / WebSocket transport and a server listener, a browser client joining a native server. **Dropped** (see the networking rule)              |
| Task 5                    | The perf pass: a browser scene budget, defined explicitly and smaller than native, with the gap documented honestly. Owed per sample in the backlog    |
| Task 6                    | An editor-in-browser smoke test, a stretch. Never examined; the editor is treated as native                                                            |
| Exit criteria             | A sample scene at target frame rate in Chrome and Firefox with WebGPU, the same debug overlay in the browser, and CI running a wasm build in a browser |
| Risk list                 | WebGPU timestamp and feature availability varies by browser, so debug tooling **degrades feature by feature and never breaks the build**               |
| The 2026-07-27 correction | GitHub Pages cannot set COOP/COEP — the rule below                                                                                                     |
| The 2026-08-09 correction | Networking removed, threading superseded by P5B, the browser boundary made canonical, the editor native                                                |

- **The constraints wasm imposes are the earlier stages' bugs, not wasm special
  cases.** If one is violated, the fix belongs in the stage that violated it:

  | Constraint                        | Where it is handled                                                                 |
  | --------------------------------- | ----------------------------------------------------------------------------------- |
  | No bindless, MDI or BDA in WebGPU | Stage 3's data-layout rule: the lesser path is a constraint on layout               |
  | No blocking file IO               | `AssetSource` is async from day one; a fetch-backed source in the browser           |
  | No UDP or QUIC sockets            | The transport trait is message-oriented, not socket-shaped                          |
  | No blocking threads by default    | The core loop is single-thread-capable; the job system has a single-thread fallback |
  | The browser owns the main loop    | The frame loop is a `fn tick(dt)` driven by an outer loop, not a `loop {}`          |
  | Swapchain acquire is implicit     | The HAL surface API lets acquire be trivial (WebGPU's `getCurrentTexture`)          |

- **The browser boundary, canonically.** This is the one list of what a browser
  cannot do. Anything relying on a row needs a stated fallback or an honest
  absence:

  | Gap                                                 | Consequence                                                                  |
  | --------------------------------------------------- | ---------------------------------------------------------------------------- |
  | No bindless / binding arrays                        | `BindingModel::ArrayPages` — texture array pages + batching                  |
  | No multi-draw-indirect or count                     | `GeometryPath::IndirectPerBatch` — compacted list, per-bucket draws          |
  | No mesh shaders                                     | same; per-instance LOD instead of per-cluster                                |
  | No ray tracing                                      | `LightingPath::Rasterised` — the raster twin is MVP for this reason          |
  | No buffer device address                            | indexed SSBO lookups                                                         |
  | No persistent mapped buffers                        | staging copies on every upload                                               |
  | No pipeline cache                                   | every page load recompiles every shader — keep permutations low              |
  | No threads without COOP/COEP                        | `Inline` spawner; sim on the main thread                                     |
  | No listening socket, no LAN discovery, no HTTPS→LAN | no networking at all; web builds are single player                           |
  | No NaN canonicalization, no fuel                    | module determinism unguarded; no hostile-module containment (topic 16)       |
  | WebCodecs audio encode uneven                       | libopus compiled to wasm if VOIP ever ships to a browser (topic 32)          |
  | `wasm32` address space                              | 4 GB architectural ceiling, browsers often lower — a wall, not a degradation |

  Timestamp queries, compute, indirect draw, `INDIRECT_FIRST_INSTANCE`, f16,
  dual-source blending and the BC/ETC2/ASTC families **are** available, so the
  profiler, GPU culling and the post stack all work; the gap is narrower than
  "Tier B" implied (`docs/plan/39-capabilities.md`).

- **The backend is `crcbl-webgpu`; do not rebuild on `wgpu`.** The plan first
  chose the `wgpu` crate for a native portability fallback and a "does it repro
  on the other backend?" triage tool. That was overturned: `crcbl-wgpu`, the
  whole `wgpu` dependency family, its CI jobs and `CRCBL_GPU=wgpu` were deleted
  on 2026-08-21 in `6b5e17a`. Wasm serialises HAL calls into a buffer it owns
  and JS replays them against WebGPU (the next section). There is no triage
  backend; `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` are the native set.
- **No `wasm-bindgen`, not even as a build tool.** No crate depends on it,
  `#[wasm_bindgen]` appears nowhere, and every `__crcbl_*` symbol is
  hand-written `extern "C"`. `web/build.sh` runs no `wasm-bindgen`, and
  `web/tools/check-exports.mjs` asserts per demo that the single-threaded
  artifact imports **nothing** — the threaded one only a shared `env.memory`,
  which a module cannot own and be attached to from a worker.
- **No `crcbl-web` crate: `crcbl::web` owns the protocol** — the status codes
  the page polls, the log queue it drains, the asset base, and `web_exports!`,
  which writes a sample's exports. A sample keeps its `WebPending` impl, because
  its boot options and its failure are its own. The symbols stay per demo, since
  two demos can be open in one browser and the shim finds each by name.
- **The audio feed is shape B**: rendered on the main thread and sent to the
  worklet as `postMessage`-transferred blocks. A second wasm instance in the
  worklet would have its own linear memory and none of the voices the game
  queued, and the audio ABI has no `play(id)` to tell it. The cost is the
  buffered lead stated in `web/engine/audio-worklet.js`.
- **GitHub Pages cannot set COOP/COEP** (the 2026-07-27 correction), so
  `SharedArrayBuffer` is unavailable on the flagship deploy target. The Pages
  demos stay single-threaded through the `Inline` spawner, the worklet feed must
  not depend on an SAB ring, and `coi-serviceworker` is the workaround only if
  adopted deliberately. Module memory is unaffected: an imported
  `WebAssembly.Memory` needs no SAB unless it is shared across threads.
  Threading itself is P5B's, which set wasm thread-topology parity as the
  target; its canonical record is `docs/plan/21-jobs.md`'s wasm threading rules.
- **The networking half is removed.** Native multiplayer is LAN and web builds
  are single player, so no browser client has a server to reach;
  `docs/plan/23-netcode.md`'s LAN correction has the reasoning and the WebRTC
  route deferred rather than refused.
- **The editor is a native target.** Its asset browser, OS drag-drop and
  notify-based file watcher are native-shaped; see `docs/plan/08-editor.md`.
- **The sequencing lesson.** The browser work finished the platform half (page,
  shim, deploy, export checks) while the graphics half still had no shader a
  browser would accept. A platform track can be finished and demonstrate
  nothing.
- **What the browser gate found that nothing else could.** Dawn enforces WGSL's
  uniformity rule where naga does not, and rejected the UI shader for sampling
  the glyph atlas under a branch on a varying — invalidating the frame's whole
  command buffer and leaving the canvas black while the simulation ran on. And a
  WebGPU backend cannot see a pipeline it failed to create, because creation
  failures go to the device error callback; `Device::take_error`, which
  `Gpu::acquire` drains before recording, is the fix.
- **The readback trap.** Three of the four obvious ways to read a WebGPU canvas
  back return transparent black whatever was drawn, varying by display and
  adapter. The gate therefore runs a known-colour clear as a control in the same
  browser with the same flags, and refuses to interpret the render checks unless
  the control reads back.

## What the deleted 41-webgpu-stream plan left behind (2026-09-24)

Record; the plan was fully built, and the coverage it left is in
`docs/backlog.md` under _What the reply channel still owes_, _Error-scope
granularity was measured and adopted_ and _Smaller things the WebGPU work
surfaced and did not fix_. It specified the encoding `crcbl-webgpu` speaks: wasm
serialises HAL calls into a buffer it owns, `web/engine/gpu-replay.js` replays
them against WebGPU, and answers come back through a second buffer wasm also
owns. It mattered because this encoding is the one part of the WebGPU track with
no external specification — every bug in it is ours alone. The canvas-sizing
ordering is recorded separately in the next section, and the two contract
findings declined after the second decoder was written are in
`docs/notes/backends.md` under _The command stream's contract, read from the
other side_.

- **Nothing but integers crosses, and wasm owns every buffer.** Every export is
  `(i32, …) -> i32`, JS reads and writes wasm memory in place and never passes a
  pointer in — the convention `crcbl-store`'s fetch ABI, the OPFS entry points
  and `crcbl-shell`'s key scratch already used, so there is one convention, not
  two. That is what let `check-exports.mjs`'s allowed-import set go empty. The
  HAL's trait objects are ids and nothing more (the `GPUDevice` lives in JS for
  its whole life), and no seam method takes a callback.
- **A pointer never goes on the wire; the `Instance` impl refuses it.**
  `StreamWriter::create_surface` takes the `u32` canvas key and
  `create_offscreen_surface` takes nothing, so a pointer-carrying
  `SurfaceTarget` has nothing to be encoded into and the refusal lives in
  `crcbl_webgpu::hal::WebGpuInstance::create_surface`, where the target is still
  whole. `Offscreen` has its own command rather than a reserved canvas id both
  decoders would have to agree on. Neither configures anything: configure takes
  a `GPUDevice`, so it belongs to swapchain creation.
- **Two channels, one byte format.** The command stream (wasm → JS, `writer` /
  `reader`, `gpu-stream.js`) and the reply stream (JS → wasm, `reply`,
  `gpu-reply.js`) share one bounds-checked reader, one writer and one error type
  in `crcbl-webgpu`'s `bytes` module, because two near-identical readers are two
  places for a bound to be wrong. Their magics differ (`CRCBLGPU` against
  `CRCBLRPL`) so a channel wired backwards fails on the first eight bytes rather
  than on whichever reused tag number is unclaimed. **The transport is what
  defers an answer**: `surface_caps` is answered inside the replayed call and is
  still a reply, because a frame boundary sits between the two halves of every
  call on this seam.
- **A reply for a sequence nothing awaits refuses the whole buffer.**
  `expect_reply` keeps a bounded set and `drain_replies` answers
  `DecodeError::UnexpectedSequence` for an unknown or already-answered number: a
  replayer answering the wrong command looks exactly like an answer otherwise.
- **The reply set is one reply per encoding shape, not per HAL method.** A new
  shape is what deserves review, because it is where a decoder gains a way to be
  wrong. **An optional fixed-width field takes a presence byte, never a
  sentinel** — `SurfaceCaps::current_extent`'s `(0, 0)` is a minimised window
  and `0xFFFF_FFFF` is Vulkan's "no opinion", so there is no value to spare.
- **Only the reply-buffer export can grow wasm memory.**
  `__crcbl_web_gpu_reply_buffer` allocates, so the `Uint8Array` is built after
  it from the pointer it returned and never stored. A committed buffer the
  engine has not drained is not overwritten: `reply_buffer` answers `0` and the
  shim keeps its replies, because a dropped reply is a command that waits for
  ever.
- **Replay happens once per frame, at the `requestAnimationFrame` boundary**;
  anything answered is read the next frame. The HAL's polled shapes
  (`PendingDevice::poll`, `request_readback` + `poll_readback`) are what survive
  that.
- **The end of the stream is its own export, and the leak line's words are
  load-bearing.** A zero length cannot mean "ended" because it is also what a
  page sees before boot, so `__crcbl_web_gpu_stream_ended` exists and the shim
  reads it after `__crcbl_web_gpu_stream_release`. `Replayer#replay` then writes
  `N object(s) still alive at device teardown (…)` — after the frame's own
  destroys — in the exact wording `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` use,
  since one grep in every e2e runner covers all four backends and a rewording
  silently stops matching.
- **Wire conventions are `crcbl-net`'s `codec.rs`'s.** Little-endian; a tag byte
  first so a decoder dispatches instead of trial-decoding; tags in contiguous
  ranges per family, sized to what each family must hold (a nibble per family
  never fit) and walked by `tag.rs`'s range tests; a `u32` length prefix before
  every variable-length field with a cap per prefix and no padding; a version
  word in the header, because the Rust and JS halves cache independently. A
  presence byte for every optional field that is not a handle, with any value
  but the two canonical ones refused. Bitflags go over as `bits()` and decode
  through `from_bits`, never `from_bits_truncate`, so an unclaimed bit is an
  error. The writer asserts the caps the reader enforces.
- **Enum tags are ours, never `as u8`.** No HAL enum has explicit discriminants
  and `Format` may gain a variant mid-list, so a cast silently renumbers
  everything after it on the far side of a language boundary. The explicit table
  in `tag.rs` is the defence, and the opcode numbers live there too, so adding a
  command touches one file.
- **The encoding refuses a malformed stream, never an invalid descriptor.** An
  unclaimed code or bit is a decode error; a zero `mip_levels` is a value the
  wire claims, and refusing it would have to happen mid-frame in a call that
  returns `Ok(handle)` before anything replayed. A bad descriptor is a creation
  failure and leaves through `take_error`.
- **`Option<Handle>` is a bare `u64`, and absence is a zero _generation_**, not
  a zero word: `Handle::from_bits` rejects any zero-generation value, so a
  decoder testing the whole word is wrong on a corrupt stream. The packing lives
  in `crcbl-core`'s `handle.rs`; read it there. **The opcode, not the handle,
  says which table an id indexes** — handles carry no kind, so one flat table
  per resource kind is correct and one table keyed on bits is not. **A slot
  remembers its generation** (`HandleTable` in `gpu-replay.js`), or a destroy of
  a reused index releases the current occupant. A second device will need the
  owner side table and `HalError::ForeignObject`, as every backend does.
- **Wasm allocates creation handles itself** and returns `Ok(handle)` at once;
  failure arrives through `Device::take_error`, drained by `crcbl::engine`'s
  `GpuContext::acquire`. `crcbl-render`'s `cached_group` therefore returns
  `Some` on this backend and the error stops the frame instead of skipping a
  pass — louder, and right for a bind group this code built wrongly.
- **A destroy naming an empty slot is a no-op, not corruption.** `crcbl-render`
  destroys pre-allocated siblings before `?` and on `Err`, including for handles
  whose creation will turn out to have failed. `Instance::destroy_surface` obeys
  the same rule.
- **Sequence numbers are positional on the command stream and a field on
  replies.** The header carries the first command's sequence and the nth command
  is `base + n`: a per-command field would cost four bytes a command, be a
  second source of truth, and a `u32` wraps within hours, while an off-wire
  counter can be `u64` and carries across buffer resets. A reply's position
  implies nothing, so it carries the `u64`, relying on the counter being
  monotonic across frames.
- **Browser errors are attributed per flush, by error scopes.** The plan settled
  on one `uncapturederror` listener feeding the device's queue unattributed;
  commit `7a0d6ee` replaced that. `Replayer#replay` wraps every flush that
  carries commands in one `pushErrorScope` per `GPUErrorFilter`
  (`ERROR_SCOPE_FILTERS` in `web/engine/gpu-replay.js` — all three, because a
  scope is exclusive with `uncapturederror` and covering one filter would hand
  the others back unattributed), pops them in a `finally` so a thrown
  `ReplayError` cannot leak the scope stack, and files what they catch into the
  same log as `during commands A–B`. Empty flushes are not scoped. The listener
  stays for errors raised with no flush open (a `mapAsync` continuation, device
  opening) and names no command, since a number there would be a guess.
  Per-command scopes were measured with `web/tools/error-scope-bench.mjs` and
  declined: settle time is superlinear (the figures are in `7a0d6ee`'s message)
  and no granularity makes the answer synchronous anyway. The replayer's own
  refusals are attributed exactly, to the command.
- **Cases that are easy to get wrong:** `ShaderModuleDesc` absence differs per
  field (an empty `spirv` is absent, `Some("")` WGSL is present and empty);
  `dxil` must be skipped correctly, two variable-length leaves under one slice;
  `spirv` is the one deliberate four-byte alignment exception; `BindGroupEntry`
  is a counted list of tagged variable-length entries, because the discriminant
  is the only thing saying which of three tables to resolve against;
  `BindGroupLayoutDesc` entries keep slice order; the depth-stencil chain is the
  deepest descriptor; `poll_readback`'s exact length is the caller's check,
  since nothing in a reply says what the descriptor asked; `bind_group` and
  `push_constants` both take the pipeline layout last.
- **Sentinels pass through for the replayer to resolve.** `WHOLE_BUFFER` and
  `ImageSubresourceRange::ALL` cross verbatim; the encoder never decides.
  Resolving does not always mean omitting the member: `SamplerDesc::lod_max`'s
  `f32::MAX` must become an explicit clamp, because WebGPU's absent
  `lodMaxClamp` means 32 and nothing reports a mip clamp.
- **`crcbl-hal`'s null backend `record::Command` was the precedent**
  (deep-copied capture, flattened pass fields, copy direction in the variant
  name, stable `name()`), except that its `PushConstants` keeps only a length —
  a replayer needs the bytes.

## What the deleted WebGPU plan left behind (2026-08-22)

Record; what the deletion left open is in docs/backlog.md under the same
heading.

- **The ordering that makes `crcbl-webgpu`'s `suboptimal: false` true is guarded
  nowhere.** Settled 2026-08-22: the hard-coded `false` is correct, and correct
  by construction of the platform rather than by anything this engine does.
  WebGPU's _Canvas Context sizing_ algorithm re-derives a canvas context's
  texture descriptor from `canvas.width`/`canvas.height` whenever either is set,
  and `configure()`'s own note says its early validation "remains valid until
  the next `configure()` call, **except** for validation of the `size`, which
  changes when the canvas is resized". A canvas context is not a swapchain that
  can outgrow a size, so the API has no out-of-date signal for a backend to
  translate. `crcbl-webgpu` now says this at the literal, and two tests hold the
  two links it can reach: `the_shim_reports_the_canvas_size_it_just_wrote` pins
  `web/engine/shell.js` to sizing the backing store and then reporting those
  same locals, from exactly one place;
  `the_acquired_extent_is_the_one_last_configured` drives the device and proves
  a reconfigure moves what a later acquire reports.

  **All three links are held now.** The third is `Loop::frame_body`'s
  pump-resize-frame ordering in `crcbl`, guarded by
  `a_resize_reaches_the_gpu_before_the_frame_that_follows_it`, which asserts on
  the extent the GPU **held when `frame` ran** rather than the one it ended on —
  both orderings leave the same extent behind, so an end-state assertion would
  have passed either way. Measured when it was written: moving the resize below
  the frame fails that test and **nothing else in `crcbl`'s suite**, which is
  what it was written for.

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

## Should the click that refocuses a canvas reach the game at all?

**DECIDED 2026-09-06 —** keep delivering the click. Activation blocking is
declined: browsers and every web game deliver the click that focuses the page,
and for a paused game the current behaviour is the friendlier one — the player
clicked on `RESUME` and got a resume. Nothing owed; what holds the line is
described below. **Behaviour that surprised us, deliberately left alone.** A
canvas has no title bar, so `web/engine/shell.js` gives it the keyboard from its
own `pointerdown` handler — which makes the click that "clicks back into the
window" also a press at a real position inside the game. With the pause menu on
screen and `RESUME` under the cursor, clicking back in resumes. That is each
half behaving correctly and the combination being surprising; it is what put the
browser gate at 23/25 for a slice, because section E clicked the canvas's
_centre_ to restore focus and the menu is centred there.

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

## `apps/hud` milestone 1: what was deliberately left out

Record; what the sample still owes is in docs/backlog.md under the same heading.

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

## Mobile input: what it decided, and the bugs it found

Record; what is left is in docs/backlog.md under the same heading.

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

## The browser gate's staleness guard did not cover Rust (fixed 2026-08-20)

Kept because the _shape_ recurs, not because the fix is pending.
`web/run-browser-e2e.sh` warns when `target/site` is older than its sources, and
that warning is the only thing standing between a reused site and a green run
about code that is not under test — the block says so in capitals.

It scanned `web/engine` and `web/tools` for `.js`/`.mjs` and nothing else. Its
comment excused the gap with "the wasm has `build.sh`'s own staleness handling",
which is true of `build.sh` and irrelevant in the branch where the warning
lives: that branch is precisely the one `build.sh` does not run in, so no
`cargo` invocation ever compares a `.rs` against the artifact.

**It cost a false red-check the day it was found.** A deliberately frozen camera
was re-run through the gate without `--build`; the gate reported
`the dolly keeps running down the face under its own steam — it took 2 values`
and passed 34/34, against a wasm built before the sabotage. With `--build` the
same tree failed 32/34. The guard printed nothing either way.

The find now also walks `apps` and `crates` for `.rs`, `.slang` and
`Cargo.toml`. **The general lesson:** a staleness guard has to cover every input
to the artifact, and an exemption reasoned from another code path is how one
ends up covering the half nobody edits.

## The browser gate's budgets are measured, not fixed (2026-08-20)

Record; the unscaled `TAP_INTERVAL_MS` and the unverified scaled second wait are
in `docs/backlog.md` under this heading.

**Both 3D demos now render in a real browser on CI**, which closes the decision
this entry used to be. What is kept is how it was closed, because the shape
recurs and the reasoning is not recoverable from the diff.

**The decision was never about the demos.** The old blocker — `crcbl-render`'s
draw-argument pass binding fourteen storage buffers against SwiftShader's
ceiling of ten — went when that pass was packed down to eight. What was left was
"nobody has run either on a GitHub runner", and turning the step on twice bought
two precise defects rather than an opinion:

1. **32 of 34.** Both failures were group E's HUD heartbeat,
   `0 HUD line(s) in 4000 ms`. `TICK_WINDOW_MS` was a constant chosen on this
   desktop; the runner advances quarry's simulated second every 27 seconds.
   Worse, "a paused demo runs no ticks at all" **passed for free** on that run —
   no heartbeat could appear in any state — which is the exact failure mode the
   group's first check exists to catch, and it caught it.
2. **33 of 34.** Group E green after the window was derived from the measured
   beat; `PADDLE_SETTLE_MS`'s flat 1500 ms failed instead, one budget along.

**So the fix was not a bigger number, it was a measured one.** The heartbeat's
nominal value is one simulated second, so `slowdown = max(1, beat / 1000)` is
how far behind real time a machine is running the demo, and every budget in the
harness is scaled by it. Clamped at 1, so nothing is ever shorter than the
constant already gave. The control prints the factor.

**Measured on the runner, 2026-08-20**, which is what the whole exercise was
for:

| demo      | beat     | factor | result | step  |
| --------- | -------- | ------ | ------ | ----- |
| breakout  | 993 ms   | 1.0x   | 47/47  | —     |
| flappy    | 1034 ms  | 1.0x   | 43/43  | —     |
| asteroids | 992 ms   | 1.0x   | 38/38  | —     |
| horde     | 1023 ms  | 1.0x   | 47/47  | —     |
| hud       | 1003 ms  | 1.0x   | 37/37  | —     |
| quarry    | 26243 ms | 26.2x  | 37/37  | 5m54s |
| lantern   | 19339 ms | 19.3x  | 37/37  | 5m23s |

The five 2D demos sit at exactly nominal and are untouched by the scaling, which
is the property that makes it safe; only the two heavy frames are behind, and by
the factor their own heartbeat reports.

**Both stale prose claims are fixed**: `web/pages/index.html`'s excusing clause
and `web/pages/lantern.html`'s "This one wants a real GPU" note, which said the
canvas stays black on a software adapter and had been false since the pass was
packed.

## The browser entry point is shared; what the move left behind (2026-08-15)

Record; `crates/crcbl/src/web.rs`'s size is in `docs/backlog.md` under this
heading.

S1B finding 2 is closed: `crcbl::web_exports!` writes the ten
`#[unsafe(no_mangle)]` symbols and the page state, and every sample's `web.rs`
invokes it. It was landed as a move, and these are the things it deliberately
did not fix.

### `asset_source` has no caller in the four samples that define it

The four samples that define it — asteroids, breakout, flappy, horde — export
`pub fn asset_source() -> Option<Rc<FetchSource>>` and none of the four calls
it. `opfs_store` is genuinely used (`crate::best` in three of them,
`crate::high_score` in breakout).

**It is no longer the speculative half, though: `apps/viewer` has a caller.**
`apps/viewer/src/shelf.rs` resolves every browser shelf key through it, which
makes the viewer the pattern any of the four would copy rather than an argument
for deleting the accessor.

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

### The macro is reachable by two paths

`#[macro_export]` puts it at `crcbl::web_exports!`, and a
`#[doc(inline)] pub use` in the module makes `crcbl::web::web_exports!` work
too. The samples all call it as `crcbl::web_exports!`. Nothing enforces that; a
future sample writing the longer path is not wrong, just inconsistent.

## Shard's Pages leg doubled, and the demo did not (2026-09-07)

`render shard in a real browser` in `pages.yml` took 1711 s on `11d0506`, 3284 s
on `ca27002` — the first run after `apps/shard`'s loot slice — and 4173 s on
`05c57a1`, against a cap of 90 minutes at the time. The checks that grew all
wait a fixed number of frames —
`a steady-state frame gives back everything it takes` went 407 s → 803 s,
`the canvas has a backing store` 262 s → 513 s — so the number is the software
rasteriser's frame time.

**It is not the loot slice, measured three ways on 2026-09-07:** the same
browser gate on this machine's hardware adapter reads 1.3 s for the steady-state
check at `11d0506` and at `89d48d5`; 200 headless frames on lavapipe take 13.2 s
at both, paced by the fixed-step clock; and the engine's own per-pass GPU table
at exit is the same 26 labels at 55.9 ms and 55.8 ms of p50, `forward` 17.6 ms
in both. The runner image (`ubuntu-24.04`) and Chrome (`152.0.7977.64`) are the
same in both CI logs. What differs between the two Pages runs is the other legs,
in both directions: `alcove` 3020 s → 1035 s, `quarry` 2435 s → 886 s, `puppet`
2194 s → 1556 s. Shared runners land on different hardware, and shard is the
heaviest scene, so it is the one that shows it.

**The page logs say which half moved**, by the rule `pages.yml`'s header gives:
`ssao` is resolution-bound, and it read 42.2 ms of p50 on the fast run and 74.0
on the slow one; `forward` read 4251 and 11198; `shadow` 22.9 and 24.0. So the
runner was the slower machine and the workload wandered on top of it, which is
the header's "spread is the workload" finding with a slower host underneath. The
cap went 90 → 150 in the same commit as this note, by that header's rule of the
slowest completion doubled; nothing in `apps/shard` changed, and the lever if it
ever fires again is `FRAMES_WATCHED`/`WINDOWS` in `web/tools/browser-e2e.mjs`'s
steady-state check, the single largest cost.
