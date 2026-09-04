# Sample Projects Overview

Small, complete, playable projects built on the engine. Each sample is the proof
artifact for one or more stage exits — a stage isn't "done" when its sandbox
demo runs; it's done when the sample that depends on it ships.

Samples live in `apps/` alongside the infrastructure crates that are not samples
and are not on this ladder: `apps/bare`, `apps/sandbox` and
`apps/render-harness`. `apps/editor` is deliberately absent until the editor
phase, and the workspace `Cargo.toml` says so where someone looking for it would
go. Samples are kept in the repo, kept building in CI, and kept small — a sample
that grows features beyond its charter gets its scope cut, not the engine bent
around it.

## Why multiple small samples instead of one big one

- One big game exercises systems _together_ but hides which system is weak.
  Small samples isolate: if `asteroids` is hard to build, entity churn is the
  problem, not "the engine".
- Each sample doubles as living documentation: "how do I make a 2D game" has an
  answer that compiles.
- The ladder gives every stage a playable milestone — motivation and regression
  suite in one.

## The ladder

Sample numbering **is build order** for 01–11 (see
[../ROADMAP.md](../ROADMAP.md) — S-phases); the table is ordered by gate, and a
sample allocated after the ladder was written keeps the next free number rather
than renumbering the rest. `12-flappy` is the first of those — allocated last,
built second. Every sample starts only when all its engine dependencies exist,
ships with spatial audio, and publishes as a wasm demo on the GitHub Pages site
— viewer included, since 2026-08-24.

| #   | Sample                       | Roadmap gate                 | Proves                                                                                                             |
| --- | ---------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| 01  | [breakout](01-breakout.md)   | S1 (P0–P4A)                  | 2D path, minimal ECS, in-memory server loop, game UI, swept-sphere CCD, panning audio                              |
| 12  | [flappy](12-flappy.md)       | S1B (needs nothing past P4A) | That the engine has no breakout-shaped assumptions; procedural churn, one-input latency, seeded determinism        |
| 02  | [asteroids](02-asteroids.md) | S2 (P5–P6)                   | Entity churn, generational ids, broadphase churn, segment CCD, first forces                                        |
| 03  | [horde](03-horde.md)         | S3 (P7–P8)                   | GPU-driven renderer at scale, flat CPU cost claim, 10k-body queries                                                |
| 04  | [hud](04-hud.md)             | P4 skeleton → P10 done       | Pure UI demo: CSS HUD + widget gallery + themes; the UI system's living fixture                                    |
| 05  | [viewer](05-viewer.md)       | S4 (P9–P10)                  | Asset pipeline as a _usable tool_, camera, inspector panels                                                        |
| 06  | [orbit](06-orbit.md)         | S5 (P11)                     | Physics acceptance test: sector space, orbits, drag, CCD, on-rails handoff                                         |
| 07  | [towers](07-towers.md)       | S6 (P12–P13)                 | **Flagship**: everything — editor content, co-op multiplayer, browser client, esports audio cues                   |
| 08  | [arena](08-arena.md)         | post-MVP                     | Client prediction driver — pulls netcode forward; audio grammar under fire                                         |
| 09  | [puppet](09-puppet.md)       | post-MVP wave 1              | Skeletal animation acceptance test; shadows + device-swap input showcase                                           |
| 10  | [sparks](10-sparks.md)       | post-MVP wave 1              | VFX gallery + live workbench; GPU particle pipeline fixture                                                        |
| 11  | [breach](11-breach.md)       | FPS-era                      | **FPS flagship**: web slice first, then native 5v5 comp — prediction/lagcomp, ballistics, FP rendering, integrity  |
| 13  | [lantern](13-lantern.md)     | S4B (P7B–P7C)                | Lighting acceptance: the same scene under `RayTraced` and `Rasterised`, every effect, both paths side by side      |
| 14  | [quarry](14-quarry.md)       | S4C (P7)                     | Geometry acceptance: meshlet clusters, QEM cluster LOD, and all three `GeometryPath` values on one scene           |
| 15  | [shard](15-shard.md)         | S6B → wave 2                 | **MMO flagship**: web slice first, then a native persistent world — sector streaming, interest management at scale |
| 16  | [bracket](16-bracket.md)     | P13                          | Matchmaking + rating + ranked auth as a service, with no game attached; the only genuinely networked web client    |
| 17  | [mirrors](17-mirrors.md)     | S4D (P7B–P7C)                | Reflection ladder: every reflection technique the engine ships, compared side by side from one frame               |
| 18  | [sundial](18-sundial.md)     | S4D (P7B–P7C)                | Shadow ladder: every filter, a moving sun, and somewhere for each named shadow artefact to appear                  |
| 19  | [alcove](19-alcove.md)       | S4D (P7B–P7C)                | AO ladder: every occlusion technique, an AO-only view, and flat surfaces that hide nothing                         |
| 20  | [options](20-options.md)     | S4E (P10)                    | Settings acceptance: the whole catalogue on a screen, saved and reloaded, on desktop and in a browser tab          |

