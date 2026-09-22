# CI — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

### The browser gate's cap says "out of time", never "too slow" (2026-08-28)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**The sum is fixed; the instrument is not.** The Pages workflow's fourteen Linux
demo gates were steps of the `build the demo site` job, run serially because
they all drive the site it builds, so the job's `timeout-minutes` had to fit
their total. It did not, five times running: 30 minutes on 2026-08-25, then 60,
then 90, then 120, and at `80022df` the job still reached 69 minutes with breach
killed on its own step bound and shard never started. A job cap firing **skips**
the deploy rather than failing it, so every one of those runs looked green with
the site a commit behind — it had not deployed since `00c92e3`.

They are the `demos` matrix job now, one job per demo, each downloading the
`site` artifact that `build` uploads before it drives anything, and `deploy`
waits on both. The wall clock is the slowest single demo rather than the sum,
`fail-fast: false` means one red gate no longer leaves thirteen unrun, and a red
gate is now **loud**: the Pages run itself goes red.

**Precisely, because the distinction is the whole point of this entry** —
observed 2026-09-02 on `d0bc715`, whose lantern gate failed. The
`deploy to GitHub Pages` job still reports **`skipped`**, because
`needs: [build, demos]` skips a job whose dependency failed; that has not
changed and cannot be read as a failure on its own. What changed is that the
_run_ carries the red instead of finishing green, so the commit wears a failing
check rather than a passing one with a stale site behind it. Anyone reading the
deploy job alone will still see "skipped" in both the old world and the new, so
read the run.

**What these gates spend is simulation, not picture.** The runner has no GPU;
its software rasteriser drives the tick at a fraction of real time, and every
check waiting on simulated progress waits in wall-clock multiples of it. The
logs name their own factor: 1.0x for the five 2D demos, 25.0x for puppet, 51.1x
for breach, 87.0x for shard. The shadow filter's early-out took 27% off the
forward pass on two adapters and moved puppet's step by one second (612 s to 613
s) — the same finding from the other side.

**It is slowness, not a hang, and the cap was below the legitimate range —
measured 2026-08-31.** An earlier revision of this entry read the bimodality as
a hang. That was wrong, and the sweep that overturned it is below. Five
consecutive Pages runs, job durations read off `gh run view <id> --json jobs`:

| Commit    | Run         | shard  | Outcome              |
| --------- | ----------- | ------ | -------------------- |
| `79a555a` | 33353650899 | 26 min | success              |
| `9bd267b` | 33360958824 | 45 min | cancelled at the cap |
| `458a5ed` | 33368904505 | 24 min | success              |
| `a1e5168` | 33371443850 | 45 min | cancelled at the cap |
| `031696a` | 33375515324 | 45 min | cancelled at the cap |

**What settles it is the spread between runs of the same code.** Each demo's
`web-e2e-<demo>` artifact holds the page log, and `PassStats` in it reports the
forward pass per frame. Across the four runs that uploaded one, in ms:

| demo    | `79a555a` | `9bd267b` | `458a5ed` | `a1e5168` |
| ------- | --------- | --------- | --------- | --------- |
| shard   | 4260.955  | —         | 3809.661  | —         |
| breach  | 3907.520  | 4034.938  | 3480.389  | 3969.237  |
| puppet  | 2512.963  | 2195.695  | 2337.706  | 2337.690  |
| quarry  | 2681.151  | 1355.780  | 2703.682  | 2714.067  |
| lantern | 1164.494  | 1457.002  | 1647.502  | 2888.790  |
| sparks  | 806.025   | 1254.571  | 835.170   | 1234.698  |
| viewer  | 237.158   | 191.151   | 231.977   | 237.494   |

quarry's slowest run is **2.0x** its fastest and sparks' is **1.6x**, with no
change to either demo between them, and that spread alone spans the
24-to-45-minute range shard shows — no stop is needed to explain it. The dashes
are the two runs where shard was killed and wrote no log.

**None of these numbers can be attributed to a commit.** lantern reads 1647 ms
at `458a5ed` and 2888 ms at `a1e5168`, which invites blaming the shadow cadence
that landed between them; quarry moves by the same factor with nothing between
its runs at all. One sample per commit cannot tell the two apart. Anything
wanting a per-commit figure needs repeated runs of the same commit, which
nothing here does.

**The cap is raised to 90 rather than the gate weakened.** Job durations at run
33375515324: breach completed in 39 minutes, puppet 21, lantern 16, quarry 12.
The slowest legitimate completion doubled is past 45, so the old cap sat inside
the range of a passing run rather than above it. `pages.yml`'s `demos` job
carries the measurement.

