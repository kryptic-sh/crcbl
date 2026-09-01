# Topic 11 — CLI + Headless Interface

First-class pillar: everything the engine and editor can do is reachable without
a window — from a terminal, a script, a CI job, or a coding agent. Not a
bolt-on: the architecture already makes this cheap, and the plan exploits it.

## Why this is nearly free (and must stay that way)

- The server is headless by construction (stage 4 exit criterion: no render
  deps). Running a game/sim without graphics is the _default_, not a mode.
- Editor edits are `Command` messages over the transport (stage 8). A CLI client
  sends the same commands the GUI sends — same validation, same undo log, same
  result. The GUI is one client; `crcbl-cli` is another.
- The console command registry (stage 7) routes over the transport too — any
  debug command is automatically a CLI command.

The invariant to protect: **no editor or engine capability is implemented
GUI-side.** If a feature only works through the GUI, that's an architecture
regression (same severity as a sample linking `crcbl-vk` directly).

## The `crcbl` CLI (`crates/crcbl-cli`, installed binary)

**What the binary parses today** — `crates/crcbl-cli/src/args.rs`'s `Command` is
the list, and it is not the one this section describes: `new`, `run`, `build`,
`screenshot`, `replay`, `crpix`, `lod`, `import`, `bench`, `sim` and `settings`.
Several of those are not sketched anywhere below and are worth naming here:

- `crcbl replay <FILE> [--json]` — read a `.crpl` recording and report its
  metadata. Topic 22 owns it, and owns the subverbs it does not yet have.
- `crcbl crpix <PNG>... -o <FILE>` — pack PNG frames into one `.crpix` sprite
  sheet, each frame named for its file stem, with nine-slice insets, a sample
  mode and an optional clip over every frame.
- `crcbl lod stats|gen <FILE>` — report a glTF mesh's resolved LOD chain, saying
  per level whether the geometry came from the file or the cluster DAG, or
  generate one.
- `crcbl bench --scenario <NAME>` — run one fixed, named workload headless and
  report its distribution beside the machine it ran on.
  `docs/plan/40-profiling.md` owns it.

Everything below that is not in that list — `scene`, `phys`, `edit` — is
unbuilt. They are kept as the specification they always were; the parser rejects
those words today.

Project lifecycle:

- `crcbl new <name>` — scaffold a game project (workspace member or standalone)
  with sandbox-style main, scene dir, CI template.
- `crcbl run [--headless] [--server-only] [--connect <addr>]` — run the project;
  headless = server + no client; server-only = dedicated server.
- `crcbl build [--target wasm]` — build the project. **`--target wasm` is
  recognized and refused**, pointing at `web/build.sh` instead: a browser bundle
  is a Cargo build plus the loader shim, the shader artifacts and the site
  layout, and a `crcbl build` that shelled out to Cargo alone would exit 0
  having produced something no page can load. The reason used to name a
  version-matched `wasm-bindgen`; that left with `crcbl-wgpu` on 2026-08-21 and
  `web/build.sh` now says the tool cannot even run, its one product having been
  replaced by `web/tools/wasm-loader.js`.

Content pipeline (agent/CI workhorses):

- `crcbl scene <file> <subcommand>` — batch scene operations against a headless
  editor server: `spawn`, `set`, `delete`, `move`, `list`, `query`. Reads
  commands from args or stdin (newline-delimited, scriptable); every operation
  is the same `Command` type the GUI emits, so undo/redo and validation apply
  identically.
- `crcbl import <gltf> [--json]` — **built.** Runs the asset import pipeline
  over one document and reports what came out of it: meshes, the primitives
  across them, materials, images, every entry of the `nodes` array, and the
  instances — one per node that draws a mesh. What was _skipped_ is not a second
  list: the importer already warns for every unsupported extension, unresolvable
  image and non-triangle primitive through `crcbl_core::log`, and the verb
  installs the stderr logger so those lines land beside the counts. A skip is
  not a failure — the run exits 0 and the skipped item is still counted.

  **`--out <dir>` is not built and is refused by name.** There is nothing for it
  to write: the importer produces an in-memory `GltfScene`, this tree has no
  on-disk scene format — the RON scene directory is still an open decision in
  `docs/backlog.md` — and no binary scene container. An `--out` that wrote
  nothing, or that invented a format, would be worse than the refusal.

