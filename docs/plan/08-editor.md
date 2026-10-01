# Stage 8 — Scene Editor

`apps/editor`: the editor is a client of the engine (locked decision). It uses
the same renderer, ECS, server loop, transport, and GUI as a game. MVP editor:
open scene, move things, edit properties, save, play.

## Status: slices 1, 2 and 3 landed 2026-09-16, slices 4 to 6 2026-09-30, slices 7 to 15 2026-10-01, and what still waits

**Performance follow-up:** `apps/editor/src/app/instances` retains each placed
entity's last description and publishes changes before `begin_frame`. Unchanged
draws become quiet while the renderer preserves its settling frame. Null-backed
editor tests cover shadow reuse, current/previous transforms, missing bounds and
undo/redo with replacement history; recorded uploads verify the frame-ring
drain. The explicitly run Vulkan screenshot test matches eager writes on
Radeon/RADV and llvmpipe/lavapipe through edits and history changes, with
validation logs checked. Disabling publications failed the image comparison;
eager writes failed the quiet-upload check. Both restored runs passed.

Paired sequential release runs presented 550 frames at 960x720, timing 500 after
warmup. The default scene contains 4 entities; the dense fixture contains 1024.
Frame, command and entity counts were checked. Timings include preparation,
acquisition/submission and frame-ring waits, exclude startup, and are not
isolated GPU timings.

| Scene and repeat | Eager p50/p95 (ms) | Filtered p50/p95 (ms) |
| ---------------- | ------------------ | --------------------- |
| Default, first   | 0.287/0.381        | 0.252/0.299           |
| Default, repeat  | 0.283/0.329        | 0.249/0.287           |
| Dense, first     | 0.270/0.307        | 0.229/0.267           |
| Dense, repeat    | 0.268/0.304        | 0.228/0.278           |

`apps/editor` exists: a native, single-process tool that loads `apps/breakout`'s
board, renders it with `crcbl_render::orbit::OrbitCamera` and the ground grid,
lists the entities per system, picks one by ray, draws its bounds, nudges it
with keys through the command enum and the undo log, and saves byte-stably with
a dirty marker in the title. That is "The smallest slice that uses only what
exists" below, delivered. `tools/check-doc-citations.sh` no longer allow-lists
the path, and `tools/run-samples-windowed.sh` runs the binary windowed.

**Two of the three things that section said the slice also needed already
existed.** Key, button and wheel events have carried `Modifiers` since the shell
seam landed (`crates/crcbl-shell/src/event.rs`, whose module docs open by saying
modifiers are stamped onto every event), and
`crcbl_render::debug_draw::r_debug_draw` is a writable console bool that
`crates/crcbl/tests/mesh_e2e/debug_draw.rs` already sets. Only the screen-to-ray
helper was missing; it is `crcbl_render::Camera::ray_through`, beside
`Camera::depth_of` whose inverse it is.

**Slice 2 removed the editor's wiring to one game.** `crcbl::registry` is the
component registry missing piece 5's second half asked for: one
`Registry::register::<T>(system)` call produces the chunk codec, the `System<T>`
a load spawns into, the `&mut dyn Reflect` an edit is applied to and the
`Placement` a collider and a bounds box come from, so those four cannot drift
apart. It lives in the umbrella because it needs `crcbl-ecs`, `crcbl-scene` and
`crcbl-reflect` at once and every lower home would gain an arrow its own docs
call deliberately absent. `apps/breakout` and `apps/puppet` register their own
components and load their own scenes through it, and slice 1's hand-written
vocabulary module is gone.

**Slice 3 drew the panels**, which is task 3's "outliner + property panels on
the inspector foundation": a `Ui::dock` layout holds the scene's entities per
system over the selected one's fields beside the viewport, the outliner's
selection _is_ the document's in both directions, every inspector edit is an
`EditCommand` so undo and the collider follow it, and the layout is saved to the
player's `settings.toml` and read back. The editor's keys became an `ActionMap`
with the reserved `ui` and `text` contexts pushed from what the panels report.

**Slice 4, the viewport, landed 2026-09-30** on the decision of the same day
(below): the pane shows a rendered view that a UI rectangle samples, and the
full-window draw under a hole in the panels is gone.

- **The UI half is general, not the editor's.** `DrawList::texture` pushes a
  `DrawCommand::Texture` naming a caller-chosen `crcbl_ui::TextureId`, and
  `UiRenderer::add_passes_with_textures` pairs each name with an `ImageId` of
  the same graph. The UI pass declares a read of the image, so the graph puts
  the barrier out of the pass that drew it; `ui.slang` samples it at set 1,
  bound per run of texture quads with a transparent 1×1 for everything else,
  because WebGPU has no texture array to index. A game's picture-in-picture is
  the same call.
- **The editor draws its primary camera into the pane, not a `create_view`
  target.** A renderer always draws its primary camera, so a second view would
  leave the primary drawing a full-window picture nobody sees. The primary into
  a transient of the pane's extent is the same picture (a default view is the
  primary's byte for byte, which `forward_e2e`'s `views` suite holds), and a
  second pane is where `create_view` comes in. A pane that changes size gets a
  target of the new size from the transient pool on the next frame.
- **Picking goes through the pane's camera**: the click less the pane's
  top-left, unprojected against the pane's extent, which is the matrix the
  picture was drawn with. Framing and panning measure the pane too.
- **Evidence.** Null-backend tests hold the draw split, the set-1 binds naming
  the view's own image and the attachment-to-`ShaderRead` barrier between the
  view pass and the UI pass (`crcbl_render::ui_pass::texture_tests`); the
  editor's tests pick in the offset pane where the pane's and the window's
  cameras disagree and resize the pane; `forward_e2e`'s
  `views::ui::a_ui_rectangle_draws_the_view_it_names_texel_for_pixel` reads a
  view back beside the frame the UI composited it into and finds the same pixels
  on Vulkan (RX 7900 XTX). Binding the blank instead of the view, and dropping
  the read declaration, each turned a test red.
- **Not verified here**: the rebuilt `ui` shader on Metal, D3D12 and a browser.
  Its SPIR-V and WGSL were rebuilt locally; the MSL and DXIL come from CI's
  regenerated shaders, and nothing but CI has run them.

**Slice 5, task 4's entity commands, landed 2026-09-30.** `EditCommand` gained
`Spawn` and `Delete`, each the other's inverse, and a duplicate is a spawn:

- **A spawn carries the id and the row.** The row is the component as one chunk
  row's RON text, read by `SystemChunk::row` and rebuilt by `attach_row`, so a
  command stays a value that could be sent or pasted. An undone delete files a
  new entity under its **old** `SceneEntityId` (`IdMap::remove` and `restore`),
  so a later command in the history that names it still finds it; `IdMap` never
  hands a removed id out again. A duplicate is a spawn of the original's row
  under `IdMap::next_id`, so its undo is the spawn's own inverse.
- **A spawn the scene cannot hold is refused before anything is attached**: an
  id in use, a system the manifest does not list (a save would drop it), or a
  row that is not the system's component.
- **Delete and Ctrl+D** act on the selection; the copy is selected.
- **The property test** plays `entity_tests::HISTORIES` seeded random histories
  of nudges, duplicates and deletes and walks each back and forward again,
  comparing the saved scene text at every step. Not `World::hash_state`: it
  hashes `Entity` bits, which a restored entity changes (the backlog records the
  decision). Restoring under a new id, skipping the sweep on delete, and
  skipping the collider on spawn each turned it or its neighbours red.
- **Entity copy and paste** (feature 8's entity half): Ctrl+C offers the
  selection as `apps/editor/src/clipboard.rs`'s clipping, the system and row per
  entity, under both the engine's RON mime and plain text; Ctrl+V reads the
  clipboard's text and spawns every entity it names under fresh ids as one
  `EditCommand::Batch`, so one undo takes a paste back and a paste with one
  entity the scene cannot hold spawns none. The field half of feature 8 landed
  in slice 10, below.
- **Still owed from task 4's list**: load/save markers (nothing for them to mean
  while a load replaces the log). Rename landed in slice 10, attach and detach
  in slice 11. The backlog's editor entry says what the markers wait on.

