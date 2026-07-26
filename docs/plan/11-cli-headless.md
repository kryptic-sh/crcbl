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

Project lifecycle:

- `crcbl new <name>` — scaffold a game project (workspace member or standalone)
  with sandbox-style main, scene dir, CI template.
- `crcbl run [--headless] [--server-only] [--connect <addr>]` — run the project;
  headless = server + no client; server-only = dedicated server.
- `crcbl build [--target wasm]` — build incl. the wasm/Pages bundle locally.

Content pipeline (agent/CI workhorses):

- `crcbl scene <file> <subcommand>` — batch scene operations against a headless
  editor server: `spawn`, `set`, `delete`, `move`, `list`, `query`. Reads
  commands from args or stdin (newline-delimited, scriptable); every operation
  is the same `Command` type the GUI emits, so undo/redo and validation apply
  identically.
- `crcbl import <gltf> [--out <dir>]` — run the asset import pipeline
  standalone; report what was imported/skipped.
- `crcbl screenshot <scene> [--camera <name>] [--size WxH] -o out.png` —
  offscreen render (no window/swapchain: render graph → readback → PNG). This is
  the visual regression primitive for CI and the "did my scene edit work"
  primitive for agents.

Simulation & verification:

- `crcbl sim <scene> --ticks N [--input script.ron] [--hash]` — run the headless
  server N ticks, optionally replaying an input script, print the state hash
  (the stage 4 determinism harness as a CLI command).
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
- **Everything scriptable is testable**: sample CI uses `crcbl sim --hash` for
  determinism runs and `crcbl screenshot` for golden-image smoke — the samples'
  own test suites are built from the CLI.
- **Agent-friendly**: stdin batch mode, stable JSON schemas, no interactive
  prompts unless a TTY is detected (and never required).
- Offscreen rendering rides the normal HAL (surface-less device + readback), so
  it works on vk, wgpu (incl. lavapipe/software in CI), and later mtl/dx12.

## Delivery (interleaved — see ROADMAP)

| Slice                                                        | Roadmap phase |
| ------------------------------------------------------------ | ------------- |
| `crcbl-cli` scaffold: `new`, `run`, `build`                  | P0            |
| Offscreen render + `screenshot`                              | P1            |
| `sim`/`--hash` (headless server, input scripts)              | P2            |
| `import`                                                     | P9            |
| `scene` batch ops + `edit --serve` (editor command protocol) | P12           |
| `phys --check`                                               | P3–P11 grow   |

## Exit criteria (MVP)

- A scripted session — `crcbl new` → `crcbl import` → `crcbl scene spawn …` →
  `crcbl screenshot` → `crcbl sim --hash` — builds and verifies a small scene
  with **zero GUI launches**.
- The towers map is _modifiable_ from the CLI (spawn a tower plot, move a
  spawner) and the result opens correctly in the GUI editor with intact undo
  history.
- All sample CI determinism + golden-image checks run through the CLI.