01–06 stay tiny (days, not weeks, each; hud is continuous — a P4 skeleton that
grows until P10). 07 is the MVP-era flagship and long-lived dogfood. 08 exists
to force post-MVP netcode work and is explicitly not started before MVP ends. 11
is the **FPS-era flagship** — the biggest sample by far, consuming topics 26–31
as one game; it starts only after arena has proven prediction/lag comp.

13 and 14 are **acceptance fixtures** rather than games, in the shape hud
already established: lantern owns lighting and quarry owns geometry, each
proving that every path renders correctly before a game depends on it.

17, 18 and 19 are a **second wave of fixtures, and they ask a different question
from lantern's**. Lantern asks whether an effect is on and whether both lighting
paths agree; these three ask which of several algorithms for one effect is
better, and at what cost. That is a comparison rather than a toggle, so it needs
what lantern does not have: two techniques resolved from the same frame, a split
screen, and a per-technique timer. They exist because
[../18-render-features.md](../18-render-features.md) grew a ladder of rungs per
effect — reflections, shadows and ambient occlusion each have more than one
answer now — and rule 13 says a technique with no sample is a technique nobody
has used. 20 is the **settings fixture**, and its subject is a round trip
nothing in the workspace closes today: an application writing a player's choice
to disk and reading it back. 15 is the **second flagship** and the one the demo
site leads with, since breach's competitive game never ships to a browser. 16 is
a **tech demo with no game in it at all** — matchmaking, rating and ranked auth
in isolation, which is the only way a matchmaker can be evaluated without a real
playerbase.

**Where the ladder stands.** Every sample on it except towers (07), arena (08)
and mirrors (17) has an `apps/` crate that builds for `wasm32`, and every one of
those but sundial (18) ships on the demo site; `web/build.sh`'s `DEMOS` array is
the list, and it is the authority — a sample missing from it is a sample nobody
visits. What each of those crates has and has not built is in its own doc's
status section and, fresher than either, in the module header of its
`src/lib.rs`. **towers, arena and mirrors have no `apps/` directory at all**,
and each doc says what it is waiting on — for 17 that is the ladder itself,
since a comparison fixture holding one technique is not a comparison. **alcove
(19) is the first of the comparison wave to ship**: `apps/alcove` was built
natively on 2026-09-04, once the occlusion chain had a second technique to
compare, and its browser demo landed the same day at `/demos/alcove/` — the one
page on the site whose controls are HTML rather than keys, because a seam walked
with `,` and `.` is a comparison a phone cannot reach. **sundial (18) followed
it natively the same day**, once the shadow ladder had three filters and a seam
to compare them across; `apps/sundial` builds for `wasm32` and is not on the
demo site yet, and `docs/backlog.md` carries what a page owes. options (20)
shipped on 2026-08-28.

**Where multiplayer lives — and where it does not.** Native sessions are LAN:
direct connect by IP, or a lobby browser over local-network host discovery.
**Every web build is single player, without exception.**

That is a decision rather than a limitation discovered late, and the reasoning
belongs here because it constrains every future sample. A browser cannot listen
on a socket, cannot discover hosts on a local network, and cannot open an
insecure connection to a LAN address from an HTTPS page. So a browser client can
only reach a server somebody is paying to host — and the project hosts nothing.
WebRTC was considered as the way around it and declined: it still needs a hosted
signalling broker, it still cannot discover LAN hosts, and talking to browser
peers would put a full WebRTC stack on the native side, which is a framework of
exactly the kind the dependency policy in
[../15-windowing.md](../15-windowing.md) rejects.

The consequence, recorded so nobody re-derives it: **WebTransport and WebSocket
are not in the plan.** The transport surface is UDP and in-memory. See topic
23's LAN correction.

**Browser multiplayer is deferred, not refused.** One route survives the
no-infrastructure constraint — WebRTC data channels with manually exchanged
connection codes, no signalling server, browser to browser only — and it is
recorded in `docs/backlog.md` with its costs so the decision can be reopened
without re-deriving it. It is not in the plan today.

## Rules for all samples

1. **Engine API only.** Samples never link `crcbl-vk`/`crcbl-webgpu` directly,
   never reach around the facade. A sample needing a backdoor = engine API gap =
   engine work item, filed and fixed in the engine.
2. **Server-authoritative always.** Even breakout runs client+server over the
   in-memory transport. No sample gets a "simple mode" that bypasses the
   architecture — the architecture is what's being proven.
3. **Scenes from files** (once stage 6 exists): samples load `.scn/` scene dirs,
   and after stage 8 their scenes are maintained _in the editor_. Hand-edited
   scene files after that point are a smell.