**The cost of getting it wrong is not a red gate.** On every hung run
`deploy to GitHub Pages` was **skipped** — confirmed by name, not inferred — so
the run reads `cancelled` and the published site silently stays at the last
commit that got through. `a1e5168` and `031696a` both did this; the site is at
`458a5ed`. Distinguish the two causes when reading the run list: **only** the
demo job cancelled is the cap, while a demo job _and_ `deploy` cancelled
together is an ordinary concurrency cancellation from the next push.

**One route is a dead end — checked 2026-08-31, so nobody repeats it.** The
cancelled step flushes no output into the job log at all, only the runner's
setup lines and its `Terminate orphan process` list. The
`Upload the browser evidence` step does still run on a cancelled job, but the
artifact then holds **only** the PNG — a successful run's holds the page log
beside it, 39,921 bytes on run 33368904505 — because that log was written when
the run finished.

**That last gap is closed.** `web/tools/browser-e2e.mjs` now appends each page
line to `<slug>-live.log` as it arrives, so a job the cap kills uploads the log
it had reached. Verified by killing a run with SIGKILL at 30 seconds: 90 lines
present, the end-of-run log and the PNG absent. The next demo to die at the cap
is the first one that will say where it stopped — and if it stopped rather than
crawled, that log is what proves it.

## Surprises worth keeping — not bugs

### A Pages run whose browser jobs never end is a lost runner, and the next push is the remedy (2026-09-05)

`31dec1f`'s Pages run (33932758980) sat `in_progress` for eight and a half hours
with `render shard`, `render breach` and `render alcove in a real browser` each
stuck in their render step and no log blob uploaded for any of the three — the
job-level `timeout-minutes` in `pages.yml` never fired, so the runners had gone
rather than the step hanging. The siblings that did finish say what the pool was
doing: quarry's render step took 21 minutes and passed, its log reading "two HUD
lines 36699 ms apart against a nominal 1000 ms" and "every budget here scaled
36.7x", and lantern, puppet and sundial took 25 to 32 minutes each where a
minute is usual. githubstatus.com reported all systems operational. Nothing here
was a defect in the tree — CI on the same SHA was green after its own lavapipe
rerun — and nothing waited on: `pages.yml`'s `cancel-in-progress` is true for
branch pushes, so the next push cancelled the dead run and started a fresh
deploy, which is the remedy. Worth keeping because the standing rule is to hold
a push until the previous Pages run completes, and a run like this one never
will.

**The next run said how slow that pool can be without being lost.** `0fb2d93`'s
Pages run (33937587672) finished green with `render shard in a real browser`
taking 72 minutes: its log reads "every budget here scaled 204.7x", the gate's
own scaling of its budgets to the demo's measured tick rate, with 17-minute and
10-minute gaps between consecutive checks. So a job on that pool can run two
hundred times slower than nominal and still pass, and the difference between
that and the three jobs above is whether the runner was still reporting — a job
past its own `timeout-minutes` with no log blob is lost; one inside it is slow.
Hold the push for the second kind.

### The Windows lavapipe leg draws runners three times apart, and a readback deadline sat inside the spread (2026-09-05)

`vk e2e (lavapipe, windows)` went red twice in a day in `Headless::readback` —
"the 196608-byte readback was still Pending" after the harness's deadline —
first on `e9e0283` with `draw_gen::every_geometry_path_draws_the_same_frame` and
three `mesh::` tests, then on `90c454a` with the forward pass's three `lights::`
tests, and on the second a rerun of the job went red again with the two
surviving `lights::` tests **passing at 29.8 s and 30.0 s**. Neither push
touched `crates/crcbl-vk`, `crates/crcbl-shaders` or the harness, and between
`e9e0283` and `55f27ac` nothing outside tests changed at all, yet the same
`depth_probe::` test took 4.4 s on one run's runner and 12.8 s on the next's,
and the `lights::` readbacks 9 s against 24 to 30 s. So the first diagnosis — a
starved pool, rerun and move on — was wrong in the half that matters: the pool
is the variable, but the spread is the ordinary spread of that runner class, and
`READBACK_DEADLINE` at 30 s sat inside it. It is 120 s now, four times the
slowest legitimate landing seen and still under `.config/nextest.toml`'s
per-test kill, so a lost copy is still reported by the harness's own message.
Worth keeping because the failure names whichever tests drew the slow runner and
reads as a regression in them, and because a green rerun of a threshold that
sits inside the spread proves nothing about the threshold.

### `--document-private-items` does not license a link to a private item (2026-09-04)

rustdoc's own note on `private_intra_doc_links` reads "this link will resolve
properly if you pass `--document-private-items`", which is not true of a
**public** item's documentation. CI's wasm32 doc gate passes that flag and still
refuses
`public documentation for `probe_bounce_grid`links to private item`leak_volume``.
The fix is to unlink the name, never to widen the flag.

Worth keeping because the note reads as an instruction and sends you to change
the command rather than the comment.

### A local `cargo doc` cannot fail without `RUSTDOCFLAGS` (2026-09-04)

