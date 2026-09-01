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

This document is the plan and the record of the decisions taken to write it. The
delivery list at the end says which slices have landed and is the thing to read
for what exists.

It opened with a survey of the tree as it stood before any of this was built —
no text field, no command registry, no autocomplete, nothing engine-level
setting a debug view. **That survey has been deleted**, because every claim in
it is now false and a "today" section that describes a tree from before the work
is worse than none: `git log` is the record of what changed, and the decisions
below are what still binds.

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

- **The domain becomes a type.** `CatalogueKey::domain` was a prose string
  (`"1 to 16; 1 is off, and the device's own ceiling clamps it"`). It is now
  `Kind` — `Bool`, `Enum(&["none", "fxaa", "smaa"])`, `Float { min, max }`,
  `Int { min, max }` — and the prose is `help`. `help` prints both;
  `apps/options` keeps its own labels and reads the same `Kind` for its rung
  count, so the two cannot disagree about a domain. The eight `KeyStatus::Named`
  rows carry kinds of their own, because there is no reader to derive one from;
  each is `READ_ONLY`, so no range of theirs decides anything, and a row that
  grows a reader takes the reader's range with it.
- **Apply lives in one place.** `apps/options` used to fan out per key to the
  setter and to the app's `Gpu` bundle. That fan-out is now
  `crcbl::settings::apply` — one function that writes the stack and applies
  through a `settings::Stage`: the mixer, the loop's frame ceiling, and a **new
  `GameGpu` pair**, `apply_video(&VideoSettings)` and
  `set_debug_view(DebugView)`, each with a default that returns `Unsupported` so
  a host that has no renderer says so rather than passing. `Stage` is a trait of
  its own rather than `GameGpu` because `GameGpu` is `Sized` (it takes `self` in
  `destroy`) and so has no `dyn`, and because a settings key reaches more than a
  renderer. `GpuContext::set_pacing` is **not** reached: the only key that would
  want it is `present_mode`, which nothing reads. `apps/options` calls the same
  `apply`, which is a refactor with no visible change and the second caller that
  earns the helper.

A `KeyStatus::Named` key (declared, unread — `brightness`, `fov`, `hdr_output`)
is a `READ_ONLY` variable whose help says "nothing reads this yet", so the
console is honest about the catalogue instead of hiding half of it.

Setting an `ARCHIVE` variable applies live and marks the stack unsaved; `save`
writes the file. **Not saved on exit**, deliberately: Source writes `config.cfg`
on quit, but this engine has no exit hook that writes settings and
`apps/options` made the same explicit-save call for the same reason — a debug
session that flips twenty variables must not silently become the player's file.

### 4. The log the console shows is the log, from a ring the funnel feeds

One bounded ring in `crcbl_core::log::console` —
`Record { sequence, level, target, message, elapsed }`, `CONSOLE_RING_LINES`
deep, behind a `Mutex<VecDeque>` — pushed from the one funnel
`StderrLogger::emit` already is, **before** the level filter so the ring can
show what the terminal filtered if asked, and from `WebLogger::log` for the
browser (the page's `log_take` queue is destructive and stays the page's). The
console reads `snapshot()` when it opens and `snapshot_since(sequence)` on every
later frame, which is what the sequence is for: a reader copies the lines that
arrived since it last looked rather than the whole ring. Everything the console
itself prints — the echoed command, a variable's value, `help` — goes through
`console::print`, at `Level::Info` under the target `console`, so it is on
stderr and in the browser console too: the terminal and the panel show the same
lines, which is what the user asked for, and a test can assert console output
through the existing `log::capture()`. (The level macros take their target from
`module_path!` and cannot set one, which is why this is a function and not an
`info!` call site.)

`log <filter>` sets the live filter, through `Filter::try_parse` — which refuses
a directive `Filter::parse` skips, because a person typing at a console can be
told — and `log::set_filter`, which swaps the filter the installed logger holds
and moves the facade's global maximum with it. The console panel's own filter is
a separate dropdown-less toggle per level, cycled with a key, so "show me debug
lines" does not mean "print debug lines to the CI log".

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

