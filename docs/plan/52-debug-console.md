# 52 — The debug console

**Asked for by the user on 2026-08-30, ranked high, and planned before the next
slice of work.** A Source-engine-style console in every demo and every build:
opened with the `` ` ``/`~` key, drawn over the frame at the top of the screen,
showing exactly the lines the engine logs to the terminal, with an input box and
a **Send** button, `Enter` sending; every setting the engine reads is a variable
the console prints (`antialiasing`) and sets (`antialiasing smaa`, or
`antialiasing = smaa`), with autocomplete, and `help` lists every command and
variable. The user's standard for the design: robust and low-maintenance — the
exposed variables and commands **come from the code that owns them**, declared
where they live, so the console updates itself as the engine grows, and there is
an annotation-style way to expose a thing. Valve's Source engine (`ConVar`,
`ConCommand`, `FCVAR_*` flags, `help`, `find`, tab completion) is the model, on
the user's instruction.

This document is the plan and the record of the decisions taken to write it.
Nothing in it is built. The tree was read on 2026-08-30 for every "today" claim
below; each names the file it came from.

## Where the tree stands today

- **Logging** — `crcbl_core::log` funnels every record through one path:
  `StderrLogger::emit` in `crates/crcbl-core/src/log.rs`, which writes to
  **stderr**, and on wasm `crcbl::web::WebLogger`, which queues lines a page
  drains through `log_take`/`log_ptr` (`web/engine/log.js` routes them to the
  browser console). The only readback is the thread-local test capture
  (`log::capture()`); there is no process-wide ring a UI could read. `Level` and
  `LevelFilter` are the `log` crate's; `CRCBL_LOG` is the filter.
- **Text input** — `ShellEvent::TextCommit { text }` exists
  (`crates/crcbl-shell/src/event.rs`) and every native backend emits it with the
  layout applied: Wayland and X11 from `xkb_state_key_get_utf8`, Win32 from
  `WM_CHAR`, AppKit through `NSTextInputClient`. The web backend emits **none**
  (`crates/crcbl-shell/src/web/mod.rs` says so) and carries the typed character
  only as `ShellEvent::Key`'s `keysym`, from `KeyboardEvent.key`. **The engine
  loop drops `TextCommit` on the floor**: `Pending::observe`'s catch-all and
  `MenuPump::observe` both pass it nowhere, and `HostedGame` has no text hook.
- **The key** — `KeyCode::Backquote` (`crates/crcbl-core/src/input.rs`, the W3C
  `code` name for the `` ` ``/`~` key) is mapped on all five backends and is
  claimed by nothing. The engine's reserved keys are `F3` (debug overlay),
  `Escape` (pause) and `F11` (fullscreen), in `crates/crcbl/src/engine.rs`.
- **UI** — `crcbl_ui` has `DrawList` (`rect`, `rect_outline`, `line`,
  `polyline`, `text`; no clip, no scissor), the 8×13 monospace `FontAtlas`
  covering printable ASCII 32–126 (backtick and tilde included), `Menu` with
  cycler and slider rows, `DebugPanel`, `HudPanel`. **There is no text field, no
  caret and no keyboard focus anywhere** — focus is `Menu::selected`.
  `FontAtlas::layout_line` returns per-glyph rectangles, which is what a caret
  is built from.
- **Settings** — `crcbl::settings` reads sixteen keys (six `[engine.video]`
  effect booleans, `render_scale`, `frame_limit`, `antialiasing`,
  `anisotropic_filtering`, six `[engine.audio]` gains) with a getter and a
  setter each, and `settings::catalogue()` lists every engine key with a
  **prose** domain string and a `KeyStatus` (`Read` or merely `Named`). The
  store is `crcbl_store::SettingsStack` (`get<T>`, `set<T>`, `contains`, `dump`,
  `save`). **Live application is per key and per app**: only
  `GpuContext::set_pacing` applies live at the engine level; render scale,
  anisotropy and antialiasing reach `ForwardRenderer` through each app's own
  `Gpu` bundle, the frame limit through `HostedGame::take_pending_frame_limit`,
  audio gains through each app's audio module. `apps/options` is the one screen
  that reads, writes and applies them, and `apps/options/src/view.rs` already
  renders `SettingsStack::dump()` as debug rows.
