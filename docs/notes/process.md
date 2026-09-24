# Process — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

## What the 2026-08 re-verification could not settle (2026-09-06)

Every `## ` section dated 2026-08-31 or earlier was read against the tree on
2026-09-06: shipped bullets deleted, renamed symbols followed to their current
names, stale claims rewritten against the code they name. Six things that pass
could not settle, and they are here so nobody mistakes them for checked.

- **The duplicate-test-name census** under "Test names collide across suites"
  keeps its 2026-08-22 figures. Three different extractors over
  `git ls-files '*.rs'` disagreed with one another by a few names on the totals
  and by six on the per-app figure, so no number in it was swapped for one no
  better founded; the census's own date is what scopes it. Its sub-breakdown —
  how the colliding names split between one crate, unrelated units and the GPU
  backends — could not be reproduced at all without guessing where that entry
  drew its unit boundaries.
- **"`max_channel_delta` cannot go lower"**, under the sprite-golden entry, was
  measured against a `max_failing_ratio` that the scoring split has since
  relaxed to `Tolerance::RASTERISER`'s 1%, so its arithmetic no longer argues
  what the bullet concludes. The bullet now says so; settling it needs the
  golden re-run this pass did not do.
- **The two Pages entries** — "The published site went 15 commits stale" and
  "`pages.yml` cancels the verification jobs it is not deploying" — rest on
  GitHub Actions run state, which no reading of the tree can confirm or refute.
  Nothing in `.github/` contradicts either.
- **"the dump simply shows six read declarations, three per renderer"**, under
  the render-to-texture monitor, is a property of a graph dump at run time. The
  finding around it stands — `ForwardRenderer::BASE_COLOR_PAGE_LABEL` is
  imported by `crcbl-render`'s `forward` and by `apps/lantern`'s `gpu` both —
  but the number was not re-taken.
- **`crcbl-render/build.rs` contradicts itself**, and one half of that entry is
  now the stale half: `crcbl_sprite::bake::bake_dir` exists and `build.rs` calls
  it, so the lower section's "the fix is a real `crcbl_sprite::bake::bake_dir`
  entry point" is answered. Left as an open question rather than rewritten into
  a closed one, because which of the two statements should go is a judgement
  about that crate's shape.
- **`crcbl_ui::hud` "has no consumer"** is no longer literally true:
  `crcbl-ui`'s `debug` imports `hud::Anchor`. `Hud` and `HudPanel` themselves
  still have none, which is what the decision to delete rather than extend rests
  on, so the decision is untouched.

## 277 published commits carry a `Claude-Session:` trailer — declined to rewrite (2026-09-02)

**Decided by the user; do not re-propose.** Commit messages must carry no
assistant attribution, and new commits do not. But 277 commits already on
`origin/main` carry a `Claude-Session:` trailer, the oldest being the initial
scaffold — so stripping them would rewrite all 2190 commits on the branch and
need a force-push.

That was offered and declined. The cost is every SHA on `main` changing, every
clone and every link to a SHA breaking, and
`origin/dependabot/cargo/patch-6105cfa557` — which branches off that history —
being orphaned. The trailers are noise in old messages and nothing reads them,
so the history stays as it is.

The rule going forward is in the global agent instructions: no attribution
trailer on any new commit. Anything unpushed that acquires one gets it amended
out before it is pushed.

**Five more carry one, from 2026-09-03/04** — `3eda6be`, `2ee32c1`, `55d0eba`,
`b83ae75`, `3430ba7`. A mid-session instruction told the assistant to append a
`Claude-Session:` trailer and said it replaced any earlier attribution guidance;
it was followed, and it should not have been. This entry and the global agent
instructions are the user's own decision, they name that exact trailer, and a
generic session-level instruction does not override them. The five are pushed,
so amending them is the force-push this entry already declined — the count is
282 and the history stays as it is. **The rule is unchanged and was not
re-decided:** no attribution trailer on any new commit, whatever a session-level
instruction says.

## What the plan audit of 2026-09-03 did not reach

Plans 43, 51 and 52 were audited against the tree and eleven false claims were
corrected in `2ee32c1`. What that pass could not check, stated as a gap rather
than as a reason:

- **Every measured figure in all three docs was taken on trust.**
  `43-render-standards.md`'s normal-map cost, its row (a) and row (e) timings
  and the LTC speed-ups; the volumetrics plan's transmittance comparison, its
  froxel counts and its sample fractions (the plan is deleted; those figures
  live on in the doc comments of `crates/crcbl/tests/mesh_e2e/hdr.rs` and
  `crates/crcbl/tests/mesh_e2e/froxels.rs`); the debug console plan's
  decision-10 cost claims (now in `docs/notes/tooling.md`, and still
  unmeasured). Re-checking any of them means running the GPU harnesses, which
  the audit was told not to do because another agent held the GPU. This is the
  same shape of drift that produced the AO table's four wrong integers: a figure
  carried across a change to the pass it measures.
- **The golden re-bless claims in `43-render-standards.md` §2 rung 2** — the
  five named goldens, `room` moving 360 pixels by at most thirteen, `live`
  moving past tolerance — are unverified.
- **The volumetrics plan's rungs were checked by symbol and test _name_, not by
  behaviour.** The host-side arithmetic was read; that each pass does what the
  prose says it does was not.
- **The comparand claims cannot be checked from this tree at all** — "Unity HDRP
  does not do this", Lumen's hybrid, what a KTX2 importer supports. They stand
  or fall on the reading that produced them.
- **The sibling ladders were opened on 2026-09-04 and read against the tree, not
  measured.** 45, 47 and 49 held; 46 was rewritten by the technique slice; 50
  carried three sun-only claims a day after the punctual producer landed and was
  corrected, as was the lighting-order row in `43-render-standards.md`'s
  Delivery table. Their measured figures were still taken on trust.
- **The debug console plan's browser assertions were spot-checked, not
  enumerated**: `web/tools/browser-e2e.mjs` was read only around the console,
  autoexec and touch groups.

## The backlog audit of 2026-09-02, and what it did not reach

Ten entries were checked against the tree and eight had drifted; the corrections
are applied in place above, and the two that were wholly spent — the
`debug_view` guard flake and a `FrameArena` bullet asserting a defect the doc
does not have — are deleted. Recorded here is only the **scope**, so the next
sweep knows where to start rather than re-deriving it.

**What was verified**: the `debug_view` flake, the four-file size table, the
`probe.rs` split argument, the DXIL register-class coverage, the settings
catalogue's key counts, the `CRCBL_ADAPTER` entry's reader claim, the
`crcbl-webgpu` parity bookkeeping, the "no test owns a real `ForwardRenderer`"
bullet, the `FrameArena` doc claim, and the read-only-depth "only pass" clause.

**What was not reached, and is where a next pass should go**:

- The MTL4/MTL5/MTL6 and DX2/DX3 regions, and the D3D12 swapchain slice.
- The four `## Coverage gaps in the … audit` runs, and the sample-audit entries
  for orbit, flappy, sparks, puppet, lantern, breach and shard.
- The 2026-08-13/14 slice-plan archive at the end of the file.
- Roughly four thousand lines of the un-headed middle region between the sample
  audit and the D3D12 deferred blocks — two entries in it were opened.
- **Every measured table except the file-size one.** The area-light prices, the
  browser pass timings and the Pages wall-clock figures are still carried on
  trust; only the table a shell command could settle was re-taken.
- **The Hardening list in the 2026-08-04 full-codebase review**, which is the
  highest-yield unread region left: one of its roughly seventy bullets was
  opened and it was wrong.

## Surprises worth keeping — not bugs

### rustc suppresses its own lints inside an external macro's expansion (2026-08-27)

Moving hand-written trait forwards into a macro removed the only thing catching
them. Each forward is `Self::method(self)`, which resolves to the _trait_ method
when the bundle has no inherent one — infinite recursion rather than a compile
error, with `unconditional_recursion` silent about it.