4. **Debug overlay on by default in dev builds, and switching it on is one
   thing.** Not a HUD each sample writes: one **modular** panel, where frame
   timing and FPS are always present and each further system contributes its own
   module — network stats appear because the sample has a connection, not
   because the sample asked for them. A sample that has no connection shows the
   panel without that module and is the check that the modularity is real.
   Samples are also the test bed for the debug tools, so a sample that cannot
   turn the panel on is a finding about the panel.
5. **CI-built, clippy-clean, same bar as engine crates.** Playtest scripts
   (input-script determinism runs from stage 4) where feasible.
6. **Scope charters are hard caps.** Each sample doc lists non-goals; feature
   ideas beyond them go to the flagship (07) or die.
7. **Web demo on the ladder.** Every game sample builds for wasm and deploys to
   the GitHub Pages demo site as part of its exit criteria. A sample that breaks
   the wasm build breaks CI.

   **Viewer's exemption was taken and then given back.** It was exempt as a
   native tool with a web build as a stretch; `apps/viewer/src/web.rs` and
   `web/demos/viewer/` exist, viewer is in `web/build.sh`'s `DEMOS`, and the
   page opens a document the module carries plus any `.glb` dropped onto the
   canvas. So the ladder has no exemption left standing.

   **The two flagships are built web slice first, native full version after.**
   breach (11) and shard (15) each ship a reduced single-player cut to the
   browser before their native game exists, because the browser runs the
   fallback paths — rasterised lighting, `IndirectPerBatch`, `ArrayPages` — and
   a fallback proven after the fact is a fallback nobody proved. The native
   milestone then layers ray tracing, meshlets and networking on top as a
   capability upgrade rather than a rewrite, which is the claim
   [../39-capabilities.md](../39-capabilities.md) makes.

   **breach's competitive game is native only**, and that is a scope decision
   rather than a degradation: anti-cheat, raw mouse input and an unreliable
   channel are things a browser cannot honestly provide, so a browser build of
   it would be a claim the platform cannot back. Its reasons are in its own doc.
   No other sample gets this exemption without the same kind of argument.

8. **Sound through `crcbl-audio`, spatial by default.** Positional game events
   use the cue grammar (topic 13) — samples are how players (and we) learn the
   grammar. No sample ships silent after P4A.
9. **All collision and motion through `crcbl-phys`.** No game-code collision
   math, ever — even breakout's ball reflection is an L0 sweep + contact normal.
   Physics is built **interleaved with the samples, demand-driven**: each sample
   doc names the physics slice it drives (breakout → first L0 sweep vertical;
   asteroids → broadphase churn + segment CCD + first forces; horde → query
   scale; orbit → full L1; towers → CCD vs moving targets, triggers, character
   controller). A physics feature no sample demands is a feature built too
   early.
10. **Game logic through the module API** (topic 16). Sample gameplay code
    implements `GameModule` (static binding for dev; breakout also ships as
    `.wasm` for the P6A equivalence gate). Engine-internal systems stay native;
    sample code is module code — proving the API games and mods will live on.
11. **Pixel art through the sprite system, for every sample that should have
    any.** Authored as `.crpix` text under the sample's `assets/`, baked at
    build time to PNG + sidecar by its `build.rs`, drawn through
    `SpriteRenderer` with `SampleMode::Pixel`. Nothing baked is committed — the
    text is the only source of truth, which is what makes the art reviewable in
    a diff. "Untextured quads for now" is not an available answer: breakout and
    flappy both gave it, and P4B is what unwinding it twice cost.

    **"Should have" is the exemption, and it is narrow.** A sample whose subject
    is not pictures does not fake one: hud is a widget gallery, viewer opens
    arbitrary glTF, sparks is a particle workbench. Everything else on the
    ladder is 2D or has 2D chrome. A sample claiming the exemption says so in
    its own doc and says why.

12. **Every sample runs on every path the device offers, and says which it
    took.** `GeometryPath`, `BindingModel` and `LightingPath`
    ([../39-capabilities.md](../39-capabilities.md)) are selected from device
    capability and degrade downward; a sample that only ever runs on the best
    one is how a fallback ships untested. Concretely: the selected paths appear
    in the debug panel and in the headless summary line, every sample accepts a
    flag forcing a lesser path, and each sample's CI run exercises at least the
    path its runner selects plus one below it.

    **This is the rule that keeps the web build honest.** The browser has no ray
    tracing, no mesh shaders and no bindless, so the lesser paths are not a
    theoretical fallback for old hardware — they are what every browser visitor
    and every Apple machine actually runs.

13. **Between them, the samples cover every engine feature.** A feature with no
    sample is a feature nobody has used, and the ladder is the list of what has
    been. When a topic lands, either an existing sample adopts it — named in
    that sample's doc — or the topic says which new sample proves it. A topic
    that can name neither is not ready to be built.