**Slice 6, the translate gizmo (task 5's first half), landed 2026-09-30.**

- **Decided 2026-09-30: handles are drawn in the pane's screen space**, through
  the UI draw list over the scene's picture, rather than through the world-space
  debug-draw layer the feature list named. On top of everything in the pane and
  a constant number of pixels long by construction — the plan's "constant
  screen-size scaling" — and hit tested in the pixels they are drawn in. Debug
  draw is lines only, depth tested and exposed with the scene, so it would have
  needed an on-top mode and a constant-size transform, and a handle exposed with
  a dark scene goes dim. `docs/plan/18-render-features.md`'s overlay rule
  records the exception.
- **A handle per axis** from the selection's projected centre
  (`crcbl_render::Camera::pixel_of`, `ray_through` run the other way); an axis
  pointing along the view has none. A press on a handle drags instead of
  picking; the entity moves along the axis line through where the drag began, to
  the point closest to the cursor's ray, by as far as that point has moved since
  the press. (Ctrl snapped the move in quarter-metre steps from the press; slice
  7 made it the absolute grid.)
- **A drag is one undo.** Each write is a `position.N` property set through
  `Document::apply_in` with the drag's `Gesture`, and `UndoLog::record_in` folds
  a gesture's writes to one leaf into one entry that keeps the first write's
  inverse. A save seals the entry, so a drag carried past it is dirty again. The
  inspector's field drags ride the same mechanism: edits to one leaf while the
  primary button is held share a gesture (`Panels::apply_edits`), so a field
  dragged over many frames is one undo too — it was one per frame.
- **Evidence**: the gizmo's tests hold handle direction, constant size at two
  distances and two scales, hidden axes and the closest-point formula against
  hand-worked rays; the editor's loop test drags the X handle through the
  headless shell and reads back an X-only move and one log entry. Writing the
  drag through plain `apply`, flipping the sign of the formula's `b` term (which
  first survived, until an oblique ray was added) and dropping the seal each
  turned a test red.
- **Owed from task 5** after slice 6: rotate and scale handles, plane handles,
  and snapping to an absolute grid. Slice 7, below, built all but rotate, and
  slice 15 built rotate.

**Slice 7, the rest of task 5's gizmos, landed 2026-10-01.**

- **Two modes, W and R.** Translate adds a square per plane in the corner
  between its two arrows, which moves the selection across that plane; scale
  draws a box-tipped line per axis and a square at the centre. A square takes a
  press before a line does — it is drawn on top and is the smaller target — so a
  press where a plane square lies over an arrow is the plane's, and one at the
  centre is the uniform scale's although every line starts there. A plane seen
  nearly edge-on has no square, since a slip of the pointer would be a long
  move.
- **Scale finds its field by name**, as translate finds `position`: a handle
  writes `half_extents.N` through the component's reflected paths, so any
  registered component with that field scales and the editor names no type. An
  entity without one shows no scale handles, and choosing scale on it says so on
  the status line. An axis adds the cursor's travel along it to that half
  extent; the centre multiplies all three by one plus the pointer's travel to
  the right in handle lengths. Nothing writes a half extent below
  `gizmo::MIN_HALF_EXTENT`.
- **Snapping is to the absolute grid**: with Ctrl held a centre lands on the
  nearest multiple of `editor.snap.grid` and a half extent on one of
  `editor.snap.scale`, both settings in the editor's `settings.toml`
  (`gizmo::Snap::load`; a value that is not a positive number falls back to the
  default and is logged).
- **A drag is still one undo.** A plane or centre drag writes its leaves as one
  `EditCommand::Batch` a frame, and `UndoLog::record_in` folds a gesture's batch
  into the entry on top when it names the same leaves in the same order.
- **Rotate cannot be entered.** `gizmo::Mode` has no rotate variant, and E puts
  a refusal on the status line and leaves the mode as it was: the scene format
  carried no rotation for a handle to write. (Slice 14 put one in the format,
  and slice 15 built the rings.)
- **Multi-select is not a question yet**: the document holds one selection
  (`Document::selected`), and the outliner's Ctrl and Shift clicks select rows
  of which the document takes the first. Every handle acts on that one entity.
- **Evidence**: the gizmo's tests hold plane-square placement, the edge-on drop,
  square-over-line priority on a hand-built overlap, plane drags writing exactly
  their two leaves through oblique rays, per-axis and uniform scale, the
  minimum, absolute snapping from an off-grid start and the settings fallback;
  the editor's loop tests drag a plane, a scale axis and the centre through the
  headless shell (one entry each, undone to the saved text), snap a nudged block
  onto the grid with Ctrl held, find no scale handles on a puppet entity without
  half extents, and press E in both modes. Each of these mutations — lines
  before squares, a plane writing all three leaves or crossing the wrong plane,
  snapping from the press, no minimum, an additive uniform scale, its sign
  flipped, batches never folding, scale handles without the field, E entering
  scale, Ctrl ignored, no settings validation, edge-on planes kept, a plane
  square off its corner, and R unbound — turned a test red.

**Slice 8, play mode's mechanism (task 7), landed 2026-10-01**, on the decisions
of the same day (below).

- **A vocabulary registers behaviour beside components.**
  `crcbl::registry::Registry::module(system, factory)` records a
  `fn() -> Box<dyn GameModule>` under the system whose presence says a scene is
  that game's, and `Registry::modules(systems)` builds a fresh instance of each
  in registration order. Keyed by a system as `Registry::check` is, rather than
  "every registered module": the shipped vocabulary holds four games, and one
  game's rules ticking on another's scene would move things it never meant to.
- **Play** (`Document::play`, F5 or the toolbar) saves the scene's files to
  memory as the snapshot, builds the scene's modules and calls each one's
  `register` on the document's world. Each frame hands `Document::advance` the
  frame's time; a `FrameClock` at the world's own tick period (read after the
  modules registered, so a module may set it) runs whole ticks, each in the
  server's order — `World::tick`, every module with `ClientInputs::empty()`, a
  sweep — and drops ticks past its catch-up cap rather than owing them. The
  colliders are rebuilt after a frame that ticked, so a click picks what a
  module moved where it is drawn. A world left with no usable period is refused
  (`EditError::TickRate`) and restored first.
- **Pause** (F6) stops the ticks and banks no time; resume picks up from there.
  **Stop** (F5 again) loads the snapshot back through `Document::open`'s own
  load into a fresh world. The undo log, the saved position and the gesture
  counter are untouched, the selection is kept when its entity is there, and
  `IdMap::reserve` carries the id map's high-water mark across — the files spell
  the ids a scene holds, not the id of a copy the log deleted, which a reloaded
  map would otherwise hand out again.
- **Every edit is refused in play mode**, paused or not, by one check at the top
  of every `Document` method that writes the scene, the log or the disk
  (`EditError::Playing`); an inspector edit's panel write is rewound before the
  refusal. Every refusal reaches the status line as a warning naming play mode:
  the keyboard's through `act`, the inspector's through `Panels::apply_edits`, a
  gizmo drag's through `move_handle`, and a paste's when the clipboard answers.
- **The toolbar** is a strip over the panes, outside the dock, with Play/Stop
  and Pause/Resume buttons and the play state, so the saved layout's panes are
  unchanged and `layout::load` keeps every saved file. F5 and F6 are bound in
  the default context; neither reserved context binds a function key, so they
  reach the editor while a panel holds the keyboard, and only the editing rule
  stops them.
- **No sample registered a module in this slice**, and the shipped vocabulary
  has no demo one: the tests play a test-only module that moves every greybox
  block. Towers' is slice 9's, below.
- **Evidence**: the document's play tests hold a tick through the bounds and a
  pick, pause banking nothing, a byte-for-byte restore after play changed the
  scene, an edit, a duplicate and its delete surviving play with the log walked
  back to the opened scene afterwards and no reused id, every edit refused
  playing and paused, the accumulator's tick counts and cap, a module-less play
  ticking the world, and the refused tick period. The loop tests drive F5, F6
  and both toolbar buttons through the headless shell and refuse a keyboard
  edit, undo and redo, a save (from a directory and from the compiled-in scene),
  a paste and a gizmo drag on the status line; the panel test refuses an
  inspector drag. Each of these mutations turned a test red: modules never
  ticked, the world's schedule never run, one tick a frame whatever the time, no
  collider rebuild after a tick, pause ignored or its time banked, stop not
  restoring, the restore dropping the id mark or the selection, a bad tick
  period not restored, nothing refused (and, separately, save, save-to, undo and
  paste unguarded or checked in the wrong order), the inspector's refusal before
  its rewind or off the status line, the gizmo's off the status line, F5
  unbound, the toolbar's buttons swapped or its clicks dropped, the loop never
  advancing play, and the module-less status reversed; the registry's system
  filter and `IdMap::reserve`'s body each turned their own test red.