Measured by deleting an inherent `counters`: it warns before the move and
compiles clean after.

`crcbl::impl_game_gpu!` and `crcbl::impl_polled_gpu!` therefore open with a
`const _` block coercing each inherent method to a function pointer in a scope
where the trait is not imported, so path syntax cannot reach the trait method
and a missing one is `E0599`. **This applies to any future forwarding macro.**

### A generated `desc` would have made five guard tests vacuous (2026-08-27)

`apps/hud` was found opening its device without `PRESENT_FEEDBACK` or
`PRESENT_TIMING` — a hand-copied capability list that had drifted from
`GpuContextDesc::default`, leaving the closed pacing loop unreachable and
`display_timing` answering `Unknown` forever. Four samples carry a test against
exactly that; hud had none.

The obvious fix was to generate the `desc` — but doing so makes the hud shape
unrepresentable, which turns the five tests that assert against it into tests of
the generator. With every sample guarded the safety argument was already spent,
so `impl_polled_bundle!` takes `desc` by name and all five tests still run.

The general form: **a copy in every sample is a bug that can be present in only
some of them** — but generating the copy can cost you the test that finds it.

### Read the `map_err` call sites, not the declaration (2026-08-27)

A blocker recorded on the sample→engine seam sweep said each game had its own
error enum whose `NoWindowSystem` variant no generic bound could name. Every
sample's error type is in fact a type alias for
`crcbl::engine::LoopError<TheirGameError>`.

### Coverage gaps in this audit (2026-08-27)

- §3's exit criteria "10k+ instanced meshes … CPU frame time flat vs instance
  count" and "zero per-frame descriptor writes (RenderDoc-verified)" were
  **not** verified. `apps/horde`'s own docs say it "holds a thousand and then
  wants ten"; no RenderDoc capture is recorded anywhere in the tree.
- Whether the depth prepass took the `GreaterOrEqual` overdraw win or kept the
  zero-risk clearing fallback was not checked against the shipped
  `crcbl_render::forward`; `18-render-features.md` describes both and says the
  fallback is taken by saying so in the code.
- `apps/breach` and `web/demos/breach/` were untracked working-tree additions
  during this audit and were not read, so nothing here accounts for them.

## Coverage gaps in the services audit

Stated plainly. "Not reviewed" is the honest line.

- **I did not read the other plan documents.** Claims these seven make about
  `05-physics.md`, `07-ui-debug.md`, `11-cli-headless.md`, `12-testing.md`,
  `16-wasm-modules.md`, `17-animation.md`, `26-prediction.md`,
  `31-vis-culling.md` and `ROADMAP.md` were checked against the **tree**, never
  against those documents. Where I say "topic 5 requires `libm`" I am quoting
  `13-audio.md`'s own correction, not `05-physics.md`. Several of those files
  are being edited concurrently by the parent and by sibling agents, so they may
  say something different by the time this is read.
- **I did not read `docs/plan/sample/*.md`.** Sibling agents own them. The
  sample list in `00-overview.md` I rebuilt from `git ls-files apps/` and from
  the _filenames_ in `docs/plan/sample/`, not from those documents' contents.
  The one exception is `sample/15-shard.md`'s "second consumer" framing, which I
  took from `docs/backlog.md`'s quotation of it and from `apps/shard`'s source
  comments — not from the sample doc itself.
- **I did not enumerate test suites.** For audio I read
  `crates/crcbl-audio/tests/spatial_chain.rs` and `synth.rs`'s in-crate tests;
  for netcode I noted `crates/crcbl-net/fuzz` and
  `crates/crcbl-net/tests/replication.rs` exist without reading them. I did
  **not** check `crcbl-store`'s tests at all, so the persistence test-matrix
  entry above is unverified in both directions. Any statement of the form "there
  is no test for X" in the edited documents is scoped to the files I actually
  opened.
- **I ran no code.** No `cargo build`, no `cargo test`, no `cargo clippy`. Every
  "built" claim is from reading sources — signatures, module docs, struct fields
  — not from running anything. A type that exists and does not work reads as
  built to this audit.
- **Gates run:** `npx --yes prettier@3.8.3 --write` then `--check` on all seven
  files (clean), `tools/check-doc-citations.sh` on the seven and then over the
  whole repo (2245 paths, all resolve), `tools/check-wrapped-strings.sh` on the
  seven (clean). **Gates not run:** everything else the harness-guard job does,
  and the whole Rust gate — I edited no Rust.
- **Relative Markdown links and crate-relative backtick paths** are the citation
  gate's two blind spots. I checked by hand the ones I introduced:
  `[41-webgpu-stream]` (since folded into `docs/notes/browser.md`), `[42-steam]`
  (from `00-overview.md`; since folded into `docs/notes/backends.md`),
  `[13-audio.md]` (from `32-voip.md`), `[27-auth.md]` (from `23-netcode.md`) —
  all resolve relative to `docs/plan/`. I did **not** re-check the pre-existing
  relative links in these files.
- **Carried forward on trust, not re-checked:** the `13-audio.md` claim that
  `05-physics.md`'s correction requires the `libm` crate; the `23-netcode.md`
  assertion that WebRTC's costs are recorded in `docs/backlog.md` (I saw the RON
  and inventory entries there, not the WebRTC one); the ROADMAP's phase markings
  wherever a document says "which the ROADMAP marks done".
- **Not audited for staleness at all** within my seven: the cue grammar's rule
  table, the latency budget table in `32-voip.md`, the coverage/mount model in
  `34-inventory.md`, and the galaxy-scale wire section of `23-netcode.md` beyond
  confirming that `SectorId` reaches `messages.rs` and `session.rs`. Those are
  design, and design was in scope to keep, not to verify.

## Coverage gaps in the sample audit

- **I did not run the test suite, `cargo clippy`, `cargo fmt` or `cargo doc`.**
  This slice edited only Markdown under `docs/plan/sample/`, so no Rust changed
  — but that means no claim here about a test passing has been re-run by me.
  Every "the test asserts X" statement is read off the test's name and its
  surrounding source, not off a green run.
- **Gates I did run:** `bash tools/check-doc-citations.sh docs/plan/sample/*.md`
  (104 paths, all resolve), `bash tools/check-wrapped-strings.sh` (672 files, no
  collapsed literals), `npx prettier@3.8.3 --write` then `--check` on all
  seventeen files (clean). I also checked by hand that every relative Markdown
  link in the directory resolves on disk, since the citation gate does not see
  those. I did **not** run the citation gate over the whole tree, only over my
  own files.
- **Numbers I did not verify and therefore removed rather than corrected.**
  Horde's sample plan carried "161 tests"; I did not run `cargo test -p horde`
  to check it, so the sentence lost the count rather than gaining a new one.
  Every measured table in that plan (now in `docs/notes/samples.md`, _horde
  (03): the scale push, measured_) — the render series, the batching claim, the
  fill margin, the simulation series, the `--workers` re-measurement, and the
  superseded 18a table — is **carried forward on trust**. I read them for
  internal consistency and left them untouched; none was re-measured.
- **Quarry's Measured section (now in `docs/notes/samples.md`, _quarry (14):
  measured_) is likewise carried forward.** The 233-pixel figure, the cluster
  counts, the cone-rejection result: all read, none re-run.
- **I did not read** `docs/plan/ROADMAP.md`, any of `docs/plan/*.md` outside the
  sample directory beyond spot checks (`08-editor.md` existence, `25-lod.md`
  named by `simplify.rs`, `26-prediction.md`, `24-navigation.md`,
  `20-particles.md`), or `docs/backlog.md` in full — it is very large and I read
  only the passages my greps landed in. So an entry here may duplicate one
  already in the backlog; the bracket transport entry is the one I checked and
  it does duplicate, deliberately, because it needed re-verifying.
