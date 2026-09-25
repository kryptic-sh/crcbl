# Tooling — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

## What the deleted 06-assets-scenes plan left behind (2026-09-25)

Record; stage 6 designed how content gets into the engine: open source formats
in, engine-owned cooked formats out, stable asset ids, and a scene format the
editor and git can both live with. Built from it: `.gitattributes` with LFS
deliberately off; `crates/crcbl-assets` (`AssetId`, `crcbl_core::Handle<Asset>`,
the `Loading | Ready | Failed` states in `AssetRegistry`, the `AssetSource` seam
with `DirSource` and `MemorySource`); glTF parsing and validation
(`crcbl_scene::{gltf_import, gltf_check}`) and the join to the renderer
(`crcbl_scene::gltf_render`), which `apps/viewer` draws; the `.scn/` directory
(`crcbl_scene::scn`) with its three writer properties in
`crates/crcbl-scene/tests/scn_roundtrip.rs`, read by `apps/breakout` and
`apps/puppet`; the reporting half of `crcbl import`; and a viewer-local polled
reload (`apps/viewer/src/watch.rs`).

What it left unbuilt is in `docs/backlog.md`: engine hot reload under _Asset hot
reload: two polled watches, and no engine reload path_; the sidecar GUIDs,
`FetchSource`, GPU retire and the `std::fs` gate under _`crcbl-assets` after
stage 6 task 2_ and _Sidecar meta RON: three items want it, and nothing writes
one yet_; and under _What the deleted 06-assets-scenes plan left unbuilt_,
`crcbl bake` with `PackSource`, the cooked mesh and `import --out`, the scene
format's editor leftovers, and the Sponza-class exit.

Code cites the plan as "stage 6" or "topic 6", by task (also called step) and by
section. Those resolve here:

| Citation                                      | What it specified                                                                                  | Status                                                                     |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Task 1                                        | `.gitattributes` and LFS                                                                           | Built, with LFS off (**LFS is off on purpose**)                            |
| Task (step) 2                                 | `AssetId`, the handle, the load states, `AssetSource`, `DirSource`                                 | Built (`crcbl-assets`)                                                     |
| Task (step) 3                                 | glTF import into the GPU pools: meshes, materials, textures, mips                                  | Built through `gltf_import` and `gltf_render`; mips per **Mips at import** |
| Task 4                                        | The `.scn/` directory, the deterministic writer, the roundtrip property tests                      | Built (`crcbl_scene::scn`); dirty-chunk tracking owed                      |
| Task (step) 5                                 | The watcher and the reload paths for assets, shaders and scene chunks                              | Owed; the viewer has its own polled reload                                 |
| Task 6                                        | `crcbl import` wiring and `crcbl bake`                                                             | The report is built; `--out`, `bake` and `PackSource` owed                 |
| Task 7, the Sponza exit                       | A real glTF scene through a `.scn/` directory at stage 3's performance targets                     | Owed                                                                       |
| Format matrix                                 | **Source formats are open standards; cooked formats are ours**                                     | Rule                                                                       |
| "Scene format: directory of chunk files"      | **A scene is a directory of chunk files**                                                          | Built                                                                      |
| "Deterministic writer"                        | **The deterministic writer**                                                                       | Built                                                                      |
| "Command journal", "Scaling", "Bake"          | The editor's autosave journal, sector-sharded chunks, the shipping blob                            | Owed                                                                       |
| Asset model, refcounted release               | Dependency tracking, and GPU retire through the stage 2 deletion queue                             | Refcount built; retire owed                                                |
| The risk section                              | **Unsupported glTF features log and skip, loudly**; hot reload's correctness bar                   | Rule                                                                       |
| The Corrections section (2026-07-27)          | **`AssetId` comes from a sidecar GUID**; the sidecar's import settings; the sRGB mipgen view alias | GUID owed; the alias is under **Mips at import**                           |
| "No synchronous IO anywhere in engine crates" | **Engine crates do no synchronous IO**                                                             | Kept by construction; the CI deny is owed                                  |
| `format: 0`, "every format the engine owns"   | **Every format the engine owns is v0 until 1.0**                                                   | Rule                                                                       |

The rules, each with its _why_:

- **Source formats are open standards; cooked formats are ours (locked
  2026-07-27).** A source format is never invented where an open standard
  suffices, and an own format exists only where the engine owns the semantics
  (scenes) or the runtime layout (cooked output). Cooked output is a build
  artifact and never tracked.

  | Asset     | Source (tracked)                   | Cooked (shipped, web)                          |
  | --------- | ---------------------------------- | ---------------------------------------------- |
  | Mesh      | glTF 2.0 (`.gltf`, `.glb`)         | packed binary matching the GPU pool layout     |
  | Skeleton  | glTF skins                         | cooked joint tables                            |
  | Animation | glTF animation channels            | sampled, compressed curves                     |
  | Texture   | PNG                                | KTX2 and Basis later; PNG passes through today |
  | Audio SFX | WAV (PCM)                          | QOA                                            |
  | Music     | WAV, FLAC                          | Vorbis or Opus, behind a decoder seam          |
  | Scene     | own: `.scn/`, RON chunk files      | a packed blob from `crcbl bake`                |
  | Config    | TOML (`crcbl.toml`, tuning tables) | as-is                                          |

- **RON for scene chunks and entity data, TOML for flat config.** Scene data is
  nested and enum-heavy (collider shapes, component variants), which RON maps
  onto Rust enums natively and TOML can only spell as stringly `type = "…"`
  tables. TOML wins where the data is flat and ubiquity matters. Both carry
  comments.
- **LFS is off on purpose.** Everything binary is small, golden images are
  re-blessed often (which LFS handles worse than plain git), and a `filter=lfs`
  line breaks `git commit` outright on a clone without the git-lfs binary. The
  glTF corpus arrived without it: `tools/fetch-shelf.sh` fetches the Khronos
  shelf at a pinned commit against `apps/viewer/assets/shelf.sha256`. **Whenever
  LFS is turned on, every `actions/checkout` step gains `lfs: true` in the same
  commit**, or CI silently tests pointer files.
- **A scene is a directory of chunk files.** `scene.ron` is the header
  (`Scene(format, name, systems)`, the manifest read in file order), `env.ron`
  the camera the scene opens on and its ambient light, and `sys/<name>.ron` one
  system's entity array (`Chunk(system, entities)`, whose `system` must match
  the file it was read as). A directory rather than a document because the two
  things a single file makes fight both matter: a save rewrites only the chunks
  that changed, and two people editing different systems merge with no conflict.
  Entities are `(system → data)` arrays mirroring ECS registration, so scene,
  ECS and replication keep one shape. Every serde type sets
  `deny_unknown_fields`, so a typo is a line and a column rather than a silently
  defaulted value, and a manifest name with no registered codec is an error, not
  a skip.
- **The deterministic writer.** Canonical field order, entities sorted by stable
  id, **stable ids persisted in the file and never regenerated on save** (the
  Unity and Godot diff-noise bug), no timestamps and no editor-session state.
  Floats are shortest-round-trip, and **no custom float writer exists or is
  needed**: ron 0.12's serializer writes Rust's float `Display`, which is
  shortest-round-trip out of `core`, not out of a platform libm. The trap that
  follows: a hand-typed `7.8000000000000005` is written back as `7.8`, so **a
  committed chunk is generated by the writer and maintained that way**, the
  discipline `crcbl_inventory::catalog`'s `CANONICAL` and
  `apps/breakout/src/scene.rs` encode. Any new serialized type must keep the
  byte-stable property, and the property tests are the gate, not review.
- **`SceneEntityId` is the file's id, not the runtime `Entity`.**
  `crcbl_core::Pool` has no insert-at-index, so an `Entity`'s bits are a
  function of spawn history, and persisting them would mean regenerating ids on
  save. `scn::IdMap` (a `BTreeMap`, so its order is the file's) is the
  correspondence while the scene is loaded. `crcbl-ecs` gains no `serde`: the
  bound sits on `scn::chunk_of::<T>(name)`, and systems are reached by name
  because the manifest names files.
- **Engine crates do no synchronous IO.** `Scene::save` returns the text of each
  file keyed by its path and the caller writes it; `Scene::load` and
  `import_gltf` read through `&dyn AssetSource`. That is also what lets the
  writer's tests run without a directory, and why `crcbl-scene` never enables
  the `gltf` crate's `import` feature (blocking `std::fs` and a second image
  decoder).
- **Loading is asynchronous from day one.** A browser has no blocking
  filesystem, so `AssetSource::read` is defined never to block: a source without
  the bytes answers `StorageError::Pending` and the caller polls. That is
  `crcbl_store::web::FetchSource`'s contract verbatim, so the browser source is
  a delegating wrapper. `AssetSource` is **not** a blanket impl over
  `StorageSource`: an asset source must not be writable, and a blanket impl
  would leave `PackSource` unable to implement it on its own terms.
- **`AssetId` comes from a sidecar GUID.** Hash-of-path orphans every reference
  on a rename, the problem Unity's `.meta` GUIDs and Godot's `.import` UIDs
  exist for. The corrected model is a `crate.glb.meta.ron` sidecar carrying a
  random 128-bit GUID, created on first import and committed; references use the
  GUID, and `AssetId::from_path` survives as a CLI and debug lookup. The sidecar
  also carries what a bare file cannot say: a texture's colour space (a
  standalone normal-map PNG cannot declare itself linear), usage, compression
  target, LOD overrides and a ragdoll link. `AssetId` is 128 bits so a GUID
  arrives through `AssetId::from_bits` without the type changing shape.
- **One handle type, no `<T>`.** Assets are `crcbl_core::Handle<Asset>` from a
  `crcbl_core::Pool`; a phantom parameter with one instantiation checks nothing.
  **No `Unloaded` state** until hot reload or GPU retire can produce one, since
  a state no value holds is a match arm no test reaches.
- **Crate homes follow the format's owner.** `crcbl-assets` is the IO seam and
  decodes nothing; decoding belongs to whoever owns the format (PNG in
  `crcbl-sprite`, WAV in `crcbl-audio`, glTF in `crcbl-scene`). The arrow runs
  `crcbl-scene` → `crcbl-assets`, because a scene references assets. The `gltf`
  feature (on by default, off in the workspace entry) gates everything spelled
  in terms of a glTF document, and `crcbl`'s `scn` feature reaches the loader
  without the parser, so a shipped game links no `gltf`.
- **Asset keys are `[A-Za-z0-9._-]` and `/`, on native too.** `DirSource` runs
  `crcbl_store::web::canonical_key` first, so a tree that loads from a directory
  is one that can be served over HTTP; the rest is under _`crcbl-assets` after
  stage 6 task 2_ in `docs/backlog.md`.
- **The importer validates the document itself.** `gltf` 1.4.1 panics on inputs
  its validation exists to reject (reproduced: an out-of-range `POSITION`
  accessor index, and a `.glb` declaring a length under 12), so
  `crcbl_scene::gltf_check` bounds-checks everything and the importer parses
  with `Gltf::from_slice_without_validation`. Buffer URIs resolve relative to
  the document's key and through the source, so the key rules govern them.
- **Unsupported glTF features log and skip, loudly.** The importer supports what
  content needs, and an unsupported extension, image or primitive is a warning
  and a counted skip, never a failed load. Refusing a file over a rule the code
  does not depend on rejects working assets for a purity nobody benefits from.