- **Debug views** — `crcbl_render::DebugView` is `Shaded`, `Heatmap`, `LodTint`,
  `Normals`, `AmbientOcclusion`, `Motion`, with five independent setters on
  `ForwardRenderer` and one precedence resolver (`ForwardRenderer::debug_view`).
  Wireframe is a separate switch. **Nothing engine-level sets a view**:
  `GameGpu` exposes none, lantern hand-wires the AO view behind its pause panel,
  the viewer binds `W` and `N`. The user wants every view in every build; today
  each is one app's.
- **Registries** — there is no command registry, no console-variable concept and
  no autocomplete anywhere in the workspace. The nearest analogue is
  `crcbl_input::ActionMap`, a string-keyed rebindable table with
  `action_names()`.
- **Hosts** — sixteen `apps/*` crates implement `HostedGame`; fifteen ship as
  browser demos; `crates/crcbl-cli/templates/main.rs.tmpl` is a seventeenth
  implementor that only the `cli-e2e` gate compiles.

## The decisions

### 1. The registry is a crate of its own, with no dependencies

`crcbl-console`, a new crate beside the others: the variable and command types,
the registry, the line parser, the value coercion, the completion and the
history. It depends on nothing but `core`/`std`, so it is testable headless,
compiles on every target and can be read by the UI, the engine and the CLI
alike. It does not draw and it does not know about settings files; those are
bindings (decision 3).

```text
ConVar      { name, help, kind: Kind, flags: Flags, default: Value, cell: Cell }
ConCommand  { name, help, run: fn(&mut Context, &[&str]) -> Result<(), Fault> }
Kind        Bool | Int { min, max } | Float { min, max } | Enum(&'static [&'static str]) | Text
Flags       ARCHIVE (persisted through the settings stack)
            READ_ONLY (prints, refuses a set — a `KeyStatus::Named` key, a device fact)
            SIM (reserved: a value the simulation reads; see decision 9)
Registry    { vars: sorted table, commands: sorted table } built once at Loop::new
```

A `ConVar` **is** the storage, as in Source: `cell` is a typed atomic
(`AtomicBool`, `AtomicI64`, an `AtomicU32` holding the float's bits, an atomic
index for an enum), so the code that owns a knob reads it with `R_AO_VIEW.get()`
and never polls the console. `Text` variables have no static cell — a `String`
in a `static` needs a lock and an allocation the engine's statics do not want —
so a `Text` var exists only as a settings binding (decision 3). Values print and
parse through one `Value` type; `Kind` is what makes `set` coerce rather than
blind-write a string, which is the trap `crates/crcbl-cli/src/settings_cmd.rs`'s
header already warns of.

### 2. Declared beside the code, listed once per crate, gathered at one seam — no linker tricks

The Source model registers a `ConVar` in its constructor, before `main`. The
Rust ecosystem's two equivalents were read on 2026-08-30 and **both declined**:

- `linkme` (distributed slices via `#[link_section]`) lists Linux, macOS,
  Windows, FreeBSD, OpenBSD and illumos and **not WebAssembly**; fifteen demos
  ship as wasm.
- `inventory` (life-before-main via `ctor`) lists WebAssembly, but this engine's
  rule is that the engine owns its state and nothing runs before `main`; our
  wasm build has no shim-side constructor call and has never exercised one, and
  a registration that silently did not run would be the "not implemented
  arriving as passed" defect the verification rules name.

So a variable is declared where it lives and listed once, by the crate:

```rust
// crates/crcbl-render/src/forward.rs — beside the thing it controls
crcbl_console::convar! {
    /// Draw the ambient-occlusion channel as grey instead of the shaded frame.
    pub static r_ao_view: bool = false;
}

// crates/crcbl-render/src/lib.rs — once per crate
pub fn console_table() -> crcbl_console::Table {
    crcbl_console::table![r_ao_view, r_normals_view, r_motion_view, /* … */]
}
```