Every doc gate in `ci.yml` sets `RUSTDOCFLAGS: -D warnings` in the job's `env:`
block rather than on the `run:` line. A doc gate run without it reports a broken
intra-doc link as a warning and exits **0** — so the local chain is green over
exactly what reddens CI. `85e4f7a` went out that way and left `main` red across
two jobs until `d06edef`.

The corollary that cost the round trip: `[`Name`](path::to::Name)` is
`redundant_explicit_links` whenever the label alone already resolves. A link
that needs a path puts the path _inside_ the brackets —
`[`crate::zone::LAYOUT`]`.

### Git LFS is off, and turning it on is a two-part change (2026-08-27)

**Deliberate, not an oversight** — `.gitattributes` carries the reasoning:
everything binary through P8 is small, golden images are re-blessed often (which
LFS handles worse than plain git), and a `filter=lfs` line breaks `git commit`
outright on a clone without the git-lfs binary. The commented-out line is in the
file.

**The trap to carry forward:** the commit that enables LFS must also add
`lfs: true` to **every** `actions/checkout` step in CI, or CI silently tests
against pointer files. `.gitattributes` says so; nothing enforces it.

**Trigger: unknown, as of 2026-09-02.** It used to be the P9 glTF corpus, and
that corpus arrived without needing LFS — fetched at a pinned commit and checked
against a sha256 manifest, with one small model committed plainly. So this waits
on some _other_ binary the tree does not have yet.

### `vk e2e (lavapipe)` segfaulted once in `viewer`'s lib tests (2026-08-29)

Run 33196877649 on `0702de9` failed the `vk e2e (lavapipe)` job with
`process didn't exit successfully: … crcbl_viewer-… --quiet (signal: 11, SIGSEGV: invalid memory reference)`,
fifteen tests into a suite of 116. Not an assertion — the binary died.
**Re-running the job with no change to the tree passed**, so it is a flake
rather than a defect the commit introduced, and that commit touched only
`apps/lantern`, docs and the browser gate — nothing `viewer` links.

Ran locally against the same driver and the same flags CI sets — `CRCBL_GPU=vk`,
`CRCBL_VK_ICD` at Arch's `lvp_icd.json`, validation, sync validation and
`CRCBL_VK_VALIDATION_FATAL` all on — and all 116 passed. So the crash is not
reproducible here, and nothing narrows it further than "lavapipe, on the runner,
once". Worth reading this entry before chasing a second one: a repeat with a
different test index is a real race, and a repeat at the same index is not.

### `gh run list --commit <sha>` returns nothing here (2026-08-29)

A CI watcher built on `gh run list --commit 8586d82 --json ...` printed no rows
for a commit that had two completed runs, so it waited out its whole timeout
without emitting an event — a watcher that looks identical to "still running"
and is the failure mode `Monitor`'s own guidance warns about.
`gh run list --limit N --json headSha,...` and filtering with `jq` returns them.
Not diagnosed further: it may be the flag, the `gh` version or the runs being
reachable only by branch. Worth knowing before writing the next watcher.

### Reproducing a lavapipe CI hazard locally

The layer _build_ decides what syncval can see, and Ubuntu's is fetchable.
Extract `vulkan-validationlayers_1.3.275.0-1_amd64.deb` and
`mesa-vulkan-drivers_25.2.8-0ubuntu0.24.04.2_amd64.deb` from
`archive.ubuntu.com` — plus `libllvm20`, `libedit2` and `libxml2`, which the
Arch host does not supply in an ABI the Ubuntu builds accept — rewrite
`library_path` in both JSON manifests to absolute paths, then point
`VK_LAYER_PATH`, `CRCBL_VK_ICD` and `LD_LIBRARY_PATH` at them. This reproduced
CI run 32653884228's `SYNC-HAZARD-WRITE-AFTER-WRITE` on this machine down to
`seq_no`, `submit` and `batch_tag`, and the fix was verified against it.

- **CI's Mesa is required, not optional.** A newer lavapipe advertises
  `VK_EXT_present_timing`, so `crcbl-vk` chains
  `VkPhysicalDevicePresentTimingFeaturesEXT`, which the 1.3.275 layer rejects at
  `vkCreateDevice` before a frame is drawn.
- **The newer layer is weaker here, not stronger.** With CI's Mesa held fixed,
  layer 1.3.275 reports the hazard and the local 1.4.357 reports nothing;
  `run-vk-e2e.sh`'s reach line agrees, printing `cross-submission=yes` under the
  old layer on this machine and `no` under the new one.

### Which suite runs on which backend, measured

Read out of `ci.yml` by mapping every `run: …run-*.sh` step to the `CRCBL_GPU`
in its own `env` block, and re-derived the same way on 2026-08-21 after
`crcbl-wgpu` went. Recorded because "the agnostic suites run everywhere" is a
claim this project rests a lot on, and it had never been read off the workflow
rather than believed:

| runner                  | backends      |
| ----------------------- | ------------- |
| `run-hal-seam-e2e.sh`   | dx12, mtl, vk |
| `run-render-e2e.sh`     | dx12, mtl, vk |
| `run-forward-e2e.sh`    | dx12, mtl, vk |
| `run-draw-gen-e2e.sh`   | dx12, mtl, vk |
| `run-sprite-e2e.sh`     | dx12, mtl, vk |
| `run-mesh-e2e.sh`       | dx12, mtl, vk |
| `run-tiling-e2e.sh`     | dx12, mtl, vk |
| `run-gltf-e2e.sh`       | dx12, mtl, vk |
| `run-lantern-golden.sh` | vk            |
| `run-quarry-e2e.sh`     | vk            |

**Every agnostic suite really does run on every native backend.** The two
exceptions are the sample golden suites, and quarry's is vk-only for a reason
that was measured rather than assumed: the fixture forces all three
`GeometryPath` values by subtracting features, and a backend with no mesh path
lands one rung lower than asked, so the harness's own
`"{path:?} was asked for by withholding features and {selected:?} opened"` is
the first failure. That assertion is the fixture working, not a defect: a green
run comparing `IndirectPerBatch` against itself under the name `MeshShader` is
exactly what it exists to stop. **Measured on `CRCBL_GPU=wgpu`, which failed 12
of quarry's 23 tests** before that crate was deleted on 2026-08-21 — dx12 and
Metal have no mesh path either, so the same thing awaits them.

**So making quarry travel is a redesign, not a job edit** — the same shape
`vk_e2e/mesh.rs` needed: split each test's backend-agnostic claim (which levels
the descent chose, how many clusters survived) from its mesh-path claim. Not
attempted, and worth weighing against what it buys: the mesh path exists on one
backend today, so the other arms would be running the indirect half twice.

### An inlined re-export's doc resolves its links in the wrong module

**Latent, not broken — recorded because the shape already cost a sweep once.**
rustdoc merges an outer `///` doc on a module _declaration_ with that module's
own `//!` header and resolves every link in the merged block against the
**parent**, which is what broke roughly sixty links across `crcbl-shell` and was
fixed by moving those docs into the modules. Two doc comments in
`crates/crcbl-shell/src/lib.rs` still have the same shape:
`pub use wayland::e2e as wayland_test_support` and
`pub use x11::e2e as x11_test_support`. Both name modules under
`pub(crate) mod`, so rustdoc _inlines_ the re-export rather than linking to it,
and inlining merges the same two blocks the same way.

Neither doc contains an intra-doc link today, which is the only reason nothing
is wrong. Adding one would resolve it in `lib.rs`'s scope while the reader is
looking at the module's own page — and it would resolve _successfully_ if the
name happens to exist in both, which is the version nothing catches.

Left alone deliberately: there is nothing to fix until a link is written, and
moving the prose into `e2e`'s header would put per-feature scaffolding notes in
a module that is compiled behind a different feature gate. Whoever adds a link
to either of those two comments should move the doc first.

### The wasm rustdoc gate cannot see two whole backends

**Measured 2026-08-21, and it had already cost three real breakages.**
`ci.yml`'s `rustdoc` job ran one command —
`cargo doc … --target wasm32-unknown-unknown` — and `crcbl-mtl` is
`#[cfg(target_os = "macos")]` from its crate root down while `crcbl-dx12` is the
same on Windows, so that run compiles **not one line** of either. A broken
intra-doc link in `crcbl-mtl` passes it.

Proved rather than argued: a link to a non-existent item put inside a
`cfg(macos)` impl left the `wasm32` command reporting success and made
`aarch64-apple-darwin` report `unresolved link`. The Metal counter-query slice
hit exactly this — three `public documentation for X links to private item Y`
errors that the prescribed gate did not see.

**Fixed**: the `rustdoc` job now also runs `-p crcbl-mtl` against
`aarch64-apple-darwin` and `-p crcbl-dx12` against `x86_64-pc-windows-msvc`.
Just those two crates rather than the workspace again — everything else is
already covered by the wasm run, and cross-target rustdoc is not free.

**The general shape, which is worth more than the fix:** a gate run against one
target says nothing about code that target does not compile. The same reasoning
already applies to `cargo clippy`, which is why this repo runs it on darwin and
msvc too; rustdoc simply never got the same treatment. Any _third_ per-target
gate added later needs the same question asked of it.

### Declined: extending the citation gate from paths to symbols

`tools/check-doc-citations.sh` resolves the paths a doc cites and nothing else.
Extending it to the identifiers docs quote was measured on 2026-08-20 and
declined, because the question it would ask is not decidable by grep.