**Slice 9, towers' play module, landed 2026-10-01.** F5 on towers' field runs
towers' game on it, and creeps visibly walk the lane.

- **A module is built from the scene's files and may refuse them.**
  `ModuleFactory` became `fn(&dyn AssetSource, &Path) -> Result<_, String>`, and
  `Document::play` hands `Registry::modules` the snapshot it already took:
  towers builds its map through the same `Map::load` `--scene` runs, so a field
  its rules refuse (a diagonal leg) does not play, and the status line names the
  rule (`EditError::Unplayable`). Every module is built before any registers, so
  a refusal leaves nothing to take out of the world.
- **Towers ticks as solo, with one local player asking for nothing.**
  `run_team_tick` holds the run still on a tick with no command frame at all, so
  towers' module (`crate::game`'s `play`, registered under `waypoints`) ticks
  through `TowersModule`'s `GameModule` half, which reads empty client inputs as
  the one solo player's empty frame. The build phase runs down, the waves come,
  creeps walk and leak, and a lost run restarts itself — the game's own rules,
  by the same call solo makes.
- **What a module spawns is a runtime component: drawn, never listed or saved.**
  `Registry::register_runtime::<T>(system)` records a placement and an entity
  list and no codec, so a manifest naming the system is refused by `Scene::load`
  and `Scene::save` has nothing to write it with — the guarantee is the missing
  codec, not a flag a save must read. Towers mirrors each creep into its own
  `walkers` system as a `Walker` (the box around its sphere), keyed by the
  creep's physics body and despawned when it dies or leaks. `Document::spawned`
  and `spawned_bounds` are what the instances draw, under a
  `Drawn::Spawned(Entity)` key beside the scene's ids; the outline, the id map
  and the colliders a click picks by are the scene's alone, and stop throws the
  world away with every creep in it. Declined: drawing every id-less entity that
  has a placement, which needs the creeps registered as scene components — and
  then a scene file could list them.
- **Evidence**: towers' own tests tick the module as play does and hold the
  first creep held back through the build phase and released after it, the
  mirror agreeing with the stage tick by tick while creeps leak and lives drop,
  and a bent path refused by name. The document's tests on the committed field
  hold the first creep after the build phase (tick counts derived from towers'
  constants), a creep drawn at its radius walking the first leg on the lane,
  every creep that leaves doing so at the exit and no longer drawn, the files
  unchanged through play and byte-identical after stop with no creep left, no
  creep listed, counted or picked over the corner it stands on, a bent path
  refused by name and playing again once undone, and breakout's, puppet's and
  the greybox scene running no module. The loop test opens the committed
  directory, plays it through F5 and counts the creeps in the renderer's live
  records, then none after F5 stops it. The registry's tests hold a refusing
  factory, a runtime component placed and absent from the vocabulary, a scene
  naming a runtime system refused, and both name clashes. Each of these
  mutations turned a test red: the registry's system filter removed, runtime
  placements or runtime entities skipped, a factory's refusal dropped, the name
  clash ignored, the stage ticked with no player, the mirror never despawning or
  never moving a creep, the module playing the committed field whatever the
  scene, the instances skipping spawned entities, play ignoring a refusal,
  `spawned_bounds` answering for scene entities, creeps given colliders, and
  stop not restoring.

**Slice 10, entity names, rename and field copy/paste, landed 2026-10-01**, on
the decisions of the same day (below).

- **The scene names its entities.** `crcbl_scene::scn::names` reads and writes
  the optional `names.ron` the header declares; `Scene::entity_name`,
  `entity_names` and `set_entity_name` hold the names at run time, and a save
  writes them in id order. Every committed `.scn/` is unchanged.
- **`EditCommand::Rename`** names an entity or takes its name away, its inverse
  the rename back; `Document::rename` turns a typed name into one (empty text
  clears, the same name records nothing) and is refused in play mode. A spawn
  carries a name, so an undone delete brings the name back with the id; a delete
  takes it, since a save refuses a name whose entity is gone.
- **The outliner shows a name, and renames in place.** A named row reads
  `Gate #2`, an unnamed one `#2`. F2 (bound in the default context, reaching the
  editor while an outliner row holds the keyboard) or a double-click on a row
  (`crcbl_ui`'s `OutlinerState::double_clicked`) puts a text input in the row
  and engages it once it is laid out (`Panels::begin_rename`); accept or a click
  elsewhere commits through `Document::rename`, back cancels, and a refusal is a
  status-line warning.
- **Feature 8's field half: one leaf at a time.** `crcbl_ui`'s `Inspection`
  names the leaf under the pointer and the one holding focus, and
  `Panels::field_target` is the focused one, else the hovered one. Ctrl+C with a
  target copies its value as the chunk file's ron text (`Document::copy_field`);
  Ctrl+V reads the clipboard as the leaf's kind through `ron::from_str`, the
  deserializer the loader calls for that leaf, and applies it as one
  `SetProperty` (`Document::paste_field`), so the leaf's own refusal (a
  non-finite position) is a refusal too. With no target the keys copy and paste
  entities, and a paste remembers its target from the key press. A whole vector
  is not a field: its ron shape is its type's serde derive, which the reflected
  value does not carry.
- **Evidence**: the scene's tests hold the named round trip and its vanishing
  when the last name goes, id order, and each refusal (unknown id on load and on
  save, empty, too long, control character, an id named twice, a declared file
  that names nothing or is missing, an unknown field) with an undeclared file
  ignored. The editor's hold rename, undo and redo against the saved text, the
  no-op, the refusals and play mode; a delete's name and its undo; the duplicate
  and paste rule; a named scene saved and reopened; the outliner's labels
  through a rename, an undo and a delete; the F2 flow typed and committed and
  backed out of; a double-click; the field target by focus then pointer; field
  copy against the file's own text, paste and undo, bad pastes and play mode;
  and, through the loop, routing by the pointer, a paste landing where it was
  asked after the pointer left, a bad paste on the status line, and F2. The undo
  property test now renames too, compares `names.ron`, and asserts some history
  named an entity. Each of these mutations turned a test red: the header always
  declaring names, the loader ignoring the declaration, each refusal removed,
  the save check removed, no `serde` default on the header field; the inspector
  never reporting hover or focus, the vector row not reporting, the
  double-click's time, row or reset checks removed; a delete keeping the name, a
  spawn dropping it, the rename's inverse forgetting, a duplicate copying the
  name, a paste ignoring names already taken, the label ignoring names, the
  panels not re-reading names on a rename or on opening, the rename's input
  never built, never engaged or never committed, a rename begun in play, the
  double-click ignored, hover before focus, paste or copy ignoring the field,
  the play checks removed, F2 unbound, a pasted float off by one, a copied float
  narrowed to `f32`, a no-op rename recorded, a cancel committed, and an unnamed
  clipping writing `name: None`. A clipping's `name` with no `serde` default
  survived, being equivalent — serde reads a missing `Option` field as `None` —
  so the attribute was removed.

**Slice 11, one entity in several systems and attach/detach, landed
2026-10-01**, on the decision of the same day (below).

- **The format.** A `SceneEntityId` may appear in several chunk files, and every
  row it has belongs to one `Entity`: `ChunkOf::read` attaches a row to the
  entity an earlier chunk (in manifest order) bound its id to, and spawns one
  only for an id not yet bound. One pass rather than a bind pass and an attach
  pass, because a chunk's rows are typed by its codec and a second pass would
  have to hold them type-erased in between; manifest order already makes the
  entity bits a function of the files. An id twice in **one** chunk is still
  `ScnError::DuplicateId`, the check moved from `IdMap::bind` to the chunk read,
  and a header naming a system twice is `ScnError::RepeatedSystem`. The writer
  is unchanged and `Scene::FORMAT` is still 0: every committed `.scn/` writes
  back byte for byte. `SystemChunk::detach_row` takes one system's component off
  an entity as a row; `scn::row_text` is the one spelling of a row.