- **Mips at import: on the host, in linear light** (corrected 2026-08-29). The
  chain is built at import and uploaded whole, for the reasons under _What the
  deleted 43-render-standards plan left behind_ in `docs/notes/rendering.md`.
  Vulkan has no sRGB storage-image format, so a mip chain the **frame itself**
  produces for an sRGB image needs a `UNORM` view alias with the encode done in
  the shader, or render-pass downsampling; no such texture exists yet.
- **Every format the engine owns is v0 until 1.0** (the user's rule,
  2026-08-30). A header carries `format: 0` from the first byte and the loader
  refuses a version it does not know; a stale file fails loudly and is
  re-authored or re-imported, and there is no migration machinery before 1.0.
- **The cooked mesh is born v0 in the compact vertex layout** (decided
  2026-08-30). It holds `crcbl_shaders::mesh::MeshVertex`'s quantised,
  split-stream layout, never the four-`float4` one, as **two streams in the file
  as in the pool** — positions alone, then attributes, each contiguous so the
  loader uploads each to its own buffer without repacking — with the cluster and
  LOD tables beside them indexing the position stream.
- **Hot reload is dev-only, and its bar is "doesn't crash, usually works".**
  When the engine path is built, a changed chunk file reloads only its own
  system's entities, server-side, and the editor's revert reuses the same path.

## What the deleted 11-cli-headless plan left behind (2026-09-25)

Record; topic 11 made the `crcbl` binary a first-class pillar: everything the
engine and the editor can do is reachable without a window, from a terminal, a
script, a CI job or an agent. Built from it, in `crates/crcbl-cli`: `new`,
`run`, `build` (`--target wasm` refused), `screenshot`, `import` (the report),
`sim` (over a seeded world) and `settings`, beside verbs other topics added —
`replay`, `crpix`, `lod` and `bench`. Every verb accepts `--json`, which
`args.rs`'s own tests hold, and CI's golden images and determinism hashes run
through `crcbl screenshot` and `crcbl sim`.

What it left unbuilt is in `docs/backlog.md`: `crcbl scene` and `crcbl edit`
under _What the deleted 11-cli-headless plan left unbuilt_; `crcbl phys --check`
under _`crcbl phys stack --check` and the solver's profiler rows_; `sim`'s scene
argument and input script under _The determinism smoke test has no input
script_; `import --out` under _`crcbl bake`, `PackSource`, the cooked mesh and
`import --out`_; `crcbl save` under _`crcbl save list|dump|diff|restore`_; and
the device bench scenarios under _Profiling: five of the eight gaps are still
open_.

Code cites the plan as "topic 11", by its invariant and by its design rules.
Those resolve here:

| Citation                                                              | What it specified                                                                             |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| The invariant, "a sample linking `crcbl-vk` directly"                 | **No capability is implemented GUI-side**                                                     |
| "`--json` on every subcommand", "stable JSON schemas", the exit codes | **Machine-readable output and meaningful exit codes**                                         |
| "No interactive prompts unless a TTY is detected"                     | **Scriptable first**                                                                          |
| "Workspace member or standalone", "a scene dir"                       | **`crcbl new` scaffolds something that builds**                                               |
| "Report what was imported/skipped"                                    | `crcbl import`'s report, whose skips ride the engine logger                                   |
| The exit criteria                                                     | CI through the CLI (built); a scripted zero-GUI session and towers edited from the CLI (owed) |
| The sketched `scene`, `phys`, `edit`, `sim <scene>`, `import --out`   | **A sketched verb or option is refused by name, not ignored**; the work is in the backlog     |

The rules, each with its _why_:

- **No capability is implemented GUI-side.** If something works only through the
  GUI, that is an architecture regression of the same severity as a sample
  linking `crcbl-vk` directly. It is nearly free to keep because the server is
  headless by construction and editor edits are `Command` values: a CLI client
  sends the same commands the GUI sends, with the same validation, undo log and
  result, and console commands route over the same transport. The `crcbl-vk`
  half protects **consumers**: nothing above the seam names a backend. The
  `crcbl` umbrella depends on `crcbl-vk` without re-exporting it, so a sample
  asks `crcbl::backend` for one by value (`crates/crcbl/src/backend.rs` has the
  argument).
- **Every verb lives in one binary.** A capability that needs a second binary
  built to reach it is the same regression as one that needs the GUI, which is
  why the `crcbl-sim` binary became `crcbl sim` on 2026-08-23.
- **Machine-readable output and meaningful exit codes.** `--json` on every
  subcommand with a stable schema, human tables otherwise; exit 0 for ok, 1 for
  a command that failed, 2 for a bad invocation. Neither is per-command, so
  `crcbl-cli`'s `report` module renders both shapes from one result.
- **Scriptable first.** Stdin batch modes, no interactive prompt unless a TTY is
  detected, and never a prompt that is required.
- **A sketched verb or option is refused by name, not ignored.** An option the
  design names and the tree lacks exits 2 with the reason, so "not built yet"
  reads differently from a typo, and a silently dropped argument never produces
  output for something nobody asked for.
- **`crcbl build --target wasm` is refused and points at `web/build.sh`.** A
  browser bundle is a Cargo build plus the loader shim, the shader artifacts and
  the site layout; a verb that ran Cargo alone would exit 0 having produced
  something no page can load.
- **Offscreen rendering rides the normal HAL**: a surface-less device, the
  render graph and a readback, so `crcbl screenshot` works on every backend,
  lavapipe included. It is the golden-image primitive and `crcbl sim` the
  determinism primitive, and CI uses both.
- **`crcbl new` scaffolds something that builds.** The topic allowed a workspace
  member or a standalone crate; `crates/crcbl-cli/src/new.rs` records why it is
  a standalone crate with a path dependency (a template that does not compile is
  worse than none), and the scaffold creates a `scenes/` directory so a game's
  paths are stable from the first commit.

## What the deleted 07-ui-debug plan left behind (2026-09-24)

Record; stage 7 designed `crcbl-ui`, the engine's one GUI — an immediate-mode
builder over a DOM-like tree of blocks and spans, laid out by a flexbox subset
and styled by `.css` stylesheets, drawn through the engine's own render graph —
and the debug tools built on it. Built from it: every rung of its ladder (table
below) in `crcbl_ui::{draw_list, image, tree, style, text, font, edit, menu}`,
on `taffy`, `cssparser` and `skrifa`; the modular debug panel (`crcbl_ui::debug`
and `crcbl_ui::budget`), to which `crcbl-render`'s `FrameTimings` and
`FrameCounters` contribute their own sections; the console, whose rules are the
52-debug-console section below; and the geometry half of debug draw
(`crcbl_render::debug_draw`, behind `r_debug_draw`). `Hud` and `HudPanel` are
deleted, and `crcbl_ui::hud` keeps only `Anchor`.

What it left unbuilt is in `docs/backlog.md`: the per-rung sections _What UI
rung 1 shipped without_ through _What UI rung 8b shipped without_, _What the
deleted 07-ui-debug plan left unbuilt_ (the exit criteria), _The debug overlay,
and what is left of it_, _Netgraph HUD, LAN discovery_, _Inspector stats carry
no per-system tick time_, _World-anchored debug text is not built_ and _The
debug draw layer's console switch is one bit, not a category set_.

Code cites the plan as "stage 7" or "topic 7", and by rung, by architecture
section and by debug-tool item. Those resolve here:

| Citation              | What it specified                                                                                                      | Status                                                             |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Section 1             | The element tree: block and span nodes, an immediate-mode builder over an identity-keyed node store                    | Built (`crcbl_ui::tree`)                                           |
| Section 2             | Layout: the flexbox subset below, on Taffy's low-level traits, held to a fixture corpus                                | Built; `z-index` and block layout owed                             |
| Section 3             | Styles: `.css` through `cssparser`, this crate's selectors and cascade, the rule index                                 | Built; some properties owed                                        |
| Rung 1                | Draw-list primitives: textured quads, clip rects, the analytic rounded rectangle, the RGBA image atlas                 | Built (`DrawList`, `crcbl_ui::image`)                              |
| Rungs 2 and 3         | Node tree, identity and pruning; layout on Taffy with scroll state and measure callbacks                               | Built (`Ui::block`, `Ui::span`)                                    |
| Rung 4                | Selectors, typed values, cascade, `var()`, the rule index and definition cache, resolve counters, reload               | Built (`crcbl_ui::style`)                                          |
| Rung 5                | Text: `skrifa` parsing, own rasteriser, shelf-packed LRU atlas, greedy wrap, pair kerning, the measure cache           | Built (`crcbl_ui::font`, `crcbl_ui::text`)                         |
| Rung 6                | Focus: scopes, beam-first scoring, `nav-*`, per-scope memory, the engaged state, the scoring overlay                   | Built (`crcbl_ui::tree`'s `focus`)                                 |
| Rung 7 (7b–7d2)       | Widgets (7b), single-line text input (7c), `Menu` on the tree (7d1), `DebugPanel` and `ConsolePanel` on the tree (7d2) | Built                                                              |
| Rung 8 (8a, 8b)       | Outliner, tabs and dock (8a); the reflection-driven property inspector (8b)                                            | Built (`Ui::outliner`, `Ui::tabs`, `Ui::dock`, `Ui::inspector`)    |
| Debug item 1          | Profiler HUD: GPU pass timestamps and CPU frame phases                                                                 | Frame and GPU rows built; the rules are the 40-profiling section's |
| Debug item 2          | Inspector: per-system entity counts and tick times; select an entity, and each owning system draws its data            | Counts only (`Inspector::collect`)                                 |
| Debug item 3          | Culling and render stats from the delayed-readback ring                                                                | Built (`FrameCounters`, `CullStatsRing`)                           |
| Debug item 4          | Console: log view, command registry, server commands over the transport                                                | Built but for the transport half (`Flags::SIM`)                    |
| Debug item 5          | Debug-draw controls, and the immediate-mode buffer they toggle                                                         | Geometry built; one switch rather than categories; world text owed |
| Reserved `ui_*` table | The navigation actions below                                                                                           | Keyboard half built (`crcbl_input::ui`); no gamepad bindings       |

The rules, each with its _why_:

- **One GUI for editor and game.** No egui and no second draw path: the debug
  overlay, the editor's chrome, sample HUDs and the hud demo are built from this
  tree and its stylesheets, so changing engine UI is editing tree code and CSS,
  as a game would. `apps/hud` (`docs/plan/sample/04-hud.md`) is the living
  fixture and gallery. O3DE runs its editor on Qt and its game UI on a separate
  system, which is the two-UI cost this rule exists to avoid.
- **Immediate-mode authoring over an identity-keyed cache** (decided
  2026-09-15). Callers rebuild the tree every frame; a persistent node store
  keyed by `hash(parent key, #id or call site + sibling index)` keeps what must
  survive a rebuild — hover, active, focus and engaged state, scroll offset, the
  engaged widget's snapshot, the resolved-style handle, the layout cache and
  last frame's rect — and prunes nodes a frame did not touch. Ryan Fleury's
  "build it every frame" series, Dear ImGui's ID stack and React's
  position-keyed state all arrive at this. **A loop's children need an explicit
  key**, because a sibling index moves focus, scroll and engaged state onto the
  wrong row when a list reorders; **a duplicate key is a debug warning**.
- **Rebuilding is not relaying out.** A build is diffed against the store by
  hashes of style inputs, child keys and content: only a changed node
  re-resolves its style, only a changed node and its ancestors clear their
  layout caches, and a paint-only change (colour, opacity, transform) touches
  neither. GPUI's whole-tree relayout every frame is the fallback if the diff
  ever proves not worth its complexity.
- **Hit-testing reads the previous frame's layout.** One frame of interaction
  latency, for classic imgui's simplicity with real layout. The frame is: build
  the tree, resolve styles (cached), flex layout, emit the draw list, one graph
  pass.
- **Layout is Taffy's, through its low-level traits** (the user's decision of
  2026-09-15, reversing "from scratch"). The reason is the correctness long
  tail, not the size of the code: Yoga had to ship `YGErrata` flags because apps
  came to depend on its non-spec behaviour, and `min-width: auto`, percentages
  against indefinite sizes, stretch re-layout, absolute containing blocks and
  pixel rounding are each documented pitfalls. The node store implements
  `TraversePartialTree`, `LayoutPartialTree` and `CacheTree`, and the resolved
  style implements Taffy's style traits, so there is no conversion into
  `taffy::Style` and no second arena. `flexbox` is the one layout mode;
  `block_layout` only when a consumer needs it, and not `taffy_tree` or `grid`
  until something does.