**What a line says**, settled while the widgets were built: the record's
message, with its target in front of it unless the console printed the line
itself (`CONSOLE_TARGET`), and the level carried by colour rather than by a
name. The elapsed seconds stay the terminal's — the panel is a hundred columns
wide at scale 1, and the ring's order already says what a timestamp would — so
"the same lines as stderr" means the same records in the same order, not the
same glyphs. The panel's own per-level view is a `LevelFilter` threshold, `Off`
through `Trace`, rather than five independent toggles: a key that cycles it
wants an order to cycle through, and the levels have one.

**Touch — built 2026-08-31, and the gap was wider than this paragraph said.** It
claimed the console opens on a device with no keyboard; it does not.
`CONSOLE_KEY` is the backtick and nothing else, so a finger had no route to the
panel at all — which `web/templates/demo-loop-keys.html` had already written
down while this decision recorded only the missing keys. So both halves land
together. `crcbl::engine::ConsoleButton` is the way in and out: a **CONSOLE**
button in the same top-right strip as `PauseControl`, hit-tested against
contacts for that control's reason, and drawn by the **loop** rather than by
each sample — which is what keeps the exit criterion's "no per-app code".
`crcbl_ui::console::TouchKeyboard` is what the open panel then offers: three
layers reaching every printable character the atlas has, laid out from the
frame's bottom edge so the control row a thumb rests on does not move when the
layer does. Both are on screen only once a contact has arrived, `PauseControl`'s
rule and its argument.

**A drawn keyboard rather than a focused DOM element**, which is the platform's
own answer on the one tier that has one. Three reasons, each in
`crates/crcbl-ui/src/console/keyboard.rs`'s module docs: no native backend here
reports a contact at all (`crates/crcbl-shell/src/caps.rs` asserts the Wayland
backend does not set `ShellCaps::TOUCH`), so a DOM input would leave every other
backend with the gap; `web/engine/shell.js` listens for `keydown` on the
**canvas** and focuses it on every `pointerdown`, so an editable element focused
while the console is open holds the focus the panel's own `Tab`, arrows,
`PageUp`, `Escape` and `Ctrl`+`V` are read from; and the built-in atlas covers
printable ASCII only, so a system keyboard's accented or CJK output would reach
the field and draw as the not-def glyph.

### 7. The commands that ship

| Command                 | Does                                                                                                                                       |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `help [name or prefix]` | every variable and command with its help, value, default and flags; with an argument, the matches only. The user's ask, verbatim           |
| `find <substring>`      | names and help lines containing the text — Source's `find`, and the way a variable is discovered without knowing its prefix                |
| `<var>`                 | prints `name = value (default: d) — help`                                                                                                  |
| `<var> <value>`         | sets, coerced through `Kind`; `<var> = <value>` is accepted because the user wrote it that way; a value outside the domain is refused      |
| `toggle <bool var>`     | flips it                                                                                                                                   |
| `reset <var>` / `reset` | back to the default; bare, every non-`ARCHIVE` variable                                                                                    |
| `save`                  | writes the settings file; `dump` prints `SettingsStack::dump()`                                                                            |
| `log <filter>`          | the live `CRCBL_LOG` filter                                                                                                                |
| `echo`, `clear`         | Source's                                                                                                                                   |
| `debug_view <name>`     | `shaded`, `normals`, `ambient occlusion`, `motion`, `heatmap`, `lod tint` — an `Enum` variable `r_debug_view` under the hood, everywhere   |
| `pause`, `quit`         | the pause toggle the loop already has; a clean `ExitReason`                                                                                |
| `fps`                   | the frame-timing row's numbers as a line                                                                                                   |
| `config <name>`         | runs `<name>.cfg` from the settings directory — Source's `exec`. A bare name, never a path; a failed line is reported and the file runs on |

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

