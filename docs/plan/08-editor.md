# Stage 8 — Scene Editor

`apps/editor`: the editor is a client of the engine (locked decision). It uses
the same renderer, ECS, server loop, transport, and GUI as a game. MVP editor:
open scene, move things, edit properties, save, play.

## Status: slices 1, 2 and 3 landed 2026-09-16, slices 4 to 6 2026-09-30, slices 7 to 15 2026-10-01, play controls and their polish, multi-selection, a scene from empty and save-as, open and the unsaved bar, recovery offered back and autosave, and the undo property test 2026-10-03, and what still waits

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
- **The property test** (since 2026-10-03 the proptest
  `document::undo_property_tests`, _The undo property test_ below) played seeded
  random histories of nudges, duplicates and deletes and walked each back and
  forward again, comparing the saved scene text at every step. Not
  `World::hash_state`: it hashes `Entity` bits, which a restored entity changes
  (the backlog records the decision). Restoring under a new id, skipping the
  sweep on delete, and skipping the collider on spawn each turned it or its
  neighbours red.
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
  inverse (since 2026-10-03 a gesture's writes merge whatever leaves they name —
  _The undo property test_, below). A save seals the entry, so a drag carried
  past it is dirty again. The inspector's field drags ride the same mechanism:
  edits to one leaf while the primary button is held share a gesture
  (`Panels::apply_edits`), so a field dragged over many frames is one undo too —
  it was one per frame.
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
  into the entry on top when it names the same leaves in the same order (since
  2026-10-03, whatever leaves it names).
- **Rotate cannot be entered.** `gizmo::Mode` has no rotate variant, and E puts
  a refusal on the status line and leaves the mode as it was: the scene format
  carried no rotation for a handle to write. (Slice 14 put one in the format,
  and slice 15 built the rings.)
- **Multi-select is not a question yet**: the document holds one selection
  (`Document::selected`), and the outliner's Ctrl and Shift clicks select rows
  of which the document takes the first. Every handle acts on that one entity.
  (_Multi-selection_, 2026-10-03, below, replaced this.)
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
  `Drawn::Spawned(Entity)` key beside the scene's ids; the outline and the id
  map are the scene's alone, and so were the colliders a click picks by until
  the play-strip polish (below) gave them to the runtime systems a play action
  picks from; stop throws the world away with every creep in it. Declined:
  drawing every id-less entity that has a placement, which needs the creeps
  registered as scene components — and then a scene file could list them.
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

**Play controls, landed 2026-10-03**, on the decisions of the same day (below):
the editor takes part in a played scene through the game's own commands, and
shows the run's numbers.

- **A vocabulary registers a play-controls description beside its module.**
  `Registry::play_controls(system, PlayControls)`, keyed by the system the
  module is registered under: a list of `PlayAction { name, params }`, each
  parameter `ParamKind::Picked(system)` or `ParamKind::Choice(labels)`; an
  `encode` function from an action's index and its `PlayArg`s to the game's
  command bytes; and `status` and `refusals` functions over the world the module
  plays in. `Registry::encode_play` holds the arguments to the action's
  parameters before the game's encoder sees them, and `Registry::keyed_modules`
  hands each module back with its system. A picked argument is the entity's
  place among its system's rows in file order, which is how a game numbers them
  — towers' plots are numbered so.
