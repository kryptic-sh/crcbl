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
  metrics and a simple layout. Not the `skrifa`-parsed rasteriser and the
  shelf/skyline atlas with LRU eviction the rendering section specifies: those
  arrive with real fonts, at rung 5.
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
  scene graph for a caller to sync.
- **The implementation is a cache keyed by identity** (decided 2026-09-15, from
  the research below). Each build pushes node records into a frame arena; a
  persistent node store keyed by
  `hash(parent key, #id or call site + sibling index)` holds what must survive a
  rebuild — hover/active/focus/engaged state, scroll offset, the engaged
  widget's snapshot, the resolved-style handle, the layout cache and last
  frame's rect — and prunes nodes a frame did not touch. Ryan Fleury's "build it
  every frame" series, Dear ImGui's ID stack and React's position-keyed state
  all arrive at this. **A loop's children need an explicit key** (`#id` or a
  keyed builder), because a sibling index moves focus, scroll and engaged state
  onto the wrong row when a list reorders — Clay's docs and React's both warn of
  exactly that — and a duplicate key is a debug warning, as ImGui's ID-conflict
  tooling reports it.
- **Rebuilding is not relaying out.** The build is diffed against the store by
  hashes of style inputs, child keys and content: only a changed node
  re-resolves its style, only a changed node or its ancestors clear their layout
  caches, and a paint-only change (colour, opacity, transform) touches neither.

### 2. Layout engine (CSS-subset flexbox, on Taffy)

**Taffy is adopted for layout** — the user's decision of 2026-09-15, reversing
this section's original "from scratch". The reason is the correctness long tail
rather than the size of the code: Yoga had to ship `YGErrata` flags because
React Native apps came to depend on its non-spec behaviour, and every one of
`min-width: auto`, percentages against indefinite sizes, stretch re-layout,
absolute containing blocks and pixel rounding is a documented pitfall in Taffy's
changelog, Yoga's errata or Gameface's divergence list. Taffy is MIT, used by
Bevy, Zed's GPUI, Servo and Slint, and ships 1,544 Chrome-generated fixtures
(677 of them flex).

- **Through its low-level traits, not `TaffyTree`.** crcbl's node store
  implements `TraversePartialTree`, `LayoutPartialTree` and `CacheTree`, and its
  resolved style type implements Taffy's style traits directly, so there is no
  conversion into `taffy::Style` and no second arena. Features: `flexbox` now,
  `block_layout` if a consumer needs it; not `taffy_tree` and not `grid` until
  something does. That keeps the dependency to `arrayvec`.
- **Per-node caches survive the rebuild**: the store keeps each node's Taffy
  cache and clears it on that node and its ancestors only when its style hash,
  child keys or measured content changed — Yoga's dirty-flag model driven by
  diffing. GPUI instead clears the whole tree every frame ("we always re-layout
  the whole app on each frame") and still ships; that is the fallback if the
  diff proves not worth its complexity.
- **Text leaves measure through a callback** cached by (string hash, font, size,
  width bucket), on Clay's measure-cache pattern, because repeated measuring of
  wrapped text under min- and max-content is where layout time goes.
- **Pinned, and upgrades gated on the fixture corpus.** Taffy is 0.x and makes
  breaking releases; an upgrade lands with its fixtures green. Its layout code
  uses `floor` and `ceil` and basic arithmetic and no `sqrt`, `powf` or
  `mul_add`, so layout output should not vary with a platform's libm — which is
  what keeps a UI golden portable.
- **The supported subset is unchanged**: `display: flex | none`,
  `flex-direction`, `flex-wrap`, `justify-content`, `align-items`, `align-self`,
  `flex-grow/shrink/basis`, `gap`; `width/height/min/max` in px / % / `auto`,
  `padding`, `margin`, `border` widths, `box-sizing: border-box`;
  `position: relative | absolute` with offsets; `overflow: hidden | scroll`;
  `z-index` within a stacking context. No floats, no tables, no animations or
  transitions in MVP. **Every divergence from the browser is written into the
  fixture corpus the day it is made** — Yoga's lesson is that a divergence you
  ship becomes a contract.

### 3. Style system (CSS-compatible-ish)

- **Stylesheets in actual `.css`-syntax files.** Tokenizing and rule-block
  parsing use **`cssparser`** (Servo's CSS Syntax Level 3 tokenizer and parser;
  MPL-2.0, which `deny.toml` allows; three small dependencies) — the user's
  decision of 2026-09-15. It parses syntax only, so **selectors, typed property
  values, the cascade and matching are this crate's own**: type (`block`,
  `span`, widget names like `button`), `#id`, `.class`, descendant and child
  combinators, and pseudo-classes `:hover`, `:active`, `:focus`, `:disabled`,
  `:engaged`. Specificity is simplified (inline > id > class > type; last wins
  within a tier) — predictable over spec-faithful. A parse error reports its
  file and line to the console and **keeps the last good sheet**.