`r_debug_view` is one enum variable holding the view every host draws, and
applying it needs the renderer: that is `GameGpu::set_debug_view` from decision
3, defaulted to `Unsupported` so a host without a `ForwardRenderer` (the options
screen, hud) reports rather than pretends. `apps/lantern`'s `AO VIEW` row stays
— it is the touch path — and reads the same variable, so the two cannot
disagree. Wireframe is not a `DebugView` and stays the viewer's key; making it
one is `47-reflections.md`'s style of question for the renderer, not this
plan's.

**Two things this sketch had wrong, corrected when it was built (2026-08-31).**
The variable is declared in **`crcbl`**, not in `crcbl-render`: nothing in that
crate can apply it — a static has no renderer to reach — and the seam that can,
`GameGpu::set_debug_view`, is the engine's, so the declaration and the code that
acts on it are in one crate, which is what decision 2 asks for. And a sample's
own view has to _be_ the variable rather than sit beside it: `apps/lantern`,
`apps/quarry` and `apps/viewer` each wrote their view into their renderer on
**every frame**, so a console line was undone by the next one. All three read
and write `crcbl::debug_view` now, and the loop is the only writer of the
renderer's switches.

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
- **Selection and IME on the web in v0** — the field is insert-and-caret first,
  and IME needs the web backend's `TEXT_IME` work, which is `15-windowing.md`'s.
  Clipboard paste was on this list as the first follow-up and landed with slice
  8, on every backend that implements `Shell::clipboard_request`; the web is not
  one of them, so a browser still cannot paste.

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
2. **Settings typed — landed 2026-08-30.** `CatalogueKey` carries `kind`
   (`crcbl_console::Kind`), `help` and `name` in place of the prose `domain`,
   with the antialiasing set derived from `Antialiasing::ALL` and every numeric
   range asserted equal to the setter's own clamp; `settings::apply` writes one
   key and applies it through a `settings::Stage`, refusing an unknown key, a
   `KeyStatus::Named` key, a value outside the `Kind` and a storage error, and
   answering `Applied::Live` or `Applied::NextStart`;
   `settings::console_bindings()` is one `ARCHIVE` `Binding` per catalogue key
   over `settings::ConsoleHost`, `READ_ONLY` where nothing reads the key;
   `GameGpu::apply_video`/`set_debug_view` default to `Unsupported` and are
   forwarded by the eight bundles with a `ForwardRenderer` through
   `settings::apply_video_to`/`set_debug_view_on`; `apps/options` writes every
   row through `apply` and applies its bus gains through `impl Stage for Audio`.

   **The console's writes are deferred, not live, and that is slice 5's to
   finish.** `Binding` reaches its host as `&mut dyn Any` and `Any` is
   implemented only for `'static` types, so the host cannot hold a borrow of the
   renderer or the mixer; `ConsoleHost` records into a `settings::Deferred` and
   the loop drains it where the bundle is in hand, which is
   `HostedGame::take_pending_frame_limit`'s arrangement. The scaffold template
   holds no `ForwardRenderer` and so keeps the defaults.

3. **The ring — landed 2026-08-30.** `crcbl_core::log::console` is the ring:
   `Record { sequence, level, target, message, elapsed }`, `CONSOLE_RING_LINES`
   deep behind a `Mutex` whose poison is stepped over, pushed by `console::push`
   from `StderrLogger::emit` and from `crcbl::web`'s `WebLogger` before either
   sink's own filter, read with `snapshot()` or `snapshot_since(sequence)`, and
   with no `clear_ring`. `console::print` puts the console's own output on
   `CONSOLE_TARGET`. `Filter` gained a `Display` that round-trips through
   `parse`, a `try_parse` that refuses what `parse` skips, and a live
   `set_filter`/`filter` pair over an `RwLock` on the installed logger; `log` is
   a `concommand!` in `crcbl_core::log` listed by `crcbl_core::console_table()`
   and held there by `crates/crcbl-core/tests/console_table.rs`.