- **The editor sends commands; the module's next tick reads them as a
  client's.** `Document::send_play` encodes an action and queues the frame for
  the module under that system alone; `Document::advance` hands each module its
  queue as `ClientInputs` (stamped with the play's tick count) and empties it,
  where every module had been ticked with `ClientInputs::empty()`. A paused
  scene holds the queue until it resumes. `Document::take_play_refusals` and
  `Document::play_status` read through the controls' functions;
  `Document::picked` is the selection as a picked argument. A command is not an
  edit: it is refused while editing (`EditError::NotPlaying`) and touches
  neither the files nor the log.
- **The play strip** (`panel::play`) is drawn under the toolbar only while a
  running game offers controls, so every other scene keeps its viewport: a
  button per action, a button per choice that shows the current label and steps
  on a click, and the run's numbers as `Label value`. A click gathers the
  arguments (a choice's current label, a picked system off the selection) and
  sends them; the status line says it was sent, or why not, and a game's refusal
  arrives on it as a warning after the tick that read the command.
- **Towers registers controls**: _Place tower_ (a picked plot and a kind),
  _Start wave_, _Upgrade_ (a picked plot, and a picked tower since the
  play-strip polish below) and _Restart_, encoded through the same conversion
  `Game::set_controls` makes, and Lives, Gold, Wave and Outcome read off a
  readout system its module registers in the world. Its towers, bolts and bursts
  are mirrored as runtime components beside the creeps, so a placed tower is
  drawn on its plot at its tier's size.
- **Evidence**: the registry's tests hold arguments that fit reaching the
  encoder and every misfit refused by name, two descriptions under one system
  refused, and keyed modules naming their systems. Towers' tests hold every
  action's bytes equal to the frame solo's client seals for the same `Controls`
  (each kind included) and decoding field by field to what was asked, the action
  indices naming their actions, a plot past the frame's range refused, the
  readout reading the stage and telling each refusal once, a built tower
  mirrored on its plot and growing in place when stepped up, and bolts and
  bursts mirrored tick by tick with every world entity one of the stage's. The
  document's tests on the committed field hold a placed tower standing on the
  selected plot only after a tick, drawn at its size, the gold dropping by its
  price; a taken plot and a short purse refused by the game's reason at no cost;
  _Start wave_ counting the wave and a creep walking before the build phase
  would end; stop restoring the files with nothing spawned and no status; and a
  command refused while editing, under a system no module runs, or with
  arguments the action does not take. The loop's tests click the strip through
  the headless shell: a tower placed and drawn and the strip's gold dropping, a
  second click refused on the status line with the game's label, a kind choice
  stepped and taken, _Start wave_ counted, a build with nothing selected saying
  what to select, stop taking the strip and the towers away, and a game with no
  controls showing no strip and keeping the viewport. The mutations each turned
  a test red are listed in the commit that landed this.

**Play-strip polish, landed 2026-10-03**: what the play-controls slice left
deferred, built.

- **A play action can pick something the run made.**
  `ParamKind::PickedRuntime(system)` names a runtime system and hands the
  encoder `PlayArg::PickedRuntime(Entity)`. The encoder (`PlayEncoder`) is
  handed the world the module plays in, so a game reads what the entity stands
  for off its own row there, and `Registry::encode_play(world, …)` refuses an
  entity the named system does not hold (`Registry::runtime_entities_in`) before
  the encoder sees it. The editor gives the entities of every runtime system a
  running game's action picks from a picking collider after each tick — no other
  spawned entity gets one, so creeps still pick nothing — and `Document::hit`
  tells a scene entity (`Hit::Scene`) from such a spawned one (`Hit::Spawned`).
  A click on a spawned one is the play's **runtime pick**
  (`Document::set_runtime_pick`, read by `picked_runtime(system)`), selecting
  nothing; a plain click elsewhere clears it, and it lives in the play session,
  so stop throws it away. Towers' _Upgrade_ picks a built tower, encoded as the
  plot on its `Turret` row. **The pick is outlined** through the selection's own
  outline (`app::selection_boxes`, the debug-draw boxes the renderer is handed
  each frame), in a green of its own (`PICKED_COLOR`) beside the selection's
  amber and blue: `Document::runtime_pick` names it and its placement is read
  afresh every frame, so the outline grows with an upgrade and goes when the
  pick is cleared, play stops, or the tower despawns (a despawned entity has no
  placement). Declined: a runtime system pickable whatever the controls say,
  which would let creeps and bolts take clicks meant for the plots under them.
- **Every refusal of a frame is told**: the status line joins them in the order
  the game made them (`Refused: THAT PLOT IS TAKEN; …`), and each is logged.
  Joined rather than counted, because two in one frame is rare and each names a
  different mistake — up to `REFUSALS_SHOWN`; past it the line counts the rest
  (`… (and 4 more in the log)`, the wording a save's problems use), so a burst
  stays one readable line and the log holds every one.
- **A played field keeps a bounded queue of untold refusals.** Towers' play
  module leaves refusals on its stage for `PlayControls::refusals`; a tool that
  ticks it without taking them would let them grow one per refused command. So
  after every tick the module drops the oldest past `UNTOLD_KEPT` and counts
  them, and the next take opens with one line saying how many went untold
  (`5 OLDER REFUSALS WENT UNTOLD`). The cap is far above what one frame can
  bring, so the editor, which takes them every frame, never loses one. The bound
  is the play module's alone — solo, a host and a dedicated server take the
  stage's refusals every tick and tell each to its sender, unchanged — and the
  untold refusals are in no state hash (`Stage::hash_state` counts refusals,
  never queues them; the readout system hashes nothing). Declined: clearing them
  each tick in the module, which would lose all but the last tick's when a frame
  runs several.
- **The number keys are the strip's actions**: `1` to `9` send the first nine
  actions across every row in the order drawn, and each of those buttons reads
  its key, as the toolbar's do. They are `keys::PLAY_ACTIONS` in the map's
  default context: `ui` does not bind digits and `text` does, so a field being
  typed into keeps them, and the editing rule stops them besides. While editing
  they ask for nothing that happens.
- **A command sent while paused waits for the tick after resume** — encoded and
  refused by the controls at once, read when the game next ticks, as a server's
  queue holds a frame. Refusing it would make a pause a mode where the strip
  does nothing; dropping it would lose a command the status line said was sent.
- **A choice lasts one play**: stop takes the strip and every choice's pick with
  it.
- **Two games' controls share the strip**, a row each in tick order, numbered
  across both; only modules the scene's systems start are running, so another
  game's controls never show.
- **Evidence**: the registry's test holds a runtime pick reaching the encoder
  read off its row and one its system does not hold — or in a world with no such
  system — refused by name; towers' tests hold _Upgrade_ encoding the picked
  turret's plot, an entity that is no tower refused, the upgrade growing the
  picked entity, and every bolt and burst keeping its entity through a
  swap-remove and an expiry off the front (a bolt's entity never moving further
  in a tick than a bolt flies, a burst's never moving). The document's tests
  hold a ray down onto a built tower hitting it as spawned, the pick naming it
  under `turrets` and nothing under `walkers`, _Upgrade_ paying for that tower,
  the pick gone after stop, and a command sent while paused read on the tick
  after resume and not before. Towers' test holds a tool that never takes the
  refusals leaving the newest `UNTOLD_KEPT` on the stage, every one still
  counted, and the next take telling the older ones dropped once. The loop's
  tests hold a click on a built tower picking it without selecting, the pick
  outlined at its tower's box, at the grown box after _Upgrade_, and not at all
  once unpicked, stopped or despawned by _Restart_ (the pick itself still set),
  a burst of refusals told as the first `REFUSALS_SHOWN` and a count with a
  warning logged for each, _Upgrade_ sent and paid for, _Upgrade_ with nothing
  picked saying what to click, two refusals of one frame both on the status
  line, `2` sending _Start wave_ while playing and nothing while editing, a
  second game's row ahead of towers' with `1` reaching it alone and the unlisted
  game absent, and a stepped choice back on its first label after stop and play.
  The mutations each turned a test red are listed in the commit that landed
  this.

**Multi-selection, landed 2026-10-03**, on the decisions of the same day
(below): several entities selected at once, moved together, and deleted,
duplicated, copied and pasted as one.

- **The document holds an ordered set** (`document::selection`):
  `Document::selection` in the order entities joined, the last the **primary**
  (`Document::primary`), with `select` (one, or none), `toggle_selected`,
  `set_selection` and `is_selected`. It is not in the log; `prune_selection`
  drops whatever a delete, an undone spawn or play's restore took away.
  `Document::selected` is gone — its callers read `primary`.
- **Clicks**: a viewport click selects alone and a Ctrl click adds or takes out
  (a Ctrl click on nothing keeps the selection). In the outliner a plain click
  replaces, Ctrl toggles and Shift takes the run from the anchor;
  `Panels::follow_outliner` keeps the document's order for what stays, appends
  what the click added in row order, and makes the row the click acted on the
  primary — the anchor for a plain or Ctrl click, the run's far end for Shift.
  Collapsing a system no longer deselects what is in it: an entity is kept by
  the outliner's whole selection, not the rows it shows.
- **Shared-pivot translate**: with several selected the translate handles stand
  at `Document::selection_pivot`, the **bounds centre** (the centre of the box
  around every selected entity's box), on the world's axes. A drag moves the
  pivot as it moved a lone entity's centre, and `gizmo::Drag::spread` moves
  every `gizmo::Member` of the drag's `gizmo::Group` — each selected entity
  whose placing component has a `position` — by the same delta, one
  `EditCommand::Batch` a frame that the log folds into one entry. Snapped, the
  pivot lands on the absolute grid. A group of one is its own pivot, so a lone
  entity snaps its own position to the bit as before. The arrow keys nudge every
  selected entity, one entry too.
- **Scale and rotate stay single-entity**: with several selected R and E show no
  handles and say why on the status line.
- **Delete, duplicate, copy and paste take the whole selection**, one entry
  each: `Document::delete`, `duplicate` and `copy` take `&[SceneEntityId]`
  (`duplicate` answers the copies), built with `EditCommand::one_or_batch`. A
  duplicate selects its copies and a paste what it pasted, the last the primary.
- **The inspector edits the primary alone** and its title says so —
  `#3 (primary of 2 selected)`. Every selected row is `:checked` and the
  primary's label is amber; in the viewport every selected entity is outlined,
  the primary amber and the rest pale blue (`app::selection_boxes`).
- **Evidence**: the document's tests hold the order, the primary passing back on
  a toggle, absent and repeated ids kept out, deleted and undone entities
  dropping out while an undo selects nothing, the pivot as the bounds centre
  rather than the mean of the centres, and a two-entity delete, duplicate and
  paste one entry each and undone byte for byte; the undo property test now
  deletes and duplicates selections of two and nudges two entities as one batch.
  The panels' tests hold plain, Ctrl and Shift clicks (a run downwards, upwards,
  and of several upwards), the outliner following a delete and keeping an entity
  whose system is collapsed, the primary's label colour, and an inspector drag
  editing the primary alone under a title that says so. The loop's tests hold
  viewport click and Ctrl click, a shared-pivot drag moving both by one delta as
  one undo, a snapped one landing the pivot on the grid from off it, R and E
  refused, delete, duplicate, nudge and copy-and-paste of a selection, and the
  outline colours. Each of these mutations turned a test red: toggling never
  removing, absent ids admitted, a delete not pruning, the pivot averaging
  boxes, members taking the pivot's value, several snapping the primary, an
  entry per member, every mode's handles shown for several, R and E silent,
  delete or nudge of the primary alone, duplicate or paste selecting one, a
  duplicate entry per copy, Ctrl ignored in the viewport, a range's primary its
  last row, the acted row not made primary, the primary's label unmarked, the
  inspector title silent, one outline colour, the property test's shared nudge
  never running, the outliner told only the primary, and the outliner's shown
  rows deciding what stays selected.

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
  per system the entity is not in — the manifest's then, since 2026-10-02, every
  other registered one, attaching to which lists it in the same undo entry
  (_Editor follow-ups_, below). Arrow keys and the gizmo move the placing
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
  turns, and the box that collides is the axis-aligned box that is drawn. (Slice
  14 creates a body at its placement's rotation, still locked; unlocked on
  2026-10-01 — _Physics rotation_, after slice 15.)
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
the exit criterion has a path in the editor except creating a scene from empty,
which landed 2026-10-03 (_A scene from empty and save-as_, below).

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
- **Picking, drawing and outlining turn.** A turned box picked by a
  twelve-triangle box mesh on a turned transform, because `crcbl_phys`'s query
  world kept a box collider axis-aligned whatever its transform; since the query
  world turns boxes (_Physics rotation_, after slice 15) every box picks by its
  box collider on its turned transform. The greybox cube's instance transform
  and a mesh's parts carry the rotation, and the selection is outlined as its
  turned box (`DebugDraw::box_edges` over `OrientedBox::corners`).
- **Decided 2026-10-01: physics keeps rotation locked.** A scene `Body` is
  created at its placement's rotation — a turned collider and transform, so a
  tipped cube lands on its edge — and keeps it: no inertia, and only the centre
  is written back. Unlocked the same day (_Physics rotation_, after slice 15).
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

**Physics rotation, landed 2026-10-01.**

- **The query world turns boxes.** `crcbl_phys::BoxCollider` carries a rotation,
  and every query of the physics query world — rays, sphere and capsule sweeps,
  overlaps, push-outs — answers for the turned box. The editor's picking boxes
  are box colliders on the placement's turned transform, so a turned block or
  mesh picks by its own box; the twelve-triangle mesh slice 14 picked turned
  boxes by is gone, with its least half extent.
- **A body turns where its placing component can show it.** In play, a dynamic
  `Body` beside a component with a `rotation` field (the editor's `Block`, a
  `scene_mesh::Mesh`) has its box's inertia, so a block landing on its corner
  tips onto a face, and each tick the module writes its orientation into the
  component's `rotation` leaves (`crcbl::registry::ROTATION`, the spelling the
  rotate gizmo writes too) beside its `position`, turning the component's offset
  from its centre with it. A component with no `rotation` (breakout's, puppet's,
  towers') keeps its body locked. Stop puts the file back byte for byte, the
  turn included.
- **Evidence**: `crcbl-phys`'s tests hold rays, sweeps, overlaps and push-outs
  against a box turned 45° hitting where the unturned box misses and missing
  where it hits, a body's box and offset turned in the query world, a cube
  dropped on its corner tipping onto a face with no energy gained, a locked cube
  keeping its orientation to the bit, a free tumble keeping its momentum, and
  the drop hashing the same twice; `scene_physics`'s hold a turned body's row
  written to the simulated orientation and centre every tick, two plays
  identical, and a component standing on its position swinging about its body's
  centre; the editor's hold a turned block tipping in play, picking where it
  rests and restored by stop, and a turned block picking by its turned box. The
  mutations each turned a test red are listed in the commits that landed this.

**A scene from empty and save-as, landed 2026-10-03**, on the decision of the
same day (below). With it the first exit criterion is met, headlessly.

- **A new scene** (`Document::new_scene`, Ctrl+N and the toolbar's New) puts
  `crate::scene::empty_source` in place: a header named `untitled` listing no
  system, the compiled-in scene's light and camera, no entity, a fresh log, no
  origin, clean. The vocabulary and the asset source stay, so the browser lists
  what it listed. Refused in play mode. A scene with unsaved edits asks first,
  through the unsaved bar (_Open and the unsaved bar_, below). An empty scene is
  framed as a unit box standing on the ground, not about the origin: the view is
  level with the box's centre, and a box about the origin put the eye on the
  ground plane, where a drop meets no ground.
- **Decided 2026-10-03, for the long term: save-as makes the directory the
  document's origin.** `Document::save_as` writes the scene and adopts the
  directory — the next save writes there, and `owned` is the set save-as wrote,
  so the first save after it removes a chunk the scene stopped naming — which is
  what the command means in every editor. The copy's safety is kept: a directory
  already holding a file the scene would write is refused before anything is
  written (`EditError::Occupied`), and nothing in the old directory is removed
  or touched; the old files are left a scene of their own. Into the document's
  own origin it is a plain save. **The asset root follows the origin**
  (`document::origin::AssetRoot`): a source derived from the old origin, or none
  at all, becomes `document::asset_root` of the new directory and is measured
  afresh; one named by `set_assets` — `--assets` — stays. `save_to` is still the
  copy that adopts nothing.
- **The directory is typed** on a path line under the toolbar
  (`apps/editor/src/panel/path_line.rs`, which Open uses too), the shell having
  no file dialog: a text input engaged as it opens, Enter commits and Escape
  cancels as a rename's does. Ctrl+Shift+S and the toolbar's Save as open it; so
  does Ctrl+S on a document with no origin, which used to refuse with
  `EditError::NoOrigin`. `document::save_target` checks the text at the boundary
  — trimmed, made absolute against the working directory, refused
  (`EditError::Target`) when empty, holding a control character, or naming
  something other than a directory. A refused save-as is on the status line and
  the line opens again holding what was typed. A toolbar click that commits a
  line being typed saves before the click's own action runs, so it is the scene
  being edited that is saved.
- **The exit criterion's proof** is
  `app::tests::exit_criterion::empty_scene_to_play_and_stop_without_a_text_editor`,
  one test through the real `Editor` loop on the headless shell and the null
  backend: Ctrl+N; the fixture triangle dragged from its browser row onto the
  ground; the translate gizmo's X arrow dragged; the inspector's add button for
  `bodies` clicked and the mass dragged; the toolbar's Save as, a directory
  under the game's folder typed, Enter; a fresh editor opened on that directory
  alone (a fresh editor rather than Open, which is the stronger reopen); F5 and
  thirty frames of play; F5. Each step asserts what it changed — the scene
  empty, the mesh measured from the asset and selected, the move along X alone,
  both systems listed and the mass raised, the scene the document's own — and
  that the game's folder is unchanged until the save-as, is the fixture plus the
  scene's files after it, and is unchanged by the reopen, play and stop. The
  reopened scene is the saved one, the mesh measured from the game's root, the
  body falls more than half a metre, and stop restores the files byte for byte.
- **What it does not cover**: nothing of it has been seen on a device — every
  step is headless, on the null backend, against laid-out rectangles. The
  fixture triangle is one flat part; the body falls into nothing (the new scene
  has no ground and the test builds none), so resting is not shown; the property
  edited is a drag-value dragged, not a number typed. `docs/backlog.md` lists
  these with what each takes.
- **Evidence**: the document's tests hold a new scene's state from a document
  that had a history, names, a selection and an origin, and its refusal in play;
  save-as adopting the directory (a system unlisted straight after it removed by
  the next save), leaving the old directory byte for byte, refusing an occupied
  one with the origin, the marker and both directories unmoved, saving in place,
  the asset root following a derived source and not a named one, and each typed
  target refusal. The panels' hold the line typing, committing and cancelling,
  and refused in play; the keys', Ctrl+N and Ctrl+Shift+S; the loop's, Ctrl+N
  dropping and saying so, refused in play, the toolbar's two buttons, Ctrl+S
  with no origin opening the line, a typed directory saved into with Ctrl+S
  following it, an occupied one refused and asked again, a file refused, and a
  toolbar click committing the line saving the scene it was typed for. Each of
  these mutations turned a test red: save-as keeping the old origin, not
  adopting what it wrote, a copy overwriting an occupied directory, the asset
  root never following, a named root following anyway, a new scene allowed in
  play, keeping its origin, or leaving the membership count, Ctrl+S with no
  origin refusing, Shift ignored on Ctrl+S, Ctrl+N unbound, a file taken as a
  target, the line committing nothing, a dropped scene unnamed, the empty scene
  framed about the origin, the save-as carried out after the frame's actions,
  the empty scene listing a system, and stop not restoring.

**Open and the unsaved bar, landed 2026-10-03**, on the decisions of the same
day (below).

- **Decided 2026-10-03, for the long term: unsaved changes are confirmed inside
  the editor**, not by an OS dialog: a bar under the toolbar
  (`apps/editor/src/panel/unsaved.rs`) saying what would be lost — "Unsaved
  edits to `X` would be lost to a new scene", "by opening `dir`", "when the
  window closes" — with Save (Enter), Discard (D) and Cancel (Escape). It guards
  a new scene, an open and the window closing, for a dirty document only; a
  clean one goes straight on. While it is up nothing else is done: the keyboard
  answers it and nothing else (`keys::unsaved` is read in place of
  `keys::actions`), the panels take no navigation, typing or press but the
  bar's, and the viewport takes no press. Save on a scene with no directory
  opens the save-as line and goes on once it saves; cancelling that line, or
  asking for anything else, drops what was waiting. A refused save puts the bar
  back. A Save while the scene plays — only a close can ask then — stops play
  first, so the authored scene is saved. The flow is
  `apps/editor/src/app/unsaved.rs`.
- **Decided 2026-10-03: the window's close is held open while the bar asks**,
  wherever the shell lets the app decline one — which is every backend the
  editor runs on: Win32 intercepts `WM_CLOSE`, AppKit's `windowShouldClose:`
  answers `NO`, X11's `WM_DELETE_WINDOW` and Wayland's `xdg_toplevel.close` are
  requests, and the headless shell models them. The request stays outstanding
  until Save or Discard accepts it or Cancel answers it `CloseReply::Keep`. The
  browser shell asks too, but the editor has no web build. **What cannot be held
  is recovered**: a window taken away without a request (`WindowDestroyed`) and
  a run whose frame fails write a dirty scene to `Document::write_recovery`'s
  copy — the authored files, even in play — in a new directory
  `<temp>/crcbl-editor-recovery/<millis>-<scene name>/` that is never
  overwritten (a taken name is passed over for `-1`, `-2`…), and the log says
  where. Under the system's temporary directory, not beside the scene, so a copy
  is never one more scene in a committed tree or in the asset browser. A run
  ending on its frame budget or limit writes none.
- **Open** (Ctrl+O and the toolbar's Open) is a typed directory on the path
  line, which starts at the directory holding the current scene's own, checked
  by `document::open_target` — refused by name, as `EditError::OpenTarget`,
  unless a directory holding `scene.ron` is there — and read by
  `Document::open_dir` with this build's vocabulary **before** the bar asks, so
  a typing slip never puts the scene being edited at stake; a refusal opens the
  line again with the text. Refused in play mode. The opened document replaces
  the old whole: its asset root follows `document::asset_root` unless `--assets`
  named one, the panels are built over it afresh (the browser relists, the
  selection is empty), its history is its own, and the renderer is rebuilt from
  its assets on the next draw.
- **Decided 2026-10-03: a new scene takes its directory's name.** A save-as of a
  scene still called `untitled` names it after the directory — its last
  component less an extension — and writes that name into the header
  (`crcbl_scene::scn::Scene::set_name`); a refused save-as puts `untitled` back.
  A scene with a name of its own keeps it wherever it is saved, so committed
  scenes are written byte for byte as before, and nothing else renames a scene.
- **Escape reaches the editor.** The engine loop's `Pending::observe` claims it
  as `PAUSE_KEY`, which the editor's own loop has no pause for; before this
  Escape backed out of no text field through a real window, only in the panels'
  tests, which hand the tree a `NavInput` directly.
- **Evidence**: the document's tests hold `open_target`'s refusals and a scene
  directory taken, a new scene named after its directory and reopened by that
  name, a named scene's files unchanged, and a refused save-as keeping
  `untitled`; the recovery writer writing the scene and leaving the document
  dirty with no origin, never overwriting a taken name, writing the authored
  scene in play, and making a directory name of any scene name. The keys' hold
  Ctrl+O and the bar's three keys read only by `keys::unsaved`; the panels' the
  bar's three answers, the panels holding still behind it, the bar closing a
  path line unsent, and the open line committing an open and refused in play.
  The loop's hold Cancel keeping everything, nothing else done while the bar
  asks, Save then a new scene, Save with no directory through save-as then on, a
  cancelled save-as doing nothing, a dirty close held open until answered and a
  clean one closed at once, Save on a close in play, a recovery copy of a dirty
  scene and none of a clean one when the window is taken away, and Open: a typed
  scene in place with its entities, origin, assets, an empty selection and a
  fresh log, a scene drawn from its own game's files where the old one's held
  the same key, a named asset root kept, a dirty open asked about, a non-scene
  refused by name, refused in play, and the toolbar's button. The mutations each
  turned a test red are listed in the commit that landed this.
- **What it does not cover**: none of it has been seen on a device, and the
  close hold is the headless shell's — each backend's own holding is its shell
  tests', not this editor's. `docs/backlog.md` lists what is deferred.

**Recovery offered back, pruning and autosave, landed 2026-10-03**, on the
decisions of the same day (below). The flow is
`apps/editor/src/app/recovery.rs`; the directory's copies are
`apps/editor/src/document/recovery/copies.rs`.

- **Decided 2026-10-03, for the long term: recovery is offered, never forced.**
  At every start — whether or not a scene was named on the command line, since a
  copy of that scene is the likeliest one wanted — a recovery directory holding
  copies puts a **recovery bar** under the toolbar
  (`apps/editor/src/panel/recovery.rs`) listing the newest few
  (`app::recovery::OFFERED`), each by name and how long ago it was written, with
  **Open copy**, **Delete** and **Later**. The bar holds nothing: the panels and
  the viewport work under it. Open copy reads the copy with
  `Document::open_recovery` — **unowned**, no origin, so a save goes through
  save-as and a copy is never written back over itself; and **dirty**, so what
  it holds is asked about before it is lost — and puts it in place through
  Open's path, the unsaved bar asking first; refused in play mode; the bar goes,
  and once the copy is open the rest are offered at the next start. If the
  unsaved bar's question ends with nothing opened — Cancel, or a Save whose
  save-as line was closed unsaved — the bar comes back as it was (built
  2026-10-03: `Editor::restore_offer`, held by a loop test for each way, and one
  that a later question cancelled after the copy opened brings back nothing,
  each shown red by a mutation). Delete removes that copy's directory by the
  path listed for it, and the bar lists the rest; Later puts the bar away for
  the run.
- **The bar's keys** (built 2026-10-03): O, Ctrl+Delete and L answer Open copy,
  Delete and Later for the first row, the newest copy (`keys::recovery`), whose
  buttons name them. The loop reads them beside the editor's own keys while the
  bar is up, the unsaved bar is not and no field is typed into — so an `o` typed
  into the path line opens nothing, and under the unsaved bar they answer
  nothing. O and L are letters no reserved context binds, so they work whether
  or not a panel holds the keyboard; Ctrl+O is still Open, and Delete alone
  still removes the selection — Ctrl+Delete asked for nothing before. **Not
  Escape for Later**: the first key pressed lands focus in the panels, after
  which the pushed `ui` context owns Escape. The bar's buttons are ordinary
  focusable buttons, so from a panel holding the keyboard Tab walks onto them
  and Enter presses one. Held by the keys' test (each answer read, Ctrl+O,
  Delete and Escape not answers, nothing while typing) and the loop's (O opens
  the newest copy, Ctrl+Delete removes it and no other and no entity, L puts the
  offer away with a panel holding the keyboard; O and L in the path line and
  under the unsaved bar answering nothing; Tab from an outliner row reaching
  Open copy and Enter pressing it), each shown red by a mutation.
- **A copy remembers where its scene lived** (built 2026-10-03, the follow-up
  the backlog deferred). `Document::write_recovery` — the autosave included —
  writes `origin.txt` (`document::SIDECAR`) beside the scene's files: a line
  `origin=<path>` for the directory the scene was opened from or last saved to
  (or, for a recovered scene, the one its own copy recorded) and `assets=<path>`
  for the asset root it followed, each made absolute; a root named by `--assets`
  is a source with no path and is not recorded, and a copy of a scene that lived
  nowhere has no sidecar. `Scene::load` reads only the header, the environment,
  the listed chunks and the names file, so the loader never sees it.
  `Document::open_recovery` reads it back **untrusted**: a path is kept only
  when it is absolute and a directory now, and anything else is passed over with
  a note the status line shows as a warning (`Document::take_recovery_notes`). A
  kept asset root is what the recovered scene's meshes read from; a kept origin
  (`Document::recorded_origin`) is the save-as line's text, which a person
  commits or not through the same checks as anything typed — so a save-as into
  the old directory is still refused while it holds the scene's files. Nothing
  is written or removed through either path. A copy without a sidecar reads its
  meshes from no asset root until a save-as gives it one (or `--assets` names
  one), as before. Held by the sidecar's own tests (a round trip; a gone
  directory, a file, a relative path and a foreign line each passed over with a
  note), the writer's (the record written, the meshes read from it, the record
  carried into a copy of the recovered scene; a copy without one unchanged; a
  stale one passed over and nothing made), and the loop's (the status line
  naming the old directory, the save-as line holding it and a commit there
  refused with the old directory untouched; a stale record a warning with
  nothing offered), each shown red by a mutation.
- **Decided 2026-10-03: a recovered copy is removed once its scene is safely
  saved elsewhere.** The document keeps the path of the copy it was read from
  (`Document::take_recovered`); the first save-as that lands removes that copy
  through `document::remove_copy`, as Delete does, because the work it held now
  lives in the scene's own directory and offering it again would only invite a
  stale restore. A failed save-as leaves it, and a new scene in its place
  forgets it. A copy that would not go is logged and offered at the next start.
- **Every removal is by a computed path, checked.** A copy is a directory
  directly under the recovery directory, not a link, named
  `<millis>-<scene name>` as `Document::write_recovery` makes it;
  `document::remove_copy` refuses anything else as `EditError::NotACopy`, and
  nothing is ever removed by a pattern. A file or a directory a person put in
  the recovery directory is never listed and never removed.
- **Pruning at start-up**: before anything is listed, `document::prune_copies`
  removes each copy older than `document::MAX_AGE` (two weeks) and each past the
  newest `document::KEEP_NEWEST` (twenty), by its own path, and the log names
  each. The scene named on the command line is never pruned; nothing is pruned
  later in a run, so a copy opened from the bar is never pruned in that run.
- **Decided 2026-10-03: autosave.** While the document is dirty, every
  `editor.autosave.interval` seconds of the editor's clock (`settings.toml`
  beside the layout and the snap steps; `app::recovery::AUTOSAVE_SECONDS` unset,
  a minute) the authored scene — never the played state — is written into the
  recovery directory as a copy like any other, so a crash or a process killed
  with its session loses at most one interval: the gap the unsaved bar's
  recovery copy left, closed without an OS hook. The interval restarts whenever
  the document is clean, so the first autosave comes a whole interval after the
  first unsaved edit, and an unchanged scene is not written again. **One slot
  per document session**: a new autosave is a new copy written before the
  session's previous one is removed, so an interrupted write never leaves the
  session with nothing, and no other copy is touched. The slot goes once the
  document is clean (a save, or an undo back to the saved state) and when the
  session ends (a discard, a new scene, an open, the window closing); a recovery
  copy written as the window is taken away replaces it. A run ending on its
  frame budget or limit leaves a dirty session's slot, which the next start
  offers.