The candidate shape was a backtick-quoted `snake_case` identifier with four or
more underscores — the shape a test name has. There are 169 of those across the
tracked markdown, and 17 do not exist as a `fn` anywhere. But most of those 17
are not meant to: `min_storage_buffer_offset_alignment` and
`optimal_buffer_copy_offset_alignment` are Vulkan limit fields,
`unsafe_op_in_unsafe_fn` is a rustc lint, `wp_commit_timing_manager_v1` is a
Wayland protocol, and `insert_barriers_from_device_tracker` belongs to `wgpu`.
Two more were correct quotations of a name in a sentence explaining what it was
renamed _from_.

So the gate would need an allow-list about as long as its findings, and an
allow-list that large stops being a record of exceptions and becomes the check
itself. Two genuinely stale names came out of the exercise and were fixed by
hand; that is the right cost for this, not a standing gate.

**What would change the answer:** a rule that docs quote a test name only in a
form the tree can confirm — say, always with its module path. That is a writing
convention, and worth proposing only if this rots again.

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

### The apt cache does not close the mirror window

`.github/actions/apt-packages/action.yml` tries its cache before the network and
carries the current worst-case timing in comments beside the code, which is the
copy to read. What none of it fixes: the first run after a cache eviction still
needs the mirror, so the cache narrows the outage window rather than closing it.

### The render-harness job fails at browser launch about a third of the time

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

Measured from the last eight Pages runs on `main` that were not concurrency
cancellations: **three failed**, and the shape is always the same —

```
render-harness-e2e: the browser never wrote DevToolsActivePort
##[error]Process completed with exit code 2.
```

preceded by `Failed to connect to the bus` from Chromium's dbus client. Exit 2
is the driver's own "could not run at all", so nothing was compared and no
golden is implicated. Runs affected: `171f385`, `6228aef`, `0be6f79`; `ae0d03b`
and `f2b8327` passed.

**This matters more now than it did**, because `web/run-cross-backend-e2e.sh`
runs in the same job and reuses that step's readbacks — so a launch failure
takes both comparisons with it. It is not made worse by the new step, which runs
after and passed on `ae0d03b`, but it does mean the deletion bar's oracle clause
rests on a gate that is down a third of the time.

**Diagnosed, and the budget was the cause.** The driver already prints the
browser's stderr on this path, and the failing run's is two dbus errors at **22
seconds** after launch and then silence until the 30-second deadline. The loop
distinguishes a browser that _exited_ — a different message — so this branch is
only reached by one that is alive and has not finished starting. For scale, a
healthy runner drives the whole phase, launch and all eleven scenes and their
readbacks, in 14 to 19 seconds.

So the browser was still starting and the gate gave up on it.

### No GPU job in CI runs on real hardware, and one defect has already proved it matters

Record; the gap and the runner decision it leaves are in docs/backlog.md under
the same heading. Enumerated from the workflows on 2026-08-18, every adapter
every GPU job opens. The `wgpu-e2e` and `cross-backend-e2e` rows went with
`crcbl-wgpu` on 2026-08-21 and are struck rather than left naming jobs that no
longer exist:

| job                          | backend | adapter                               |
| ---------------------------- | ------- | ------------------------------------- |
| `vk-e2e`                     | vk      | **lavapipe** (`CRCBL_VK_ICD` pins it) |
| `dx12-e2e`                   | dx12    | **WARP** (`CRCBL_ADAPTER=cpu`)        |
| `mtl-e2e`                    | mtl     | **Apple Paravirtual device**          |
| Pages probe (Linux, Windows) | webgpu  | **SwiftShader**                       |
| Pages probe (macOS)          | webgpu  | Apple Paravirtual, through Metal      |

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

### The Win32 pointer-clip tests are held out of the ordinary sweep

Record; the one thing still open is in docs/backlog.md under the same heading.
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

## The published site went 15 commits stale, and three causes did it (2026-09-02)

Record; the decision and what it leaves owed are in docs/backlog.md under the
same heading. `231175d` is the last commit whose Pages run deployed. Fifteen
commits later the live site is still showing it. The five runs since break down
as three different failures, which is the point — no single fix reaches all of
them:

- **`e450a0b`, `3fa0204`, `24a3cea` — `cancelled`.** Superseded by the next push
  before they finished. This one was **self-inflicted**: I pushed three times
  inside an hour, and each push cancelled the run in flight. `deploy` never ran
  on any of them and none of the three shows as a failure, so the commits look
  fine and the site simply did not move. The remedy costs nothing — batch
  commits and push once — and it is only obvious after reading a run list.
- **`d0bc715` — `failure`.** The lantern leak check false-fired on ring
  saturation; see the entry on that check. A red demo skips `deploy`. Fixed.