- **The registry answers per system.** `Registry::component` takes the system;
  `systems_of` lists every system holding an entity (name order) and replaces
  `system_of`. **The placement rule**: `placing_system` is the first system in
  name order whose `Placement` answers, and `placement` reads it. Name order is
  the registry's own, a function of the names alone, so a pair of components
  places an entity the same way in every scene and every tool of one vocabulary,
  where manifest order would differ between scenes and registration order would
  be a fact about which game's line ran first; a component answering `None` (a
  sun) passes the question on. It stands in for one placement per entity, which
  a scene-level transform system would be.
- **A new component starts at its type's `Default`**, which `register` now
  requires, for `Placement`'s reason: a component a tool cannot make fails to
  compile rather than arriving as nothing to attach, and the game chooses the
  value beside its fields. Copying another entity's row was declined: it ties a
  new component to whichever entity comes first, and a scene holding none of a
  system has nothing to copy.
- **The editor.** `EditCommand::Attach` and `Detach` are each other's inverse,
  through the log and refused in play. **Detaching the last system is refused**
  (`EditError::NoComponent`), not turned into a delete: a delete's inverse is a
  spawn carrying the name too, so a detach that sometimes deleted would have an
  inverse of two shapes, and "remove this component" removing the entity is not
  what was asked. `Spawn` carries every system's row, so delete, its undo,
  duplicate and paste carry all of them; `SetProperty` names its system. The
  clipping writes the other systems' rows in `others`, only when there are any,
  so older clippings paste. The outliner lists an entity once, under the first
  manifest system holding it; the inspector draws a section per system (manifest
  order) with a Remove button while there is more than one, and an add button
  per **manifest** system the entity is not in — a component in an unlisted
  system would be dropped by the next save, and adding a manifest entry is a
  header change no command makes yet. Arrow keys and the gizmo move the placing
  component. A detach that takes the placement away removes the entity's pick
  collider and its instance.
- **There is no `crcbl scene` subcommand** in `crates/crcbl-cli` to update: the
  CLI's verbs are `new`, `run`, `build`, `screenshot`, `replay`, `crpix`, `lod`,
  `import`, `bench`, `sim` and `settings`.
- **Evidence**: the scene's tests hold one entity across two chunks loading as
  one `Entity` with both components and writing back byte for byte, an id twice
  in one chunk and a system twice in a manifest refused, and a detached row that
  leaves the rest and attaches back; the registry's hold per-system answers, the
  placement rule with a non-placing component in the way, and the default row.
  The editor's hold the two-system document listed once, attach/detach with undo
  and redo against the saved text, every refusal, a delete's undo restoring both
  rows, duplicate and paste carrying both, an older clipping pasting, refusal in
  play, the outliner's one row per entity, the inspector's sections and buttons,
  a field edit landing in its section's system, a lost placement leaving the
  drawn instances, and the undo property test, which now runs on the two-system
  document with attach and detach steps and asserts both ran. Each mutation
  listed in the commit that landed this turned a test red.

**Slice 12, physics on scene components, landed 2026-10-01**, on the decisions
of the same day (below).

- **A body is a scene component.** `crcbl::scene_physics` (behind the umbrella's
  `scn`/`scene` features, so any vocabulary takes it with
  `scene_physics::register`) registers
  `Body { kind, mass, friction, restitution }` under `bodies`, with
  `BodyKind::{Dynamic, Static, Kinematic}`, the parameters `crcbl_phys` takes
  (`RigidBody::new_dynamic(mass)`,
  `SurfaceMaterial::new(friction, restitution)`). It derives `Reflect` and
  serde; its `Default` is dynamic, one kilogram, `SurfaceMaterial::DEFAULT`'s
  surface. Values are validated on load through serde's `try_from` — a mass not
  finite and above zero (for every kind, so switching a row to dynamic never
  makes it invalid), a friction not finite and non-negative, a restitution
  outside `0..=1` is `ScnError::Parse` naming the file, line and field — and a
  `Registry::check` under `bodies` reports the same of a value a panel set.
  `crcbl-phys` gained no dependency: the component lives in the umbrella, which
  depends on both sides.
- **The shape is the placement.** A body collides as the box
  `Registry::placement` gives its entity; `Body` itself answers no placement, so
  the block beside it places the entity. `ModuleFactory` became
  `fn(&Registry, &dyn AssetSource, &Path)`, so the bodies' factory loads the
  scene's files with the vocabulary's codecs and refuses, naming the entity, a
  body with nothing placing it, a placement with no extent on an axis, or a
  placing component with no `position` leaves.
- **Play builds simulated bodies and the simulation's pose wins.** The module
  registers a `Simulation` system — a `PhysicsSystem` with contacts and Earth
  gravity, under a type of its own — with a box per body at its placement
  (static: a transform and a collider with no body; kinematic: a body with zero
  velocity; dynamic: a body of its mass). The world's schedule steps it, and the
  module writes each dynamic body's centre into the placing component's
  `position.N` through the registry's reflected path (`registry::POSITION`, the
  leaf the gizmo writes too), keeping the component's own offset between its
  `position` and its placement centre. The editor draws and picks the motion
  through the paths it already has; stop throws the world away.
- **Rotation is locked**: a dynamic body gets no inertia tensor, so it never
  turns, and the box that collides is the axis-aligned box that is drawn. What
  rotation needs is in the backlog. (Slice 14 creates a body at its placement's
  rotation, still locked.)
- **The picking boxes are not the simulated bodies.** `sync_colliders` keeps one
  kinematic box per placed entity in the document's own `PhysicsSystem`; the
  bodies live in the `Simulation`, a different type, so the sync after a tick
  cannot overwrite them and picks follow the written-back placements.
- **Play runs the world its snapshot loads into.** `Document::play` reloads the
  snapshot before the modules register, so storage order — the order bodies are
  created and stepped in — is the files' order whatever an edit history did, and
  two plays of one scene end in the same poses to the bit.
- **Evidence**: `scene_physics`'s tests hold a dynamic body falling (a quarter
  second in, near `½gt²`) and resting on a static slab, a body dropped from 40 m
  resting on a slab thinner than a tick's travel, static and kinematic bodies
  never moving, a component standing on its `position` coming to rest with it at
  the ground, a block landing half off a narrow pillar resting level and
  unrotated, two plays of a stack identical to the bit, the round trip and an
  entity with a block and a body as one entity placed by the block, the default
  row, every invalid value refused on load by field and reported by the check,
  each play refusal, and the simulation apart from any `PhysicsSystem`. The
  editor's hold a block falling in play with its bounds and pick following and
  coming to rest on the middle step, which never moves, stop restoring the files
  byte for byte, the picking boxes kinematic in edit mode and in play, a body
  with no block refusing play by name, a body attached in the editor falling,
  and two plays identical even after a delete and its undo reordered the stack's
  storage; the instances' test sees the renderer's record of the falling block
  move. The mutations each turned a test red are listed in the commit that
  landed this.

**Slice 13, the asset browser, scene meshes and drag-spawn (task 6), landed
2026-10-01**, on the decisions of the same day (below). With it every step of
the exit criterion has a path in the editor except creating a scene from empty.