- Properties: colors (`background`, `color`, `border-color`), `border-radius`,
  `opacity`, `font-size`, `font` (family id), text align, plus every layout
  property above. Custom properties (`--vars`) + `var()` for theming.
- Cascade sources: engine `default.css` → game/app stylesheet(s) → inline
  overrides on the node. **Hot reload** via the asset watcher — editing a `.css`
  restyles the running app; the editor's own look is a stylesheet, and game HUD
  theming = shipping a different stylesheet. A reload bumps a stylesheet
  generation, which invalidates every cached definition for one full re-resolve.
- **Matching follows RmlUi's index and cache**: rules are bucketed by the id,
  class or type of their rightmost compound selector; a node gathers candidates
  from its own buckets, matches right to left, and the merged definition is
  cached keyed by the matched-rule set and a pseudo-state bitmask. **Each rule
  records which pseudo-classes it depends on**, so a node none of whose
  candidate rules mention `:hover` never re-resolves when the pointer moves —
  Unity's documentation names `:hover` restyling whole subtrees as "the main
  culprit", and RmlUi's issue tracker has a `:hover` border shorthand forcing
  relayout. A changed property is diffed, so a paint-only change dirties no
  layout. The UI inspector shows resolve counts so thrash is visible early.

### Pipeline per frame

`build tree (immediate) → resolve styles (cached) → flex layout → emit draw list → graph pass`.
Hit-testing against the _previous_ frame's layout (one-frame interaction latency
— same simplicity win as classic imgui, now with real layout).

## Rendering

Revised 2026-09-15 from the research below; the draw list stays the one
interface between `crcbl-ui` and the renderer.

- **One UI uber shader with an analytic rounded rectangle**: per-corner radii,
  border and optional shadow evaluated as a signed distance in the fragment
  stage, as GPUI and Bevy do. Unity tessellates rounded corners (a resize
  rebuilds geometry) and RmlUi assumes MSAA for smooth corners; this engine has
  MSAA off by default ([49-antialiasing.md](49-antialiasing.md)), so the
  distance field is the answer that looks right on the default view.
- **Textured quads and a clip rectangle per command.** `DrawCommand` gains a
  texture id with UVs and a clip rect. Rectangular clips are applied in the
  shader or on the CPU (Bevy clips polygons on the CPU) so a batch survives a
  scroll view; GPU scissor only at window or scroll-container boundaries, and no
  stencil masks in MVP — Unity's stencil masks break batches and nest at most
  seven deep.
- **Two atlases**: the existing single-channel glyph coverage atlas and an RGBA
  image atlas. That moves nine-sliced frames out of `crcbl_render::MenuArt` and
  into styled widgets, which is what `menu`'s note above says is blocked today.
  Pages are 2048² or smaller: WebGPU's compatibility mode caps a 2D texture
  at 4096.
- **Batching by stacking context, then texture**, with CSS paint order kept —
  RmlUi's lack of batching is its top performance issue (thousands of draw
  calls).
- Uploads into the per-frame vertex buffer, dedicated graph pass, orthographic
  projection. World-space UI = the same tree with a world transform.