The `convar!` macro is the annotation: it defines the static, its default, its
help (the doc comment) and its kind from the type. **The ident is the console
name** (through `stringify!`), so a static is written in Source's lower-case
spelling and the expansion allows the lint; the registry matches, sorts and
de-duplicates names without regard to ASCII case, so a `SCREAMING_CASE` static
still answers to `r_ao_view` (built 2026-08-30). A crate's `console_table()` is
the one list a new variable joins, and **a test in that crate holds the list to
the source**: it reads the crate's own `src/` for every `convar!`/ `concommand!`
name and asserts each is in the table — the same text-guard shape
`crcbl_shaders::volumetric::both_shaders_spell_the_same_atlas_walk` already
uses, so forgetting the list is a red test, not a missing command. The engine
gathers crate tables at one seam in `Loop::new`, and a second guard reads the
workspace manifests: every crate that depends on `crcbl-console` appears in the
gather. A game declares its own variables the same way and hands its table over
through one new `HostedGame` method, `console_table()`, defaulted empty.

Names follow Source's prefixes for code-declared variables — `r_` renderer,
`ui_`, `snd_`, `phys_`, `net_`, `cl_`/`sv_` — and the settings-backed ones
(decision 3) keep their key name bare, because that is what the user typed in
the example. Two tables cannot declare one name: the registry refuses a
duplicate at gather time with both crates named, and a test proves the refusal.

If `linkme` ever lists wasm, the per-crate lists collapse into one distributed
slice with no change to `Table` or to a single call site; the escape is cheap
because the list is the only thing the linker would have replaced.

### 3. Every settings key is a variable, automatically, and typed

The user's rule: the console exposes every setting the engine can set or use.
`crcbl::settings::catalogue()` already lists them all, so the registry derives
one `ARCHIVE` variable per catalogue key at gather time and nothing has to be
declared twice — a key added to the catalogue is a console variable the same
day. Two things change in `crcbl::settings` to make that mechanical:

- **The domain becomes a type.** `CatalogueKey::domain` is a prose string today
  (`"1 to 16; 1 is off, and the device's own ceiling clamps it"`). It becomes
  `Kind` — `Bool`, `Enum(&["none", "fxaa", "smaa"])`, `Float { min, max }`,
  `Int { min, max }` — and the prose moves to the help text. `help` prints both;
  `apps/options` keeps its own labels and reads the same `Kind` for its rung
  count, so the two cannot disagree about a domain.
- **Apply lives in one place.** Today `apps/options` fans out per key to the
  setter and to the app's `Gpu` bundle. That fan-out moves into the engine as
  `crcbl::settings::Apply` — one function per key that writes the stack and
  applies live through the seams that exist (`GpuContext::set_pacing`, the frame
  limit, and a **new `GameGpu` pair**, `apply_video(&VideoSettings)` and
  `set_debug_view(DebugView)`, each with a default that returns `Unsupported` so
  a host that has no renderer says so rather than passing). `apps/options` calls
  the same `Apply`, which is a refactor with no visible change and the second
  caller that earns the helper.

A `KeyStatus::Named` key (declared, unread — `brightness`, `fov`, `hdr_output`)
is a `READ_ONLY` variable whose help says "nothing reads this yet", so the
console is honest about the catalogue instead of hiding half of it.

Setting an `ARCHIVE` variable applies live and marks the stack unsaved; `save`
writes the file. **Not saved on exit**, deliberately: Source writes `config.cfg`
on quit, but this engine has no exit hook that writes settings and
`apps/options` made the same explicit-save call for the same reason — a debug
session that flips twenty variables must not silently become the player's file.

### 4. The log the console shows is the log, from a ring the funnel feeds