- **A mesh is a scene component.** `crcbl::scene_mesh` (behind `scn`/`scene`,
  registered by `scene_mesh::register`, which the editor's vocabulary calls)
  puts `Mesh { asset, position }` under `meshes`: a glTF key in the asset source
  and where the asset's own origin stands. `check_asset` admits a key —
  relative, no `..`, a `.glb` or `.gltf` extension, a canonical asset key, or
  empty for a mesh with no asset chosen (what attaching one starts as) — and a
  row is read through it, so a key that would walk out of the asset root is
  `ScnError::Parse` naming the file, line and field, and the check reports one a
  panel typed.
- **Its box is measured from the asset and never saved.** A `Placement` answers
  from the row alone, so `MeshLibrary` (feature `scene`) imports each asset once
  through the asset source, boxes every vertex through its node transforms, and
  writes the box into the row beside the key it was measured for; a row retyped
  to another key answers the placeholder until it is measured again. The
  document resolves after its load, play's restore and every command, undo and
  redo, and rebuilds the picking boxes that moved. A missing, broken or unchosen
  asset is a `PLACEHOLDER_HALF_EXTENT` cube about the origin and a
  `MeshProblem`, which `Document::problems` names by entity — never a panic. A
  flat model gets `MIN_HALF_EXTENT` across its plane.
- **The viewport draws the asset.** `apps/editor/src/app/meshes.rs`'s `Shelf` is
  the greybox pack plus every measured asset's meshes, converted by
  `crcbl::scene::build_render_scene` and appended, materials by their factors
  alone (a renderer has one page per kind). A renderer's geometry is fixed at
  `with_scene`, so a mesh naming an asset the shelf lacks rebuilds the renderer
  (device drained first) with every asset it held; a mesh not on the shelf, or
  missing, is drawn as the greybox cube of its box.
- **The browser is a fourth pane.** `apps/editor/src/panel/assets.rs` walks
  `AssetSource::list` when the panels open and on its Refresh button, listing
  the keys `is_mesh_asset` admits under the folders holding them, capped at
  `MAX_DEPTH` folders and `MAX_LISTED` entries, and saying so when the source
  cannot list or holds no mesh. `--assets DIR` names the root; a scene opened
  from a directory reads from the directory holding it, the compiled-in scene
  from nothing. An older build's saved three-pane layout is migrated by
  `crate::layout::migrate`: the browser docked below the outliner, sharing its
  slot, every other pane and divider kept; the old default becomes the new.
- **Drag-spawn.** A press on a mesh row and a release over the viewport stand a
  mesh of it on the point the release pixel's ray first strikes, or on the
  ground plane `y = 0` (`Document::drop_point`) — the bottom centre of its box,
  or the placeholder's, on that point (`Mesh::standing_on`). Enter on a focused
  row places it where the view's centre meets the ground. Either is
  `Document::spawn_mesh`: one undoable entry, the new entity selected, refused
  in play mode, and in a scene listing no `meshes` a `Batch` of
  `EditCommand::ListSystem` (new, with its inverse `UnlistSystem`, over
  `Scene::list_system`) and the spawn, so one undo puts the files back.
- **Evidence**: the umbrella's tests hold the round trip with the box left out
  of the file, every key refusal on load and by the check, the placement from a
  measured box and the placeholder, a missing or broken asset as a placeholder
  and a named problem, and a retyped key measured again; the editor's hold
  meshes measured on open, spawn, undo, redo, a retype, play and stop, a mesh
  picked by its measured box, the viewport drawing the asset's part at its row
  through its nodes and a missing one as a cube, a newly measured asset
  rebuilding the renderer, the browser listing only mesh assets and their
  folders, the notes for a source that cannot list or holds none, refresh, the
  layout migration from a committed older `settings.toml` and a current one
  round-tripping, a drop on a surface and on the ground, outside the viewport,
  in play, and Enter at the view's centre, one undo each, and a drop into a
  scene without meshes undoing to its files byte for byte. The mutations each
  turned a test red are listed in the commits that landed this.

**Slice 14, rotation in the scene format, landed 2026-10-01**, on the decisions
of the same day (below).

- **Decided 2026-10-01: a rotation is an optional field on a placing
  component**, a unit quaternion stored as `crcbl::registry::Rotation` and
  written `rotation: (x, y, z, w)`, serde-defaulted to the identity and left out
  of the file while it is exactly the identity — so every committed `.scn/` is
  byte-identical. Four numbers rather than Euler angles, which have several
  spellings of one orientation and lose an axis at a right-angle pitch, so a
  file of them would not read back to what was saved; rather than glam's own
  serde form, which needs a glam feature the workspace does not turn on.
- **Decided 2026-10-01: refused, not normalised, on load.** A number that is not
  finite and a length further than `ROTATION_TOLERANCE` from one are each
  `ScnError::Parse` naming the file, line and why (`RotationError`); a value
  within the tolerance is kept exactly as written, so a hand-typed `0.7071`
  saves back as itself, and `Rotation::quat` reads it normalised. A length of
  `0.5` or `3` is a typo, and normalising it would save numbers nobody wrote.
- **Who carries one**: the editor's `Block` and `scene_mesh::Mesh` (turned about
  the asset's origin, so its box's centre swings round `position`). Breakout's
  `Brick`, puppet's `Surface` and `Spawn` and towers' `Waypoint` and `Plot` do
  not: each game builds its colliders and its picture from a position and an
  extent, and a field the game ignores would show a turn the game never plays.
- **Decided 2026-10-01: `Placement` returns an orientation.** It answers a
  `crcbl::registry::OrientedBox` — centre, half extents along the box's own
  axes, rotation — and every impl spells the turn, `OrientedBox::axis_aligned`
  for none, rather than a provided method defaulting to unturned that a
  component growing a rotation could forget. `Document::placement` hands it out;
  `Document::bounds` is the world-axis box around it (`OrientedBox::bounds`),
  exactly the old numbers for an unturned box.
- **Picking, drawing and outlining turn.** A turned box picks by a
  twelve-triangle box mesh on a turned transform, because `crcbl_phys`'s query
  world keeps a box collider axis-aligned whatever its transform; an unturned
  one keeps the box collider. The greybox cube's instance transform and a mesh's
  parts carry the rotation, and the selection is outlined as its turned box
  (`DebugDraw::box_edges` over `OrientedBox::corners`).
- **Decided 2026-10-01: physics keeps rotation locked.** A scene `Body` is
  created at its placement's rotation — a turned collider and transform, so a
  tipped cube lands on its edge — and keeps it: no inertia, and only the centre
  is written back. What unlocking needs is in the backlog.
- **Evidence**: the umbrella's tests hold the tolerance kept as written, both
  refusals by name and on load naming the file, only the exact identity left
  out, an off-unit reflected write read as its direction, turned corners and
  bounds, an unturned reach exact to the bit, a turned mesh round-tripping and
  its box swinging about its origin, and a tipped body resting on its edge and
  keeping its rotation; the editor's hold a turned block saved, reopened and
  undone to the saved text, a ray the turn swings under picking it and one it
  swings away from passing it, its bounds, and its cube drawn turned. The
  mutations each turned a test red are listed in the commit that landed this.

**Slice 15, the rotate gizmo (the last of task 5), landed 2026-10-01.**

- **E shows a ring about each world axis** (`gizmo::Mode::Rotate`): the circle
  square to the axis through the selection's centre, sampled at
  `gizmo::RING_SEGMENTS` points and scaled on screen so a ring facing the eye is
  `gizmo::RING_PX` across — the arrows' constant-size rule. The polyline is both
  what is drawn and what a press is measured against, within `HIT_PX`. An entity
  whose placing component has no `rotation` field of the `Rotation` type shows
  no rings, and E says so on the status line.
- **A drag turns by the angle swept round the centre on screen**
  (`gizmo::swept`), counter-clockwise as seen from the axis's tip, the other way
  when the axis points away from the eye; about the ring's world axis through
  the placement's centre, composed onto the rotation the press found. It writes
  the four `rotation` leaves and, where the component has a `position`, that
  position swung round the centre — unmoved for a block, whose position is its
  centre, and carried round for a mesh, whose origin is not. Each frame's writes
  are one `EditCommand::Batch` of the same leaves, so the drag is one undo; play
  refuses every write.
- **Ctrl snaps a turn to `editor.snap.angle`** (degrees, 15 by default, beside
  the grid and scale steps in `settings.toml`), measured from the press: an
  orientation has no absolute grid about one axis.
- **Scale's lines follow the box's own axes**, since a half extent is along
  them, so a turned block's scale handles point along its faces; translate's
  arrows and the rings are the world's.
- **The inspector draws a rotation as three angles in degrees**
  (`EulerRot::XYZ`) over the quaternion, writing all four leaves when one angle
  is dragged; `Panels::apply_edits` makes a frame's edits to one section one
  command (`Document::record_edits`), so an undo never takes back one leaf of a
  quaternion. Euler angles are a view only — the file keeps the quaternion.
- **Evidence**: the gizmo's tests hold a ring per axis on its own plane and the
  facing ring's size near and far at two scales, a press within and just past
  the hit radius, the swept angle's direction and wrap, a quarter turn about
  each axis from each side composed onto a start, snapping to the step, a
  position swung round the centre, scale lines along a turned box's axes and the
  angle setting's fallback; the editor's loop tests show E's rings and their
  status, drag the Y ring to a turn about Y alone that is one undo, snap a ring
  drag with Ctrl, find no rings on a puppet entity, and refuse a ring drag in
  play; the panel's drag a rotation angle over four frames to one command and
  one undo, and the angles compose back to the quaternion. The mutations each
  turned a test red are listed in the commit that landed this.