- **`f9982ae` — deployed, in the end.** Every other demo finished while
  `shard`'s render step ran 44 minutes, with no step-level bound and only the
  job's ninety to stop it; it then finished on its own and
  `deploy to GitHub Pages` succeeded. **So the site is current again as of this
  commit** — the fifteen-commit gap above is closed history, not a live outage.
  What it cost was real and the decision below still stands: the run took over
  an hour because one demo needed most of it.

**What this says about the shape of the problem.** The site's freshness depends
on _every_ demo gate passing _and_ on no push arriving first, so its failure
modes are the union of everything that can go wrong in fifteen browser jobs plus
a scheduling accident. Three of the five gaps are invisible in the commit's own
checks: a cancelled run is not a red mark.

**What the wait costs when nothing goes wrong** (measured 2026-09-02, over the
four most recent green runs): the run's wall time is the slowest demo's, because
`deploy` waits on all of them. `shard` was the longest job in every one, at 22,
22, 47 and 49 minutes. `breach`, `lantern` and `puppet` are the next three, all
in the 16-to-35-minute band, and `deploy` itself takes 14 seconds.

**`shard` ranges from 22 to 49 minutes across the six most recent green runs** —
22, 22, 34, 44, 47, 49. That is a **spread, not two clusters**. This entry twice
called it bimodal, on four points and then five with a gap in the middle, and
the sixth run landed at 34 and filled the gap in. Two lessons, one about the job
and one about reading it: the job's cost varies by more than a factor of two run
to run for reasons nobody has looked at, and a gap in five samples is not a
structure.

## `pages.yml` cancels the verification jobs it is not deploying (2026-08-22)

Decision record; the decision and the work it leaves are in docs/backlog.md.
Filed as work; not done yet. **The three-hour Windows leg is fixed and the
cold-cache theory it spawned is dead — do not resurrect either.** For the
record, because both were written down here as established and both were wrong:
the leg was never slow. Its work took 54 seconds (30 of them the wasm build) and
then `node render-harness-e2e.mjs` sat in teardown until the job's timeout
killed it, three hours later. The cache being skipped was a _consequence_ of the
cancellation, not its cause, and raising the timeout — tried twice, at 90 and at
180 minutes — could never have worked. Reordering the teardown so the browser
dies before the server is awaited, plus `closeAllConnections()` in `serve.mjs`,
ended it: on run **32585681819** the Windows leg finished in **2m04s** and the
watchdog line never printed, so the process exited on its own rather than being
rescued.

**Measured 2026-08-23, and it sharpens the choice.** The two seam-probe jobs
(`probe the seam on Windows`, `probe the seam on macOS`) `needs: build`, and
`build the demo site` took **12m25s** on run 32585681819 and **15m09s** on run
32587146042 — the two most recent Pages runs that finished. The three
browser-golden legs, which do not wait on it, were done inside three minutes of
their own start on both. So the probe gate reports something like fifteen to
twenty minutes after a push, and **any push cadence faster than that cancels it
before it has run at all** — not occasionally, but every time. That happened
repeatedly on 2026-08-23: group AI landed with its browser gate verified only
locally, because each following push superseded the run that would have checked
it on Windows and macOS. The golden legs are unaffected, which is why the
workflow still looks like it is verifying things.

**Still open, and it is the user's call**, because it is about runner minutes
rather than correctness. `pages.yml` sets `cancel-in-progress` at the workflow
level for everything but a tag push, on the argument that a superseded deploy is
worth cancelling while two racing to publish are not. That argument is right
about the **deploy** and is applied to the whole run, including three
verification jobs that are not deploys. Job-level `concurrency:` cannot rescue
them — workflow-level cancellation kills the run outright, whatever a job
declares. The deploy is not gated on the harness, so the site ships either way
and what a cancellation costs is only the signal. Now that the leg is minutes
rather than hours the stake is smaller, but a burst of pushes still lands with
none of the three legs having answered:

- _Leave it._ Cheapest, and the gate does answer on a quiet branch — but least
  often exactly when the tree is moving fastest.
- _`cancel-in-progress: false`._ Every push queues a full three-OS run and each
  finishes; the newest still publishes last. A burst of nine pushes becomes nine
  queued runs.
- _Split the verification jobs into their own workflow_ with
  `cancel-in-progress: false`, leaving `pages.yml` a fast deploy pipeline. The
  repo already has this shape in `cron.yml` for `miri`.
- _Move the harness to `cron.yml`._ Always finishes, costs least, and turns a
  per-push gate into a nightly one — a regression is then found by a scheduled
  run rather than by the push that caused it.

## A `ConfigureRequest` went missing under openbox, and nothing explains why (2026-08-23)

**What happened.** On run **32585681821** the `windowed swapchain e2e (Xvfb)`
job's second pass — the one with `CRCBL_E2E_X11_WM=openbox` — failed
`a_resize_from_outside_forces_a_reconfigure_at_the_new_extent` on its 20-second
deadline. The no-manager pass on the same commit ran all four tests green in
4.2s, openbox was alive at the end (the runner checks `kill -0` on it before
reporting), its log holds nothing but the missing-menu-file message, and the two
commits on that push touched `web/tools/` and this file — neither is an input to
that job. The seven runs before it were green.

