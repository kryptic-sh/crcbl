# Stage 7 — Immediate-Mode GUI + Debug Tools

Own GUI (`crcbl-ui`) rendered through the engine's own draw path, then the debug
tooling built on top of it: profiler HUD, inspector, console. This is the
toolkit the stage 8 editor is made of.

## Goals

- One GUI system for editor and game (locked decision) — no egui, no foreign
  draw path.
- **Web-like layout + styling**: the UI is a DOM-ish tree of nested blocks/spans
  laid out by a CSS-subset engine, styled via stylesheets with selectors.
  Layouts read like web layouts; styles hot-reload like web dev.
- **Engine-wide**: every UI surface the engine itself ships — debug overlay,
  editor chrome, sample HUDs, the hud demo — is built from this system and its
  stylesheets. There is no second UI path; changing engine UI = editing tree
  code + css, same as a game would. The [hud sample](sample/04-hud.md) is the
  living fixture and gallery.
- GUI renders as ordinary engine content: vertices into a per-frame buffer, one
  pass in the render graph, textures from the bindless/material system.
- Debug tools become visible: everything instrumented in stages 2–6 gets a live
  surface.

## What is built, and how far it is from this design

`crates/crcbl-ui` exists and is **not** the DOM/CSS system described below.
Nothing in the workspace parses a stylesheet, lays out a flex box or builds a
block/span tree; no engine crate reads a `.css` file, and `crcbl_ui::widget`'s
`Style` is a plain struct of five colours rather than a cascade. What shipped is
the pre-CSS toolkit the debug panel and the samples needed first:

- **`draw_list`** — `DrawList`, `DrawCommand`, `Vertex2d`: the one interface
  between the UI and the renderer, as the rendering section below describes.
- **`text`** — `FontAtlas`, a built-in **monospace bitmap** ASCII font with
  metrics and a simple layout. Not the `fontdue`/`swash`-class rasterizer, and
  not the shelf/skyline atlas with LRU eviction the corrections below specify:
  those arrive with real fonts, and nothing has needed one yet.
- **`widget`** — `Label`, `Button`, `Style`, `SkinInsets`, `PointerInput`,
  `UiState`, `WidgetId`. The rest of the MVP widget set below is unbuilt.
- **`menu`** — `Menu`, `MenuItem`, `Slider`, `Cycler`, `MenuSet`:
  keyboard-first, with the pointer optional. Worth reading before designing on
  top of it, because it already meets one constraint this document has not: the
  UI pass's atlas is a single-channel glyph coverage mask and `DrawList` has no
  textured-quad command, so a menu's nine-sliced frames live in
  `crcbl_render::MenuArt` and `Menu::render` emits text alone. Any styled widget
  with a picture in it splits the same way.
- **`touch`** — `TouchStick`, `TouchButton`; see [19-input.md](19-input.md).
- **`debug`** and **`budget`** — the modular panel described under "Debug tools"
  below, and the frame CPU-vs-GPU row [40-profiling.md](40-profiling.md) owns.