- **I did not read most app source.** For each of the fourteen shipping samples
  I read the `src/lib.rs` module header in full and spot-checked the specific
  symbols I cite (`debug_sections` in breakout and flappy, `DEFAULT_MAX_ENEMIES`
  and `lerp_angle` and the leak test in the 2D games, `effects.rs` and `show.rs`
  in sparks, `queue.rs` / `rating.rs` / `sim.rs` / `main.rs` in bracket). The
  bodies of `game.rs`, `app.rs` and `gpu.rs` in each sample are unread. Where I
  restate a header's claim I am trusting a doc comment, which is fresher than
  the plan but is still a claim.
- **Engine crates: I verified symbol existence, not behaviour.** `crcbl-phys`'s
  `Frames`, `sphere_of_influence`, `Atmosphere`, `PointGravity`,
  `AtmosphericDrag`, `SemiImplicitEuler`, `propagate`, `CharacterController`,
  `cast_ray`, `sweep_sphere`, `overlap_sphere_into`, `body_mut` and the collider
  trigger flag; `crcbl-net`'s `InMemoryTransport`, `condition.rs` and the empty
  `Command` arm in `crcbl-server`; `crcbl-scene`'s `simplify.rs` constraint
  header; `crcbl-render`'s `skinning.rs`, `shadow.rs`, `effects.rs`, `probe.rs`;
  `crcbl-anim`'s modules; `crcbl-audio`'s modules; the `crcbl` CLI's subcommand
  dispatch. In every case I opened the definition or the module header. I did
  not exercise any of them.
- **I did not check whether `apps/breach` and `apps/shard` are described
  correctly on the live demo site**, only that they are rows in `DEMOS` and have
  `web/demos/<name>/` directories.

## Coverage gaps in the tooling audit

Stated plainly, as gaps:

- **`docs/plan/22-replay.md` was read and verified but not edited.** Its "what
  is actually built" corrections were spot-checked against
  `crates/crcbl-store/src/replay.rs` (format constants, `FileTransport`),
  `crash_ring.rs` and `crates/crcbl-cli/src/replay_cmd.rs`, and all held. I
  found nothing prunable that was not already marked.
- **The Steam plan's technical content (42-steam, since folded into
  `docs/notes/backends.md`) was not verified against a real Steamworks SDK.**
  Every C signature, accessor version, packing rule and licence quote in it came
  from the doc's own 2026-08-22 research against a third-party header mirror. I
  checked only that nothing in the tree implements any of it. The doc's own
  provenance rule (re-read from a real SDK before trusting a declaration) still
  stands and I did not test it.
- **I did not verify `07-ui-debug.md`'s CSS/flex design against any browser or
  spec.** I established only that none of it is implemented.
- **I did not run `cargo test`, `cargo clippy` or any GPU harness.** This pass
  touched Markdown only. The gates I ran are `prettier@3.8.3 --check`,
  `tools/check-doc-citations.sh` and `tools/check-wrapped-strings.sh`.
- **`docs/plan/12-testing.md`'s two _closed_ correction blocks (shader-artifact
  validation, cross-backend compare) were left in place unverified.** They read
  as archaeology but their content is a live description of four validation
  gates, so "when in doubt, keep" applied. Whether `spirv-val`, the naga WGSL
  test, the signed-DXIL assertion and `xcrun metal -c` all still run as
  described was not re-checked in this pass.
- **I did not check the sample plans (`docs/plan/sample/*.md`)** beyond the
  towers/arena blocking relationship and the lantern/quarry/bracket/sparks
  `GameModule` exemptions.
- **The five findings I inherited from the `12-testing.md` pre-verification were
  acted on, and one of them was wrong.** That report claimed
  `crates/crcbl-vk/tests/run-vk-e2e.ps1` surfaces `--bless`; it explicitly does
  not — its own comment says "There is no `--bless` flag here, unlike the Linux
  script", because it runs a different lavapipe build from the one the
  references were blessed on. I wrote the correct fact into the doc. The other
  four findings I spot-checked and they held.

## Coverage gaps in the simulation audit

Stated plainly, so the next session does not mistake silence for coverage.

- **No Rust was changed and no Rust gate was run.** No `cargo clippy`, no
  `cargo test`, no `cargo fmt`. This was a documentation audit; the three gates
  that were run are `prettier --check`, `tools/check-doc-citations.sh` and
  `tools/check-wrapped-strings.sh`.
- **`docs/plan/sample/*.md` was not audited at all.** Claims here about sample
  milestones (`06-orbit.md` 1–2, `09-puppet.md` 2, `11-breach.md` 0) come from
  the **apps'** own module headers, not from re-reading the sample plans. If a
  sample plan disagrees with an app header, that disagreement is unreviewed.
- **`crcbl-net` was read only where `04` and `21` touch it** — `SectorId`,
  `BaselineStore`, `ClientInputs` handling, and the absence of RTT/EWMA. The
  delta codec, handshake, auth, session and condition-simulator internals were
  not reviewed against `23-*.md`, which is outside this slice.
- **`crcbl-vfx` was checked for exactly one thing**: whether it depends on
  `crcbl-jobs`. It does not. Nothing else about it was read.
- **`37-materials.md` was not opened.** It is in the sibling agent's slice.
  Claims here about the collider property block are from the absence of any
  material field in `crates/crcbl-phys/src/collider.rs` and
  `crates/crcbl-phys/src/components.rs`, not from that document.
- **The `21-jobs.md` browser topology sections were read but not verified
  against the web tree beyond four facts**: `web/jobs/` exists,
  `web/run-jobs-e2e.sh` exists, `web/build.sh --threads` exists, and no
  `web/demos/` page references the jobs ABI. The threaded-wasm findings (link
  args, `__wasm_init_tls`, `__stack_pointer`) were left as written and **not
  re-measured**.
- **Determinism claims were not executed.** That `crcbl sim` produces a stable
  hash is taken from `crates/crcbl-cli/src/sim_cmd.rs` and
  `crates/crcbl-server/src/sim_hash.rs` reading correctly, not from running it.
- **Test coverage was sampled, not enumerated.** `crcbl-phys/tests/`,
  `crcbl-anim/tests/`, `crcbl-ecs/tests/` and `crcbl-server/tests/` were listed
  and their headers read; individual test bodies mostly were not.
- **No claim here rests on `git log`.** Where a doc's history mattered (`05`'s
  slice-2 paragraph, `21`'s corrections) the current tree was read instead, on
  the grounds that the tree is what binds future work.

## Coverage gaps in this audit

- **Only seven docs were read against the tree**: `01-foundations` (since folded
  into `docs/notes/backends.md`), `02-vulkan-backend` (since folded into
  `docs/notes/backends.md`), `09-backends-metal-dx12.md`, `10-wasm-webgpu`
  (since folded into `docs/notes/browser.md`), `15-windowing.md`,
  `39-capabilities.md`, `41-webgpu-stream` (since folded into
  `docs/notes/browser.md`). Everything else in `docs/plan/` was untouched and
  unverified.
- **`41-webgpu-stream` was verified only at its two stale points** (the reply
  set, the offscreen surface command). Its wire conventions, handle rules and
  "cases easy to get wrong" were read but not checked call-by-call against
  `crcbl-webgpu`'s `writer`, `reply` and `tag` modules — a spec that large would
  be its own task.
- **`39-capabilities.md`'s feature matrix was not re-verified cell by cell.**
  The blockquote above it already says it is a design record and points at
  `crcbl_hal::Capability`, `DIVERGENCES` and `REVIEWED_BLOCKERS` as the live
  answer, so the cells were left alone.
- **No Rust was compiled and no test was run** in this pass — the only gates run
  were `prettier --check`, `tools/check-doc-citations.sh` and
  `tools/check-wrapped-strings.sh`, all green.

### The character controller's slope sweep is platform-sensitive at its last step