**What was changed.** The test asked once and waited; it now re-asks on every
pump turn until the shell reports the size, and its timeout names the extent the
shell last reported. That removes the whole class — a request that goes nowhere
is indistinguishable from one not yet answered, and only the client can tell the
difference by asking again — and the new message separates "never answered" from
"answered with a different size", which the old one could not.

**What is not established.** Whether openbox dropped the request, or granted it
at a size nobody recorded, is unknown; the failure predicted only
`Some(RESIZED)` and printed nothing when it did not arrive. It did not reproduce
locally: twelve runs pinned to one core with `taskset -c 0`, against openbox
3.6.1, all passed. The evidence for "dropped" rather than "altered" is indirect
— `crcbl-shell`'s `a_resize_from_outside_is_reported_exactly_once` asserts the
same 800×600 lands under the same manager on the same runner and has stayed
green — so it is a reading of the odds, not a measurement. If the wait ever
expires again, the extent in the message is the thing to read first.

**Deliberately not changed:** the sandbox suite's own resize test still asks
exactly once. It counts the `Resized` events carrying the new size and asserts
there is one, so a repeated request would break the assertion it exists for.

**The same shape exists elsewhere and was left alone.** `crcbl-shell`'s
`x11_e2e.rs` waits on "the window manager's answer" after a `set_mode`, in
`a_mode_request_is_a_request_and_the_effective_mode_is_the_answer` and
`a_window_created_borderless_does_not_report_its_own_request_as_the_answer`: one
`_NET_WM_STATE` client message, then a deadline. If a manager can drop a
`ConfigureRequest` it can drop one of these, and the failure would look
identical. Re-asking would be safe — `WmStateAction` has only `Add` and
`Remove`, no `Toggle`, so a repeat is idempotent — but nothing has been observed
failing there, and hardening a green suite on a hypothesis is churn. Recorded so
that if one of those waits ever expires, this is the first thing to try rather
than the last.

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

## A weekly job that goes red is invisible, and it happened twice at once

Record; what is still open is in docs/backlog.md under the same heading.
`cron.yml`'s miri job was failing from **2026-08-17 to 2026-08-23** and nothing
said so. Its own comment already warns that "a job that runs once a week is a
job nobody is watching" — written after `crcbl-audio` gaining `cpal` broke it in
August and went unnoticed for the same reason. It happened again, and this time
two independent causes had stacked up before anybody looked:

- **A test that cannot run under miri.** `crcbl-core`'s
  `the_environment_variable_is_what_turns_the_gate_on` re-executes the test
  binary in a child process, which is the right way to test a process-global
  without racing the other tests in the binary. Miri does not implement
  `posix_spawn`, so it aborted the whole binary. Fixed by skipping it under miri
  with a reason, so the run reports `1 ignored` rather than staying silent.
- **A `cdylib` example.** `crcbl-jobs` gained `web_worker_gate` with the Web
  Worker spawn backend on 2026-08-23, and `cargo miri test -p` builds every
  example of every crate it is told about. Miri's target cannot produce a
  `cdylib`, so the step died before interpreting anything. Fixed with `--tests`,
  which was measured to select the identical set of test binaries.

- **A test that is seconds natively and hours interpreted.**
  `crates/crcbl-ecs/tests/churn_soak.rs` landed on 2026-08-22 — thousands of
  ticks of spawn/despawn churn against hash maps — and under miri it **did not
  finish in fifty minutes** on the machine it was measured on. Confirmed on the
  runner: the 2026-08-23 manual `gh workflow run` reached **`cancelled` at the
  job's 60-minute timeout**, having got past both failures above and then
  stalled there. It now carries `#![cfg(not(miri))]`, which costs nothing this
  job is for: `crcbl-ecs` contains no `unsafe` at all, so there was nothing
  there for the interpreter to find.

All three are fixed and `cargo miri test --tests -p crcbl-jobs` now runs **per
commit** in `ci.yml`, because that is the crate the value is concentrated in —
its unsafe is concurrent, and x86-64's total store order makes a weakened
ordering invisible to everything else here.

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

Record; what is unverified and what is owed are in docs/backlog.md under the
same heading.

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

Two failures in one session, both on commits that had nothing to do with
windowing (a `Revert` and a mesh-shader reland), both cleared by re-running the
same job unchanged.

**Decided, 2026-08-24, and none of the three options above is what shipped.**
All of them treat the contention as foreign, and most of it was not: nextest
runs tests in parallel _processes_, so this suite was contending with itself —
several of its tests take the foreground, and one of them empties the clipboard
while a sibling is reading back what it just wrote. `.config/nextest.toml` now
puts every `win32::shell::tests::` test in a `windows-desktop` test group capped
at one thread. No assertion was weakened, no retry was added, and no test left
the default suite.