**What slice 2 did not settle.** `chunk_of::<T>` is typed, so a statically
linked binary cannot learn a component type at run time: a build of the editor
opens the vocabularies it was compiled with. The shipped build registers its own
greybox block and both samples' components, so it still opens breakout's board;
a build for another game adds a line to `apps/editor/src/scene.rs::vocabulary`.
Run-time discovery needs a link-time distributed slice (`linkme` or
`inventory`), which is a new dependency and the user's call.

Everything else below stands unchanged: the server still drops commands, there
is one schedule per `World`, there is no snapshot of a `World` (play restores
from the scene's text, slice 8), the samples' state is outside the ECS, and
there are no `serve`/`scene`/`edit` subcommands. (One entity in several systems
landed in slice 11.) (Debug draw is still not a gizmo layer; the gizmo does not
need it to be — slice 6, above. `AssetSource` lists since 2026-09-30.)

Two things sit behind it, in both directions:

- **It no longer waits on stage 6.** Feature 5 (scene IO) has a format to open
  and save: stage 6's task 4 landed 2026-09-07 as `crcbl_scene::scn`, with
  `Scene::load` over an `AssetSource` and a `Scene::save` whose text is
  byte-identical for equal scenes, and `apps/breakout` reads its brick grid
  through it. (This bullet used to claim there was "no RON reader anywhere in
  the workspace", which was already false when it was written:
  `crcbl_render::stack::CameraStack::from_ron` and
  `crcbl_inventory::catalog::Catalog::from_ron` both predate it.) Feature 6, the
  asset browser, landed in slice 13 without the rest of stage 6 — a refresh
  button where a watcher would be, and glTF imported directly rather than baked
  — and what it would gain from them is in the backlog; there is no watcher and
  no `crcbl bake` (both owed in `docs/backlog.md`; stage 6's rules are in
  [../notes/tooling.md](../notes/tooling.md)). Feature 3 landed in slice 3 on
  stage 7's inspector (`Ui::inspector_with`; the UI's rules are in
  [../notes/tooling.md](../notes/tooling.md)).
- **Two sample plans wait on it.** [sample/07-towers.md](sample/07-towers.md)'s
  milestone 2 _is_ this document's dogfood pass — its exit criterion is "map
  authored 100% in the editor, zero hand-edited scene text". **Towers' map is
  scene data since 2026-09-30**: its path and build plots are
  `apps/towers/assets/scenes/field.scn/`, the shipped vocabulary registers
  towers' `Waypoint` and `Plot` beside breakout's and puppet's components, and
  the editor opens the committed field, so the dogfood pass has a scene to edit
  and is what is left of that milestone. And
  [sample/08-arena.md](sample/08-arena.md) wants an editor-built map too. Towers
  has an app directory; arena is one of five sample plans without one, beside
  mirrors, meadow, mane and relief.

## Where the tree stands against this design (surveyed 2026-09-15)

**Unparked 2026-09-15 by the user**, to be built after the UI system's rungs
(topic 7, since built; its rules are in
[../notes/tooling.md](../notes/tooling.md)), because every panel below is made
of that system. A read-only survey of the tree found the editor further away
than the rest of this document suggests; each line was checked in the source.

**What already exists and the editor can stand on:**

- `crcbl_scene::scn` loads a `.scn/` directory into a caller's `World` and saves
  it back byte-stably, with stable `SceneEntityId`s through `IdMap` —
  `crates/crcbl-scene/tests/scn_roundtrip.rs` holds the round trip.
- `PhysicsSystem::cast_ray` returns the `Entity` a ray hits, over the dynamic
  BVH — for entities with a collider.
- `crcbl_render::orbit::OrbitCamera` (orbit, pan, zoom, frame an AABB — its docs
  name the editor viewport) and `crcbl_render::fly::Flyer`.
- `crcbl_render::debug_draw::DebugDraw` and `crcbl_render::grid`.
- `crcbl_reflect::Reflect` and `#[derive(Reflect)]` (2026-09-16) describe a
  component's editable fields — name, label, range, step, and a
  `&mut dyn Reflect` per row — and `get_path`/`set_path` reach one leaf by name,
  which is the shape feature 3's property-set command and the undo log both
  need. `apps/breakout`'s `Brick` and `apps/puppet`'s `Surface`, `Shape`,
  `Spawn` and `Sun` carry it.
- `crcbl_ui::tree`'s `Ui::inspector` (2026-09-16) builds a component's rows from
  that description and reports each edit as a path and the value it replaced —
  feature 3's property-set command without its carrier. The editor does not draw
  it yet: it has no panel.
- `World::hash_state` and `crcbl_server::sim_hash::hash_world`, which the undo
  property test in the exit criteria needs.
- The shell's clipboard with a RON mime type, cursor shapes, `set_title` for a
  dirty marker, close requests, IME text; X11 drops now arrive through XDND and
  Win32 drops through `WM_DROPFILES`.
- `apps/viewer` as the nearest tool shell: file open and drop, orbit camera,
  grid, a read-only information panel, a polled reload.

**What this document assumes and the tree does not have:**

1. **Commands are dropped on arrival.** `ClientToServer::Command` is encoded,
   but the server's message handler matches it and does nothing, `Client` has no
   way to send one, and `ServerToClient::Event` has no consumer. A server hosts
   one session, so a GUI and a CLI client cannot share one.
2. **One schedule per `World`** and no per-system gating, so there is no
   edit-mode schedule to switch from.
3. **No snapshot or restore of a `World`**, so play/stop has nothing to restore
   from. Replication is one-way and carries transforms only.
4. **Most samples keep their state outside the ECS**, in a `Stage` behind a
   mutex the renderer locks. Towers — whose milestone 2 is this document's
   dogfood pass — still simulates in one, but its map is a `.scn/` directory
   since 2026-09-30, read into a `Map` the stage plays on, so what the editor
   edits there is the scene the game loads rather than live entities.
5. **No inspector _in the editor_**: `crcbl_ecs::Inspector::collect` returns a
   system's name and entity count, and the per-system debug-UI callback is an
   empty stub. The per-component half is built — `crcbl-reflect`,
   `Ui::inspector` and `crcbl::registry`, which is what a tool reads a component
   through, all 2026-09-16 — and what is missing is the editor drawing a panel
   with it.
6. **The scene format cannot hold one entity in two systems**: each chunk row
   spawns its own entity, so the same id in two chunk files is a duplicate-id
   error. The "attach/detach system data" command needs that first. `IdMap` has
   a removal since 2026-09-30; there is no dirty tracking or per-chunk reload.
7. **Debug draw is not a gizmo layer**: lines only, depth-tested, off by default
   — no on-top mode, no filled handles, no constant screen size.
8. **No screen-to-ray helper**, and only collider-bearing entities pick.
9. **No undo or command log** anywhere, and the inventory kit's
   optimistic-then-reconcile shape exists only as prose in
   [34-inventory.md](34-inventory.md).
10. **`AssetSource` could not list** (it had `read` alone) — `list` landed
    2026-09-30, for `DirSource` and `MemorySource`, with an `Unsupported`
    default for a source that cannot enumerate — and `crcbl import` writes
    nothing, and there is no watcher but the viewer's polled file.
11. **The UI cannot host an editor yet**: no textured quad or clip rect in the
    draw list, no layout, no keyboard focus, text input without selection, an
    ASCII bitmap font, and a DPI scale passed as `1.0` everywhere — the rungs of
    topic 7, all built since.
12. **No `serve`, `scene` or `edit` CLI subcommands**, and no native file
    dialogs or menus.
13. **Two statements in the 2026-08-09 corrections below are now out of date**:
    X11 does have drag-drop (through XDND), and Win32 OS drops work; only the
    Win32 clipboard file-list half (`CF_HDROP`) stands.