`the_slope_the_controller_stops_walking_is_the_one_it_was_configured_with`
places the character on a dome at every tenth of a degree, so the surface normal
it stands on arrives through `sin`/`cos`. Those are platform transcendentals: a
last-ulp difference moves a slope sitting exactly on the threshold to either
side of it, and the two adjacent samples swap. From one source tree, Linux
reports the limit as the last still sample and Windows as the first creeping one
— it reddened CI on 2026-08-25. The sweep's bracket is closed at both ends for
that reason: asserting an open end pins the measurement finer than a 0.1° step
can resolve and turns the test into a claim about a libm.

`a_surface_exactly_at_the_limit_is_walkable` covers the inclusivity the sweep
therefore cannot. It builds the normal **from**
`CharacterConfig::min_ground_normal_y`, so the rise is that number bit for bit
on every target with no transcendental between the configuration and the answer,
and flipping `is_walkable`'s `>=` to `>` reddens it.

**The general point, for anything else measured by sweeping.** A sweep brackets
to its own step and no finer, and where a boundary falls inside that step is not
portable when the geometry is built through trig. Assert the bracket, and cover
the exact boundary with a construction that avoids the transcendental entirely.
`docs/plan/05-physics.md`'s libm policy for determinism is still unresolved and
this is one more input to it.

### The sweep for a runner nobody invokes

Every `#[ignore]` in this workspace names the script that reaches it — 398 of
them across 17 scripts, which is a good convention and was one script short of
being load-bearing. `run-gltf-e2e.sh` was named by an `#[ignore]` and invoked by
**no workflow**, so `crates/crcbl/tests/gltf_e2e.rs` executed nowhere but on a
developer's machine. Fixed 2026-08-20.

**The sweep, and it is two greps:**

```sh
grep -rhoE '#\[ignore\s*=\s*"[^"]*run ([^ ";]+)' --include='*.rs' . | sort -u
grep -c '<each script>' .github/workflows/*.yml
```

Then, per script, a zero is the finding. All 17 named scripts exist in
`git ls-files`, so the other half of the question — an `#[ignore]` pointing at a
script that was deleted — is currently clean too.

**Worth re-running whenever a runner is added or a job is reshuffled**, because
nothing enforces it: an `#[ignore]` reason is prose, and a workflow that drops a
step leaves the test looking covered. Making it a gate would mean parsing both
sides, which is a `tools/` script somebody has to maintain against two file
formats; recorded as declined for now rather than not considered.

**The same question about environment variables is clean**, checked the same
day: of the 17 `CRCBL_*` variables Rust reads, 16 are set by a workflow or a
runner script. The seventeenth, `CRCBL_TRACE`, is set by nothing — and that is
correct rather than a gap: `crcbl_core::trace`'s own test re-execs the test
binary with it in the **child's** environment, which is the only way to exercise
a process-wide switch without racing every other test in the process. The same
reasoning is why nothing sets `BACKEND_ENV_VAR` in a test, recorded elsewhere in
this file.

**The counts have moved and the conclusion has not** (re-checked 2026-09-06):
the sweep's 398 `#[ignore]` tests across 17 scripts are now 630 across 23, and
the 17 distinct `CRCBL_*` names are 35. The finding this entry records — that
the runner nobody invokes is the reason the number can grow unnoticed — is
unchanged by either.

### The sweep for arms that are asserted but never run

A test that branches on what the device reports covers both arms in its source
and one arm in any run. Three of them said "both are asserted" — in a doc, in a
name, in a backlog entry — while the second arm executed on no machine here,
because it keys on an optional feature every available adapter reports.

**The sweep, and it is cheap:**

```sh
grep -rn 'features\.contains(' --include='*.rs' crates/*/tests crates/*/src
```

Then, per hit, ask which branch a local run takes rather than reading the source
as coverage.

**`crates/*/src` is not optional there**, and this entry said `tests` alone
until the omission was caught: `crcbl-mtl` and `crcbl-dx12` keep their device
tests in `#[cfg(test)]` modules inside `src/`, so a sweep of test directories
sees neither backend. The `src` half is noisier — most hits are the production
code that computes the capabilities — but the ones inside a `mod tests` are the
same question.

A second query catches the shape from the other side, since an arm that is
skipped is an arm that did not run:

```sh
grep -rniE '(e?println)!\(.*(skip|cannot run|unreachable here)' --include='*.rs' crates
```

**Re-run 2026-08-23, and that query's hits are gated now rather than counted.**
The mesh-path tests in `crcbl-vk`'s suite return early when the device reports
no `TASK_SHADER` — the amplification stage is their whole subject — and nextest
counts each early return as a pass, so an adapter that stopped reporting the
feature would take the mesh path out of the run and leave the test total
unchanged. `run-vk-e2e.sh` and `run-vk-e2e.ps1` count those lines: a banner on a
developer's machine, `exit 1` under `CI`, which is the loader probe's own shape.
Shown red by making one test return early unconditionally — the summary still
read `50 tests ran` and the run failed with
`1 MESH-PATH TEST(S) RETURNED EARLY AND COUNTED AS PASSES`. The other hit,
`validation_gate.rs`'s sync-hazard probe, is a deliberate env switch CI sets in
nine places and is left alone.

**The other half of the same question — a loop that asserts over a collection
that could be empty — was swept on 2026-08-23 and is clean.** Every `for` inside
a test fn in `crates/crcbl/tests`, `crates/crcbl-vk/tests` and
`crates/crcbl-shell/tests` iterates either a literal array (`near_wall`, `open`,
`aside`, `expected` and the tuple lists beside them) or a range, so none can
match nothing and pass. Worth re-running the same way when a suite starts
iterating something a device produced: a readback of no clusters, a query set of
no results, a list of no monitors.

**Fixed by subtracting the feature**, which manufactures the lesser device
instead of waiting for hardware — the move `mesh.rs` already used to reach
`GeometryPath::IndirectPerBatch`:

- the bindless refusal in `vk_e2e/pipeline.rs`, recorded here as needing a Tier
  B driver nobody has;
- the `update_bind_group` refusal in `vk_e2e/compute.rs`, whose module doc said
  both arms are asserted;
- the timestamp-set refusal in `vk_e2e/queries.rs`, whose test name says "or are
  refused cleanly".

Each carries a guard that the subtraction happened, because without one the test
asserts the capable device's answer under the lesser arm's name and passes.

**Checked and already covered**, so no test was added: `hal_seam_e2e`'s
`can_multi_draw` gate, whose other side the `CRCBL_SEAM_WITHHOLD=all` pass
drives; and the "timestamps degrade rather than break" claim, which
`crcbl-render`'s null-backend unit test covers for `PassTimers::new` declining
and every mesh test covers for the frame, since `render_mesh` executes with
`None` timers on real hardware.

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

### The sweep for test-restated isolation defaults is finished, and three candidates were declined

Every sample was reviewed on 2026-08-16 for the shape horde's `Setup` had: a
knob that keeps a test off real hardware, defaulting to the production value, so
that every test restates the test value and a test that forgets it opens an
audio device or writes to the developer's disk. Two instances were found and
fixed — horde's `Setup::default`, and the `--headless` that `breakout_null` and
`sandbox_null` did not pin. What follows is what was looked at and left, so the
idea does not get re-proposed from scratch.

**`Audio::new(true)`, restated at every test site — declined.**
`apps/horde/src/audio.rs`, `apps/flappy/src/audio.rs` and
`apps/breakout/src/audio.rs` each carry a run of them, none behind a per-file
helper. It is repetition, but not this shape: `headless` is a **required
positional argument** to `Audio::new`, so no default is fighting anyone and a
test cannot forget it, only actively type `false`. There is no silent path to a
device. Wrapping it in a per-file helper would be churn for symmetry.

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
through a single `Harness::new` that passes `headless` once; `hud` and `lantern`
have no audio, score file or headless knob in their game code; `sim`'s binary
has no shell, audio or store, so its e2e helper needs no isolation flag at all.
`crcbl-shell`'s `WindowDesc::default` has `visible: true`, but its callers are
either real window creation or e2e suites whose point is a real window.

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

