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