4. **`crcbl_ui::console` — landed 2026-08-30.** `TextField` is the crate's first
   editable widget: a line, a caret counted in characters rather than bytes,
   `insert` (which drops control characters), `backspace`/`delete`, the four
   cursor motions, and a `window` that scrolls a long line under a caret held in
   the last column it can occupy — the draw list has no clip, so a field that
   drew its whole line would draw it over the button beside it. `LogView` takes
   `crcbl_core::log::console::Record`s through `push_records`, keeps the
   `cursor` the next `snapshot_since` needs, is bounded at `CONSOLE_RING_LINES`,
   wraps at the panel's column count, culls whole rows, colours by level,
   scrolls in lines — holding still while the log fills up behind it — and
   carries a `LevelFilter` of its own that hides lines rather than dropping
   them. `ConsolePanel` lays the two out at `ConsoleStyle::pixel_art`'s scale
   over the top `CONSOLE_HEIGHT_FRACTION` of the frame: the log, then the `]`
   `PROMPT`, the field and the **Send** button on one row, with up to
   `COMPLETION_ROWS` candidates hanging below the panel with their matched head
   highlighted. `ConsolePanel::point` submits through the same
   `ConsolePanel::submit` that `Enter` will call, and the scale chosen is the
   largest whose panel still shows `MINIMUM_LOG_ROWS` rows and
   `MINIMUM_FIELD_COLUMNS` columns. Nothing here reads the ring, the registry, a
   clock or a keycode — the records, the candidates and the caret's blink
   (`caret_shown`) all arrive as values, which is what slice 5 wires up.