- **Decided 2026-10-03: a live session's autosave is marked as in use.** Each
  editor marks its slot (`document::mark_in_use`) with a file beside it, named
  for it with `document::IN_USE_SUFFIX`, holding an **exclusive lock** on it
  (`std::fs::File::lock`) for as long as the slot is its own; the file also
  names the process. Another editor's listing, Delete (`EditError::CopyInUse`)
  and pruning pass over a copy whose marker is locked. **A lock rather than a
  process-id check**: the standard library cannot ask whether a process runs, a
  recorded id can be reused after a crash, and the operating system releases a
  lock when its process ends however it ends — so a crashed session's marker is
  unlocked, its autosave an ordinary copy that is offered, and the marker goes
  with the copy. No `unsafe` and no new dependency: the lock is `flock` on Linux
  and macOS and `LockFileEx` on Windows, all through the standard library.
  **Fail safe**: a marker that is there but cannot be opened or checked counts
  as held, so nothing live is ever removed. Verified on Windows only; the Linux
  and macOS sides compile for both targets but have not run.
- **Where**: `--recovery <DIR>` names the directory; unnamed, it is
  `<temp>/crcbl-editor-recovery/` as before. **A headless run without the flag
  keeps none** — no offer, no prune, no autosave, no recovery copy — as it keeps
  no settings file: a test or a CI job must never reach into a person's copies.