One bounded ring in `crcbl_core::log` —
`Record { level, target, message, elapsed }`, `CONSOLE_RING_LINES` deep, behind
a `Mutex<VecDeque>` — pushed from the one funnel `StderrLogger::emit` already
is, **before** the level filter so the ring can show what the terminal filtered
if asked, and from `WebLogger::log` on wasm (the page's `log_take` queue is
destructive and stays the page's). The console reads a snapshot once per open
frame. Everything the console itself prints — the echoed command, a variable's
value, `help` — goes through `info!` under the target `console`, so it is on
stderr and in the browser console too: the terminal and the panel show the same
lines, which is what the user asked for, and a test can assert console output
through the existing `log::capture()`.

`log <filter>` sets the live filter (`Filter::parse` exists); the console
panel's own filter is a separate dropdown-less toggle per level, cycled with a
key, so "show me debug lines" does not mean "print debug lines to the CI log".

### 5. The key, the takeover, and where the text goes

`CONSOLE_KEY = KeyCode::Backquote` joins the engine's reserved keys beside `F3`;
a press toggles, in every hosted game, with no per-app code — the console is the
engine's the way the debug overlay is. Only the bare key toggles: with `Ctrl` or
`Meta` held it is the browser's devtools shortcut and is left alone.

While the console is open the loop claims **every** key event (the game gets
releases for keys it already held, through the same repair `MenuPump` does when
the pause menu opens, so a game never sees a key stuck down), every
`TextCommit`, the wheel, and the pointer over the panel. The `TextCommit` that
follows the toggling `Backquote` in the same batch is discarded, or every
opening would type a backtick. The game's `key_event` never sees the console's
keys, and `Escape` closes the console before it pauses the game.

Editing keys: `Backspace`, `Delete`, `ArrowLeft`/`ArrowRight`, `Home`/`End`,
`ArrowUp`/`ArrowDown` for history, `Tab` for completion (fills the common
prefix, then cycles the candidates on repeat), `Enter` submits, `PageUp`/
`PageDown` and the wheel scroll the log, `Escape` closes. The web backend gains
a `TextCommit` for a printable `KeyboardEvent.key` with no `Ctrl`/`Meta`, which
is what the survey found it already has in hand and does not emit; IME
composition on the web stays out of scope and `TEXT_IME` stays clear there. The
shim's swallowed-key set gains `Backquote` so a page does not also act on it.

### 6. The panel: a text field and a log view in `crcbl_ui`, drawn topmost

Two new widgets in `crcbl_ui::console`, both drawn with `DrawList::rect` and
`DrawList::text` and laid out with `FontAtlas::layout_line`:

- **`TextField`** — the crate's first editable field: content, caret index,
  insert/delete, the cursor keys above, a blinking caret rectangle from the
  glyph rectangles. No selection in v0. It is a general widget; the console is
  its first consumer and a settings screen with a name field is its second.
- **`LogView`** — a scrollable list of lines coloured by level, newest at the
  bottom, that culls whole lines outside its rectangle because `DrawList` has no
  clip. Long lines wrap at the panel's width by glyph count, which the monospace
  atlas makes exact.

The console panel is the top `CONSOLE_HEIGHT_FRACTION` of the frame (Source's
drop-down): scrim, the log view, then one row with the `]` prompt, the field,
and the **Send** button at the right edge — `crcbl_ui::Button`, and `Enter`
fires the same submit. Below the field, when a completion has candidates, a list
of up to `COMPLETION_ROWS` names with the matched prefix highlighted. The panel
uses `MenuStyle::pixel_art`'s whole-number scale rule so it is crisp at every
size, and it draws **after** the debug overlay, last in the frame, so nothing
covers it.

Touch: the panel has no on-screen keyboard, so on a device with no keyboard the
console opens (a Send button and history are usable) but typing is not. Recorded
as a known gap, not designed around.

### 7. The commands that ship

| Command                 | Does                                                                                                                                     |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `help [name or prefix]` | every variable and command with its help, value, default and flags; with an argument, the matches only. The user's ask, verbatim         |
| `find <substring>`      | names and help lines containing the text — Source's `find`, and the way a variable is discovered without knowing its prefix              |
| `<var>`                 | prints `name = value (default: d) — help`                                                                                                |
| `<var> <value>`         | sets, coerced through `Kind`; `<var> = <value>` is accepted because the user wrote it that way; a value outside the domain is refused    |
| `toggle <bool var>`     | flips it                                                                                                                                 |
| `reset <var>` / `reset` | back to the default; bare, every non-`ARCHIVE` variable                                                                                  |
| `save`                  | writes the settings file; `dump` prints `SettingsStack::dump()`                                                                          |
| `log <filter>`          | the live `CRCBL_LOG` filter                                                                                                              |
| `echo`, `clear`         | Source's                                                                                                                                 |
| `debug_view <name>`     | `shaded`, `normals`, `ambient occlusion`, `motion`, `heatmap`, `lod tint` — an `Enum` variable `r_debug_view` under the hood, everywhere |
| `pause`, `quit`         | the pause toggle the loop already has; a clean `ExitReason`                                                                              |
| `fps`                   | the frame-timing row's numbers as a line                                                                                                 |

A command with a `Fault` prints the fault and leaves state alone. Unknown names
print "unknown command or variable, try `find`". **An enum value may hold a
space** — `debug_view ambient occlusion` is one value — so a set joins
everything after the name into the value it coerces, and completion treats
everything after the name as one token rather than completing per word; both are
built and tested, and a per-token "simplification" would break the table above.
Every command is a `concommand!` in the crate that owns the behaviour (`quit`
and `pause` in `crcbl`, `debug_view` in `crcbl-render`, `save`/`dump` in
`crcbl`'s settings module), so the built-in list is itself an instance of
decision 2.

### 8. The AO view, and every debug view, in every build — the user's specific ask

`r_debug_view` is declared in `crcbl-render` beside `DebugView`, so every host
has it the moment the console lands. Applying it needs the renderer, which the
engine cannot reach today: that is `GameGpu::set_debug_view` from decision 3,
one forwarding line in each of the sixteen `Gpu` bundles and the CLI template,
defaulted to `Unsupported` so a host without a `ForwardRenderer` (the options
screen, hud) reports rather than pretends. `apps/lantern`'s `AO VIEW` row stays
— it is the touch path — and reads the same variable, so the two cannot
disagree. Wireframe is not a `DebugView` and stays the viewer's key; making it
one is `47-reflections.md`'s style of question for the renderer, not this
plan's.

### 9. Determinism, replay and the network — reserved, not built

Console commands are host input, like a key press, and are **not** part of the
tick input stream in v0: a variable that changes what the simulation computes
would break the same-binary determinism `05-physics.md` and `22-replay.md` rest
on. The `SIM` flag is reserved for those, and the rule when it is taken: a `SIM`
variable is set through the transport as a `Command` message, applied by the
server on a tick boundary, recorded by the replay stream, and refused by a
client that is not the host — which is `07-ui-debug.md` item 4's "works
identically over a network connection", deferred with a named trigger: the first
`SIM` variable anyone wants.

### 10. Cost, on three tiers

Closed, the console costs one ring push per log record — a lock and a `VecDeque`
insert on a path that already formats a string. Open, it is a lock and a copy of
at most `CONSOLE_RING_LINES` records a frame and a draw list of at most the
visible lines plus one field, on a UI path that already draws a menu and a debug
panel a frame. No GPU work, no allocation the ring does not already own after
warm-up, identical on the browser tier. The completion is a prefix scan over a
sorted table of well under a thousand names. There is nothing here to price per
tier beyond stating that.

## Considered and declined

- **`linkme` / `inventory` registration** — decision 2, for the wasm and
  life-before-main reasons.
- **A proc-macro `#[convar]` attribute** — it cannot register anything the
  declarative `convar!` cannot, adds `syn`/`quote` to the build, and the
  workspace declined a proc-macro once already (`Format::ALL`) on the same
  ground. `convar!` is the annotation.
- **Parsing the settings TOML for the variable list** — the catalogue is the
  authority already and is derived from the readers; parsing a file would list
  keys nothing reads.
- **A console-side value cache** — Source's model, where the variable is the
  storage, is what lets the owning code read it without a lookup; a cache is a
  second copy that drifts.
- **Auto-saving `ARCHIVE` variables on exit** — decision 3.
- **Selection, clipboard paste and IME on the web in v0** — the field is
  insert-and-caret first; paste rides `Shell::clipboard_request`'s async answer
  and is the first follow-up; IME needs the web backend's `TEXT_IME` work, which
  is `15-windowing.md`'s.

## Delivery, in order

Each slice is tested headless, sabotaged red first, and lands with its CHANGELOG
entry; the browser gate runs on every slice that touches a demo.

1. **`crcbl-console` — landed 2026-08-30.** `Kind`/`Value` with coercion and
   range refusal; `ConVar` over a typed atomic cell and `Binding` for a variable
   whose storage is elsewhere; `ConCommand` over a `Context` that collects
   output and carries the `clear` request; `convar!`/`concommand!`/`table!`;
   `Registry::gather` with the duplicate refusal; the parser (`name`,
   `name value`, `name = value`, quoted arguments, `;`); `Registry::execute`
   with `help`/`find`/`echo`/`clear` built in; `Registry::complete`; `History`;
   and `guard::declared_names`, the source reader slice 5's per-crate guards
   call. Unit-tested to the value, every test shown red first.
2. **Settings typed**: `CatalogueKey::domain` becomes `Kind`; the derived
   `ARCHIVE` variables; `crcbl::settings::Apply` and `GameGpu::apply_video` /
   `set_debug_view`; `apps/options` moved onto `Apply` (no visible change; the
   options browser gate and the seam test hold it).
3. **The ring**: `crcbl_core::log`'s console ring fed from both funnels; the
   `console` target; `log <filter>`.
4. **`crcbl_ui::console`**: `TextField`, `LogView`, the panel layout, the Send
   button, the completion rows — draw-list snapshot tests like the menu's.
5. **The engine**: `CONSOLE_KEY`, the takeover and the held-key repair, the
   `TextCommit` pump, the draw after the overlay, the gather at `Loop::new` with
   both guards, `HostedGame::console_table`, the built-in commands, and the CLI
   template. Headless e2e through `HeadlessShell`: toggle, type `antialiasing`,
   `Enter`, the value line in the ring; `antialiasing smaa` applied and read
   back off the renderer; `help` lists every catalogue key (the count asserted
   against the catalogue, so the test cannot pass on an empty list); `find`;
   `Tab`; `Escape` closes before it pauses.
6. **Every debug view everywhere**: `r_debug_view`, the sixteen forwarders,
   lantern's row reading the variable; the AO view proven on a demo that never
   had it (breakout) through the headless e2e and the browser gate.
7. **The web**: `TextCommit` emission in the web backend, `Backquote` swallowed
   by the shim, the browser e2e typing a command in one demo.
8. **Follow-ups, each its own slice**: clipboard paste; `bind`/`unbind` over
   `ActionMap::action_names`; the `SIM` flag over the transport; a `config`
   command that runs a file of commands; a touch keyboard.

## Exit criteria

- `` ` `` opens the console in every `apps/*` demo, native and browser, with no
  per-app code beyond the one `GameGpu` forwarder.
- The panel shows the same lines as stderr, in order, coloured by level.
- `help` prints every variable in `crcbl::settings::catalogue()` and every
  `convar!` in the workspace; a `convar!` missing from its crate's table is a
  red test in that crate.
- `antialiasing` prints the value; `antialiasing smaa` and `antialiasing = smaa`
  set it, the frame changes, and `save` writes it.
- `debug_view ambient occlusion` shows the AO channel in a demo that never
  exposed it.
- No golden moves; the closed console adds no measurable frame cost on the
  browser tier.