> **Re-checked 2026-09-25: slices 1 to 3 closed four of those lines.** Item 5's
> editor panel is drawn (slice 3's inspector), item 8's helper is
> `Camera::ray_through`, item 9's command log is `apps/editor`'s `EditCommand`
> and `UndoLog` (in-process; property, spawn and delete since 2026-09-30), and
> item 11 was already marked built. The other nine still hold as written, except
> that the viewport's renderer half now exists (see _Status_, above).

**The missing pieces, ranked, with owner and rough size**: the UI foundation
(large, `crcbl-ui` and the UI pass); a viewport pane that samples a rendered
view (medium); `World` snapshot and restore (medium, and it needs games' state
in systems); server command handling with a client send path and reason-coded
replies (medium); the command and undo log with its property test (medium); an
edit schedule (small to medium); gizmos (medium); the scene format change for
entities spanning systems (small to medium); asset listing and a watcher
(medium); input chords and a context stack (small to medium); a multi-session
server with `crcbl edit --serve` (large).

**The smallest slice that uses only what exists** plus a screen-to-ray helper,
modifier-carrying key events and a way to force debug draw on: a native,
single-process `apps/editor` that loads breakout's board scene, renders it with
the orbit camera and grid, lists entities per system, picks one by ray, draws
its bounds, nudges it with keys, and saves with a dirty marker in the title —
proving load, pick, mutate, save and a byte-stable diff before any protocol
exists.

**Decided by the user, 2026-09-16** — the options were the ones evidenced above:

- **The command enum and the undo log exist from day one, applied in-process,
  and are routed over the transport later.** So every edit is a `Command` value
  from the first slice even while the editor mutates the server `World`
  directly, and "nothing GUI-only" is kept by construction rather than
  retrofitted: what the transport gains later is a carrier, not a vocabulary.
- **Games keep editable state in ECS systems**, towers ported first, so the
  editor sees and edits live entities rather than only the scene a game reads at
  load. Most samples' state is outside the ECS today, so each port is its own
  slice and the backlog carries them.
- **Play/stop restores by reloading the scene.** Restore is the load path the
  engine already tests, at the cost of losing unsaved edits when play starts and
  of a load's worth of time on stop. Per-system snapshots stay declined: a
  system that forgets one loses state silently. _Amended 2026-10-01, below_: the
  text reloaded is held in memory from the moment play began, so unsaved edits
  are no longer lost.
- **A component's editable fields come from `#[derive(Reflect)]`** in a new
  proc-macro crate, one annotation per component, checked at compile time. This
  is the workspace's first proc-macro dependency (`syn`, `quote`,
  `proc-macro2`), approved with the decision.

**Decided 2026-09-30** (the owner asked for open decisions to be taken for the
long term and recorded):

- **The viewport is a secondary view rendered to a texture a UI rect samples.**
  The renderer half exists (`ForwardRenderer::create_view`); what is missing is
  a UI image command that can name a rendered target rather than an atlas region
  filled from host bytes. It is the next slice (landed the same day; see
  _Status_). The full-window, scissored-hole alternative is declined: it cannot
  show two views, and the render graph has no sub-rect to scissor with.
- **Docking is splitters only for now.** Tabs are added when a second panel
  competes for one slot; nothing does yet.
- **File dialogs are an in-UI browser over storage listing**, the same on every
  backend including the browser, with no native dialog dependency.
  `AssetSource::list` exists since 2026-09-30. In a browser, served assets
  cannot be listed — a URL fetch has no directory to ask, and the default says
  `Unsupported` rather than empty — so a browser build browses its OPFS storage
  (`StorageSource::list`) and would need a baked manifest to browse served
  assets.
- **The scene format change for entities spanning systems** is decided when the
  towers port first needs one entity in two systems. The format is v0, so the
  break is allowed then. The map's port (2026-09-30) did not: a corner of the
  path and a build plot are one component each, in one system each. _Superseded
  2026-10-01, below_: decided now, for physics on scene components and
  attach/detach.

**Decided 2026-10-01** (taken for the long term and recorded, as above):

- **A vocabulary registers behaviour beside components.** The registry gains a
  module factory; play builds every module registered for the scene's systems,
  registers it on the play world and ticks it with empty client inputs at the
  world's tick rate on a fixed step from the frame's time.
- **Play snapshots the scene to memory and stop restores from it** (the scene's
  files, read back through the load `Document::open` runs). This amends the
  2026-09-16 decision's accepted cost: unsaved edits survive play. The undo log
  survives play and stop, because the restored scene is the pre-play scene under
  the same ids; the selection survives when its entity does.
- **Edits are refused during play**, every path, with a status line naming play
  mode; a save is refused too, since saving a played state is a footgun. Pause
  stops ticking and stays in play, still refusing edits; stop restores.
- **Play, pause and stop are a toolbar outside the dock and F5 and F6**, so the
  saved layout stays compatible.
- **The first slice builds the mechanism only.** No sample registers a module in
  it, and no demo component joins the shipped vocabulary; towers' play module is
  the next slice.

**Decided 2026-10-01, for entity names and spanning systems** (taken for the
long term and recorded, as above):

- **Names live in an optional names chunk**, `names.ron` in the `.scn/`
  directory: a list of `(SceneEntityId, "name")` pairs in id order, written by
  the deterministic writer and left out entirely while no entity is named, so
  every scene from before stays byte-identical. **The header declares it**
  (`names: true`, skipped when false) rather than the loader looking for the
  fixed name: a browser build seeds a `MemorySource` file by file, and an
  undeclared file left out of the seeding would load as a scene whose names had
  silently gone, where a declared one is a missing key that names itself; it
  also spares an unnamed scene's every load a failing read, which over a network
  source is a round trip. A list rather than a RON map, so an id named twice is
  refused rather than the later entry winning. A name is optional, trimmed,
  non-empty, at most `scn::MAX_NAME_CHARS` characters and free of control
  characters; a name for an id the scene does not hold is refused on load by key
  and id, and on save. An unnamed entity is shown as before.
- **A duplicate is unnamed**, and a pasted entity keeps its clipping's name only
  while nothing in the scene bears it. A name says which entity this is, and
  `Gate (2)` would invent a name nobody chose that a person then renames anyway;
  the one rule covers both, since a duplicate's original always bears its name.
- **One entity in several systems is allowed, keyed by the shared
  `SceneEntityId` across chunks.** Needed for physics on scene components and
  for attach/detach. Built in slice 11, above.

**Decided 2026-10-01, for physics on scene components** (taken for the long term
and recorded, as above):

- **A scene-carried body component**,
  `Body { kind, mass, friction, restitution }`, in a module of the umbrella
  behind its scene feature (`crcbl::scene_physics`), registered under `bodies`
  with one call; the editor's vocabulary registers it. No new crate, and no
  serde or scene dependency in `crcbl-phys`.
- **Shape from the placement**: the collider is the entity's placement box; a
  body with no placement is refused at play start by name.
- **Play builds simulated bodies through a module registered under `bodies`**,
  and each tick the simulated pose is written into the placing component's
  `position` through the registry's reflected path, never naming a game's type.
  Rotation is locked (above, slice 12).
- **Edit mode keeps kinematic pick bodies**, and during play the pick boxes are
  kept apart from the simulated bodies; stop's restore discards everything.
- **Static and kinematic bodies collide and do not move**; a kinematic velocity
  is not added until something needs one.

**Decided 2026-10-01, for the asset browser and drag-spawn** (taken for the long
term and recorded, as above):

- **A scene-carried mesh component**, `Mesh { asset, position }` in
  `crcbl::scene_mesh` behind the scene features, `#[derive(Reflect)]`, serde,
  `Default`, registered with one call the editor's vocabulary makes. Its
  placement comes from the asset's bounds, measured at load through the asset
  source and cached; an unloaded or missing asset is a named placeholder box and
  a problem, never a panic. The asset path is validated at the boundary
  (relative, no `..`, a known extension) by name.
- **The editor draws the real mesh** through the engine's existing renderer and
  glTF bridge, not a renderer of its own; every other entity stays greybox.
- **The browser is a fourth pane**, and an older three-pane `settings.toml`
  layout is **migrated** — the browser added at a default size, below the
  outliner — rather than refused. The pane lists the `AssetSource::list` entries
  a mesh can be made of, with their folders.
- **Drag-spawn** spawns a mesh where the pointer's ray meets the surface under
  it, or the ground plane `y = 0`, as one undoable spawn, refused in play mode,
  the new entity selected; Enter on a browser entry spawns at the viewport
  centre's ground point. One spawn into a scene that lists no `meshes` also
  lists the system, in the same undo entry, since a save writes only the
  manifest's chunks.

