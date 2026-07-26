# Stage 6 — Immediate-Mode GUI + Debug Tools

Own imgui (`crcbl-ui`) rendered through the engine's own draw path, then the
debug tooling built on top of it: profiler HUD, inspector, console. This is the
toolkit the stage 7 editor is made of.

## Goals

- One GUI system for editor and game (locked decision) — no egui, no foreign
  draw path.
- GUI renders as ordinary engine content: vertices into a per-frame buffer, one
  pass in the render graph, textures from the bindless/material system.
- Debug tools become visible: everything instrumented in stages 2–5 gets a live
  surface.

## GUI architecture (`crcbl-ui`)

- Classic single-pass imgui: widgets emit draw data + hit-test against
  _previous_ frame layout (Dear-ImGui-style, one frame of interaction latency,
  drastically simpler than retained/two-pass). Post-MVP revisit only if the
  editor genuinely hurts.
- `UiContext` per frame: input state in (stage 1 normalized input), draw list
  out. ID stack (hash of label + parent) for widget identity, hot/active
  tracking for interaction state.
- Draw list: triangles + scissor rects + texture id per command; tessellation
  for rects/rounded rects/lines/text quads on CPU (UI vertex counts are small;
  not worth GPU tessellation complexity).
- Renderer integration: draw list uploads into the per-frame bump-allocated
  vertex buffer, drawn by a dedicated graph pass, ortho projection (the 2D path
  from stage 3 — UI is the first consumer of it). Game world can also render UI
  in-world (it's just meshes) — free win for 3D-space UI later.
- Text: `fontdue`/`swash`-class rasterizer → glyph atlas (grows-on-demand, lives
  in the texture system). SDF text post-MVP; bitmap atlas at fixed sizes for
  MVP.
- Widget MVP set (editor-driven): label, button, checkbox, slider, drag-value,
  text input (single line), tree node, collapsing header, window (move/resize),
  dockable split panes (simple splitter, not full docking), list/table, color
  swatch. Nothing speculative — widgets are added when the editor or debug tools
  need them.
- Game-UI usable: styling is a small style struct (colors, padding, font) —
  enough for game HUDs; theming systems post-MVP.

## Debug tools (`crcbl-ui::debug` or thin crate atop it)

Surfaces for instrumentation that already exists:

1. **Profiler HUD** — stage 2/3 GPU pass timestamps + CPU frame phases as
   rolling graphs; frame-time budget bar. Toggle key.
2. **Inspector** — stage 4 system registry: per-system entity counts, tick
   times; select entity → each system that owns it renders its data via the
   system's debug-UI callback (systems describe themselves; inspector is
   generic).
3. **Culling/render stats** — stage 3 delayed-readback ring: visible/total, draw
   counts, pool occupancy.
4. **Console** — log sink view with filtering + command registry (`Fn(&str)`
   handlers registered by systems). Server commands route through the normal
   transport as `Command` messages — the console works identically over a
   network connection (server-authoritative debugging, free).
5. **Debug draw controls** — toggle the stage 3 debug-draw categories (AABBs,
   system overlays) per system.

## Tasks

1. `UiContext`, ID/hot/active core, draw list + tessellation, hit-testing.
2. Glyph atlas + text rendering.
3. Graph pass + per-frame buffer integration; input routing (UI consumes input
   first; unconsumed falls through to game — capture rules explicit).
4. Widget set, driven by building tool #1 (profiler HUD) first as the proving
   ground.
5. Inspector + console + stats panels.
6. Sandbox: full debug overlay over the Sponza scene from stage 5.

## Exit criteria

- Debug overlay (profiler, inspector, console, stats) runs in the sandbox over a
  live scene; interaction solid (drag sliders, select entities, run console
  commands against the server).
- UI pass cost visible in its own profiler row and within budget (<0.5 ms GPU
  for the debug overlay at 1080p on target hardware).
- A minimal "game HUD" demo (health bar + crosshair from the same API) proving
  the game-UI story.
- No renderer specifics leak into `crcbl-ui` (it produces draw lists;
  `crcbl-render` owns the pass) — checked by dependency direction in CI.

## Risks

- **Text rendering rabbit hole.** Bitmap atlas, Latin-1 + basic UTF-8, two font
  sizes. Shaping/RTL/emoji are post-MVP; the atlas design just mustn't preclude
  them.
- **Docking complexity.** Simple splitters only. Full docking is the classic
  imgui time sink; the editor layout (stage 7) is designed around splitters.
- **Input-latency purism.** One-frame hit-test latency is fine; do not build the
  two-pass layout engine in MVP.
