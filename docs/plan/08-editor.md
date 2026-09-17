# Stage 8 — Scene Editor

`apps/editor`: the editor is a client of the engine (locked decision). It uses
the same renderer, ECS, server loop, transport, and GUI as a game. MVP editor:
open scene, move things, edit properties, save, play.

## Status: slices 1, 2 and 3 landed 2026-09-16, and what still waits

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

**The viewport decision below is still open, and slice 3 took neither branch.**
The tree can draw neither: `DrawCommand::Image` carries no texture identity,
`ImageAtlas::register` takes host bytes, and the render graph sets every pass's
scissor from the attachment's full extent with no sub-rect. So the scene is
drawn full-window, the panels are composited over it, and the viewport pane is a
hole whose rectangle gates picking. Closing it needs an image command that can
name a rendered target, or a render area the graph takes from the caller.

**What slice 2 did not settle.** `chunk_of::<T>` is typed, so a statically
linked binary cannot learn a component type at run time: a build of the editor
opens the vocabularies it was compiled with. The shipped build registers its own
greybox block and both samples' components, so it still opens breakout's board;
a build for another game adds a line to `apps/editor/src/scene.rs::vocabulary`.
Run-time discovery needs a link-time distributed slice (`linkme` or
`inventory`), which is a new dependency and the user's call.

Everything else below stands unchanged: the server still drops commands, there
is one schedule per `World`, there is no snapshot, the samples' state is outside
the ECS, the editor draws no inspector panel, the format cannot hold one entity
in two systems, debug draw is not a gizmo layer, `AssetSource` cannot list, and
there are no `serve`/`scene`/`edit` subcommands.

Two things sit behind it, in both directions:

- **It no longer waits on stage 6.** Feature 5 (scene IO) has a format to open
  and save: [06-assets-scenes.md](06-assets-scenes.md)'s task 4 landed
  2026-09-07 as `crcbl_scene::scn`, with `Scene::load` over an `AssetSource` and
  a `Scene::save` whose text is byte-identical for equal scenes, and
  `apps/breakout` reads its brick grid through it. (This bullet used to claim
  there was "no RON reader anywhere in the workspace", which was already false
  when it was written: `crcbl_render::stack::CameraStack::from_ron` and
  `crcbl_inventory::catalog::Catalog::from_ron` both predate it.) Feature 6, the
  asset browser, still waits on the rest of stage 6 — there is no watcher and no
  `crcbl bake`. Feature 3 waits on stage 7's inspector, which is also unbuilt
  ([07-ui-debug.md](07-ui-debug.md)).
- **Two sample plans wait on it**, and they are the only two samples with no app
  directory. [sample/07-towers.md](sample/07-towers.md)'s milestone 2 _is_ this
  document's dogfood pass — its exit criterion is "map authored 100% in the
  editor, zero hand-edited scene text" — and
  [sample/08-arena.md](sample/08-arena.md) wants an editor-built map too.

## Where the tree stands against this design (surveyed 2026-09-15)

**Unparked 2026-09-15 by the user**, to be built after the UI system's rungs in
[07-ui-debug.md](07-ui-debug.md), because every panel below is made of that
system. A read-only survey of the tree found the editor further away than the
rest of this document suggests; each line was checked in the source.

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
   dogfood pass — says in its own source that it has no entity and no ECS
   system. An editor has no world to edit in it until it is ported.
5. **No inspector _in the editor_**: `crcbl_ecs::Inspector::collect` returns a
   system's name and entity count, and the per-system debug-UI callback is an
   empty stub. The per-component half is built — `crcbl-reflect`,
   `Ui::inspector` and `crcbl::registry`, which is what a tool reads a component
   through, all 2026-09-16 — and what is missing is the editor drawing a panel
   with it.
6. **The scene format cannot hold one entity in two systems**: each chunk row
   spawns its own entity, so the same id in two chunk files is a duplicate-id
   error. The "attach/detach system data" command needs that first. `IdMap` has
   no removal, and there is no dirty tracking or per-chunk reload.
7. **Debug draw is not a gizmo layer**: lines only, depth-tested, off by default
   — no on-top mode, no filled handles, no constant screen size.
8. **No screen-to-ray helper**, and only collider-bearing entities pick.
9. **No undo or command log** anywhere, and the inventory kit's
   optimistic-then-reconcile shape exists only as prose in
   [34-inventory.md](34-inventory.md).
10. **`AssetSource` cannot list** (it has `read` alone), `crcbl import` writes
    nothing, and there is no watcher but the viewer's polled file.
11. **The UI cannot host an editor yet**: no textured quad or clip rect in the
    draw list, no layout, no keyboard focus, text input without selection, an
    ASCII bitmap font, and a DPI scale passed as `1.0` everywhere — the rungs of
    [07-ui-debug.md](07-ui-debug.md).
12. **No `serve`, `scene` or `edit` CLI subcommands**, and no native file
    dialogs or menus.
13. **Two statements in the 2026-08-09 corrections below are now out of date**:
    X11 does have drag-drop (through XDND), and Win32 OS drops work; only the
    Win32 clipboard file-list half (`CF_HDROP`) stands.

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
  system that forgets one loses state silently.
- **A component's editable fields come from `#[derive(Reflect)]`** in a new
  proc-macro crate, one annotation per component, checked at compile time. This
  is the workspace's first proc-macro dependency (`syn`, `quote`,
  `proc-macro2`), approved with the decision.

**Decisions still open:**

- **Docking**: splitters only, as the UI plan fixes, and whether tabs are in.
- **The viewport**: a secondary view rendered to a texture a UI rect samples, or
  the scene drawn full-window with UI panes around a scissored region.
- **File dialogs**: native per backend, or an in-UI browser over
  `StorageSource::list`.
- **The scene format change** for entities spanning systems (the format is v0,
  so a break is allowed).
- **A file watcher dependency** for hot reload (`notify`), already open in the
  backlog.

## Architecture

- **The editor is a client+server pair**, exactly like the sandbox: an editor
  _server_ runs the scene in a paused/edit-mode simulation; the editor _client_
  renders it and hosts the UI. All edit operations are `Command` messages
  through the transport — which means:
  - Undo/redo = command log with inverse commands (the transport already
    serializes them).
  - Collaborative/remote editing is structurally possible later (not MVP),
    because editing is already message-based.
  - Play-in-editor = tell the server to switch from edit-mode schedule to the
    game schedule (edit-mode systems freeze, game systems run). Stop = restore
    the pre-play snapshot (the stage 4 snapshot machinery, reused).
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
   snapping. Drawn via debug-draw layer, interact via viewport input.
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
6. Asset browser + drag-spawn.
7. Play/stop with snapshot restore.
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
- **The editor is a native target.** `10-wasm-webgpu.md` lists editor-in-browser
  as a stretch that "should mostly work by construction"; the asset browser, OS
  drag-drop import, `crcbl import` and hot reload's notify-based file watcher
  are all native-shaped, and nobody has examined what a browser would do with
  them. Treated as native-only until something makes the case; recorded so the
  stretch goal is not mistaken for a plan.