- **Evidence**: the copies' tests hold only copies listed, newest first, a copy
  removed and every other path refused and left (outside the directory, not a
  copy's name, nested, the directory itself, a `..`), and a prune removing the
  old and the excess and keeping the open one and what is not a copy; the
  recovery writer's a copy opened unowned and dirty, with save refused for no
  origin and the copy untouched. The panels' each of the bar's buttons naming
  its row, and a click under the bar still selecting. The loop's start-up
  offering the newest copies (and nothing with none, or headless), Open copy
  opening it unowned with Ctrl+S asking for a directory, refused in play, Delete
  removing only that copy, a Delete of a path outside the directory refused on
  the status line, Later, an open keeping the offer up, start-up pruning old and
  excess copies but not the scene opened, an autosave only after the interval
  and only while dirty, not rewritten unchanged, replacing only its own slot,
  the authored scene in play, removed by a clean save and by a discard, and
  replaced by the recovery copy of a window taken away. The mutations each
  turned a test red are listed in the commit that landed this. The two follow-up
  rules add: a live autosave not listed, its Delete refused by name and not
  pruned, and an ordinary copy once released (the copies' tests, the lock held
  in-process); a crashed session's unlocked marker making an ordinary copy; a
  marker that cannot be checked keeping its copy; only a copy marked; a document
  remembering its copy once and forgetting it at a new scene; and in the loop,
  an autosave marked so another start beside it offers nothing and offers it
  once the session ends, a save-as of a recovered scene removing its copy and no
  other, and a failed save-as keeping it until one lands.
- **What it does not cover**: none of it has been seen on a device.
  `docs/backlog.md` lists what is deferred.

**The undo property test, the second exit criterion, landed 2026-10-03.**
`document::undo_property_tests::random_histories_walk_back_through_every_state`
is a `proptest` property over random histories of every edit, replacing
`entity_tests`' hand-seeded loop; its module docs are the specification.

- **Every step goes through the UI's entry point**, so a refusal is played and
  counted, not skipped: a property set through `Document::apply`, the
  inspector's write reported to `record_edits`, a field paste, an inspector drag
  under one gesture, an arrow-key nudge and a translate drag of one or two
  entities (`apply_in`, one gesture), a turn and a scale as the gizmos' batches,
  F2's `rename`, delete, duplicate and copy-and-paste of a selection, a garbled
  paste, a mesh drop, attach and detach, `ListSystem` and `UnlistSystem`, undo
  and redo. Targets include an id nobody holds and systems that do not hold the
  entity; values include other kinds, non-finite floats, magnitudes no `f32`
  holds and texts `check_asset` refuses.
- **What each history checks**: a refused step changed nothing and recorded
  nothing; an accepted one recorded exactly one entry, dropping any redo above
  it; every undo and redo, interleaved anywhere, lands on the state recorded at
  the position it moved to; the selection names only held entities and the dirty
  marker follows the position; then the whole log undone to the opening state
  and redone to the top, every step checked.
- **What the run checks**, so it cannot pass by doing nothing: every
  `EditCommand` variant was recorded (`variant` is a match with no wildcard, so
  a new variant does not compile until it is named, and naming it fails the run
  until a step records it), every kind of step was accepted, each shape of edit
  in `MUST_REACH` happened (two-entity edits, a gesture folding writes, a drag
  whose leaves change part-way, a drag that ends where it began — once with redo
  above it, whose depth it must leave alone — listing drops and attaches, an
  unlisting from the manifest's middle, a name, an edit dropping redo, a refusal
  with redo above it), something was refused, and at least
  `LEAST_ACCEPTED_PERCENT` of the edits played were accepted. A run is `CASES`
  histories of up to `MAX_STEPS` steps.
- **What "state" is** (`undo_property_tests::state::State`): `Document::files` —
  the manifest in order, `names.ron`, every listed chunk, byte-identical for
  equal scenes — plus, per id, every registered system's row (listed or not) and
  the picking collider's transform and world box, plus a count of live entities
  filed under no id. Keyed by `SceneEntityId`, not `World::hash_state` or
  `hash_world`, for the 2026-09-30 decision's reason. **Not compared**: the
  selection beyond having no holes (undo does not restore it, by design), a
  mesh's measured box except through its collider, and the gesture and next-id
  counters.
- **It found one bug, and fixing it showed two more**: a block turned back to
  nothing about a negative axis, `(-0, 0, 0, 1)`, came back from a delete's undo
  as `(+0, 0, 0, 1)`. `Rotation::is_identity` compared with `==`, under which
  `-0.0 == 0.0`, so the row left the rotation out and the restore read the
  default. It now compares bits, so such a rotation is written and reads back as
  itself
  (`registry::rotation::tests::a_negative_zero_is_written_and_reads_back_as_itself`,
  `document::rotation_tests::a_negative_zero_turn_survives_a_delete_and_its_undo`).
  With the identity exact, `panel::tests::rotation`'s drag test saw what `==`
  had hidden: the rotation row's composed quaternion wrote `-0.0` over a held
  `+0.0` in `x` and `z`, and `crcbl_ui`'s `FieldRow::set` (and the plain rows'
  write) compared with `==` and reported nothing — a write in the field that no
  command recorded and no undo took back. Both now compare floats bit for bit
  (`tree::widgets::tests::inspector::a_write_that_flips_a_zeros_sign_is_reported`),
  and the rotation row writes `+0.0` for a zero, so a turn about one axis leaves
  no `-0.0` in the scene that nobody chose. The property test's seed is in
  `apps/editor/proptest-regressions/`, committed as `crcbl-core`'s and
  `crcbl-water`'s are.