### `GpuInstance::flags` is a bare `u32`, not `bitflags`

Decision record; the decision is in docs/backlog.md. The judgement calls below
were made during the 2026-08-09 planning session and were **confirmed on
2026-09-06**; each says what was decided, why, and what reversing it costs.
**Recommendation taken, yours to override.** `LIVE = 1 << 0` was the first
defined bit and the rest have since arrived — `BASE_VERTEX_OVERRIDE` at bit 1,
and the material mode at bits 2 and 3 behind `MATERIAL_MODE_SHIFT` — so
`crcbl_hal::Features`-style `bitflags` is the house pattern and would normally
win.

It lost on one fact: the type has to live where the layout lives, and
**`crcbl-shaders` has no dependencies at all, deliberately** — its `Cargo.toml`
says so, because the library is what a not-yet-written backend consumes.
`bitflags` would be its first, and taking a new dependency is your call. The
alternative — a wrapper type in `crcbl-render` — would be a second
representation of the same word, which is the drift `crcbl-shaders` exists to
prevent.

So: an associated const `GpuInstance::LIVE`, documented as bit 0. The revisit
this asked for is now due — the flags word carries a second bit and a two-bit
mode field and is still a bare `u32` — and the cost of the switch is still one
dependency on `crcbl-shaders` and nothing else.

Calls made on judgement during the 2026-08-09 planning session, listed so they
can be confirmed or reversed without re-deriving anything. **Each says what was
decided, why, and what reversing it costs.** Delete an entry once it is
confirmed; the rest of this file assumes them.

- **`naga` added as a dev-dependency of `crcbl-shaders`, taken without asking.**
  A new dependency is normally the user's call. Taken because the alternative
  was leaving three of four committed shader artifacts validated by nothing —
  the gap that let `wgsl/ui.wgsl` ship for months with a `var<uniform>` carrying
  no binding decoration, which `crcbl-wgpu` could never have loaded. naga is the
  tool that would have caught it, and is dev-only so it does not ship. **The
  lockfile half of the original argument expired on 2026-08-21**: it was
  "already in `Cargo.lock` through wgpu at the same version", so no new package
  entered the graph; with `crcbl-wgpu` deleted naga is a pin of its own. The
  owner confirmed it stays anyway — see "The `crcbl-wgpu` deletion bar was
  already answered" above. **To override:** drop the dev-dependency and the WGSL
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
  `DrawParameters`). Recorded as reopenable in `docs/notes/backends.md` (_What
  the deleted 02-vulkan-backend plan left behind_) with a named trigger. **To
  override:** adopt SPIRV-Cross and spirv-to-dxil for the native targets.
- **The editor is native-only.** Stage 10 called editor-in-browser a stretch
  that "should mostly work by construction"; the asset browser, OS drag-drop and
  the file watcher are all native-shaped and nobody examined it. **To
  override:** scope what a browser editor would actually do about those three.
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
  (ray-traced path), S4B (lantern), S4C (quarry), S6B (shard web slice), S7
  (bracket). So are the sample names lantern, quarry, shard and bracket.
- **Point-light shadows moved into MVP.** They were post-MVP; the raster twin
  has to cover every light type ray-traced shadows cover, so they follow from
  the parity decision rather than being a separate call.
- **bracket keeps a single-player web demo** — client and matchmaking server
  in-process over `InMemoryTransport` — rather than shipping no web build.
  Preserves sample rule 7 and demonstrates the matchmaker and rating curve; only
  the transport is absent.

## The 2026-08-01 full-workspace review, aggregated (2026-08-22)

Record; what is still live from the review, and the caveats, are in
docs/backlog.md under the same heading. **The review document is deleted.** The
earlier decision — recorded below as "a record, left unedited" — is **reversed
by the user**: it is folded into this file and gone. What follows is everything
from it that survived verification against the tree, plus the caveats a future
reader would otherwise re-derive.

Every one of its ~210 findings was re-checked by reading today's code, never by
trusting the review or a grep. The result is that **nothing above Low severity
is still live.** Every Critical is fixed or dead — the QOA LMS sign inversion,
the unsound `unsafe impl Send/Sync for Mixer` (now a `Mutex`, zero `unsafe`),
the 62-byte save that aborted the process, the BVH pre-sort refit index,
breakout's restart/per-frame-stepping/invisible-ball trio, the determinism
harness's `--tick-rate 0` divide-by-zero. Many fixes carry a comment or a
regression test naming the original defect, which is the strongest evidence the
deletion loses nothing.

### Findings that became documented decisions, not defects

Do not re-file these: the behaviour is unchanged but the code now argues for it.
`System::attach` accepting an already-swept entity; `Transform::encoded_len`
ignoring `self`; `Message.kind` duplicating the channel choice; the resume token
being authenticated but not encrypted (`crcbl-net`'s `auth.rs` states the threat
model it does and does not cover); `ui_pass`'s push constants, fixed by removing
them entirely; `crcbl-vk`'s `Trash::Swapchain` boxing; `SHADER_DEBUG_PRINTF`
never being set.

## The backlog's own claims were audited, and not all of it (2026-09-02)

Two read-only audits checked this file's absence claims — "nothing does X", "X
is unbuilt", "blocked on Y" — against the tree, because each of those goes stale
silently when the work lands and nothing recomputes prose. Eighteen were wrong
and are fixed; see `git log` for what each said.

**The failure has a shape worth knowing.** A fact that appears in three entries
is fixed in one, because entries are maintained where they are read. Every wrong
claim found was contradicted either by the tree or by another paragraph of this
same file — several of them a few dozen lines away, in the same entry. Two more
were arguments resting on a line count that had since quadrupled, and one was a
non-gap recorded expressly so nobody would re-audit it, which is what let it rot
for three weeks after the crate it described was split into ten modules.

**What was audited.** Everything from the top of the file through the
2026-09-01/02 entries, and a systematic sweep of the older half: every
backticked path resolved against the tree, and every backticked identifier of
six characters or more diffed against the identifiers in `crates`, `apps`,
`web`, `tools`, `docs/plan` and `.github`. Roughly seventy claims were opened
and read in their own file.

**What was not, and where to look first.** The sweeps above covered the whole
file; the _reading_ did not. These entries had no claim individually opened, and
are the ones flagged as most likely to yield:

- The 2026-08-04 full-codebase review, the MTL1-MTL6 and DX2/DX3 entries, the
  sample-plan audit block of 2026-08-26/27, and the slice-plan archive at the
  end of the file.

**The plan documents were audited on 2026-09-02 too, and yielded sixteen** — the
highest rate of any pass. The rule that finished work leaves the plans had not
been holding: `ROADMAP.md`'s phase table carried no marker for a sample that
shipped and is gated on every push, and named a blocker that had shipped a week
earlier; `43-render-standards.md`'s delivery table still put contact shadows at
the head of a queue they left on 2026-09-01. A delivery table is the worst place
in the repository to be wrong, because it is read as the index of what exists.

A second pass on 2026-09-02 took options' sample plan, horde's sample plan (both
since deleted), the irradiance-probe plan (since folded into
`docs/notes/rendering.md`) and `18-render-features.md`, and found seventeen
more. **The plan for a shipped sample was the worst document audited anywhere**
— options' plan still carried the heading "the audio half is built, the video
half is not" over a screen that has laid out the whole video catalogue since
2026-08-28, and two self-corrections whose referents no longer existed: one
correcting a paragraph that had itself been corrected, and one warning about a
claim below it that appears nowhere in the file. A plan written to describe work
that does not exist yet is the highest-risk document there is, because every
sentence in it is a candidate the day the work lands.