- **Text leaves measure through a cached callback**, keyed by string hash, font,
  size and width bucket (Clay's measure cache), because re-measuring wrapped
  text under min- and max-content is where layout time goes.
- **Taffy is pinned, and an upgrade lands with the fixture corpus and the UI
  goldens green**; so are the fontations crates, which bump minor versions about
  monthly. Taffy's layout uses `floor`, `ceil` and basic arithmetic and no
  `sqrt`, `powf` or `mul_add`, so its output does not vary with a platform's
  libm — which is what keeps a UI golden portable. **Every divergence from the
  browser is written into the fixture corpus the day it is made**
  (`crates/crcbl-ui/tests/taffy_fixtures.rs`): Yoga's lesson is that a
  divergence you ship becomes a contract.
- **The layout subset is the contract**: `display: flex | none`,
  `flex-direction`, `flex-wrap`, `justify-content`, `align-items`, `align-self`,
  `flex-grow`/`-shrink`/`-basis`, `gap`; `width`, `height` and their minima and
  maxima in px, % or `auto`; `padding`, `margin`, border widths,
  `box-sizing: border-box`; `position: relative | absolute` with offsets;
  `overflow: hidden | scroll`; `z-index` within a stacking context. No floats,
  no tables, no animations or transitions.
- **`cssparser` parses syntax; selectors, typed values, the cascade and matching
  are this crate's** (the user's decision of 2026-09-15; MPL-2.0, which
  `deny.toml` allows). The grammar: type (`block`, `span`, widget names), `#id`,
  `.class`, descendant and child combinators, and pseudo-classes. **Specificity
  is simplified** — inline over id over class over type, last wins within a tier
  — predictable over spec-faithful, and a feature rather than a gap. **A parse
  error reports its file and line and keeps the last good sheet.**
- **Cascade sources**: the engine's `default.css`, then the application's
  stylesheets, then inline overrides. Reload restyles a running app; the
  editor's look is a stylesheet, and a game's HUD theme is a different
  stylesheet. A reload bumps a stylesheet generation, which invalidates every
  cached definition for one full re-resolve.
- **Matching follows RmlUi's index and cache.** Rules are bucketed by the id,
  class or type of their rightmost compound selector, a node matches its
  candidates right to left, and the merged definition is cached by matched-rule
  set and pseudo-state bitmask. **Each rule records the pseudo-classes it
  depends on**, so a node none of whose candidates mention `:hover` never
  re-resolves when the pointer moves — Unity's documentation names `:hover`
  restyling whole subtrees "the main culprit". A changed property is diffed, so
  a paint-only change dirties no layout, and resolve counts are shown so thrash
  is visible early.
- **CSS scope creep is the risk to hold against**: a property is added only when
  the editor or a sample needs it, and "the browser does it" is not a
  requirement.
- **One UI uber shader with an analytic rounded rectangle**: per-corner radii,
  border and optional shadow evaluated as a signed distance in the fragment
  stage, as GPUI and Bevy do. Unity tessellates corners and RmlUi assumes MSAA
  for smooth ones; MSAA is off by default here (`docs/notes/rendering.md`), so
  the distance field is what looks right on the default view.
- **A texture and a clip rectangle per command, and no stencil masks.**
  Rectangular clips are applied in the shader or on the CPU so a batch survives
  a scroll view, and GPU scissor only at a window or scroll-container boundary;
  Unity's stencil masks break batches and nest at most seven deep.
- **Two atlases, pages 2048² or smaller**: the single-channel glyph coverage
  atlas and an RGBA image atlas, which is where the menu's nine-sliced frames
  moved from a sprite pass of their own. WebGPU's compatibility mode caps a 2D
  texture at 4096.
- **Batching is by stacking context, then texture, keeping CSS paint order.**
  RmlUi's lack of batching (thousands of draw calls) is its top performance
  issue. Not built: there is one image page.
- **Fonts: `skrifa` parses; rasterising and the atlas are this engine's** (the
  user's decision of 2026-09-15, superseding the 2026-07-27 design review's
  naming of `ttf-parser`, which is in maintenance mode and recommends the
  fontations crates). Font parsing is a sanctioned exception, like cpal, Opus
  and RustCrypto: font formats are a standards-compliance surface, not a
  learning goal. The glyph atlas is **shelf-packed pages allocated on demand,
  with least-recently-used eviction per page and a per-frame re-raster budget**,
  a structure chosen so SDF and emoji are not precluded. **Grayscale
  antialiasing only**: LCD subpixel rendering needs dual-source blending, which
  WebGPU offers only as an optional feature. Latin-1 with pair kerning first;
  `harfrust` shaping and UAX #9 bidi only when non-Latin text is needed
  (`rustybuzz` is archived). Greedy line breaking. SDF text is post-MVP and for
  world-space text only, since SDF and MSDF look worse than hinted bitmaps at
  small UI sizes.
- **Widgets are block and span compositions with behaviour, added on demand.**
  Each ships default rules in `default.css` that a game overrides, and a widget
  is added when the editor or a debug tool needs it, never speculatively.
- **Focus is ordinary interaction state.** One focused element per context, so
  `:focus` styles it and **focus rings are stylesheet-driven**. Interactive
  widgets are focusable by default; containers form scopes — a modal traps
  focus, a scroll container scrolls the focused element into view, windows and
  panes are scope roots. **Mixed input**: pointer mode shows hover, pad or
  keyboard mode shows the ring, a click also sets focus, and pad input after
  mouse use resumes from the last focus or hover.
- **Navigation is the reserved `ui_*` action set**, in the `ui` input context,
  rebindable like everything else:

  | Action              | Keyboard        | Gamepad          | Semantics                                               |
  | ------------------- | --------------- | ---------------- | ------------------------------------------------------- |
  | `ui_move` (Axis2)   | arrows and WASD | dpad, left stick | spatial focus movement; every screen drivable by either |
  | `ui_next`/`ui_prev` | Tab, Shift+Tab  | LB/RB            | tree-order traversal, the always-works fallback         |
  | `ui_accept`         | Enter, Space    | South            | the same event path as a click; a widget cannot tell    |
  | `ui_back`           | Esc             | East             | close a modal or pop a screen                           |

  WASD in a menu conflicts with nothing by construction: the `ui` context is
  active while a menu has input and gameplay's WASD sits in the `gameplay`
  context under it, so the context stack disambiguates, not special cases. The
  table shadows more game keys than the samples assumed (2026-08-09): Space and
  the WASD movement most 2D samples bind are both in it, so what changes when
  menus push the context is that the samples stop handling menu keys directly.

- **Focused versus engaged (LOCKED): focus never captures navigation.** Moving
  focus onto any input widget — text, number, slider, dropdown, drag-value,
  colour picker — is inert, and `ui_move` keeps navigating past it. A widget
  consumes input only after **explicit engagement**: a click, Enter or
  `ui_accept`. Engaged is a third state after hover and focus, styled by
  `:engaged`. While engaged the widget owns the navigation actions (text types
  and moves the caret, a slider adjusts, a dropdown traverses options, a list
  traverses rows). **Exit is symmetric**: `ui_accept`, Enter or a click away
  commits; `ui_back` or Esc cancels to the snapshot taken on engage. At most one
  engaged widget per context, and engaging another commits the first; buttons
  and checkboxes fire on `ui_accept` with no engaged state. Getting stuck on a
  slider while arrowing down a settings list is the console-menu failure this
  bans. **Two exceptions, both horizontal only** (decided 2026-09-25): a
  `Menu`'s slider and cycler rows take left and right while merely selected, and
  a tree row answers left and right as the WAI-ARIA tree view pattern does (open
  or step in, close or step out), falling through to spatial navigation where
  that pattern does nothing. Up and down always pass, so arrowing down a list
  never stops on either; engaging first would cost an accept per fader and per
  expand, which no common menu or tree view asks for.
- **Spatial navigation is beam first, distance second** (revised 2026-09-15).
  Candidates overlapping the band the current rect projects in the move's
  direction beat any outside it, and only then does distance decide. Godot 4.4's
  regression — a plain gap-plus-misalignment score let a taller neighbour steal
  `ui_down` — is why; Android's `FocusFinder`, Gameface, RmlUi and Godot 4.8 all
  converge on the beam. No per-screen wiring: a menu is navigable the moment it
  lays out. **Overrides are data**: `nav-up`, `nav-down`, `nav-left`,
  `nav-right` and `nav-wrap` (Unity UI Toolkit's lack of explicit neighbours is
  its most reported navigation complaint). Scope boundaries and scroll
  containers clamp candidates. **Each scope remembers its last focused
  element**, so returning to a pane resumes where the player left it. With
  nothing in a direction, focus stays put or wraps under `nav-wrap`.
- **One debug panel, assembled from modules, that every sample switches on** — a
  standing requirement (sample rule 4 in
  `docs/plan/sample/00-samples-overview.md`), not a feature a sample opts into.
  What its perf rows measure is the 40-profiling section's; here they are
  ordinary `DebugModule`/`DebugSection` rows. **Frame timing is unconditional**:
  every sample has a frame, so the first module has no precondition. **Every
  other module is contributed by the system it reports on**, and appears because
  that system is present: the netgraph belongs in `crcbl-client`, so a
  `crcbl-client` dependency on `crcbl-ui` is decided, with no cycle, and
  `crcbl-render`'s `FrameTimings` and `FrameCounters` are the precedent.
  Breakout and flappy, both on `InMemoryTransport`, are the check that the
  composition is real — a panel that cannot render without a network module is
  broken. **Switching it on is one thing**; a sample needing more is a finding
  about the panel, the failure being a per-sample surface written once per game.
- **The inspector is generic; systems describe themselves.** Selecting an entity
  should have each system that owns it draw its data through that system's
  debug-UI callback. Not built: `Inspector::collect` reports counts only.
- **`crcbl-ui` names no renderer.** It produces draw lists and `crcbl-render`
  owns the pass. Its dependencies are `glam`, `bytemuck`, `crcbl-core`,
  `crcbl-reflect`, `taffy`, `cssparser` and `skrifa`, and `crcbl-reflect` sits
  at the bottom of the graph. Nothing in CI checks it.