- **Mutations each turned it red and shrank to a short history**: a rename's
  inverse renaming to the new name (one rename), a delete's inverse dropping its
  last system's row (one delete of the entity in two systems), a gesture's fold
  keeping the newest inverse (one two-frame drag), an unlisting's inverse
  listing at the end (a listing at the front, then its unlisting), a batch's
  inverse in forward order (one mesh drop), a delete's inverse dropping the
  name, and an undo not rebuilding the collider (one nudge). Removing detach
  from the played steps failed the run's command coverage, and raising the
  accepted share to ninety percent failed its acceptance check.
- **Not covered**: play mode (every edit is refused in it, `play_tests`), a save
  between edits (which seals the entry on top), the inspector's widgets
  themselves — its steps report edits to `record_edits` directly, so
  `FieldRow::set`'s reporting is the panel tests' to hold — the rotation row
  reporting four leaves to one `record_edits`. `docs/backlog.md` has them.
- **Decided 2026-10-03, for the long term: one gesture is one undo entry.**
  `UndoLog::record_in` merges a gesture's writes whatever leaves each names,
  keeping per leaf the earliest value from before the gesture wrote it and the
  latest it left, and dropping a leaf whose two are bit-identical — and the
  entry, when no leaf is left, so a drag back to its start records nothing. It
  used to fold only writes of the same leaves, so a drag whose frames reported
  different leaves split into several undos. The property test's drag step
  writes a second leaf part-way and can end where it began;
  `command::tests::a_drag_whose_frames_report_different_leaves_is_one_undo_restoring_all`
  is the regression. Keeping the newest inverse instead of the earliest, and
  dropping a leaf only early frames wrote, each turned both red.