`crcbl-ui` depends on `glam`, `bytemuck` and `crcbl-core` (the latter for the
shell's `ContactId`/`TouchPhase`, which `touch` hit-tests). It names no
renderer, so the dependency-direction exit criterion below holds.

## Architecture: immediate-mode authoring, DOM-like model, CSS-subset styling

Three cleanly separated layers:

### 1. Element tree (the "DOM")

- Two node kinds, browser-shaped: **block** (container, participates in layout)
  and **span** (inline leaf: text, image, custom-draw). Every widget is composed
  of these — a button is a block with a span child and behavior.
- Nodes carry `id` (unique name), `classes`, inline style overrides, and
  interaction state (hover/active/focus — usable as selector pseudo-classes).
- **Authoring stays immediate-mode**: game/editor code rebuilds the tree every
  frame through a builder API —
  `ui.block("#health-bar.hud", |ui| { ui.span("75/100"); … })`. No retained
  scene graph to sync; identity comes from the id path (same as the old ID-stack
  plan). Internally the tree is diffed/cached per frame for layout- and
  style-resolution reuse, but that's an optimization detail, not the API.

### 2. Layout engine (CSS-subset, from scratch)

- **Flexbox subset** as the one layout model (covers game HUDs and editor
  panels; grid post-MVP if something demands it): `display: flex | none`,
  `flex-direction`, `flex-wrap`, `justify-content`, `align-items`, `align-self`,
  `flex-grow/shrink/basis`, `gap`.
- Box model: `width/height/min/max` in px / % / `auto`, `padding`, `margin`,
  `border` (widths), `box-sizing: border-box` semantics only.
- `position: relative | absolute` (+ `top/right/bottom/left`) for overlays,
  tooltips, drag ghosts; `overflow: hidden | scroll` (scroll = scissor +
  offset); `z-index` within a stacking context.
- Not full CSS spec, deliberately: no floats, no inline flow beyond span runs
  inside a block, no tables (a table widget composes flex rows), no
  animations/transitions in MVP (style values can still be tweened by code).

### 3. Style system (CSS-compatible-ish)

- **Stylesheets in actual `.css`-syntax files** (subset parser, from scratch):
  type (`block`, `span`, widget names like `button`), `#id`, `.class`,
  descendant combinator, and pseudo-classes `:hover`, `:active`, `:focus`,
  `:disabled`. Specificity = simplified (inline > id > class > type; last-wins
  within a tier) — predictable over spec-faithful.
- Properties: colors (`background`, `color`, `border-color`), `border-radius`,
  `opacity`, `font-size`, `font` (family id), text align, plus every layout
  property above. Custom properties (`--vars`) + `var()` for theming.
- Cascade sources: engine `default.css` → game/app stylesheet(s) → inline
  overrides on the node. **Hot reload** via the stage 6 asset watcher — editing
  a `.css` restyles the running app; the editor's own look is a stylesheet, and
  game HUD theming = shipping a different stylesheet.
- Style resolution is cached per node id + class-set + pseudo-state; only dirty
  nodes re-resolve (style thrash is the classic perf trap here).

### Pipeline per frame

`build tree (immediate) → resolve styles (cached) → flex layout → emit draw list → graph pass`.
Hit-testing against the _previous_ frame's layout (one-frame interaction latency
— same simplicity win as classic imgui, now with real layout).

## Rendering (unchanged from original plan)

- Draw list: triangles + scissor rects + texture id per command; CPU
  tessellation for rects/rounded rects/borders/text quads.