- **Docking is splitters plus tabs.** Full docking is the classic time sink and
  was declined for scope; Bevy archived its editor prototypes citing "ballooning
  scope".
- **A dependency arrives with the rung that reads it**, through `cargo add`,
  never ahead of the code.

## What the deleted 40-profiling plan left behind (2026-09-24)

Record; the built part is `crcbl_core::trace` (spans and counters, gated at
runtime by `CRCBL_TRACE`), `crcbl::perf`'s span vocabulary for the loop's
phases, `crcbl_core::stats`' percentiles, `crcbl_render::timing` and
`crcbl_render::PassStats`, `crcbl_render::counters`, `crcbl_render::cull_stats`,
`crcbl_jobs::Pool::stats`, `crcbl bench --scenario jobs|phys`
(`crates/crcbl-cli/src/bench/`) and the debug panel's Frame row
(`crcbl_ui::budget`). What it left open is in `docs/backlog.md` under
_Profiling: five of the eight gaps are still open_ and _Profiling and
benchmarking: decisions taken 2026-08-13, before any code_. The decisions of
2026-08-13 are recorded under that heading further down this file; this section
holds the rest of what binds.

Code cites the plan as "topic 40" and by the number of a **missing piece** in
its original list of eight. The numbering is kept so each citation resolves:

| Piece | What was missing                                                                                 | Status                                                                                                         |
| ----- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| 1     | CPU-side spans: nothing measured the tick, schedule, physics, upload or frame, or said GPU-bound | Built as `crcbl_core::trace` and `crcbl::perf`; the ECS schedule, physics and asset upload are still unspanned |
| 2     | A benchmark harness: fixed scenarios, warm-up, statistics beyond a mean, machine-readable output | Built as `crcbl bench` for `jobs` and `phys`; the device-backed sample scenarios are owed                      |
| 3     | Baseline storage and regression detection                                                        | Owed                                                                                                           |
| 4     | Trace export a profiler UI can open                                                              | Owed                                                                                                           |
| 5     | Memory and occupancy accounting                                                                  | Owed                                                                                                           |
| 6     | Job-system instrumentation                                                                       | Built as `Pool::stats`; no reader puts it on the trace or a panel, and `crcbl-jobs` has no spans               |
| 7     | Counters in one place rather than piecemeal                                                      | Built as `crcbl_render::counters` (`FrameCounters`)                                                            |
| 8     | Culling stats that leave the GPU                                                                 | Built as `crcbl_render::cull_stats` (`CullStatsRing`)                                                          |

The rules, each with its _why_:

- **The tooling lands before the perf work, not with it.** A profiler bolted on
  afterwards never covers the code written before it — the argument that put
  timestamp queries in the HAL at P0 applies to every span.
- **One span shape, so a trace has one timeline.** A scoped CPU span with a
  static name, opened and closed by RAII and nesting freely, the frame
  outermost; spans carry their thread, so job workers appear as their own
  tracks. **GPU spans come from `crcbl_render::timing`'s per-pass timestamps**
  on their own track, aligned to the CPU timeline by one calibration point per
  frame rather than by assuming the two clocks agree. **Counters are spans'
  siblings**, a named `u64` sampled per frame, not log lines.
- **The span module is `crcbl_core::trace`**, beside `time` and `log`, because
  `crcbl-core` is the bottom of the graph. `crcbl-jobs` is the one crate that
  must gain the dependency, and it gains it with its first span (CI's
  `cargo machete` refuses an unused edge). A separate `crcbl-trace` crate was
  declined: structure to carry forever for a small module, and moving it later
  is a re-export away, which the dependency edge is not.
- **The compile-time off switch is `--cfg crcbl_trace_off`, never a Cargo
  feature.** A feature is on under CI's `--all-features` runs, so every test
  would assert the compiled-out arm, and feature unification means a binary
  could not turn it off again. The cfg, declared by a `build.rs` with
  `cargo::rustc-check-cfg`, is built only when a shipping build needs it.
- **A pass's GPU span includes its barriers**, because the encoder's scope rules
  put query writes outside any pass; a pass whose barriers cost more than its
  draws is a finding a neighbour's bucket would hide. **A pass label is summed
  within the frame**, with the occurrence count on the row (`lantern`'s two
  views report `shadow`, `forward` and `tonemap` twice).
- **A device without `Features::TIMESTAMP_QUERY` gets no timers and an empty
  report**, which is what browsers do; degrade rather than break.
- **Benchmark scenarios are named and fixed**, live with the samples that own
  them, and name their seed, frame count and warm-up. Output is JSON by default
  with the environment block, and that JSON is the baseline's storage format.
- **Baselines are per machine, never committed and never compared across
  hosts.** A comparison against a baseline from different hardware is refused,
  not printed. Thresholds are per metric and recorded in the scenario, because a
  5 % move in a 0.1 ms pass is noise and a 5 % move in frame time is not. The
  scheduled first form (2026-08-30): `PassStats`' per-pass timings written per
  machine, a rerun reported as a ratio per pass, and **above 1.15× is the red
  line, locally** — CI publishes numbers and never gates on them.
- **A rung is priced on three tiers**: the desktop adapter, lavapipe and the
  browser, each preset measured, because the software and browser tiers pay tens
  of times the desktop cost and are the tiers every golden runs on.
- **Counters that must move are asserted in tests**, the way the culling counts
  are: a counter that stops being incremented when a path changes reads as an
  improvement. A count the CPU cannot know is `None` and prints `indirect`,
  never a zero that reads as "nothing was drawn".
- **The profiler's own enabled cost is measured by a benchmark of the
  profiler**, the only honest way to know that instrumentation is not changing
  what it measures.
- **Perf panel rows follow the `DebugModule`/`DebugSection` shape**, and labels
  share one namespace with nothing detecting a collision, so a test that finds a
  row by its label can silently read the wrong one. The Frame row shows two
  rolling distributions, not one frame's CPU and GPU numbers paired, because the
  GPU report is frames latent.
- **Tracy or another external profiler** is an optional feature over the same
  span data if ever wanted, on demonstrated need and a dependency decision —
  never a second instrumentation pass.

## What the deleted 52-debug-console plan left behind (2026-09-24)

Record; the plan was built. It specified a Source-engine-style console
(`ConVar`, `ConCommand`, `help`, `find`, tab completion) in every game and every
build: opened with the backtick, drawn over the top of the frame, showing the
lines the engine logs, with every setting a variable it prints and sets. What it
left open is in `docs/backlog.md` under _What the deleted 52-debug-console plan
left unbuilt_, _What the start-up autoexec left uncovered_ and the two
touch-console entries.

Code comments cite the plan's **decisions** and **delivery slices** by number —
"debug-console decision 3", "debug-console slice 8". Both numberings are kept
below so each citation still resolves. The backlog once held an open question
about this (whether to keep a one-line stub per slice, rewrite every citation,
or keep the plan); it was settled on 2026-09-24 by the owner's rule that a fully
built plan is deleted: the numbers live here, every citation outside
`CHANGELOG.md` points here, and the changelog keeps the old path as history.

**The decisions**, by the number the code cites:

- **Decision 1 — the registry is a crate of its own with no dependencies, and a
  variable is its own storage.** `crcbl-console` depends on nothing but
  `core`/`std`, so it compiles on every target, tests headless and is read by
  the UI, the engine and the CLI alike; it neither draws nor knows about
  settings files. A `ConVar`'s cell is a typed atomic, so the owning code reads
  it with `.get()` and never polls the console. **A `Text` variable has no
  static cell** (a `String` in a `static` needs a lock and an allocation), so it
  exists only as a `Binding` over storage elsewhere. `Kind` is what makes a set
  coerce and refuse rather than blind-write a string.
- **Decision 2 — declared beside the code, listed once per crate, gathered at
  one seam; no linker tricks.** `convar!`/`concommand!` are the annotation: the
  ident is the console name (matched, sorted and de-duplicated without regard to
  ASCII case), the doc comment is the help, the type is the `Kind`. Each crate
  lists its declarations in one `console_table()`, and **a test in that crate
  reads its own `src/` and fails when a declaration is missing from the table**
  (`crcbl_console::guard`); a second guard reads the workspace manifests and
  fails when a crate depending on `crcbl-console` is missing from the gather in
  `debug_console::engine_tables`. A game hands its table over through the
  defaulted `HostedGame::console_table`. Two tables declaring one name are
  refused at gather time with both crates named. Code-declared names take
  Source's prefixes (`r_`, `ui_`, `snd_`, `phys_`, `net_`, `cl_`/`sv_`);
  settings-backed ones keep their key name bare. **`linkme` was declined**
  because it does not list WebAssembly, **`inventory` because it runs code
  before `main`**, which this engine never does and whose silent failure would
  be "not implemented arriving as passed". If `linkme` ever lists wasm, the
  per-crate lists collapse into one distributed slice with no change to `Table`.
- **Decision 3 — every settings key is a typed variable, and applying one lives
  in one place.** `settings::console_bindings` derives one `ARCHIVE` binding per
  `settings::catalogue()` key, so a key added to the catalogue is a console
  variable the same day. `CatalogueKey::kind` is a `crcbl_console::Kind`, not a
  prose domain, and `apps/options` reads the same `Kind`, so the two cannot
  disagree; every numeric range is asserted equal to its setter's own clamp, so
  the console never accepts a value the file reads back as a different one. A
  `KeyStatus::Named` key (declared, unread) is `READ_ONLY` and its help says
  nothing reads it, so the console is honest about the whole catalogue.
  `settings::apply` writes one key and applies it through a `settings::Stage`;
  `GameGpu::apply_video` and `GameGpu::set_debug_view` default to `Unsupported`,
  so a host with no renderer says so rather than passing. `Stage` is its own
  trait because `GameGpu` is `Sized` (it takes `self` in `destroy`) and has no
  `dyn`, and because a key reaches more than a renderer. **Settings are not
  saved on exit**: `save` writes the file, so a debug session that flips twenty
  variables never silently becomes the player's file — the same call
  `apps/options` made. **The console's writes are deferred**: a `Binding`
  reaches its host as `&mut dyn Any`, which cannot hold a borrow of the renderer
  or the mixer, so `ConsoleHost` records into `settings::Deferred` and
  `Loop::drain_console` applies it where the bundle is in hand.
- **Decision 4 — the log the panel shows is the log.** One bounded ring,
  `crcbl_core::log::console`, pushed from `StderrLogger::emit` and from the web
  sink **before** each sink's own filter, read with `snapshot_since(sequence)`
  so a reader copies only what arrived. Everything the console prints goes
  through `console::print` at `Info` under `CONSOLE_TARGET`, so the terminal and
  the panel show the same exchange and a test can assert it through
  `log::capture`. `log <filter>` goes through `Filter::try_parse`, which refuses
  what `Filter::parse` skips, because a person at a console can be told. The
  panel's own view is a separate `LevelFilter` threshold, so "show me debug
  lines" never means "print debug lines to the CI log".
- **Decision 5 — the key and the takeover.** `CONSOLE_KEY` is the bare backtick,
  reserved by the loop in every game with no per-app code; with `Ctrl` or `Meta`
  held it is the browser's devtools shortcut and is left alone, on the page's
  side too (`SWALLOWED_BARE` in `web/engine/shell.js`). The open console claims
  every key, every `TextCommit`, the wheel and the pointer over the panel;
  releases whatever the game was holding when it opened, so no key sticks down;
  swallows the character the toggling press commits; and `Escape` closes it
  before it pauses. The web backend commits text for a printable
  `KeyboardEvent.key` only when neither `Ctrl` nor `Meta` is held.