- **Decided 2026-10-03, for the long term: a drag back to its start keeps the
  redo above it.** `UndoLog::push` holds the entries a gesture's first write
  truncates (`UndoLog::held`) instead of dropping them, and `record_in` appends
  them again when the gesture's entry goes for netting to nothing; the next push
  replaces them, since only the open entry on top can go. Chosen over holding
  the gesture's first write aside until it ends — the log has no gesture-end
  event, and the entry on top would be missing while the drag runs. The property
  test asserts a net-nothing drag leaves the redo's depth alone and keeps every
  state above in its model, which the walk up at the end holds exactly;
  `MUST_REACH` gained "a gesture back to its start under redo", and `CASES` went
  from 256 to 512 so that shape is reached 14 to 18 times a run (5 to 12 at 256,
  measured over five runs each).
  `command::tests::a_gesture_back_to_its_start_keeps_the_redo_above_it` and
  `a_gesture_that_changes_something_still_drops_the_redo` are the unit tests;
  removing the restore turned the first and the property test red (its shrunk
  case — delete, undo, a drag of offset zero — is committed in
  `proptest-regressions/`), and not truncating at all turned the second.

**Switching a body's kind in the inspector landed 2026-10-03**, closing the item
slice 12 left (a `kind` shown and edited only in the file).