- `crcbl screenshot <scene> [--camera <name>] [--size WxH] -o out.png` —
  offscreen render (no window/swapchain: render graph → readback → PNG). This is
  the visual regression primitive for CI and the "did my scene edit work"
  primitive for agents.

Simulation & verification:

- `crcbl sim [--ticks N] [--tick-rate HZ] [--seed S]` — **built.** Runs the
  headless server tick loop over a seed-generated world and prints
  `hash:<hex> ticks:<n> final_tick:<n>`; same input, same hash. This is the
  stage 4 determinism harness, and it is a CLI verb rather than a binary of its
  own — the `crcbl-sim` binary was deleted on 2026-08-23 and its tests moved to
  `crates/crcbl-cli/tests/cli.rs`.

  **It takes no scene and no input script.** The sketch above was
  `crcbl sim <scene> --ticks N [--input script.ron] [--hash]`, and neither the
  scene argument nor `--input` is built: this tree has no scene file format and
  no RON reader, so there is nothing for a scene to name and no script to
  replay. Both are refused by name rather than ignored. `--hash` is not a flag
  either — the hash is the output. The world comes from `--seed` alone.

- `crcbl phys <scene> --check` — physics sanity suite against a scene (overlaps
  at rest, NaN scan, island stats).

Editor session control:

- `crcbl edit <scene> --serve [--listen <addr>]` — headless editor server; GUI
  editor, CLI, or scripts connect as clients (including concurrently — the
  command log is the sync point).
- `crcbl edit <scene> -e '<cmd>' ...` — one-shot edits without a session.

## Design rules

- **Output is machine-readable by default flag**: `--json` on every subcommand;
  human tables otherwise. Exit codes are meaningful (0 ok, 1 command failed, 2
  bad invocation).
- **Everything scriptable is testable**: `crcbl screenshot` is the golden-image
  smoke primitive and CI uses it, including in the cross-backend compare, and
  `crcbl sim` is the determinism primitive beside it. Both are verbs of the one
  binary: a capability that needed a second binary built to reach it would be
  the same architecture regression as one that needed the GUI.
- **Agent-friendly**: stdin batch mode, stable JSON schemas, no interactive
  prompts unless a TTY is detected (and never required).
- Offscreen rendering rides the normal HAL (surface-less device + readback), so
  it works on vk, wgpu (incl. lavapipe/software in CI), and later mtl/dx12.

## Delivery (interleaved — see ROADMAP)

What the verbs that exist still owe:

- **`sim`'s scene argument and `--input script.ron` are not built** and are
  refused by name; there is no scene format and no RON reader in this tree.
- **`import <gltf>`'s `--out <dir>` is not built** and is refused by name, for
  the same reason: there is no on-disk scene format in this tree to write.
- **`crcbl save` is topic 14's verb and is still unbuilt**;
  `settings get\|set\|list\|preset` over `crcbl-store`'s stack is the whole of
  the store's CLI surface. `preset` is the one that is not a catalogue key: it
  writes a whole quality tier through `crcbl::settings::presets::select`, which
  `set` could never reach because a preset is a writer rather than a key.
- **`bench`'s device scenarios are not written** — `jobs` and `phys` are the
  headless pair, and [40-profiling.md](40-profiling.md) owns the rest.

| Slice                                                        | Roadmap phase |
| ------------------------------------------------------------ | ------------- |
| `scene` batch ops + `edit --serve` (editor command protocol) | P12           |
| `phys --check`                                               | P3–P11 grow   |

## Exit criteria (MVP)

- A scripted session — `crcbl new` → `crcbl import` → `crcbl scene spawn …` →
  `crcbl screenshot` → `crcbl sim` — builds and verifies a small scene with
  **zero GUI launches**.
- The towers map is _modifiable_ from the CLI (spawn a tower plot, move a
  spawner) and the result opens correctly in the GUI editor with intact undo
  history.
- All sample CI determinism + golden-image checks run through the CLI. Both
  halves do: `crcbl screenshot` for the golden images, `crcbl sim` for the
  determinism hash.