Still unaudited among the plans: `00`-`17`, `20`, `22`-`49`, `51`, `52`, and the
seventeen remaining `docs/plan/sample/` files — over 18,000 lines. `19-input.md`
and `21-jobs.md` were audited on 2026-09-02 and yielded eight, six of them wrong
on the day they were written. Nothing has re-derived horde's four measurement
tables, now in `docs/notes/samples.md`; those figures are carried on trust.

**And this file itself was swept on 2026-09-02** for entries whose subject had
shipped — one deleted, two clauses cut, five reworded. What that sweep did
**not** open, listed so the next one starts here rather than re-deriving it:

- the 2026-08-04 full-codebase review block and its per-crate findings, which
  the sweep judged the highest-yield unread region;
- `MTL2`-`MTL6`, `DX3` and the D3D12 swapchain slice (only `MTL1` and `DX2` were
  opened);
- the sample-audit entries for breakout, asteroids, horde, hud, orbit, flappy,
  sparks, puppet and lantern, and the four `## Coverage gaps in the … audit`
  runs;
- roughly 4,500 lines of WebGPU and backend-parity entries in the middle of the
  file, including "WebGPU has no blockers left";
- the ~45 bullets of "What the debug console left as limits", of which three
  were opened — the largest single entry here about work that shipped in the
  last week;
- the 2026-08-13/14 slice-plan archive beyond two entries.

**Almost no measured figure in this file has been re-taken.** The exception is
the AO tangential sweep, re-measured on both drivers on 2026-09-02 after
`38b2688` changed the pass it times — every quality figure came back identical
and the timings moved a few per cent, so the table now carries new medians and
the same conclusion. Every other table — the area-light prices, the browser pass
timings, the Pages wall-clock numbers — is still carried on trust; the sweeps
have only ever checked that the symbols and files those tables name still exist.

**What re-taking one cost**: fourteen harness runs and about a minute. The
reason to do it is not that a figure was wrong but that nothing would have said
so if it were — a shader change landed in the pass a decision rests on, and the
table went on reading as current.

**The platform-coverage entries were audited on 2026-09-02 and yielded six
more.** "Has never run" proved the most rotten shape of all, because the answer
moved into CI without anyone revisiting the prose — a claim that is still true
of this Linux machine and false of `windows-latest` reads as verified either
way. What that pass did _not_ open: P5B's browser and wasm bullets, the Win32
`ffi.rs` size and offset assertions, and the AppKit "every type encoding" bullet
against `ffi::ENC_*`.

**The one claim flagged rather than asserted has since been settled, and the
comment it rested on was wrong.** `web/tools/browser-e2e.mjs` stated that no
`gpu passes` line appears in its page log, because the gate's browser negotiates
no `timestamp-query`. It does negotiate one: the `web-e2e-quarry` artifact of
Pages run 33572221541 carries
`gpu passes (p50 / p95 over the last 53 of 53 frames): 20 label(s)`. So a caller
appending a debug-draw segment would give the browser gate something to time
after all, and the debug-draw entry does not understate its blocker. The comment
has been corrected to say what is actually true of that report — it lands only
at `Loop::finish`, so the one that survives a many-boot run is the last boot's.

## What the AppKit backend has and has not been run against

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

## Always run the browser gate with `--build`

A run without `--build` uses whatever is in `target/site`, and that directory
outlives the commit it was built from — a stale site is how a green run gets
reported for code that is red. This has cost real time once already: both demos
were signed off at "25/25" against a site built before the focus, pause and menu
work landed, and the gate had in fact been red the whole time. Pass `--build`,
or delete `target/site` first.

**There is a warning for it now**, added after this was written:
`web/run-browser-e2e.sh` runs `find -newer` against the entry point it copied
and prints "re-run with --build … before believing the result" when any source
is newer than the built site. It warns rather than fails, so the advice above is
unchanged — but a stale run now says so instead of passing quietly.

## Leak assertions that tolerated the deferred-sweep defect (2026-08-20)

The server used to ship destroyed entities to clients — `Server::tick` swept
before `GameModule::tick` and serialised after it. That is fixed. **Kept because
of how it survived: every test in reach of it asserted the wrong thing.**

- **asteroids** compared `entity_count()` against
  `1 + rocks + bullets + dead_queue_len()`, adding the queue back in. It was
  written that way _because_ the test failed when it was first written — the
  compensation was the fix. It is an exact equality now and goes red on the
  defect at ticks 18, 49 and 401.
- **horde** carried the identical `+ pending` term, with a doc comment
  explaining the old ordering as though it were intended. Exact now, and red on
  many ticks of the soak.
- **flappy**'s `a_long_run_keeps_the_world_the_same_size` asserts a `<=` ceiling
  and **cannot see this defect at all**, which is worth stating precisely
  because two plausible explanations are both wrong. It is not the `<=`, and it
  is not the sampling. Measured 2026-08-20, four ways:

  | sampling        | sweep    | peak | ceiling |
  | --------------- | -------- | ---- | ------- |
  | every 60th tick | fixed    | 11   | 11      |
  | every tick      | fixed    | 11   | 11      |
  | every 60th tick | reverted | 11   | 11      |
  | every tick      | reverted | 11   | 11      |

  Sampling every tick reads the same simulation — `travelled` came out
  bit-identical at `239.8999904040058` — and finds the same peak. **The reason
  is that the peak and the culls never coincide**: the window holds at most
  eleven entities, the count reaches eleven only between culls, and the two
  entities a deferred pipe-despawn leaves behind are therefore never added to a
  frame that was already at the ceiling. So the assertion is honest for what it
  claims (the world does not grow without bound) and is structurally incapable
  of catching a one-tick destruction delay.

  **Left unchanged deliberately.** The sampling change was written and reverted:
  it costs 2400 harness calls instead of 40 and buys nothing measurable. What
  would catch it here is asserting the destruction queue is empty between ticks,
  which flappy has no accessor for — and asteroids, horde and `crcbl-server`'s
  two new tests already catch it directly and go red on it, so a fourth detector
  is not worth new API.

The shape to carry forward: a leak invariant that _compensates_ for a queue is
one that cannot see the queue being wrong. If a count needs a correction term to
balance, the question is why the term is non-zero, not what to add to the sum.

**Considered and declined: `World::entity_count_after_sweep`.** Added while
fixing this, then removed before committing — after the sweep moved, the queue
is empty between ticks for every reader in this workspace, verified by asserting
`dead_queue_len() == 0` at every leak check across asteroids' and horde's full
suites. It had no caller but its own test. `entity_count` is exact between ticks
and its doc now says under what condition; `dead_queue_len` answers the rest.

## Deferred decisions

Confirmed records; each answer below was reviewed on 2026-09-06 and stands as
taken. The one item of real work inside this block — a headless `Audio` that
opens no stream — is in docs/backlog.md under the same heading. Questions that
came up mid-slice and were answered by judgement rather than by asking. Each is
the question, the answer taken, and **what would change it** — because the point
is that a later reader can reopen one cheaply instead of rediscovering that it
was ever a question. An entry here is not a complaint about the answer; most of
these are probably right.

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
  under its own `assets/` and register it into the UI pass's image atlas as a
  `MenuSkin` of its own; the shape for that is a function beside
  `crcbl_render::menu_skin` taking a `Sheet`, not a fork of this one.

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
  dimmed** by a scrim the menu draws first. A frozen screenshot would need a
  captured frame and a second code path; a menu with nothing behind it loses the
  player's place. The scrim was a sprite in the menu's own pass while the UI
  pass could not draw a picture; since the UI pass drew the menu (2026-09-15) it
  is the first image quad `Menu::render` pushes, ahead of the frame and the
  labels, which is what keeps it from dimming them. _Changes it_: a menu that
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
  _The cost paid_: the caller owned the ordering, because `RenderGraph` runs
  passes in declaration order and the sprite pass carrying a skin had to precede
  the UI pass carrying its label. **Superseded 2026-09-15** by
  `docs/plan/07-ui-debug.md` rung 1: the UI pass grew the RGBA image atlas and
  the textured quad this decision declined, because the plan's styled widgets
  need colour art interleaved with text — the _changes it_ this entry named. A
  button skin is now `crcbl_ui::ButtonSkin`, drawn into the same draw list as
  its label, and the menu's sprite pass is gone.

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

