# Stage 8 — Scene Editor

`apps/editor`: the editor is a client of the engine (locked decision). It uses
the same renderer, ECS, server loop, transport, and GUI as a game. MVP editor:
open scene, move things, edit properties, save, play.

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