5. **The engine — landed 2026-08-31.** `crcbl::engine::CONSOLE_KEY` is the
   fourth reserved key, folded by `Pending::observe` into `toggle_console` for
   the bare key only; the open console claims every key, every `TextCommit`, the
   wheel and the pointer over the panel, releases whatever the game was holding
   when it opened, and swallows the character the toggling press commits — which
   matters on the _closing_ press, since the panel is not yet up on the opening
   one. `Escape` closes it before it pauses. `crcbl::debug_console::Console` is
   the loop's state: the gathered `Registry`, the `ConsolePanel`, a `History`,
   and the `settings::ConsoleHost` every command and binding runs over.
   `Loop::new` gathers `debug_console::engine_tables()` — `crcbl-core`'s table
   and `crcbl::console_table()` — plus the new defaulted
   `HostedGame::console_table()`, and both guards are red tests:
   `crates/crcbl/tests/console_table.rs` over this crate's source and
   `crates/crcbl/tests/console_gather.rs` over the workspace manifests. The
   engine's own commands are `pause`, `quit` (a new `ExitReason::Quit`), `fps`,
   `save` and `dump`, each recording on `debug_console::EngineLink` where it
   needs the loop. `Loop::drain_console` is slice 2's owed drain: the
   `[engine.video]` section to `GameGpu::apply_video` and the ceiling to the
   loop's clock, on the frame the line was typed. The panel draws last, after
   the debug overlay. `Tab` completes then cycles, the arrows walk the history,
   `PageUp`/`PageDown` and the wheel scroll, and `CONSOLE_LEVEL_KEY` (`F2`)
   cycles the panel's level threshold. Every check is headless, through
   `HeadlessShell`, and every one was shown red by breaking the mechanism it
   guards. The CLI template needed no change — `console_table` is defaulted and
   it declares nothing.

   **What it did not do**, each in `docs/backlog.md`: `toggle` and `reset` (they
   belong in `crcbl-console`'s `builtin.rs`, which this slice did not own), the
   web half of `log` (slice 7's), and a console audio-gain write reaching the
   running mixer, which needs a `HostedGame` seam the loop does not have.

6. **Every debug view everywhere — landed 2026-08-31.** `crcbl::debug_view` is
   the module: the `r_debug_view` enum variable over `DebugView::label`'s six
   names, the `debug_view` command decision 7's table spells (`debug_view` alone
   prints, `debug_view ambient occlusion` sets — the value keeps its space),
   `current`/`set`/`toggle` for a sample's own row, and `for_test`, a guard that
   serialises the checks which move a process-global and hands it back `Shaded`.
   `Loop::apply_debug_view` runs beside `drain_console`, before the tick and the
   draw, and hands a **change** to `GameGpu::set_debug_view` — an edge, so a
   renderer is left exactly as its sample set it up until something moves the
   variable, and a bundle with no renderer prints that the view drew no frame
   rather than passing (a `shaded` ask is silent, or the eight samples that hold
   no renderer would open with the line). The forwarders were slice 2's: the
   eight bundles with a `ForwardRenderer` already had them and the other eight
   keep the default, so this slice added none. What it did add is the other half
   of "so the two cannot disagree": `apps/lantern`'s `AO VIEW` row,
   `apps/quarry`'s `LOD VIEW`/`HEATMAP` rows and `--lod-tint`/`--heatmap`, and
   `apps/viewer`'s `N` all write the variable now, and
   `Lantern::occlusion_view`, `Quarry::view`, `Viewer::normals`,
   `lantern::Gpu::set_occlusion_view`, `viewer::Gpu::set_normals_view` and
   `quarry::menu::toggled_to` are gone with the per-frame writes that used them.

   **The exit criterion's demo is `apps/quarry`, not breakout.** Breakout has
   had no forward pass since 2026-08-28 — its paddle is a sprite and its bundle
   holds no `ForwardRenderer` at all — so no debug view can ever draw there, and
   this plan named it before that was checked. quarry has the renderer, runs the
   occlusion pass (`RenderEffects::DEFAULT_STACK` carries it), had no
   ambient-occlusion control anywhere, and has a device suite of its own:
   `apps/quarry/tests/device/console.rs` opens the console with `` ` `` on a
   `HeadlessShell`, types the line, and reads the view back off the renderer,
   then measures the picture — 47 104 of 49 152 pixels grey against none in the
   shaded frame, 559 of them darker than the placeholder on radv and 557 on
   lavapipe.

7. **The web — landed 2026-08-31.** `__crcbl_web_key` queues a
   `ShellEvent::TextCommit` after the `Key` when the composed
   `KeyboardEvent.key` is a single non-control character, the press is an edge
   and no `Ctrl` or `Meta` is held — the shape decision 5 asked for, with `Alt`
   alone left alone so a European layout's third level still types, and a repeat
   committing because a held key types over and over everywhere else. `text_of`
   is the filter, and it is the rule every other backend already applies.
   `ShellCaps::TEXT_IME` stays clear: there is no input method behind the
   commit, so a dead key and a candidate window still compose nothing. The shim
   gained `SWALLOWED_BARE`, a second set checked only when neither `Ctrl` nor
   `Meta` is held, so a bare `` ` `` is `preventDefault`ed and the devtools
   shortcut is not — the same condition `Pending::observe` applies, spelled on
   the page's side. Every demo page's controls list gains the row. `apps/quarry`
   carries the browser gate, for the reason slice 6 gives:
   `EXPECTATIONS.quarry.console` opens the panel, types
   `debug_view ambient occlusion` one `keydown` at a time, reads the console's
   own echo back out of the page log — the whole line, character for character —
   and then reads `view: ambient occlusion` off quarry's heartbeat, which is the
   variable's effect on the frame rather than the command's echo. It toggles out
   and back in and types `debug_view shaded`, which is what proves the panel
   closes as well as opens, and leaves the demo as group D found it.
   `web/run-browser-e2e.sh` holds the claim and its control by name.

   **Each of the six new checks was shown red first**, on the SwiftShader
   adapter the gate picks: emptying `SWALLOWED_BARE` reds the two swallow
   checks, dropping its bare condition reds the devtools-modifier control, and
   severing the text path — both by making the backend commit nothing and by
   having the shim send an empty `KeyboardEvent.key` — reds the echo, the
   applied view and the toggle round trip.

   **The restore check was rewritten during that sabotage.** As first written it
   asked only "does a later heartbeat say `view: shaded`", which is true of
   every heartbeat on a page where nothing was ever typed — it passed with the
   whole feature removed. It reads the state _going in_ now and fails unless the
   view was `ambient occlusion` when the line was typed.

   **What it did not do:** the web half of `log` (decision 4's live filter still
   faults in a browser — the fix is `crcbl-core`'s, and this slice owned the web
   backend, the shim and the gate), and `AltGr`, which reports as `Ctrl`+`Alt`
   and so commits nothing. Both in `docs/backlog.md`.

8. **The paste key and the rebinding commands — landed 2026-08-31.**
   `CONSOLE_PASTE_KEY` is `V` under `Ctrl` or `Meta`: the open console records
   the press, `Loop::ask_for_paste` issues `Shell::clipboard_request` after the
   pump has let the shell go — a command cannot ask the shell for anything from
   inside the pump's own closure — and the `ShellEvent::ClipboardData` that
   answers it lands in the field through `TextField::insert`, matched by request
   id so a game's own read is not stolen. A backend that refuses the read says
   which half is missing; the web backend is that backend, and
   `EXPECTATIONS.quarry.console.pasteRefused` is the browser check that it says
   so. `bind`/`unbind` reach the game's `ActionMap` through a new defaulted
   `HostedGame::actions`, with the ask recorded on `EngineLink` and applied by
   `Loop::drain_binds` where the game is in hand; `debug_console::apply_bind`
   owns the reporting, so what a binding is called in a printed line is the
   console's business and not the loop's. `toggle` and `reset` — decision 7's
   table, and slice 5's leftovers — are `crcbl-console` built-ins now, the bare
   `reset` skipping every `ARCHIVE` variable so a debug session cannot empty the
   player's settings file.

   Every sample that keeps an `ActionMap` overrides `actions` — the four whose
   map lives on their `Game` through a new `Game::action_map_mut`, since a
   `HostedGame` impl is a sibling module away from a private field — and
   asteroids and breach each drive a console rebind end to end, one per shape.

   **What it did not do**, each in `docs/backlog.md`: six of the eight overrides
   are compile-checked rather than driven; a browser still cannot read a
   clipboard; a pasted newline joins two lines; and `bind` spells keys only, not
   the other `Binding` variants.

9. **`config` — landed 2026-08-31.** `crcbl::console_config` runs a file of
   console lines through the same `Registry::execute`, the same `Context` and
   the same **host** a typed line goes through, so no second execution path
   exists to disagree with the first and what a file sets actually lands. It is
   in `crcbl` rather than in `crcbl-console` because it has to reach a file and
   that crate depends on nothing: the bytes come from
   `SettingsStack::with_platform_storage`, the seam `save` already writes
   through, which makes "the settings directory" one answer on both platforms —
   the platform config directory natively, the page's OPFS store in a browser,
   so a browser runs a config file for real rather than reporting that files do
   not exist there.

   The argument is a **bare name and never a path** — ASCII letters, digits, `-`
   and `_`, with `.cfg` optional — refused by `file_named` before the storage
   layer is asked for anything, which is decision 1's "no dependencies" meeting
   the rule that a filesystem path is never built from console input. Two guards
   bound recursion because they end different things: a file already running is
   refused by name, which ends every cycle at its first repeat, and
   `CONFIG_NESTING_LIMIT` bounds a chain of _distinct_ files, which no cycle
   check can see. A failing line is reported as `file.cfg:3: …` against the
   file's own line numbers and the file runs on, with a closing count of lines
   run and lines failed — a file that half-applied says so.

   **And one file runs without being asked — landed 2026-09-02.**
   `console_config::AUTOEXEC` is Source's `autoexec.cfg`, run by
   `Console::run_autoexec` from `Loop::new` after the console is gathered and
   before the first frame, so a variable can be set ahead of everything that
   reads one — which is the only way to configure a run that is over before
   anybody could type: a frame-budget capture, a golden run, a demo started from
   a launcher. Same file, same read, same `run_text`. What differs is the
   silence: a run that reads no settings file runs no autoexec and asks no
   storage for anything — `EngineLink::app_name` is the gate and
   `with_platform_storage` is not, because natively that answers `Some` and
   would hand a headless run `~/.config/<game>/autoexec.cfg` — and a machine
   with no such file says nothing, since almost none have one and absence is not
   an event. A file that is there and will not run is printed and the boot
   carries on. The read is one read for both callers: `NotRead` splits its
   failure so the boot can tell "no such file" from "could not read it", and
   `NotRead::into_fault` collapses those back into the line `config` has always
   printed.

10. **The console a finger reaches — landed 2026-08-31.**
    `crcbl::engine::ConsoleButton` is the route in, beside `PauseControl` in the
    corner that control's docs vetted, drawn by `Loop::frame` after the panel so
    it is the way out as well; it reads contacts and answers `takes_pointer`, so
    the finger that presses it does not also flap or serve.
    `crcbl_ui::console::TouchKeyboard` is the panel's own keyboard:
    `Layer::Lower`/`Upper`/`Symbols`, whose rows a test holds to the atlas's
    whole printable range; `KeyCap::Shift` and `KeyCap::Symbols` are swallowed
    by the keyboard and change its layer, `KeyCap::Enter` comes back as
    `ConsoleInput::Submitted` through the same `ConsolePanel::submit` **Send**
    calls, and everything else reaches the field through `Console::tapped`,
    which makes the same two edits `Console::key` does so a tapped `q` and a
    typed `q` leave the cycle in one state. Both controls are gated on
    `Console::note_contact` and `ConsoleButton::touched`, so a run nobody has
    touched draws and claims nothing — the guard is
    `an_untouched_run_keeps_every_click_the_console_would_have_taken` in
    `crates/crcbl/src/engine.rs`, and it was shown red by showing the keyboard
    from the first frame and again by laying it out while it reports itself
    hidden. `web/tools/browser-e2e.mjs`'s group F opens breakout's console with
    a tap, types `echo it works` key by key and taps the return key, and reads
    the console's own answer — not its echo, which is printed before anything
    runs.

11. **`Flags::SIM` is deferred, not outstanding.** Decision 9 reserves it and
    names its trigger — the first `SIM` variable anyone wants — and nothing in
    the workspace declares one. Building the transport half now (a `Command`
    message applied on a tick boundary, recorded by the replay stream, refused
    off the host) would be machinery with no caller, so this list is complete
    until that trigger fires.

## Exit criteria

- `` ` `` opens the console in every `apps/*` demo, native and browser, with no
  per-app code beyond the one `GameGpu` forwarder.
- …and a **tap on the CONSOLE button** opens it in every demo on a device that
  has been touched, with no per-app code at all: the button and the keyboard are
  both the loop's.
- The panel shows the same lines as stderr, in order, coloured by level.
- `help` prints every variable in `crcbl::settings::catalogue()` and every
  `convar!` in the workspace; a `convar!` missing from its crate's table is a
  red test in that crate.
- `antialiasing` prints the value; `antialiasing smaa` and `antialiasing = smaa`
  set it, the frame changes, and `save` writes it.
- `debug_view ambient occlusion` shows the AO channel in a demo that never
  exposed it — `apps/quarry`, for the reason slice 6 gives, and proven on radv
  and lavapipe, **and proven in a browser**: `web/tools/browser-e2e.mjs` types
  the line at quarry's console on SwiftShader and reads
  `view: ambient occlusion` back off the demo's own heartbeat.
- No golden moves; the closed console adds no measurable frame cost on the
  browser tier.