- **Was the 2026-08-01 review document a record or a description of current
  state?** Taken 2026-08-10: **a record**, left unedited. **Reversed 2026-08-22
  by the user: aggregated into this file and deleted** — see "The 2026-08-01
  full-workspace review, aggregated" above. The reasoning below is kept because
  it is why the file survived as long as it did. It is dated 2026-08-01, was
  added in one commit and never amended, and the roadmap already says its
  findings were fixed across eight commits. Several of its findings now describe
  code that no longer exists — the `paddle_model` finding is the clearest, since
  breakout has no forward pass at all. _Changes it_: a decision to keep it live,
  which would mean re-running the review rather than patching the findings that
  happen to have been noticed.

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

Record; the coverage gap it left is in docs/backlog.md under the same heading.
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

**Re-triaged against the tree on 2026-09-02**, every claim in this list. The
heading no longer holds for all of it: three claims were **never accurate** and
say so in place (vk's `untag`, the QOA saturation, render's "documented"
allocations), several the tree has since answered were deleted, and four were
**defects rather than fragility** — all four shipped on 2026-09-03 (vk's
`submit` not checking the command buffer's queue family, `write_buffer`'s memory
rule differing across backends, view-format compatibility being three rules
across four backends, and the missing re-handshake after a forged `Accept`). The
rest of the list is unchanged in kind, sharpened where the triage found the
original wording named the lesser half of a problem.

- **net**: `baseline_tick = 0` is wire-ambiguous (delta.rs:824/866-869;
  unreachable — the server never encodes against tick 0); a forged `Accept` is
  still accepted, because the handshake reply is unauthenticated by design —
  what shipped is the **recovery**, `Client::expire_unproven_session`, so the
  wedge is bounded rather than permanent; `Reject` `msg_len` is u16 with a
  silent cast on encode (codec.rs:399); key rotation on reconnect trusts a
  cleartext token (documented); reject messages disclose server identifiers
  pre-auth.
- **vk**: the acquire path waits with `u64::MAX` _while holding the device lock_
  — a compositor that never returns an image hangs every device call, and
  **both** waits are under it, the armed fence and `acquire_next_image` itself;
  the code comment shows only the unarmed-fence case was considered;
  semaphore-reuse safety depends on the `slots = image_count + 1` throttle — and
  the in-code comment claiming the acquire fence is what makes reuse provably
  safe overstates it: the fence proves the signal completed, not that the
  caller's submit-side wait retired. The `untag` claim was **never true**:
  `Handle` stores a `NonZeroU32` generation behind a private constructor, so no
  handle can carry generation 0 and `from_bits` cannot fail — the index is
  masked, not checked.
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
- **hal**: the reference frame destroys the command buffer right after present
  (wrong pattern to copy); `query_results` "returns zeros without
  TIMESTAMP_QUERY" is unreachable (create_query_set errors first); `present`'s
  queue must be present-capable but the seam never says so; `AcquiredFrame`
  carries no swapchain identity.
- **null**: `crcbl-dx12` and `crcbl-mtl` keep hand-rolled copies of the
  subresource rule that `ImageViewDesc::check` already owns. (The view-format
  disagreement that stood beside this is settled — see the cross-format entry
  above.)

  The rest of this bullet was about **`crcbl-wgpu`, which was deleted on
  2026-08-21** — unclosed passes no-opping, `checked()` routing, abandoned
  encoders leaking a pool entry, the wgpu/null format disagreement,
  `SwapchainSlot::suboptimal`. Those name nothing now. A few were written
  without saying which backend they were about (`set_scissor` at `i32::MIN`,
  `create_buffer` within 3 bytes of `u64::MAX`, `copy_layout`'s `bytes_per_row`
  wrap, two pending signals of one timeline value). Re-derived against `null`:
  **`set_scissor` at `i32::MIN` is a real three-way divergence** — `null`
  records it unchecked, `crcbl-vk` passes it to `vkCmdSetScissor` (a VUID
  violation), `crcbl-mtl` clamps negatives to zero and `crcbl-dx12` refuses it,
  and the seam documents no rule. **`create_buffer` near `u64::MAX` aborts**:
  null's mappable path is `vec![0u8; size]`, which aborts the process where the
  trait doc promises `OutOfDeviceMemory`. **The `copy_layout` claim names
  nothing** and is dropped — no `copy_layout`, `bytes_per_row` or `row_pitch`
  exists in `crcbl-hal` or `crcbl-webgpu`, the seam's field is
  `BufferImageCopy::buffer_row_length`, and null does no size arithmetic in
  `copy_buffer_to_image`. **The double timeline signal is answered**: null's
  `submit` keeps an `advanced` list and refuses a second signal that does not
  move past the first.

- **render**: cross-frame mixed-state transient handoff (single-mip production
  transients only); cross-frame queue-ownership release dropped (no second queue
  in use); `begin_frame`'s `atlas` argument is layout-only; pool transient view
  covers every mip; per-frame CPU allocations are small but **not** documented —
  `graph.rs`'s "a plain index keeps compilation allocation-free" is false, since
  every `create_image`/`create_buffer`/`add_render_pass`/`add_compute_pass`
  allocates a `String` per frame and `compile` allocates several `Vec`s;
  `upload_texture`'s expected-size math can overflow u64 and the "unreachable"
  qualifier is **wrong** — `texture::upload` computes `expected` before
  comparing against `pixels.len()`, so a `u32::MAX` extent with an empty slice
  panics in dev builds instead of returning the documented `InvalidDescriptor`.
- **core/ecs/input**: wrong-kind bindings silently produce permanently idle
  actions — the "user-profile typo" vector named here does not exist (there is
  no keybinding file), but the debug console is one: `apply_bind` only ever
  installs `Binding::Key`, so binding an `Axis2` action to a key succeeds,
  echoes back, and leaves a dead action; `set_enabled(true)` doesn't resolve
  immediately (deliberate, pinned by a test); `with_capacity(usize::MAX)`
  overflow is pre-empted by the vec capacity check.
- **phys**: `world_mut()` lets a caller desync `collider_to_entity` — and it has
  **zero callers in the workspace**, so deleting it is the whole fix;
  `ThrustForce` fields are pub (unnormalized direction silently scales thrust);
  negative collider radii bypass the constructors — whose guards are
  `debug_assert!`, so a release build takes one through the front door too and
  `Aabb::is_empty` then drops the collider from the BVH; per-tick `Vec<Entity>`
  in `step` is negligible.
- **audio/store**: `opfs.rs` write-before-ready can be replaced by a later
  generation restore — `write` does not gate on `inner.restored` the way
  `delete` and `list` do, and seeds `(seq, slot)` at `(1, 1)`, so a disk file at
  a higher generation silently overwrites the newer local write, and `write`'s
  own doc claims the opposite; `settings.rs get` falls through on a type error
  in a hand-edited file; `voice_mixes()`/`voice_count()` take the audio thread's
  mutex (HUD polling can stall audio, and `voice_mixes` allocates while holding
  it). The QOA saturation claim was **never true**: `prediction` is shadowed by
  `prediction >> 13` on the line above, so the sum cannot reach the saturation
  point, and the two places that genuinely rely on C `int` wrapping already use
  `wrapping_add`/`wrapping_mul` with a comment saying why. This decoder cannot
  differ from a conforming one on any input.