**Decided 2026-10-01, for rotation** (taken for the long term and recorded, as
above):

- **Rotation is an optional field on placing components**: a unit quaternion,
  serde-defaulted to the identity and omitted from the file when it is the
  identity, so committed scenes stay byte-identical; validated on load by name.
  Added where the component's game honours it — the editor's `Block` and `Mesh`
  — and not to components whose games ignore it. Built in slice 14.
- **`Placement` returns an orientation**, and picking, instance transforms and
  the selection outline use it. Built in slice 14.
- **The rotate gizmo is E**: three screen-space ring handles hit-tested against
  the projected ring, a drag turning about the ring's world axis through the
  entity's centre, Ctrl snapping to an angle step that is a setting beside the
  grid and scale steps, one undo per drag, refused in play. Built in slice 15.
- **Physics keeps rotation locked** for now: a body is created at its rotation
  and does not spin. Built in slice 14; unlocking is in the backlog.

**Still the owner's:** a file watcher dependency for hot reload (`notify`),
because adding a crates.io dependency is the owner's call by the workspace's
rules. It stays open in the backlog.

## Architecture

- **The editor is a client+server pair**, exactly like the sandbox: an editor
  _server_ runs the scene in a paused/edit-mode simulation; the editor _client_
  renders it and hosts the UI. All edit operations are `Command` messages
  through the transport — which means:
  - Undo/redo = command log with inverse commands (the transport already
    serializes them).
  - Collaborative/remote editing is structurally possible later (not MVP),
    because editing is already message-based.
  - Play-in-editor, as built (slice 8, in process): the scene's files are held
    in memory, the games' modules the vocabulary registers for the scene's
    systems are registered on the same world and ticked beside its schedule on a
    fixed step, and every edit is refused. Stop loads the held files back into a
    fresh world through the scene load. There is no edit-mode schedule to switch
    from and no `World` snapshot: nothing ticks while editing, and the restore
    is the load path. Over a transport, play and stop become commands to the
    editor server; the mechanism is the same.
  - **Headless/CLI is a peer client** (topic 11): `crcbl edit --serve` runs the
    editor server windowless; `crcbl scene …` sends the same commands the GUI
    sends. Nothing editor-side may be implemented GUI-only — the command
    protocol is the editor's real API, the GUI just drives it.
- Edit-mode ECS additions: selection system, gizmo system, editor-camera system
  — ordinary systems in the edit schedule, demonstrating the ECS's own extension
  story.

## Features (MVP)

1. **Viewport** — engine-rendered scene view in a UI pane; editor camera
   (orbit + fly); click-pick via `crcbl-phys` L0 raycast (stage 5 BVH,
   server-side query command; a GPU picking pass is post-MVP).
2. **Hierarchy/outliner panel** — scene entities grouped by system (the natural
   shape of scene files); select, rename, delete, duplicate.
3. **Property panel** — reuses the stage 7 inspector: systems render editable UI
   for their data; edits become update commands.
4. **Transform gizmos** — translate/rotate/scale, axis/plane constrained,
   snapping. Drawn in the viewport pane's screen space over the scene's picture
   (decided 2026-09-30; see _Status_), interact via viewport input.
5. **Scene IO** — open/save `.scn/` scene dirs (stage 6 loader in both
   directions), dirty-state tracking, revert (= stage 6 scene-reload path).
6. **Asset browser** — list `AssetSource` contents, drag mesh into viewport →
   spawn command with placement.
7. **Play mode** — play/pause/stop toolbar; state restore on stop; debug overlay
   (stage 7) available in play mode.
8. **Copy/paste + drag-drop** (shell clipboard, topic 15):
   - **Fields**: any property-panel value copies as plain text; paste parses
     through the same serde path the scene loader uses (bad paste = validation
     error, not corruption). Text inputs get standard select/copy/paste.
   - **Entities**: copy = selected entities serialized by the stage 6
     deterministic RON writer onto the clipboard (dual-mime: engine RON + plain
     text — pasteable into a text editor or a chat, readable either way). Paste
     = spawn commands through the normal command protocol → full undo, works
     cross-instance (two editors, or editor → CLI via `crcbl scene paste -`),
     and entity IDs are re-minted on paste (no collisions by construction).
   - **Assets (future-proofed now, full support post-MVP)**: OS file paste and
     OS drag-drop into the asset browser = import (same `crcbl import`
     pipeline); asset-browser-internal drag already covered by drag-spawn. Shell
     carries file-list clipboard/DnD mimes from day one so this is editor work,
     not seam work, when it lands.

## Explicitly not in MVP editor

- Multi-scene/prefab editing, material editor, animation tools, terrain,
  build/export wizard. (Shipped-game packaging is post-MVP overall.)
- GPU-accurate picking (mesh-precise); AABB picking suffices.
- Multi-select transforms beyond shared-pivot translate.

## Tasks

1. Editor app scaffold: client+server pair, edit-mode schedule, editor camera
   system.
2. Viewport pane + picking command + selection system.
3. Outliner + property panels on the inspector foundation.
4. Command/undo infrastructure (command log + inverses; ~10 command types covers
   MVP: spawn, delete, duplicate, rename, transform, property-set, attach/detach
   system data, scene-load/save markers).
5. Gizmos.
6. Asset browser + drag-spawn. Landed in slice 13 (meshes from glTF assets, the
   browser as a fourth pane, drag and Enter to place).
7. Play/stop with snapshot restore. The mechanism landed in slice 8, restoring
   from the scene's text; towers registers the first module (slice 9).
8. Dogfood pass: build a small playable scene start-to-finish in the editor; fix
   what hurts.

## Exit criteria

- Create scene from empty → place meshes from asset browser → transform with
  gizmos → edit properties → save → reopen → play → stop, all without touching a
  text editor.
- Undo/redo correct across all MVP commands (property-based test: random command
  sequence + full undo → state hash equals initial).
- Editor never links `crcbl-vk` directly (only through the engine facade) —
  proof the engine API is sufficient to build real tools.
- MVP COMPLETE at this stage exit: all overview MVP features exist on
  Linux/Vulkan.

## Risks

- **Gizmo math/UX time sink.** Constrain to the classic axis/plane handles;
  study existing implementations; no screen-space fanciness beyond constant
  screen-size scaling.
- **Undo edge cases.** Command inverses are validated by the property test, not
  by manual enumeration.
- **Editor-only engine APIs creeping in.** Every editor need is met by
  commands/systems available to games too; anything else is a smell worth a
  design pause.

## Correction (design review, 2026-07-27)

**Concurrent editing needs stated semantics** (the doc promised concurrent GUI +
CLI clients without defining them). MVP rules:

- **One global undo log**, not per-client — the command log _is_ the document's
  history; a client's Ctrl-Z undoes the most recent command regardless of author
  (with the author shown in the undo entry).
- **Commands validate against current state**; stale operations (transforming an
  entity another client just deleted) **fail with a reason code** rather than
  resurrecting or corrupting — the same optimistic-then-reconcile shape the
  inventory kit uses.
- Last-writer-wins for conflicting property sets, which server serialization
  gives for free.

## Corrections (2026-08-09)

- **"Shell carries file-list clipboard/DnD mimes from day one so this is editor
  work, not seam work" is false on three of four desktop backends.** Win32
  publishes `text/uri-list` as a _registered format_ and never reads `CF_HDROP`,
  so an Explorer file copy is invisible — and the shared `parse_uri_list` cannot
  round-trip a Windows path (`file:///C:/a` decodes to `/C:/a`). macOS reads
  only `public.file-url`, not `NSFilenamesPboardType` or the promised-file form.
  X11 has no XDND at all and `ShellCaps::DRAG_DROP` is honestly clear there.
  Closing it is **seam work** — a Windows-aware `file:` encoder and matching
  decoder, or delivering `CF_HDROP` through a different route, plus a seam
  question for promised files ("where should a promised drop land?"). Owed
  before the asset browser wants OS drops; `docs/backlog.md` has the detail per
  backend.
- **The editor is a native target.** Stage 10 listed editor-in-browser as a
  stretch that "should mostly work by construction"; the asset browser, OS
  drag-drop import, `crcbl import` and hot reload's notify-based file watcher
  are all native-shaped, and nobody has examined what a browser would do with
  them. Treated as native-only until something makes the case; recorded so the
  stretch goal is not mistaken for a plan.