- **Text** (the user's decision of 2026-09-15): **`skrifa`/`read-fonts`** parse
  fonts — `ttf-parser`, which the 2026-07-27 correction named, is now in
  maintenance mode and its README recommends the fontations crates; `skrifa`
  includes hinting and is tested on wasm32. **Rasterisation and the atlas stay
  this engine's**: a coverage rasteriser over hinted outlines, the shelf/skyline
  atlas with LRU eviction below, subpixel horizontal positioning in a few bins
  (GPUI uses four), and **grayscale antialiasing only** — LCD subpixel rendering
  needs dual-source blending, which WebGPU offers only as an optional feature.
  Latin-1 with pair kerning first; `harfrust` shaping and UAX #9 bidi only when
  non-Latin text is needed (`rustybuzz` is archived and HarfRust replaces it).
  Greedy line breaking; SDF text post-MVP, for world-space text, since SDF and
  MSDF look worse than hinted bitmaps at small UI sizes (Godot's and Unreal's
  documentation both say so).

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

- Directional moves resolve **geometrically from the laid-out rects**, and the
  scoring is **beam first, distance second** (revised 2026-09-15): candidates
  whose rect overlaps the band the current rect projects in that direction win
  over any outside it, and only then does distance decide. Godot's 4.4
  regression — a plain gap-plus-misalignment score let a taller neighbour steal
  `ui_down` — is why; Android's `FocusFinder`, Gameface's 50% cross-axis
  overlap, RmlUi's ×10,000 cross-axis weight and Godot 4.8's new default all
  converge on the beam. **No per-screen wiring** — a new menu is navigable the
  moment it lays out.
- **Explicit overrides where auto is wrong**: stylesheet/inline props
  `nav-up: "#id"`, `nav-down`, `nav-left`, `nav-right` (+ `nav-wrap` for
  grids/carousels) — the escape hatch is data, matching the CSS-subset
  philosophy. Unity UI Toolkit's lack of explicit neighbours is its most
  reported navigation complaint. Focus-scope boundaries clamp candidates (a
  modal never leaks focus), and so do scroll containers.
- **Each focus scope remembers its last focused element** (added 2026-09-15), so
  returning to a pane or reopening a menu resumes where the player left it.
  Gameface does this per area; RmlUi, UI Toolkit and Godot do not, and it is a
  gap in each.
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
   network connection (server-authoritative debugging, free). **Planned in full
   and pulled forward in [52-debug-console.md](52-debug-console.md)**
   (2026-08-30); the transport half is that plan's reserved `SIM` flag.
5. **Debug draw controls** — toggle the stage 3 debug-draw categories (AABBs,
   system overlays) per system. **The geometry half of the layer those controls
   toggle is built (2026-08-31)**: `crcbl_render::debug_draw` is the
   immediate-mode buffer any system appends to, `DebugDraw::line`, `aabb`,
   `box_edges`, `sphere` and `frustum` are its primitives,
   `ForwardRenderer::debug_draw` hands it out, and the pass is recorded
   immediately before the tonemap and writes the HDR scene target as
   `18-render-features.md`'s interaction rule already fixed. It is off by
   default behind the console variable `r_debug_draw`, and a frame that appends
   nothing records no pass at all.

   **The control is one switch, not a set of categories, and that is
   deliberate**: no system appends yet, so a per-category filter would be a
   parameter with one value. The first two systems that need separating are what
   splits it, and `docs/backlog.md` carries the question.

   **World-anchored text is not built.** It needs a glyph atlas and a rasteriser
   seam — `crcbl_ui::text::FontAtlas` and `crcbl_render::ui_pass` own one
   between them — which is a second consumer of that atlas and a screen-space
   pass beside a world-space one; it is its own slice and `docs/backlog.md` says
   what it needs.

   The four owed views still wait on the _callers_, not on the layer:
   `45-shadows.md`'s cascade overlay and atlas view, `25-lod.md`'s cluster
   bounds and `44-lighting.md`'s light reach each now need only the system that
   appends its own geometry.

## Tasks

The rung ladder, revised 2026-09-15. Each rung unblocks the next and is useful
on its own; the first two need no new dependency.

1. **Draw-list primitives.** `DrawCommand` gains textured quads, a clip rect and
   the analytic rounded-rectangle primitive; the UI pass binds an RGBA image
   atlas beside the glyph atlas. Unblocks nine-slice frames in `crcbl-ui`,
   images in spans, `border-radius`, and a viewport pane that samples a rendered
   image.
2. **Node tree and identity.** Block/span builder, key hashing, the persistent
   node store with pruning, hover/active from last frame's rects, the
   duplicate-key warning. Unblocks stateful widgets that survive rebuilds.
3. **Layout on Taffy.** The store implements Taffy's low-level traits; flex
   subset, absolute, overflow hidden/scroll with scroll state in the store,
   measure callbacks; Taffy's flex fixtures plus this engine's own divergences
   as the corpus. Unblocks deleting `Hud`/`HudPanel`, auto-sized debug panels,
   scroll views.
4. **Styles.** `cssparser` tokenizing; selectors, typed values, cascade
   (`default.css` → app → inline), pseudo-classes including `:engaged`, `var()`,
   the rule index and definition cache with per-rule pseudo dependencies,
   resolve counters, hot reload keeping the last good sheet. Unblocks the "HUD
   by editing CSS plus ~30 lines" exit criterion, and themes.
5. **Text.** `skrifa` parsing, own coverage rasteriser, shelf/skyline + LRU
   atlas, greedy wrap, pair kerning, the measure cache. Unblocks real fonts,
   wrapped labels, a proportional console, and world-anchored text.
6. **Focus.** Scopes (modal trap, scroll into view), beam-first spatial scoring,
   `nav-*` and `nav-wrap`, per-scope memory, engaged state with snapshot, commit
   and cancel, the candidate-score overlay. Unblocks pad-only menus and the
   settings and rebind screens.
7. **Widgets.** Button, checkbox, slider, drag-value, single-line text input
   with selection and clipboard, collapsing header, tree node, split pane, and a
   fixed-row-height virtualized list/table — Unity's `ListView` virtualizes only
   at a fixed height and Godot's `Tree` stalls past ten thousand rows, which is
   what an outliner reaches. The existing `Menu`, `MenuSet`, `DebugPanel`,
   `ConsolePanel` and `ReadoutPanel` are re-implemented on the tree **behind
   their current APIs**, so the samples that call them do not change and only
   their goldens move.
8. **Editor-grade surfaces.** A reflection-driven property inspector with
   per-type overrides (Unreal's Details panel and Fyrox's `Reflect` inspector
   are the shape), a virtualized outliner, splitter layouts, then tabs. Unblocks
   stage 8.

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
- **Layout engine correctness rabbit hole.** Adopting Taffy moves most of it
  upstream; what stays is the subset's divergences and the measure callbacks.
  Fixture-driven; when a case is ambiguous, match what the browser does for the
  subset, document divergence otherwise.
- **Dependency churn.** Taffy is 0.x and the fontations crates bump minor
  versions roughly monthly (egui pins them). Pin each; an upgrade is its own
  change with the fixture corpus and the UI goldens green.
- **Style resolution perf.** Cache by (id, classes, pseudo-state) from day one;
  the UI inspector shows resolve counts so thrash is visible early.
- **Text rendering rabbit hole.** Bitmap atlas, Latin-1 + basic UTF-8, two font
  sizes. Shaping/RTL/emoji post-MVP; atlas design mustn't preclude them.
- **Docking complexity.** Split panes via flex + dividers only; full docking is
  the classic time sink. The editor layout (stage 8) is designed around
  splitters.

## Decisions (2026-09-15)

Taken by the user after a survey of the tree and a research brief on shipped
CSS-style game UI (Unity UI Toolkit, Coherent Gameface, RmlUi, NoesisGUI, Bevy
UI, Godot, Dear ImGui, egui, Clay, Morphorm, Zed's GPUI, Unreal's Slate, Fyrox):

- **Layout: adopt Taffy** — section 2.
- **CSS: adopt `cssparser`** for tokenizing and rule blocks; selectors, values
  and the cascade are built — section 3.
- **Fonts: `skrifa` for parsing, own rasteriser and atlas** — the rendering
  section. This supersedes the 2026-07-27 correction's naming of `ttf-parser`.

Adding each crate is done with `cargo add` when its rung starts, not before, so
no dependency arrives ahead of the code that reads it.

**Two findings that did not need a decision but shape the work.** Bevy chose no
CSS at all — its scene proposal says "a BSN style system _should look like BSN_"
— and archived its editor prototypes citing "ballooning scope"; this document
keeps CSS, and keeps docking to splitters for the same scope reason. And O3DE
runs its editor on Qt and its game UI on a separate system, which is exactly the
two-UI cost this document's one-GUI rule exists to avoid.

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