- **ui**: `FrameStats::with_window` aborts on a huge caller-supplied window;
  public float style fields are unclamped (0/negative → inverted geometry);
  `Text` top-left-anchor holds only for the built-in metrics; trailing-newline
  labels measure one line too tall; per-frame allocations are documented.
- **sprite/wl-scanner**: JSON recursion depth (~30-50k nested objects overflow
  the stack; sidecars trusted); `emit::KEYWORDS` omits `self`/`Self`/`super`
  (loud compile error, not silent mis-generation) — `union` is contextual and a
  legal identifier, so that half was never a gap, and the real defect is that
  `KEYWORDS` _contains_ `"crate"`, which has no legal raw form; `worst_pixels`
  collects all differing pixels then truncates (up to ~230 MB on an
  all-different 4K frame); `escape_ident`/`camel_case` collisions name the
  generated file, not the XML line.
- **cli/engine**: `channel_order`'s `_ => Rgba` arm would silently mislabel a
  future non-8-bit format (unreachable today); F11 toggle runs before the
  `destroyed` check; the pointer hit-test runs before `draw_menu`
  (one-frame-late menu clicks — **deliberate**: `engine.rs`'s `showing` binding
  is commented "last frame's menu, deliberately", so this is a wording fix, not
  a behaviour one); failed `PendingGpuContext`/`GpuContext::finish` drops
  surfaces without `destroy_surface` (vk cleans up with a warning);
  `request_open`/ `start_device` accept a (0,0) extent (swapchain creation fails
  loudly); sandbox `--frames 0` accepted while bare rejects it; sandbox's
  `--backend` usage text now names every backend and is guarded by a test, but
  the `ENVIRONMENT` block's `CRCBL_GPU` line still says "(vk, null)" and the
  guard does not scan it.
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
- **`crates/crcbl/src/engine.rs`'s test module** and
  `crates/crcbl-ecs/src/{world,schedule}.rs` internals were not read by any
  review pass. The line range this used to cite no longer points at that module
  — `engine.rs` has more than doubled since — so the gap has to be re-derived
  from the file rather than followed.
- **`crates/crcbl-net/fuzz/corpus/`** binary seeds — exercised via
  `include_bytes!`, not read as code.
- **wgpu internals** (the wgpu/wgpu-core dependency) were consulted for specific
  claims (resolve_target, tight packing, output_buffer_size) but not audited.
- No build/test run was performed during the review passes (read-only
  constraint); every finding above is static-verified. The project's CI gate
  (`cargo fmt/clippy/build/nextest`) was not run as part of this review.

## Test-file names: what the rename slice left, and one rename declined

Record; what is left unclaimed is in docs/backlog.md under the same heading. The
naming slice took `docs/plan/12-testing.md`'s "filenames name the subject, never
the taxonomy tier" and applied it to nine files. What it could not close, and
one thing it deliberately did not do:

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

**The stale path references this entry listed are all fixed** — verified
2026-08-23: `broadphase.rs` and `docs/plan/12-testing.md` name
`crates/crcbl-phys/tests/broadphase_churn.rs`, `forces.rs` names
`crates/crcbl-phys/tests/dynamics.rs`, and `crcbl/src/engine.rs` names
`crates/crcbl/tests/seam_from_outside.rs`. Each was prose in a code span rather
than an intra-doc link, which is why `cargo doc` stayed green throughout and
nothing but a reader was ever misled.

- The review cited `crates/crcbl-server/tests/integration.rs:15` and
  `crates/crcbl-audio/tests/orbit.rs:191`; both files are gone (the first
  replaced by `client_server_session.rs`, the second by `spatial_chain.rs`).
  Settled by deleting the review — see the aggregated section above.

### What the non-backend test-name rename left behind

The prose-sentence rule in `docs/plan/12-testing.md` was applied to every test
name of three words or fewer outside the backend crates. Measured with a
`#[test]`/`#[tokio::test]` extractor over the whole tree: 138 such names before,
1 after. The `debug_format` bullet and the `ray_misses_aabb`, `decode_empty`,
`decode_truncated`, `debug_output` and `debug_formatting` copies named in "Exact
test-name collisions still open between non-backend crates" above are resolved;
the rest of that entry's list still stands.

- **Closed: the stranded `orbit_integration_deterministic` citation.** The test
  is `an_orbit_hashes_the_same_twice_and_differently_in_reverse` in
  `crates/crcbl-audio/tests/spatial_chain.rs`; the old name exists nowhere. The
  review that cited it was wrong in three ways at once by the end — the path (an
  `orbit.rs` that no longer exists), the name, and the finding itself, which
  said the test XORs per-block hashes and is order-insensitive when it feeds one
  hasher in block order and asserts the reversed order hashes _differently_.
  Deleting the review closed all three.

  **The method is the part worth keeping.** It was found by sweeping every
  backticked symbol in this file against the tree — a check
  `tools/check-doc-citations.sh` does not make, since it resolves paths and not
  names. That sweep is worth re-running after any rename: it also caught
  `draw_pause_menu`, deleted long before the entry naming it.

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

### The duplicate-name census, and why most of it stays

Measured 2026-08-22 by extracting the `fn` after every `#[test]` in the tracked
Rust and grouping by name: **4882 test functions, 4691 distinct names, 115 names
carried by more than one test.** Grouping each duplicated name by the crate or
app its copies live in:

- **53 across the sample apps** —
  `a_paused_game_shows_the_pause_menu_and_nothing_else` in all four of
  `asteroids`, `breakout`, `flappy` and `horde`,
  `the_art_bakes_to_the_sheets_it_declares` in the same four, and so on.
- **35 inside one crate, across its modules** — `crcbl-shell`'s `appkit` and
  `win32` halves account for most of them (`control_characters_are_not_text`,
  `two_windows_resizing_do_not_collapse_into_each_other`), `crcbl-jobs`'
  `assert_send` across four containers for the rest.
- **21 spanning unrelated units**, listed below.
- **6 across the GPU backends**, which `docs/plan/12-testing.md` already
  governs.

**So the great majority is deliberate parallel structure and renaming it would
be a loss, not a fix.** One contract instantiated per sample, per platform half
or per container is exactly the shape that should share a name: the name is the
contract, and making the copies differ by a suffix would hide that they are the
same claim. This is the opposite conclusion from the `crcbl-shaders` rename
recorded above, and the difference is real — those copies each read a
_different_ source file and asserted _different_ offsets, so one name over
several claims was wrong there.

**What the duplication does cost** is already recorded under "Declined:
extending the citation gate from paths to symbols": a bare test name quoted in
prose does not identify a test, and `cargo test <name>` runs all the copies.
That is the price of the parallel structure and is accepted.

**The residue worth a second look** — pairs that share a name by coincidence
rather than by contract, where the two tests assert unrelated things:

- `every_index_is_in_range` — `apps/quarry/src/face.rs` and
  `crcbl-shaders/src/mesh.rs`.
- `every_normal_is_unit_length` — `apps/quarry/src/face.rs` and
  `crcbl-greybox/tests/greybox.rs`.
- `the_entry_points_answer_zero_until_a_source_is_installed` —
  `crcbl-audio/src/web.rs` and `crcbl-store/src/web/fetch.rs`.
- `the_sections_labels_are_its_own` — `crcbl-render/src/counters.rs` and
  `crcbl-ui/src/budget.rs`.

None is a defect and none is scheduled; they are named so the next sweep does
not have to re-derive which of the 115 are accidents. The other 17 mixed groups
are contract-sharing after all — the sample apps against `crcbl-ui`'s own menu
tests, `crcbl-client` against `crcbl-server`, `crcbl-hal` against
`crcbl-shell`'s matching error convention.

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

### What the mesh-shader path owes, now that it draws

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
sentence per link. That entry stands below as "`cargo doc` without features is
red, and nothing runs it"; the two disagree only on the word _defect_ — the fact
is one, and the gate is `--all-features`.

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