- Uploads into the per-frame bump-allocated vertex buffer, dedicated graph pass,
  ortho projection (stage 3's 2D path — UI is its first consumer). World-space
  UI = same tree rendered with a world transform (3D nameplates free).
- Text: `fontdue`/`swash`-class rasterizer → glyph atlas in the texture system.
  Bitmap atlas at fixed sizes MVP; SDF post-MVP. Spans wrap within their block
  (simple greedy line-break; no shaping/RTL in MVP).

## Widgets

Widgets = block/span compositions + behavior, styled by the same stylesheets (a
widget ships default rules in `default.css`, overridable by games — the pikr
pattern, proven). MVP set (editor-driven): label, button, checkbox, slider,
drag-value, text input (single line), tree node, collapsing header, window
(move/resize = absolute-positioned block), split panes (flex + draggable
divider), list/table (flex rows), color swatch. Widgets are added when the
editor or debug tools need them, never speculatively.

## Focus + gamepad/keyboard navigation

Every UI screen is fully drivable without a pointer — gamepad, keyboard, and
(later) on-screen controls navigate the same trees the mouse clicks. This is the
bridge between topic 19's device-agnostic input and the DOM-like tree:
console-grade menu UX, designed once, free for every screen.

### Focus model

- **One focused element per context** (the topic 19 `ui` context); focus is
  ordinary interaction state next to hover/active — the `:focus` pseudo-class
  already styles it, so **focus rings are stylesheet-driven** (`outline`-style
  properties in `default.css`, themable per game).
- Focusable = interactive widgets by default (button, checkbox, slider, input,
  tree node, list row…); blocks opt in/out via a `focusable` attribute.
  Containers form **focus scopes**: modals trap focus inside themselves, scroll
  containers auto-scroll the focused element into view, windows/panes are scope
  roots.
- **Mixed-input rule** (last-active device, topic 19): pointer mode shows hover
  affordances; pad/keyboard mode shows the focus ring. A click also sets focus
  (so switching devices mid-flow continues from the obvious place); pad input
  after mouse use resumes from last focus or last hover.

### Navigation = reserved UI actions (topic 19)

Engine-reserved action set in the `ui` context — rebindable like everything,
default-bound on every device class:

| Action              | Keyboard          | Gamepad           | Semantics                                                                                                  |
| ------------------- | ----------------- | ----------------- | ---------------------------------------------------------------------------------------------------------- |
| `ui_move` (Axis2)   | **arrows + WASD** | dpad + left stick | spatial focus movement — arrows/WASD are a strict 1:1 of dpad; every UI screen is fully drivable by either |
| `ui_next`/`ui_prev` | Tab / Shift+Tab   | LB/RB             | tree-order traversal (fallback/lists)                                                                      |
| `ui_accept`         | Enter/Space       | South             | same event path as click — widgets can't tell                                                              |
| `ui_back`           | Esc               | East              | close modal / pop screen (context stack)                                                                   |

WASD-in-menus conflicts with nothing by construction: the `ui` context is active
while a menu has input, gameplay's WASD binding lives in the `gameplay` context
underneath — the context stack (topic 19) is the disambiguator, not special
cases.

### Focused vs engaged: the universal two-state rule (LOCKED)

**Focus never captures navigation.** Moving focus onto _any_ input widget —
text, number, slider, dropdown, drag-value, color picker, all of them — is
inert: `ui_move` keeps navigating right past it, always. A widget only starts
consuming input after **explicit engagement**: click it, press Enter, or press
`ui_accept` (pad South). No getting stuck on a slider while arrowing down a
settings list — the classic console-menu UX failure, banned by rule.

- **Engaged** is a third interaction state after hover/focus (`:engaged`
  pseudo-class — stylesheet-visible, so an engaged widget looks unmistakably
  different from a merely focused one).
- While engaged, the widget owns the nav actions per its semantics: text =
  type/caret (WASD types letters, arrows move the caret), slider/number =
  `ui_move` adjusts the value, dropdown = `ui_move` traverses options,
  list/table = row traversal (inner focus scope).
- **Exit is symmetric and universal**: `ui_accept`/Enter/click-away **commits**;
  `ui_back`/Esc **cancels** and reverts to the pre-engagement value (widgets
  snapshot on engage — part of the widget contract). Either way, focus returns
  to normal navigation on the same element.
- At most one engaged widget per context; engaging another commits the first.
  Buttons/checkboxes are instant-activation widgets — `ui_accept` fires them
  directly, no engaged state (nothing to adjust).
- Pointer feel unchanged: click = engage where it always did; dragging a slider
  = engage-adjust-commit in one gesture.
- On-screen keyboard for engaged text inputs is post-MVP (tracked with touch
  controls, topic 19).

### Spatial navigation (automatic, from layout)

- Directional moves resolve **geometrically from the laid-out rects** (the CSS
  spatial-navigation approach): candidates = focusables whose rect lies in the
  direction's half-plane from the current rect; score by along-axis distance +
  cross-axis overlap/misalignment penalty; best score wins. **No per-screen
  wiring** — a new menu is navigable the moment it lays out.
- **Explicit overrides where auto is wrong**: stylesheet/inline props
  `nav-up: "#id"`, `nav-down`, `nav-left`, `nav-right` (+ `nav-wrap` for
  grids/carousels) — the escape hatch is data, matching the CSS-subset
  philosophy. Focus-scope boundaries clamp candidates (a modal never leaks
  focus).
- Degenerate layouts (nothing in that direction): stay put, or wrap if
  `nav-wrap`; `ui_next` order = depth-first tree order as the always-works
  fallback.

### Debug + testing

- UI inspector grows: focus path display + **candidate-scoring overlay** (why
  focus went there — every candidate's score rendered on request); focus-history
  log.
- Tests (topic 12): spatial-scoring unit table on fixture layouts; headless e2e
  — scripted `ui_move`/`ui_accept` sequences traverse the hud gallery end-to-end
  and assert the focus path; golden frames with focus ring per theme. The
  settings/rebind screens (topic 14/19, P10) are the first real consumers;
  **puppet's device-swap showcase is the acceptance test** (full menu flow on
  pad alone).

## Debug tools (`crcbl_ui::debug`)

**One panel, assembled from modules, that every sample switches on.** This is a
standing requirement on samples, not a feature they opt into — see
[ROADMAP.md](ROADMAP.md)'s standing requirements and sample rule 4 in
[sample/00-samples-overview.md](sample/00-samples-overview.md). Three
consequences for how it is built:

- **The perf rows are specified in [40-profiling.md](40-profiling.md)**, not
  here. That topic owns what is measured and how — CPU frame time beside GPU
  frame time with percentiles and which of the two is the budget, the per-pass
  list sorted by cost, the CPU breakdown, counters, memory and pool occupancy,
  job-system utilisation, and a freeze toggle so a spike can be read rather than
  chased. This topic owns how they are drawn: they are ordinary
  `DebugModule`/`DebugSection` rows and get no special treatment.
- **Frame timing and FPS are unconditional.** Every sample has a frame, so the
  first module has no precondition and no configuration. This is the part
  **pulled forward out of P10** and built before S2, because breakout and flappy
  both want it now and asteroids and horde arrive before P10 does. The rest of
  the list below stays at P10.
- **Every other module is contributed by the system it reports on**, and appears
  because that system is present rather than because the sample asked. The
  netgraph (topic 23) is the first: a sample with a connection gets it, and
  breakout and flappy — both `InMemoryTransport` — get the panel without it.
  Those two are therefore the check that the composition is real, because a
  panel that cannot render without a network module is broken and only a
  connectionless sample proves it.
- **Switching it on is one thing.** If a sample needs more than that, the
  finding is about the panel. The failure to avoid is the one `web.rs` already
  demonstrated: a per-sample surface written out once per game until nobody
  notices there are four copies.

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
6. **UI inspector** (the web-dev devtools payoff): hover any element → its box
   outlines (content/padding/margin), matched style rules, computed values,
   id/class path. Nearly free because the tree + resolved styles are real data
   structures.

## Tasks

1. Element tree + builder API, id/class/pseudo-state, hover/active/focus
   tracking, hit-testing.
2. CSS-subset parser (+ tests against a fixture corpus), cascade/specificity,
   resolution cache, `default.css`.
3. Flex layout engine + property-test suite (layout invariants: children fit
   parent under constraints, gap/grow math vs hand-computed fixtures).
4. Draw-list emit + tessellation; glyph atlas + text; graph pass + input routing
   (UI consumes first; unconsumed falls through — capture rules explicit).
5. Widget set, driven by building the profiler HUD first as proving ground.
6. Stylesheet hot reload; UI inspector.
7. Inspector + console + stats panels.
8. Sandbox: full debug overlay over the Sponza scene from stage 6.

## Exit criteria

- Debug overlay (profiler, inspector, console, stats) runs in the sandbox over a
  live scene; interaction solid (drag sliders, select entities, run console
  commands against the server).
- A non-trivial HUD (health bar + minimap frame + wave banner) is built **purely
  by editing a `.css` file + ~30 lines of tree code**, restyled live without
  recompile — the web-workflow claim, demonstrated.
- Layout engine passes the fixture corpus (side-by-side spot-checks against
  browser flexbox for the supported subset).
- UI pass cost visible in its own profiler row and within budget (<0.5 ms GPU
  for the debug overlay at 1080p on target hardware).
- Draw-list snapshot tests + layout property tests green (topic 12).
- No renderer specifics leak into `crcbl-ui` (it produces draw lists;
  `crcbl-render` owns the pass) — checked by dependency direction in CI.

## Risks

- **CSS scope creep** — the subset above is the contract; a property gets added
  only when the editor or a sample needs it, and "browser does it" is not a
  requirement. Simplified specificity is a feature, not a gap.
- **Layout engine correctness rabbit hole.** Flexbox subset only, fixture-
  driven; when a case is ambiguous, match what the browser does for the subset,
  document divergence otherwise.
- **Style resolution perf.** Cache by (id, classes, pseudo-state) from day one;
  the UI inspector shows resolve counts so thrash is visible early.
- **Text rendering rabbit hole.** Bitmap atlas, Latin-1 + basic UTF-8, two font
  sizes. Shaping/RTL/emoji post-MVP; atlas design mustn't preclude them.
- **Docking complexity.** Split panes via flex + dividers only; full docking is
  the classic time sink. The editor layout (stage 8) is designed around
  splitters.

## Corrections (design review, 2026-07-27)

- **Font policy decided**: TTF/OTF _parsing_ is a sanctioned exception
  (`swash`/`ttf-parser` class), same policy as cpal/Opus/RustCrypto — font
  formats are a standards-compliance surface, not a learning goal, and shaping
  lives there too when it lands. Rasterization and atlas management are ours.
- **Glyph atlas lifecycle specified**: **shelf/skyline packing** into fixed
  atlas pages, new pages allocated on demand, **LRU eviction** per page with a
  per-frame re-raster budget. Stated now because the "atlas design mustn't
  preclude SDF/emoji" note implies exactly this structure.

## Corrections (2026-08-09)

- **The reserved UI action set shadows more game keys than the samples
  assumed.** `ui_move` is bound to **arrows _and_ WASD** and `ui_accept` to
  Enter and Space, which is the whole of the movement and confirm surface most
  2D samples use. `docs/backlog.md` records a per-sample analysis concluding
  "Space is never shadowed" and "asteroids' `KeyW` is not shadowed" — true of
  the ad-hoc menu handling the samples have today, false under this document's
  context stack. The mechanism that resolves it is already specified here (the
  `ui` context is active while a menu has input; `gameplay` sits underneath), so
  what changes at P10 is that the samples stop handling menu keys directly.
  Recorded because the backlog's conclusion reads as settled and is not.
- **The netgraph's crate dependency is decided here, not open.** "Every other
  module is contributed by the system it reports on" means `crcbl-client` gains
  a `DebugModule` impl and therefore a dependency on `crcbl-ui`. The backlog
  treats that as an open call ("the first time a simulation crate would depend
  on the UI"); this document already made it, and there is no cycle — `crcbl-ui`
  depends only on `glam`, `bytemuck` and `crcbl-core`, which is the bottom of
  the graph. The dependency-direction check in the exit criteria is about
  `crcbl-ui` not naming the renderer, which is unaffected.

  **The precedent is already set, by a different crate.** There is still no
  netgraph and `crcbl-client` still does not depend on `crcbl-ui`, but
  `crcbl-render` does: `FrameTimings` and `FrameCounters` both implement
  `DebugModule` and contribute their own sections. So "the system contributes
  its own module" is a shape the tree holds to, and the client's turn is the
  next instance of it rather than the first.

- **`crcbl_ui::hud`'s `Hud` and `HudPanel` have no consumer and should be
  deleted rather than extended.** Both are used by nothing in the workspace —
  `lib.rs` re-exports them and no other file names them; every sample hand-rolls
  its own HUD instead, because `Label` has no per-label colour and `HudPanel`
  auto-sizes where a measured constant is wanted. **The module file is not the
  unit to delete**, though: `Anchor` lives in it too and
  `crates/crcbl-ui/src/debug.rs` imports it, so a deletion takes the two types
  and leaves `Anchor` a home. Adding a `color` field would build on the pre-CSS
  model this document replaces — colour is a style property here, and panel
  sizing is flex layout. **Delete them when the widget set lands**, and let the
  samples adopt the styled widgets instead.