- **Reflection switches an enum's variant.** `crcbl_reflect::Reflect` gained
  `variants` and `set_variant`, provided (a non-enum has none and refuses every
  name) and written by `#[derive(Reflect)]` for every enum: a switch makes the
  new variant with each field at its type's `Default` — decided for the long
  term over a constructor per variant, since `Default` is what every field type
  in the workspace already has and a switch then needs no attribute — and a
  switch to the active variant changes nothing. **No field carries over**, even
  one of the same name and type: a field means what its variant means by it.
  `Snapshot` reads a whole value (each enum tagged by its variant) and writes it
  back, putting the value back when it does not fit; it holds the rows only, so
  a `#[reflect(skip)]` field comes back at its default after a switch away and
  back.
- **The inspector offers the variants** with `InspectorOptions::variants`: an
  enum's group opens on a strip of options, the active one `:checked`, each
  focusable and picked by click or accept — the toolkit's choice widget (the tab
  strip's shape), since there is no pop-up layer for a drop-down. A pick is a
  `VariantEdit` of the enum's path and its `Snapshot` before and after, in
  `Inspection::switches`; the switch is made after the frame's rows are built,
  so a caller undoes a frame's switches before its field edits.
- **The editor records a switch as one `EditCommand::SetVariant`** carrying the
  whole enum, whose inverse is the snapshot read as it is applied — bit for bit,
  as a property's is. `Document::record_edits` rewinds the switches and then the
  edits and applies them as one command; the component's rule judges it like any
  property write (`validation::writes`), and it is never folded into a gesture.