**What that does not fix** is a foreign process on the runner taking the
foreground or the clipboard, which is what the tests' own retry budgets are for.
If the class recurs after this, that is the remaining half, and it is a
different question from the one this entry was open on.

### Vulkan on Windows: the loader ignores its environment when elevated

Record; what is still unknown and what is owed are in docs/backlog.md under the
same heading. **The cause, in the loader's own words** (`44bdf32`, with
`VK_LOADER_DEBUG=all` finally on the right job):

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

### The two PowerShell harnesses keep their own copy of the nextest summary guard

Decision record. The work it scheduled is done: both harnesses dot-source
`tools/nextest-summary.ps1`, and `tools/nextest-summary-test.sh` runs every
fixture through it as well as through the bash helper. **The option, stated and
not taken: make the Windows harnesses bash and delete the `.ps1` copies.** It is
demonstrably possible for at least one of them —
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

## Where the Windows vk e2e leg's time goes, and the one measurement nobody has taken

Record; the unmeasured suspicion and the decision on it are in docs/backlog.md
under the same heading. The leg is 4.39x the Linux one (142.05s against 32.35s
over the same 95 tests), read out of the two jobs' logs. Inside that:

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
- **The other harnesses' `--test-threads 1` is effective and deliberate.**
  `run-cli-e2e.sh`, `run-wayland-e2e.sh`, `run-x11-e2e.sh` and
  `run-win32-e2e.ps1` pass it _without_ `--no-capture`, so nextest honours it —
  `run-win32-e2e.ps1`'s header says why. The one harness that carried the same
  dead pair was `crcbl-wgpu`'s, and it went with that crate on 2026-08-21.

## `CARGO_NET_OFFLINE` on the vk e2e steps: looked at, not applied

**DECIDED 2026-08-30 — left as is.** `Updating crates.io index` costs about 7.4s
per cargo invocation on the Windows leg despite `--locked`, and
`CARGO_NET_OFFLINE=true` would remove it. **Not applied, because the workflow
cannot be read as making it safe.** `Swatinem/rust-cache@v2` restore is
best-effort — a cold key, an evicted entry or a changed lockfile hash all leave
the registry index absent — and no step in either vk e2e job runs `cargo fetch`
first, so an offline cargo would fail the job outright rather than fetch. Making
it safe means an explicit `cargo fetch --locked` step before the suite, and only
then the variable; that is a workflow change with its own failure mode and wants
the user's call.

The other cost in the same job, also untouched: the pinned LunarG SDK install is
about 46s and `Swatinem/rust-cache` does not cover it, since it caches cargo
directories and not `C:\VulkanSDK`. An `actions/cache` keyed on
`VULKAN_SDK_VERSION` would, at the cost of a second cache to reason about.

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

## The Windows clipboard test failed once on a shared runner (2026-08-24)

`crcbl-shell`'s
`win32::shell::tests::an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not`
failed the `build + test (windows-latest)` job on CI run 32685206238 with:

```
an empty slice releases: Backend("the clipboard could not be opened within 70ms (Refused { attempts: 8, error: 5 }); another process is holding it")
```

`error: 5` is `ERROR_ACCESS_DENIED`. The Windows clipboard is a single
machine-wide resource with no fair queue, so any other process on the runner
holding it starves this one; the test's own retry budget gave up after eight
attempts inside 70 ms.

**It recurred on `e7d09be`**, in a different test of the same suite and with a
different symptom:

```
both_offered_formats_round_trip_and_the_reader_picks
assertion `left == right` failed: the engine's own format is lossless, padding and all
  left: Empty
 right: Bytes([40, 107, 105, 110, 100, ...])
```

Not a refused open this time — a successful read that came back **empty**, which
means something emptied the clipboard between the write and the read.

**Measured before changing anything, as this entry asked.** Of the last
twenty-five CI runs, two failed on the Windows leg for this class and no others
did; a third Windows failure in that window was a compile error that failed
every leg. Two observations of a shared-resource race in twenty-five runs.

**What the second symptom identifies is us.** nextest runs tests in parallel
processes, and this very suite contains
`an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not` — a test
whose whole job is to empty the machine-wide clipboard. A sibling reading back
its own payload getting `Empty` is exactly what that looks like from the other
side.

**Fixed by serialising the suite against itself**, not by a retry or a wider
budget: `.config/nextest.toml` groups every `win32::shell::tests::` test into
`windows-desktop` with `max-threads = 1`. The foreground half of the class — see
the entry above — is the same shared-desktop problem and is covered by the same
group. What remains is contention from processes that are not ours, which the
tests' retry budgets already exist for.