- **Decision 6 — the panel is drawn last, and touch is a drawn keyboard.** The
  panel is the top `CONSOLE_HEIGHT_FRACTION` of the frame at a whole-number
  scale, drawn after the debug overlay so nothing covers it. A line is the
  record's message with its target in front unless the console printed it, the
  level carried by colour; "the same lines as stderr" means the same records in
  the same order, not the same glyphs. **Touch uses `ConsoleButton` and
  `TouchKeyboard`, drawn by the loop, not a focused DOM element**, for the three
  reasons in `crates/crcbl-ui/src/console/keyboard.rs`'s module docs: no native
  backend reports a contact, the shim focuses the canvas on every `pointerdown`,
  and the atlas covers printable ASCII only. Both are on screen only once a
  contact has arrived, `PauseControl`'s rule.
- **Decision 7 — the commands, each declared by the crate that owns it.** The
  built-ins are `help`, `find`, `echo`, `clear`, `toggle` and `reset`
  (`crcbl-console`'s `builtin.rs`); `pause`, `quit`, `fps`, `save`, `dump`,
  `config`, `bind`/`unbind`, `debug_view` and `quality` are in `crcbl`, and
  `log` in `crcbl-core`. A set is `name value` or `name = value`. **An enum
  value may hold a space** (`debug_view ambient occlusion`), so a set joins
  everything after the name and completion treats the rest of the line as one
  token; a per-token "simplification" breaks it. **A bare `reset` skips every
  `ARCHIVE` variable**, so a debug session cannot empty the player's settings
  file. A `Fault` prints and leaves state alone.
- **Decision 8 — one debug-view variable, declared in `crcbl`, and the loop is
  the only writer of a renderer's view.** `crcbl::debug_view`'s `r_debug_view`
  lives beside `GameGpu::set_debug_view`, the only seam that can apply it, not
  in `crcbl-render`, where a static has no renderer to reach.
  `Loop::apply_debug_view` hands a renderer a **change**, an edge, so a renderer
  is left as its sample set it up until something moves the variable. A sample's
  own view row _is_ the variable (lantern's `AO VIEW`, quarry's
  `LOD VIEW`/`HEATMAP`, viewer's `N`): a sample that wrote its own view every
  frame undid every console line.
- **Decision 9 — `Flags::SIM` is reserved, not built.** Console commands are
  host input and not part of the tick stream, so a variable that changes what
  the simulation computes must not be a console variable yet. The rule for the
  first `SIM` variable, and what building it takes, is the backlog entry.
- **Decision 10 — the cost needs no per-tier pricing.** Closed, one ring push
  per log record on a path that already formats a string; open, a copy of at
  most `CONSOLE_RING_LINES` records and a draw list of the visible lines; no GPU
  work; completion is a prefix scan over a sorted table of under a thousand
  names. None of it has been timed (see the limits section below).

**`config` and autoexec**, which the code cites as slice 9:

- **`config` takes a bare name, never a path.** ASCII letters, digits, `-` and
  `_`, `.cfg` optional, refused by `file_named` before storage is asked for
  anything, so a filesystem path is never built from console input. The bytes
  come from `SettingsStack::with_platform_storage`, so "the settings directory"
  is the platform config directory natively and OPFS in a browser.
- **A file runs through the same `Registry::execute`, `Context` and host a typed
  line does**, so there is no second execution path to disagree. A failing line
  is reported as `file.cfg:3: …` and the file runs on, with a closing count.
- **Recursion has two bounds because they end different things**: a file already
  running is refused by name, which ends every cycle, and `CONFIG_NESTING_LIMIT`
  bounds a chain of distinct files, which no cycle check sees.
- **A run that reads no settings file runs no autoexec.** `AUTOEXEC` runs from
  `Loop::new` before the first frame; the gate is `EngineLink::app_name`, not
  `with_platform_storage`, which natively answers `Some` and would hand a
  headless run `~/.config/<game>/autoexec.cfg`. A missing file is silent; a file
  that will not run is printed and the boot carries on.

**The delivery slices**, by the number the code cites:

| Slice | Name                    | Landed                 | What it delivered                                                                                                                                                                                                                                      |
| ----- | ----------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1     | `crcbl-console`         | 2026-08-30             | The registry crate: `Kind`/`Value`, `ConVar`, `Binding`, `ConCommand`, the macros, `Registry::gather`, the parser, `help`/`find`/`echo`/`clear`, completion, `History` and `guard::declared_names`.                                                    |
| 2     | Settings typed          | 2026-08-30             | `CatalogueKey::kind`, `settings::apply` over `Stage`, `settings::console_bindings`, and the defaulted `GameGpu::apply_video`/`set_debug_view` forwarded by every bundle with a `ForwardRenderer`; the console's writes deferred to slice 5's drain.    |
| 3     | The log ring            | 2026-08-30             | `crcbl_core::log::console`, `console::print`, `Filter::try_parse`, the live `set_filter`, and the `log` command.                                                                                                                                       |
| 4     | The panel's widgets     | 2026-08-30             | `crcbl_ui::console`: `LogView`, `ConsolePanel` and the completion rows, drawn from values; its `TextField` was replaced by `Ui::text_input` over `LineEdit` on 2026-09-16.                                                                             |
| 5     | The engine              | 2026-08-31             | `CONSOLE_KEY`, the takeover, `debug_console::Console`, the gather and its two guards, `pause`/`quit`/`fps`/`save`/`dump`, `Loop::drain_console`, `CONSOLE_LEVEL_KEY`, and the panel drawn last.                                                        |
| 6     | Every debug view        | 2026-08-31             | `crcbl::debug_view` and `Loop::apply_debug_view`; lantern, quarry and viewer write the variable. The exit criterion's demo is `apps/quarry`, since breakout has no forward pass, proven on radv and lavapipe by `apps/quarry/tests/device/console.rs`. |
| 7     | The web                 | 2026-08-31             | `TextCommit` from `__crcbl_web_key`, `SWALLOWED_BARE` in the shim, a controls row on every demo page, and `EXPECTATIONS.quarry.console` in the browser gate.                                                                                           |
| 8     | Paste, `bind`, `toggle` | 2026-08-31             | Paste (now `TextPump`'s since 2026-09-16), `bind`/`unbind` over `HostedGame::actions` and `Loop::drain_binds`, `toggle` and `reset` as built-ins; asteroids and breach each drive a rebind end to end.                                                 |
| 9     | `config`                | 2026-08-31, 2026-09-02 | `crcbl::console_config`: `config <name>`, then `AUTOEXEC` run by `Console::run_autoexec` before the first frame.                                                                                                                                       |
| 10    | Touch                   | 2026-08-31             | `ConsoleButton` and `TouchKeyboard`, gated on a contact; the guard `an_untouched_run_keeps_every_click_the_console_would_have_taken`, and browser group F.                                                                                             |
| 11    | `Flags::SIM`            | deferred, not built    | Decision 9's reserved flag; the backlog entry says what building it takes.                                                                                                                                                                             |

**Considered and declined**, so none is re-proposed:

- **`linkme`/`inventory` registration** — decision 2.
- **A `#[convar]` proc-macro attribute** — it registers nothing the declarative
  `convar!` cannot, adds `syn`/`quote`, and the workspace declined a proc-macro
  once already (`Format::ALL`) on the same ground.
- **Parsing the settings TOML for the variable list** — the catalogue is the
  authority and is derived from the readers; a file would list keys nothing
  reads.
- **A console-side value cache** — the variable is the storage; a cache is a
  second copy that drifts.
- **Auto-saving `ARCHIVE` variables on exit** — decision 3.
- **IME on the web** — `ShellCaps::TEXT_IME` stays clear there; it is the
  windowing plan's work, not the console's.

**A sabotage lesson from slice 7**: the browser gate's restore check first asked
only whether a later heartbeat said `view: shaded`, which every heartbeat on an
untouched page says, so it passed with the feature removed. It now fails unless
the view was `ambient occlusion` going in. A check of a restore has to read the
state before the restore.

## What the debug console left as limits (2026-08-31)

Every delivery slice in the table above landed except `Flags::SIM`, which is
deferred by decision 9 rather than outstanding. What follows is what those
slices left as limits rather than fixed — each stated in the code as well as
here. The open work they left is in `docs/backlog.md` under _What the deleted
52-debug-console plan left unbuilt_.

What slice 1 left as limits rather than fixed, each stated in the code:

- **`Flags::SIM` is declared and nothing sets it** — reserved by decision 9,
  deliberately; its doc comment says what lands with the first one.
- **`guard::names_in` strips line comments, not block comments**, and splits a
  line at its first `//` — a declaration inside `/* … */` is counted as real,
  and a `//` inside a string literal ends the scan of that line. Line comments
  are what doc examples use, which is the case that bites; a block-comment and
  string-aware stripper is a parser's job and the crate has no dependencies.
- **`Context::new` takes a registry and `Registry::execute` takes `&self`;
  nothing enforces they are the same registry.** They are at every call site,
  and the console is built once at `Loop::new`. If a second registry ever
  exists, make `Registry::context(&self, host)` the only constructor.
- **`echo =b` echoes `b`.** The `=` between a name and its first value is
  optional, and the rule cannot tell that first argument from a value;
  `echo "=b"` echoes `=b`, and the quoted exemption is tested.
- **Not measured:** completion is a linear scan over the sorted table, which
  decision 10 says needs no per-tier pricing at the table's size.

What slice 3 — the log ring, the `console` target, `log <filter>` — left as
limits rather than fixed:

- **The ring only holds what a sink was offered.** The push in
  `crcbl_core::log::console::push` is before each sink's own filter, so a record
  a _per-target_ directive silenced is in the ring; a record above
  `log::max_level()` is not, because the level macros' `__enabled` gate drops it
  before any sink is asked. So `log off` narrows the ring as well as the
  terminal, and slice 4's per-level panel toggle can only show lines that
  reached a sink. Verified: `the_ring_holds_records_the_filter_refused` in
  `crates/crcbl-core/src/log.rs` and
  `a_filter_installed_at_runtime_decides_the_next_record` in
  `crates/crcbl-core/tests/console_log.rs` assert both halves.
- **`log::set_filter` moves the facade's global maximum with the filter**, which
  is what makes a widened directive reach a sink at all — and means a
  `log::capture` running on another thread stops seeing records the narrowed
  maximum drops. Stated on `set_filter`. It is why the filter tests are their
  own binary and serialise on one mutex; the capture tests stay in
  `crates/crcbl-core/tests/log_capture.rs`, which never writes the filter.
- **`CONSOLE_RING_LINES` is a judgement, not a measurement.** It was picked for
  scrollback past a demo's boot, and the ring's memory was never measured on any
  tier. The same goes for the cost decision 10 prices: every record now renders
  its message and allocates a target `String`, where before a filtered-out
  record cost nothing — no frame-time measurement was taken either way.
- **The web sink's ring push is verified natively, not in a browser.**
  `crcbl::web` is deliberately not `wasm32`-gated, so
  `the_web_sink_rings_a_line_its_filter_refused` in `crates/crcbl/src/web.rs`
  exercises the real `WebLogger::log`. The panel reads the ring in a browser —
  `crcbl_ui`'s `LogView` drains it — but no browser check asserts what the ring
  _holds_: the gate's console lines come from `Runtime.consoleAPICalled`, which
  is the sink's own output rather than the ring. So the browser gate can still
  only show the change is harmless.
- **`log` joins its arguments with a comma**, so `log warn crcbl_vk=trace` reads
  as the two-directive list a space-separated typing meant. Safe only because a
  filter directive never contains a space — unlike an `Enum` value, which
  `Registry::run_statement` joins with one.
- **The log filter is process-wide, and one sink at a time is the honest case.**
  `crcbl_core::log::FILTER` is a module static that `StderrLogger::permits` and
  `sink_permits` both read, so a process that installed both sinks shares one
  filter between them. That happens only under `cargo test`, where `crcbl`'s own
  binary installs `StderrLogger` while the web tests drive `WebLogger` directly;
  the shared filter is correct there rather than merely tolerated, since a
  filter is the engine's and not a sink's.
- **`Filter::INITIAL` spells the default filter a second time.** `RwLock::new`
  in a `static` needs a const value and `Filter::parse` is not const, so the
  initial filter is a struct literal beside the string.
  `the_const_initial_filter_is_the_default_one_parsed` holds the two together; a
  `const fn parse` would remove the duplication and nothing needs it yet.
- **`crcbl::web::set_log_level` still takes a bare level, not directives.** The
  shim's `logLevel(n)` maps a small range onto `off..trace` and installs that as
  the whole filter, so a page cannot ask for a per-target directive from JS the
  way the console can from the keyboard. Nothing wants one; the console is the
  way in.
- **The browser gate reads the filter and does not move it.**
  `EXPECTATIONS.quarry.console.filter` types `log` and asserts the answer, which
  is what proves `install_logger` registered on a real page. A check that
  `log warn` silences the demo's heartbeat would be stronger and needs an
  absence-with-a-timeout assertion plus a restore; the per-target half is
  asserted natively instead, against the real `WebLogger`.
- **`crcbl::web`'s filter-writing tests serialise on a mutex, and they have
  to.** `filter_at` takes that order, calls `init_logging` and then
  `register_sink`, and the guard is the caller's to hold for the rest of its
  test. Both halves were found by running `cargo test -p crcbl --lib` in a loop:
  without the mutex the tests overwrite each other's directives, and without the
  `init_logging` a `capture` elsewhere in the binary can win the logger slot
  mid-test. `nextest`, which CI runs, gives each test a process and would never
  have shown either.

What slice 4 — `crcbl_ui::console`, the panel's widgets — left as limits rather
than fixed:

- **The panel's line is not stderr's line glyph for glyph.** `LogView`'s
  `line_text` draws a record's message with its target in front of it, unless
  the console printed the line itself (`CONSOLE_TARGET`), and carries the level
  as a colour; there is no elapsed-seconds column and no level name, because the
  panel is about a hundred columns wide at scale 1 and the ring's order already
  says what a timestamp would. So the plan's exit criterion "the panel shows the
  same lines as stderr" is met as _the same records, in the same order, coloured
  by level_ — read it that way when slice 5 is checked against it. `line_text`
  is the single place a timestamp would be added if one is ever wanted.
- **The panel's per-level view is a `LevelFilter` threshold, not five toggles.**
  `LogView::set_filter` takes `Off` through `Trace` and hides lines without
  dropping them; decision 4's wording was "a separate toggle per level". A
  threshold is what a key can cycle, which is what that decision asked for in
  the same sentence. `crcbl::debug_console::CONSOLE_LEVEL_KEY` is the key, and
  it is `F2` because while the panel is up the loop claims every key, so the
  choice takes nothing from a game and no other key was a better one.
- **The caret is measured, not read off `layout_line`'s rectangles.** Decision 6
  says the caret rectangle comes from the glyph rectangles;
  `FontAtlas::layout_line` drops every zero-ink glyph, so its `n`-th rectangle
  is not the `n`-th character of a line holding a space and a caret placed from
  it slides left by a column per space. `TextField::caret_rect` measures the
  text before the caret with `FontAtlas::text_width` instead — the same advances
  `layout_line` walks — and
  `the_caret_lands_on_the_column_it_names_across_a_space` is the check.
- **`TextField` was deleted by UI rung 7d2 (2026-09-16)**, and the console's
  line is `Ui::text_input` over `crcbl_ui::edit::LineEdit`, which selects,
  word-moves and pastes. The two records below describe that widget as it was;
  they are kept because the caret-column trap and the control-character rule
  outlived it.
- **`TextField` had a caret-following window and no other scrolling.** A line
  longer than the box shows its last columns with the caret pinned to the last
  one; there is no stored scroll offset, no selection, no clipboard and no IME
  (all declined for v0 by the plan). A caller that passes `usize::MAX` columns
  gets the whole line and no window at all.
- **The pointer reaches the Send button and nothing else.**
  `ConsolePanel::point` hit-tests that one rectangle: the completion rows are
  not clickable and the log cannot be selected or dragged. The wheel is not read
  here either; the loop drives `LogView::scroll_by` from a wheel event and from
  `PageUp`/`PageDown`, a page being whatever `ConsoleLayout::log_rows` reported
  for the frame that was last drawn.
- **Not measured.** `Loop` builds and draws a `ConsolePanel` now, and its
  per-frame cost (one draw command per visible row, and a `String` per wrapped
  row because `DrawList::text` takes an owned one) has still never been timed on
  any tier. Decision 10 priced the shape and nothing has priced the numbers. Nor
  has the panel been _looked_ at: every check of it is headless and asserts on
  rectangles and text, so nothing has confirmed it is legible on a screen.

What slice 5 — `CONSOLE_KEY`, the takeover, the gather and the drain — left as
limits rather than fixed:

- **Four of the five app overrides of `HostedGame::set_bus_gain` are checked by
  the compiler and not by a test of their own.** The loop → game → `Mixer` path
  is proven in `crcbl::engine`'s
  `a_gain_typed_at_the_console_reaches_the_running_mixer`, which reads the gain
  back off a real mixer the fixture game holds; each `apps/*` override is a
  one-line forward to its `Audio::set_bus_gain`, which is a one-line forward to
  the mixer. Nothing exercises those two lines in asteroids, breakout, flappy or
  horde — a forward to the wrong bus would compile. `apps/options`' override is
  the exception: it moves the screen's own fader as well, and
  `a_gain_typed_at_the_console_moves_the_fader` drives it.
- **`fps` reads the loop's own frame clock, not the GPU's.**
  `debug_console::EngineLink::set_frame_timing` is fed
  `FrameClock::render_dt_secs` once a frame, so the number is the wall time
  between frames; the per-pass GPU timings the debug overlay shows are not in
  it.
- **`save` writes nothing in a headless run, and says so; the browser path is
  unexercised.** `ConsoleHost::saving_as` is set for every arm but
  `SettingsSource::None`, and `save` goes through
  `SettingsStack::save_platform`. A headless run gets a stack over
  `crcbl_store::MemoryStorage` — `SettingsSource::open_editable`, writable, so
  every variable is still settable — and no name, so `save` faults with "nowhere
  to save to". A browser run _is_ `Platform`, so it saves through whatever OPFS
  store the page installed, and that path has not been exercised by any gate.
- **Nothing stops a game handing the loop a second stack after start-up.**
  `HostedGame::settings` is asked once, by `Loop::new`, and a game that replaced
  its own `SharedSettings` afterwards would be back to two writers with nothing
  to notice. No game does; the method's docs say so and no check enforces it.
  Enforcing it would mean the loop owning the handle and the game borrowing it,
  which every `HostedGame` hook's signature would have to carry.
- **`SharedSettings` borrows can panic and nothing proves they do not.** It is
  an `Rc<RefCell<SettingsStack>>`, so `stack_mut` panics while another borrow is
  live. Every caller in the workspace takes a borrow, reads or writes, and drops
  it inside one statement or function — `ConsoleHost`'s
  `read`/`write`/`save`/`dump`, `Screen::write`, `Screen::save_to`,
  `Screen::debug_sections` — and the test suite exercises all of them, so a
  re-entrant borrow would have to arrive with new code. There is no lint or
  type-level guard against one.
- **The options browser gate was not re-run for this change.** No settings key
  and no `VIDEO_KEYS` entry moved, so `web/tools/browser-e2e.mjs`'s `toFader`
  index is untouched; the browser path exercises faders and never types at the
  console, and the change to `Screen::set_bus_gain` is only reachable through
  the console drain. Stated as a coverage gap rather than a verdict: the gate
  itself was not run locally.
- **`apps/options` on a headless run now keeps its edits in memory rather than
  refusing them.** A behaviour change nothing outside the sample reads — the
  `SAVE` row still says `NOWHERE TO SAVE`, because `SettingsSource::None`'s save
  still answers `Ok(false)` — but a summary line that used to carry
  `save failed: no user settings layer in the stack` for a scripted headless
  edit now carries `unsaved edits`. No gate reads that field.
- **A fault prints at `Level::Info`, like every other console line.**
  `console::print` is the only sink the console has and it is fixed at `Info`,
  so a refused command is the same colour in the panel as a value. A
  `console::print_at(level, …)` is the fix and nothing needed it yet.
- **The pointer test is the keyboard's.** `Console::point` is wired and reports
  whether the cursor was over the panel, and no headless check drives it: the
  `HeadlessShell` pointer path is exercised for menus elsewhere, and the
  console's **Send** button is covered by `crcbl_ui`'s own tests over
  `ConsolePanel::point`. So "a click on Send submits" is proven in the widget
  and not through the loop.

What slice 6 — `crcbl::debug_view`, the shared view, the samples that gave up
their own — left as limits rather than fixed:

- **`crcbl::debug_view::r_debug_view` is process-global, which is a test
  hazard.** A `ConVar` **is** the storage — decision 1 — so two loops in one
  process share the view, and `cargo test` runs a crate's tests as threads of
  one process. `debug_view::for_test()` is the answer and it is public for that
  reason: it serialises the checks that move the view and restores `Shaded` at
  both ends. The damage does not arrive where the view was moved:
  `ForwardRenderer::resolved_effects` drops the antialiasing tier while any view
  is on, so a bystander check asserting a bundle's effects fails instead —
  measured at about one `cargo test -p quarry --lib` run in three before the
  guard. In `apps/lantern`, `apps/quarry` and `apps/viewer` the guard is
  therefore held by the test fixture — the `Scripted` wrapper each crate's
  `scripted` helper now returns, which derefs to the `Loop` — so a check added
  later inherits it rather than having to remember it. The guard nests per
  thread — a check that builds two loops holds two of them, and
  `let mut engine = scripted(..)` twice in one scope does not drop the first —
  which it has to: against a plain mutex that check hangs rather than fails, and
  arrives as a 240-second nextest timeout. Anywhere else, a check that moves the
  view and forgets the guard is a **flake**, not a failure, which is the shape
  nobody diagnoses from a CI log. `cargo nextest`, which CI runs, gives each
  test a process and would not show it at all.
- **lantern's in-scene monitor now greys with the main view.** The deleted
  `lantern::Gpu::set_occlusion_view` applied the channel to the main renderer
  alone and its doc argued for that — a reviewer keeps the shaded frame beside
  the grey one — and `GameGpu::set_debug_view`, which is the only path now,
  writes both. The argument was traded for one path rather than answered;
  putting it back is a main-renderer-only body in
  `lantern::Gpu::set_debug_view`, which the forwarder seam allows a bundle to
  write for itself.
- **A renderer rebuilt mid-run has to carry the view itself, and only one does
  it.** `Loop::apply_debug_view` writes on an edge, so a bundle that replaces
  its `ForwardRenderer` while a view is showing loses it with nothing to put it
  back. `apps/viewer`'s `Gpu::reload` is the only such path in the workspace
  today — a re-export builds a new renderer — and it now carries the view beside
  the exposure, the effect request, the render scale and the wireframe;
  `a_debug_view_survives_a_reload` is the guard. A sample that grows a second
  rebuild path has to remember the same line, and nothing enforces that.
- **The occlusion channel over quarry's face is shallow, and the pose was swept
  to find any of it.** `OCCLUSION_POSE` is three quarters down the dolly because
  that is where the most pixels darken: 559 of 49 152 on radv, 557 on lavapipe,
  and **zero** at the dolly's end, where the camera is inside the quarry. The
  face is a displaced ridge with no corners, so this is the fixture and not the
  pass. A sample with an interior — `apps/lantern`'s room — would show far more,
  and that is where a richer picture of this view belongs if anyone wants one.
- **Nothing reads pixels through the `Loop`.** The device check draws its frames
  from the quarry harness's own renderer, joined to the console's path at
  `crcbl::settings::set_debug_view_on` — the body the forwarder calls — while
  the loop half asserts on `ForwardRenderer::debug_view` rather than on a
  picture. A single check that did both would need `--screenshot`, which quarry
  is one of the three samples not to have.

What slice 7 — the web backend's `TextCommit`, the shim's swallow, the browser
gate — left as limits rather than fixed:

- **`is_text` is now spelled in three places.** `win32::keys::is_text`,
  `appkit::keys::is_text` and the new `web::text_of` all say "a committed
  character is text unless it is a control character", and `linux::xkb::text`
  says it a fourth way over a whole string. Two copies were already there before
  this slice; it added the third rather than lifting one helper into
  `crcbl-shell`'s root, because slice 7's write set was the `web` module alone.
  The lift is about ten lines and would put the rule, and the comment arguing
  it, in one place.
- **The browser gate types on a US layout and only on a US layout.** The
  `physical` helper in `web/tools/browser-e2e.mjs` maps a character to a
  `code`/virtual-key pair for lower-case letters, the space and the underscore,
  and throws for anything else — so a console line added to `EXPECTATIONS` with
  a digit or a punctuation mark in it fails loudly rather than dispatching a
  wrong `code`. Widening it is a table, not a design.
- **Nothing in a browser reads the panel back.** Every console check in the gate
  reads the _log_ — the echoed line and quarry's own heartbeat — because the
  panel is drawn into the frame and the gate has no way to find a glyph in it.
  So "the console draws legibly in a browser" is still unproven, which is the
  same gap the slice-4 bullet records for every other tier.
- **Only one demo carries the block.** `EXPECTATIONS.quarry.console` is the only
  row with one, because quarry's heartbeat is the only one that prints a value a
  console command moves. Any demo could carry the _echo_ check; none of the
  others can carry the effect check without a new HUD field.

What slice 8's first two follow-ups — the paste key, `bind`/`unbind` — left as
limits rather than fixed:

- **Only the one clipboard is read.** `Shift`+`Insert` is deliberately not a
  second spelling of the paste key: on X11 it means the _primary selection_,
  which is a different clipboard from the one `Shell::clipboard_request` reads,
  and binding it here would paste the wrong text for the users who expect it.
  `crcbl-shell` has no primary-selection seam and nothing has asked for one.
- **Verified on the headless shell and in a browser, and on no native backend.**
  The paste path is backend-agnostic — `Shell::clipboard_request` and the
  `ClipboardData` that answers it — and `HeadlessShell` implements the whole
  seam including Wayland's focus gate, but no test drives a real X11, Wayland,
  Win32 or AppKit clipboard through the _console_. Each of those backends has
  clipboard tests of its own; what is unproven is the console's use of them on a
  real display.

### A proc-macro dependency for identifier concatenation (2026-08-27)

`crcbl::web_exports!` shipped its impl half as `crcbl::impl_web_pending!`. What
is left un-extracted is the ten literal symbol names each sample declares.
Collapsing those needs a macro that can _build_ identifiers: `concat_idents!` is
unstable, and the names must stay per-sample so two demos in one browser cannot
collide.

That means **a new dependency, which is the owner's call**. Ten lines per sample
against one new crate in the tree.

**DECIDED 2026-09-06 —** no proc-macro dependency is taken for identifier
concatenation: `paste` was archived by its author in 2024-10, and the literal
names stay, with `check-exports` guarding drift at build time.

### The first contact of a run can press a button that was never drawn (2026-08-31)

`ConsoleButton::touched` gates `render` and `takes_pointer`, not
`TouchButton::offer` — so the first finger to land sets the latch and is offered
to the button in the same call, and if it happens to land in that corner it
opens the console before anything was on screen there. `PauseControl::touch` has
exactly the same shape and the same window, so this is consistency with the
precedent rather than an oversight; closing it would mean refusing the offer
until a frame has drawn the control, **in both places**.

Found while sabotaging the browser check: setting `touched = false` left every
check green, because the button still fires. That is what says this is a real
property of the design and not a slip.

### `config` cannot write a file, and a file takes no arguments (2026-08-31)

**Considered and left out, both.** Source's `writeconfig`/`host_writeconfig` has
no counterpart here: `save` writes `settings.toml` and nothing dumps the
console's current state as a runnable `.cfg`. A dump would have to decide which
of several hundred variables are worth writing, and the `save`/`dump` pair
already covers the settings half. Separately, Source's `.cfg` files are often
`alias`-driven; this one is a flat list of lines, and nobody has asked for
arguments or `alias`. Recorded so neither is re-derived.

### XDND action negotiation is copy, and only copy (2026-08-31)

**Deliberate, not a gap in the handshake.** `crates/crcbl-shell/src/x11/xdnd.rs`
answers every accepting `XdndStatus` with `XdndActionCopy` whatever the source
suggested, and reports the same in `XdndFinished`. The engine reads a path
another process handed it and never takes ownership of the file, so answering
`move` would be a promise to delete something. A source that offers only `move`
still gets `copy` back and decides for itself — the specification lets it.
`XdndActionList` and `XdndActionDescription` are not interned.

**What would change it:** a consumer that wants to _move_ a dropped file, which
needs `ShellEvent::DroppedFile` to carry the action first. The Wayland backend
made the same choice (`data::ACTION_COPY`) and would need the same change.

### The phys bench's default is a debug-build size, and its guard is quadratic

`--bodies` defaults to 2000, far below the scale `docs/plan/ROADMAP.md` talks
about, because the run's correctness guard is an `O(bodies²)` scan and the
default has to finish in a couple of seconds in a checked build. At
`--bodies 10000` that scan is ~100M predicate calls. Anyone sweeping upward
should use `--release`.

Considered and declined: giving the guard a cheaper independent reference, such
as a uniform grid. A second spatial index checking the first is exactly the
shape of check that agrees with the bug — the brute-force scan's whole value is
that it shares no structure with the thing it checks. If the quadratic cost ever
actually blocks a sweep, the honest fix is to check a sampled subset of queries
exactly rather than to check every query approximately.

**Not verified for this scenario:** any non-Linux target.

**The changelog's `--ticks` numbers were re-taken on a release build,
2026-08-24**, because a performance figure from a checked build is not a
performance figure. They moved: the query phase's p50 rises a little over
eight-fold from 1 tick to 100000 at `--bodies 2000`, where the debug run had
said fifteen. The structural half was unaffected — 3999 nodes, depth 12, one
build, at both tick counts — and the refit phase turned out not to be flat but
to get _cheaper_, 0.125 ms to 0.099 ms. Two runs of twenty iterations each side,
agreeing to within 0.07 on the ratio. One number in that entry is still a debug
figure and says so: the ~2% cost of folding which bodies answered, which cannot
be re-taken without removing the fold from the bench.

The commit message's numbers were left alone — history is history, and a commit
message cannot be corrected without rewriting it.

### DECIDED — the ten export names, and whether a proc macro earns a dependency

**DECIDED 2026-09-06 —** option (c): the ten literal export names stay, and no
proc-macro dependency is taken for concatenating them — `paste`, the crate that
would have done it, was archived by its author in 2024-10. `check-exports`
guards the drift at build time, which is what makes the literals safe.

The other half of `web_exports!`'s residue. `crcbl::impl_web_pending!` took the
forwarding impl; what is left is ten literal symbol names per sample, six
samples, and they cannot become constants: each is an `extern "C"`
`#[unsafe(no_mangle)]` export and the names must stay per-sample so two demos on
one page cannot collide.

Collapsing them to one token — `web_exports!(hud)` building `__crcbl_hud_boot`
and the rest — needs a macro that can **construct** identifiers.
`concat_idents!` is unstable, and `macro_rules!` cannot paste tokens into a
name. So this is a dependency question rather than a refactor.

- **(a) Take a small external helper** such as `paste`. One dependency, widely
  used, and the change is contained. The workspace has no `paste`, `syn`,
  `quote` or `proc-macro2` today, so this is genuinely new surface rather than
  one more use of something already vendored in.
- **(b) Write a workspace proc-macro crate.** No external dependency, and it
  could grow other jobs later. The costs are real: a new crate, and a proc-macro
  crate builds for the **host** even when the samples build for `wasm32`, so
  every sample's build gains that step.
- **(c) Leave the names literal.** Ten lines a sample of pure boilerplate, and
  no new anything.

**(c) is safer than it first looks, and this entry said the opposite until it
was checked.** `web/tools/check-exports.mjs` compares every symbol the shim
calls against the artifact's actual exports, and `web/build.sh` runs it **per
demo on every build**. A name that drifts from the JS calling it therefore fails
the build, not the browser: the mistyped export lands in the artifact, the shim
asks for the right one, and the check reports it missing. That holds whether or
not the names are macro-generated, because the comparison is against the built
artifact rather than against the Rust source.

So the choice is boilerplate against a dependency, with no safety difference —
which is a smaller question than it looked, and is why it is stated plainly
rather than argued.

### SHIPPED — how the glTF corpus becomes a gate

Record of the decision and of what landed. Option (b) was taken on 2026-09-06
and finished the same day.

**What is in the tree now.** `apps/viewer/assets/shelf.expect` is a committed
manifest in `shelf.sha256`'s shape — a value, two spaces, a key — naming the
import outcome of every model on the shelf: `Ok` for a document this importer
honours in full, `Unsupported(<extension>, …)` for one it draws without an
extension the file declared in `extensionsRequired`. `apps/viewer`'s
`every_shelf_model_imports_as_this_manifest_says` opens each fetched model
through `shelf::open_at` — the same path a file named on the command line takes,
meshlet build and framing box included — and asserts the outcome against that
line. A model the manifest does not name, a manifest line naming no shelf row,
and a model that no longer imports at all are each a failure that names the
model. Unfetched models are skipped loudly by name, on
`every_shelf_file_is_on_disk_once_the_shelf_is_fetched`'s terms exactly, and
CI's `test (linux)` job is where the fetch makes it ask its real question.

**The importer gained the one observable the manifest needs.**
`GltfScene::unsupported_required_extensions` carries what
`warn_unsupported_extensions` had only ever logged, and `apps/viewer`'s
`model::Model::unsupported` carries it past the point the imported scene is
dropped. Before this the required-extension report was a warning line and
nothing could assert it.

**All nine shelf models are `Ok`, and none of them declares an extension at
all** — measured, not assumed: `grep extensionsRequired` over the fetched shelf
matches nothing. So the `Unsupported` arm is a form the manifest can express and
the corpus does not yet exercise; it is parsed and round-tripped by
`the_expectation_manifest_covers_the_shelf_exactly` rather than left untested.
That is the shape of the first content-policy question below — a document whose
required extension this importer lacks — made blessable in advance of a corpus
that holds one.

**What the gate does not catch.** It asserts that a model imports and which
extensions it was drawn without. A change that silently degrades a model it
still imports — dropped normals, a lost texture, a coarser LOD — moves no
outcome and passes. That is a golden-image question, not a manifest one, and
`crates/crcbl/tests/gltf_e2e.rs` is still one synthetic textured quad.

**Cost, measured:** the walk adds 27.5 s to a debug `cargo test -p viewer` on
this machine with the shelf present (nine documents through meshlet build and
simplification), and nothing on a machine that has not fetched it.

The argument that produced the decision follows.

- **(a) Vendor a pinned subset.** A dozen or two models committed in a corpus
  directory beside `crcbl-scene`'s own tests, so the gate is hermetic, runs
  offline, and a regression is bisectable against the exact bytes. The cost is
  binary blobs in a tree that has none, which is the objection `gltf_fixture`'s
  header states in as many words — and these are blobs nobody can shrink, since
  the point is that they are real files.
- **(b) Download at gate time**, pinned to a commit of `glTF-Sample-Assets` and
  sparse-checked to a named list. Nothing is vendored and the corpus can grow by
  editing a list, at the cost of a network fetch in CI — a new failure mode for
  a job, and one that reads as a red gate rather than as an outage.
- **(c) Leave it a local script** that takes a corpus directory a developer
  already has. Honest about what it is, and it is the shape this file elsewhere
  calls out as a trap: a runner no workflow invokes is a test that executes
  nowhere, which is exactly how `run-gltf-e2e.sh` sat until 2026-08-20.

**(b) is what the hand-run actually did**, so it is the smallest step from
measurement to gate; **(a)** is the only one that is hermetic. Whichever lands
forces the two content-policy questions recorded with the measurement below — a
document whose required extension this importer lacks (18 of the 116 load
anyway), and the non-ASCII asset key that refuses `Unicode❤♻Test` — because a
gate has to assert an expected outcome for each. The first is answered above:
the manifest can say `Unsupported(<extension>)` and no model on the shelf needs
it yet. **The second is not**, and it is not the importer's to answer alone —
see the entry below.

### A non-ASCII asset key is refused by the key rule, not by the importer

Written 2026-09-06, while finishing the corpus gate above, because the decision
recorded in `docs/backlog.md` ("non-ASCII asset keys must load") names the
importer and the importer is not where the refusal is.

**Where it actually is.** `crcbl_store::web::is_key_byte` allows
`[A-Za-z0-9._-]` and nothing else, and `canonical_key` applies it per component.
`crcbl_assets::DirSource::read` calls `canonical_key` before handing the key to
`NativeStorage`, deliberately — that is what makes an asset tree which loads
from a directory one that can be served over HTTP. `NativeStorage::resolve`
itself allows any byte a filesystem does; it only refuses `..` and a prefix. So
`apps/viewer`'s `model::load`, which uses the file's own name as the key, and
`crcbl_scene::gltf_import`'s `uri_sibling`, which uses a glTF `uri` as one, both
hit the same rule from different directions and neither owns it.

**The corpus does not reach it.** None of the nine shelf models has a non-ASCII
file name or `uri` — checked over the fetched shelf — so
`every_shelf_model_imports_as_this_manifest_says` cannot see this, and adding
`Unicode❤♻Test` to the shelf would be a licence read and a panel row rather than
a test fixture.

**What the decided fix costs, so the next slice does not re-derive it.** The
decision is to percent-encode on the way in, keeping the key ASCII while the
file is not. That is coherent — a percent-encoded key is exactly the URL a
server wants, and a glTF `uri` is already percent-encoded by the specification,
so the importer's `uri_sibling` would need no change at all. It lands in three
places:

- `canonical_key` accepts `%XX`, and validates the **decoded** component: no
  `/ \ : ? # % @`, no whitespace, no control byte, not `.` or `..`, valid UTF-8.
  That keeps every property the rule exists for —
  `a_buffer_uri_that_is_not_a_legal_asset_key_never_reaches_the_filesystem`'s
  three cases all still fail, since `%20` decodes to a space and `%2e%2e` to
  `..`.
- `DirSource::read` percent-decodes the canonical key before `NativeStorage`
  sees it, and the OPFS backend does the same before `getFileHandle`.
  `FetchSource` needs nothing: the encoded key _is_ the URL path.
- `apps/viewer`'s `model::load` percent-encodes the file name it takes off the
  command line, and `USAGE` stops stating a rule it would no longer have.

**Why it was not done here.** It is a change to the browser security boundary in
two crates the corpus slice does not own, and it needs a percent codec —
"encoding and escaping" is named in CLAUDE.md as a thing not to hand-roll, and
taking a dependency for it is the owner's call. It is its own slice, and it
wants `Unicode❤♻Test` (or a fixture with the same shape) as its gate.

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

### DECIDED — `data:` URI glTF needs base64, and the workspace has no decoder

Decision record; the decision is in `docs/backlog.md`.

as base64, and `git grep` finds no base64 decoder anywhere in the workspace and
no such crate in `Cargo.lock`. CLAUDE.md's reach-outward rule puts a dependency
the project already has ahead of writing one, and names **encoding**
specifically as a thing not to hand-roll — while also making a _new_ dependency
the owner's call. So the two halves of the rule point in different directions
here and only the owner can settle it.

- **(a) Take a base64 crate.** `base64` is the ecosystem's answer, is widely
  used, and has had its edge cases found in public — padding, whitespace, the
  URL-safe alphabet, and rejecting trailing bits that decode to nothing. One new
  dependency in `crcbl-scene`, which today has none of this kind.
- **(b) Write the decoder.** RFC 4648 §4 is small and the workspace has a
  precedent for transcribing a named algorithm with specification test vectors.
  The risk is precisely what the rule warns about: a decoder that is right on
  every file anyone tries and wrong on one padding case, which compiles, reads
  plausibly and passes every test somebody thought to write. It also has to
  decide what to do with the things real exporters emit — whitespace inside the
  payload, a missing `;base64` for a plain-text URI, percent-encoding.
- **(c) Keep refusing, and say so better.** The current message already tells
  the user to re-export as `.glb` or keep the `.bin` beside the `.gltf`, which
  is honest and actionable. Costs nothing and closes nothing.

**Worth knowing before choosing:** whichever way this goes, the payload is
untrusted input from a file the user was handed, so the decoder needs a length
bound before it allocates — `crcbl-sprite`'s PNG path already refuses a tiny
file declaring a huge size, and this wants the same treatment. That argues
mildly for (a), since a maintained crate has already been made to care.

## Profiling and benchmarking: decisions taken 2026-08-13, before any code

Decision record; the work these leave owed is in docs/backlog.md under the same
heading. The profiling plan these came from was deleted on 2026-09-24; the rest
of its rules are under _What the deleted 40-profiling plan left behind_ at the
top of this file. It was a cross-cutting track in the roadmap alongside CLI,
testing, audio, persistence, debug tools and pixel art. The decisions, so they
are not re-argued when a slice starts:

- **Trace export is Chrome Trace Event JSON**, which Perfetto and
  `chrome://tracing` both read. Still unwritten: `crcbl_core::trace::Snapshot`
  has `report()`, a human summary, and no JSON emitter. Text, no dependency, and
  `crcbl-cli` already has JSON machinery. **Tracy was considered and declined
  for now**: it is a client library, therefore a new dependency and the user's
  call, and its wire protocol is not something to hand-roll. If it is wanted
  later it is an optional feature over the same span data rather than a second
  instrumentation pass.
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
`DebugModule`. That half is good. What is absent is: no baseline or comparison,
no trace export, no memory or pool-occupancy accounting, no `crcbl-jobs`
instrumentation, and counters scattered across `SceneStats`, `visible_count` and
each sample's own rows rather than one place. `crcbl bench` has since landed —
`crates/crcbl-cli/src/bench/` with its `jobs` and `phys` scenarios, warm-up,
p50/p95/p99/max, `MIN_PERCENTILE_SAMPLES`, `--json` and a mandatory environment
block — and that module's own header records that `--compare <baseline>` and
`--trace <path>` are the rows it did not start.

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

**`crcbl_render::MAX_TIMED_PASSES` bounds this crate's renderers, not the
caller's own passes** — a deliberate call, taken 2026-08-13. Every renderer here
carries a `MAX_PASSES` and the constant is their sum, so a pass added anywhere
below moves it; but a sample that records a pass of its own — the 2D samples
each have a clear — is that much over. Today none of them is close (they record
four against a bound far above that), and the once-per-`PassTimers` warning is
the backstop if one ever is. The alternative was every sample writing
`MAX_TIMED_PASSES + 1`, which is the guessing this constant exists to end.

## Re-affirmed: shader artifacts stay committed (2026-08-13)

Record; the coverage gap it leaves is in docs/backlog.md under the same heading.
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

## `crcbl_scene::meshlet`: decisions taken, and what it does not do

Record; what the bake step does not do is in `docs/backlog.md` under this
heading.

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

## A broken intra-doc link poisons rustdoc's report for the same name (2026-09-07)

Writing `[`impl_game_gpu!`]` above that macro's own definition in
`crates/crcbl/src/engine.rs` made `cargo doc` also report two correct links to
the same macro, further down the file, as unresolved. Verified by documenting
the previous commit's `engine.rs`, which is clean. So a doc run's error list is
not a list of independent defects: fix the first and re-run before chasing the
rest. The fix was the explicit path `[`impl_game_gpu!`](crate::impl_game_gpu)`.

## Prettier's object expansion is sticky, so one wrong run leaves churn behind (2026-09-10)

`~/.editorconfig` on this machine sets `indent_size = 4` for `[*]` and names no
JavaScript extension, and prettier reads it unless told not to. A run without
`--no-editorconfig` over `web/tools/browser-e2e.mjs` therefore reindents the
whole 11,000-line file, which CI's runner — where no such file exists — would
then reject. That much was already known.

**What was new is that re-running with the flag does not undo it.** Prettier
preserves an object literal's multi-line-ness: once a `{` has a newline after it
in the input, the object stays expanded however short it is. So the four-space
pass broke several short objects across lines because they no longer fitted, and
the two-space pass that "reverted" it kept them broken. The result is a diff
that passes `prettier --check` and still carries seven unrelated reformats in
other demos' blocks, which is exactly the churn a reviewer has to read and
decide about.

**So: the flag goes on the first run, not the second.** If a wrong run has
already happened, the repair is to collapse each expanded object by hand against
`git diff` and re-check, not to re-run the formatter. Verified by doing it: the
towers slice's gate-file diff went from 312 insertions and 9 deletions to 273
and 3, the three remaining deletions being the comment it meant to replace.