- **Evidence**: `crcbl-reflect`'s `tests/variants.rs` (listing, defaults, the
  active variant kept, an unknown name refused, the exact inverse through a
  nested enum, a misfit put back, a NaN restored over itself) and the derive's
  token tests; `crcbl-ui`'s strip tests (one switch with the whole enum before
  and after, made after the frame's rows, the active option marked and not
  pickable, reached by focus and accept, absent without the option); the
  editor's `document::variant_tests` (one entry undone and redone exactly, saved
  and read back, a static body standing through a second of play, refused by the
  rule and put back, an unknown variant refused) and `panel::tests::variants`
  (the strip clicked in a body's section). The undo property test gained a
  switch step through the strip's report and through a command, and `SetVariant`
  in its command coverage. The mutations each turned a test red are listed in
  the commit that landed this.

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
   system's name and entity count, and `SystemTrait::debug_fields` lends an
   entity's row as `&dyn Reflect`, which the sandbox's debug panel shows
   read-only (2026-10-03). The per-component half is built — `crcbl-reflect`,
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
  world's tick rate on a fixed step from the frame's time. _Amended 2026-10-03,
  below_: a module is handed the commands the editor sent it.
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
  Rotation is locked (above, slice 12; unlocked 2026-10-01).
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

**Editor follow-ups, 2026-10-02.**

- **Attach offers every registered system**, the manifest's first in its order
  and then the rest grouped by the game that registered them (below). Attaching
  to one the manifest does not list is one `EditCommand::Batch` of `ListSystem`
  (at the manifest's end) and `Attach`, the shape a mesh drop already had
  (`Document::listing_first` builds both), so one undo puts every file back byte
  for byte. **Decided: detaching a system's last entity leaves it listed** — an
  empty entry saves an empty chunk and loads as nothing, and unlisting is an
  explicit `UnlistSystem`, so a detach's inverse keeps one shape.
- **The undo property test plays drops and the manifest's edits**: a mesh drop
  (listing `meshes` when the manifest lacks it), a `ListSystem` at a random
  place, an `UnlistSystem` of an empty listed system and an attach to an
  unlisted one, beside the earlier steps, comparing every file — `scene.ron`
  included — at each step down and back up, and failing if any of those steps
  never ran. It found that an unlisting's undo put the system back at the
  manifest's end, saving a reordered `scene.ron`: `ListSystem` now carries its
  place (`at`) and `Scene::list_system_at` inserts there. Putting the undo back
  at the end, and an unlisting whose inverse did nothing, each turned it red.
- **One validation rule per component, at load, at the edit and at the save**
  (decided and built 2026-10-02). `crcbl::registry::Validate`, a bound on
  `Registry::register` with a provided `Ok` (`impl Validate for T {}` states no
  rule), is a component's rule over its own values, after the registry's own
  check of every `Rotation` it carries. One function runs it at every door:
  every row a registered codec reads (a file's, a pasted one, an undone
  delete's), refused as `ScnError::Parse` with file, line and column through
  `crcbl_scene::scn::chunk_ruled`; `Registry::validate`, which `Document::apply`
  runs on every component a `SetProperty` or `Batch` wrote, putting the whole
  command back and refusing it as `EditError::Invalid` by entity, system and
  field; and `Registry::problems`, which reads every listed chunk back through
  its codec, so the save reports exactly what the next load would refuse.
  `Body`'s rule is `Body::check`, `Mesh`'s `check_asset`, and the editor's
  `Block` refuses a half extent a box collider would panic on; the picking
  collider is rebuilt only after a command passes. The rotation check that was
  the editor's own (`document::rotations`, `EditError::Rotation`) is this rule
  now, and the bodies' and meshes' `Registry::check`s are gone. **Whole-scene
  rules stay `Registry::check`** — towers' path needs every waypoint — **and are
  not run per edit**: authoring passes through layouts the game would refuse (a
  corner placed before the leg it bends), so they are reported at the save.
  Removing the codec's rule, the edit check, the rewind, the rows from
  `problems`, the registry's rotation walk, or checking only a batch's first
  write each turned a test red.
- **The add list is grouped by game** (decided and built 2026-10-02).
  `Registry::group(label, f)` names the systems `f` registers; each sample
  registers in its own group, the engine's body and mesh in `ENGINE_GROUP`.
  `Document::attachable_groups` heads the manifest's systems "In this scene",
  then each game's group by label, and the inspector draws one line per group,
  so towers' `waypoints` is offered on a breakout scene under "towers".
- **A save removes the chunks the scene stopped owning** (decided and built
  2026-10-02, for the long term). A document opened from a directory owns the
  files its manifest there names, and each save into it owns what it wrote; a
  save there removes the owned files it did not write this time — an unlisted
  system's `sys/<name>.ron`, `names.ron` once the last name is cleared — and
  nothing else, after every file has landed, so a failed save loses no chunk.
  **A save into any other directory is a copy**: it removes nothing, and
  refuses, writing nothing, if a file it would write is already there
  (`EditError::Occupied`) — merging would overwrite another scene's `scene.ron`
  and orphan its chunks. `document::ownership` holds the rule;
  `document::save_tests` hold it against a real directory.

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
  and does not spin. Built in slice 14; unlocked on 2026-10-01 (_Physics
  rotation_, in _Status_).

**Decided 2026-10-03, for taking part in play** (taken for the long term and
recorded, as above):

- **Commands, not a bespoke panel per game.** The editor feeds play the same
  command bytes a networked client would, through the module's next
  `ClientInputs`, and the game validates them like any client's. A vocabulary
  registers a small play-controls description beside its module — named actions
  whose parameters are a picked scene entity or a choice from a fixed list, and
  an encoder to the game's command bytes — which the editor renders generically.
  A parameter kind "none" was not added: an action that takes nothing has an
  empty parameter list. This amends the 2026-10-01 decision's "ticks it with
  empty client inputs".
- **The status is registry-side, read off the world**: `PlayControls::status`
  (and `refusals`) are functions of the world the module plays in, and a game
  puts what a tool may read into that world — towers registers a readout system
  holding its stage. Declined: a provided `GameModule::status` method answering
  an empty list. It is the cheapest for every other game, but the module trait
  is the engine's seam with every game, its server and its wasm binding, none of
  which needs a tool's readout, and a change there is one EW has to read; the
  world is already the seam a tool reads a module through (runtime components).
- **Towers, bolts and bursts are mirrored as runtime entities**, the `Walker`
  pattern, so what play builds is drawn.

**Decided 2026-10-03, for multi-selection** (taken for the long term and
recorded, as above):

- **The document holds an ordered selection set with a primary**, the last
  clicked. A viewport click replaces, Ctrl and a click toggles, and Shift and a
  click in the outliner takes the range. The selection is editor state and not
  in the undo log, as it was; entities that stop existing drop out of it.
- **Shared-pivot translate**: the gizmo sits at the selection's pivot, named the
  bounds centre — the centre of the union of the selected entities' boxes — and
  a drag applies one delta to every selected entity's placing position, as one
  undo entry folded across the drag. Snap applies to the pivot on the absolute
  grid, and the same delta to all. Declined as the pivot: the primary's centre,
  which puts the handles at one end of a wide selection, and the mean of the
  centres, which a cluster drags away from the middle of what is drawn.
- **Scale and rotate stay single-entity.** With several selected, R and E show a
  status-line message and no handles: whether each entity resizes and turns
  about its own centre or the group about its pivot is a design choice left for
  later, and either is a different drag from the one-entity one.
- **Delete, duplicate, copy and paste act on the whole selection**, one undo
  entry each; a paste selects what it pasted. The inspector edits the primary
  only, and says so in its header.
- **The outliner and the viewport highlight every selected entity**, the primary
  distinctly.

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
  text editor. **Met 2026-10-03, headlessly**: `app::tests::exit_criterion`
  drives it through the loop (_A scene from empty and save-as_, above, says what
  it covers and what it does not).
- Undo/redo correct across all MVP commands (property-based test: random command
  sequence + full undo → state hash equals initial). **Met 2026-10-03**:
  `document::undo_property_tests` plays random histories of every edit command
  through the UI's entry points, interleaving undo and redo, and checks every
  state the log stands at, the full undo back to the opening state and the full
  redo to the top (_The undo property test_, above, says what it covers and what
  it does not). The "state hash" is a digest keyed by `SceneEntityId` — the
  saved files, every system's rows and the colliders — not the world hash, for
  the 2026-09-30 decision's reason. Scene load and save markers are not commands
  (task 4's list; the backlog says why).
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
