# Crucible

A cross-platform GPU engine written in Rust, targeting **Vulkan**, **Metal**,
**D3D12** and **WebGPU** through a single unified API — and its own windowing on
every one of those platforms, written against the protocols rather than on top
of `winit`.

> Crate: `crcbl` · Repo:
> [`kryptic-sh/crcbl`](https://github.com/kryptic-sh/crcbl) · Demos:
> [crcbl.kryptic.sh](https://crcbl.kryptic.sh)

## Why "Crucible"

A crucible is the vessel where raw metals are melted and fused. Crucible fuses
four graphics backends into one mold — write once, forge to Vulkan, Metal, D3D12
or the browser.

## Status

> **Breaking changes, every release, until 1.0.** Crucible is early in its
> development and nothing is locked: the public API, the settings keys, the
> scene and mesh formats, the shader interfaces and the save layout all change
> whenever the engine is better for it, with no migration path. Every versioned
> format is **v0** — a number that says "may change without notice", not a
> promise. Pin a commit if you build on it, and read `CHANGELOG.md`'s `Breaking`
> sections before moving.

**Pre-1.0 and moving.** Frames draw on every backend, several samples are
playable, and fifteen of them ship as browser demos that double as the engine's
continuous cross-backend regression test. The API breaks when a caller needs it
to.

What is real today:

- **Four backends behind one seam.** `crcbl-hal` is the seam; `crcbl-vk`,
  `crcbl-mtl`, `crcbl-dx12` and `crcbl-webgpu` implement it. Parity is enforced
  structurally rather than by hand: an exhaustive `Capability` enum every
  backend answers for through a `match`, agnostic end-to-end suites driven from
  that enum in **both** directions — a declared capability must work, a declared
  refusal must refuse with the documented error — and a reviewed exception list
  that a snapshot test refuses to let drift. What is left on that list is in
  `docs/backlog.md`, per row, with what each is waiting on.
- **Windowing from scratch.** Wayland (with its own protocol scanner), X11,
  AppKit, Win32, the browser canvas, and a headless backend that CI runs
  everything through.
- **A GPU-driven forward renderer.** Culling and draw generation on the GPU, a
  shadow pass, depth prepass, ground-truth ambient occlusion with bent normals,
  SSR, clustered lights — point, spot and rectangular area lights shaded by
  linearly transformed cosines — an irradiance probe clipmap whose probes each
  carry a visibility map, captured on the GPU, so a probe a fragment cannot see
  past a wall lends it no light, and a reflective shadow map that refills those
  probes from the sun every frame for a scene that asks for it; volumetric fog,
  bloom, auto-exposure, SMAA and FXAA, render-scale upscaling, GPU skinning,
  tonemapping and a screen-space grid, with mesh shaders and bindless where the
  device has them.
- **glTF import** with meshlet building, a cluster DAG and QEM simplification.
- **A server-authoritative game stack** — fixed-tick simulation, snapshots,
  interpolation, an ECS, physics, input mapping, audio, persistence and a job
  system.

What is not:

- **No editor.** `apps/editor` is deliberately absent until there is something
  to put in it.
- The viewer opens a file from the command line, from a drop on its window
  (Wayland, X11, Win32 and AppKit), from a drop on the canvas in the browser,
  and from the shelf of Khronos CC0 models on its `ESC` panel.

`docs/plan/ROADMAP.md` is the canonical build order and carries a fuller status;
`docs/backlog.md` is what was raised and not finished, and why.

## Try it

The toolchain is pinned in `rust-toolchain.toml`, so `cargo` picks the right one
on its own. On Linux, `libasound2-dev` at build time and a Vulkan loader plus a
driver at run time (`libvulkan1`, `mesa-vulkan-drivers`) are the only system
packages.

```sh
# A window, an event loop, a frame.
cargo run -p sandbox                      # needs Wayland or X11
cargo run -p sandbox -- --headless        # needs neither; what CI runs

# A game.
cargo run -p breakout
cargo run -p lantern                        # one room, every lighting effect

# The glTF viewer: Suzanne, or bring your own model.
cargo run -p viewer
cargo run -p viewer -- your.glb
./tools/fetch-shelf.sh                      # the rest of the Khronos CC0 shelf
```

Every sample takes the same flags: `--backend vk|mtl|dx12|null`, `--headless`,
`--frames N`, `--fullscreen`, `--pacing adaptive`, `--debug-overlay`. `F3` opens
the debug panel, `F11` toggles fullscreen, `ESC` opens the menu, and `` ` ``
opens the debug console — `help` lists every command and every setting the
engine reads, `antialiasing smaa` sets one for the running frame,
`debug_view ambient occlusion` draws a renderer's debug channel instead of the
shaded picture (`shaded`, `heatmap`, `lod tint`, `normals`, `ambient occlusion`,
`motion`, `bent normal`), `toggle` and `reset` flip and restore one, `bind`
lists what drives every action in a sample that has them and `bind fire KeyJ`
moves one, and `log` prints the log filter and `log warn,crcbl_vk=trace`
installs one for the running process — in a browser as well as a terminal —
`master_volume 0.5` moves a bus on the mix that is already playing, `save`
writes it to the settings file, and `config video` runs `video.cfg` out of that
same settings directory, one console line per line. An **`autoexec.cfg`** in
that directory needs no asking: the engine runs it once at start-up, before the
first frame, which is the only way to set a variable in a run that is over
before anybody could type — a `--frames N` capture, say. A machine without one
boots in silence. `Ctrl`+`V` pastes into the input line wherever the platform
has a clipboard to read. The same keys work in the browser demos: the shim
leaves `` ` `` to the engine and passes every character typed at the console
through. On a device with no keys there is a **CONSOLE** button beside the pause
button once the canvas has been touched, and the panel it opens draws its own
keyboard: three layers, every printable character the engine's font has, and a
return key that sends the line.

`CRCBL_SHELL=x11` forces a windowing backend and `CRCBL_LOG=debug` prints every
shell event.

### The viewer

`docs/plan/sample/05-viewer.md`'s sample, and the asset pipeline's acceptance
test. Open a model from the command line, drop a `.glb`/`.gltf` on the window
(the browser demo takes a drop on the canvas), or pick one off the shelf. Orbit
with the mouse, and:

| Key     | What it does                                                      |
| ------- | ----------------------------------------------------------------- |
| `F`     | frame the model again                                             |
| `I`     | the listing panel — what the document holds, and what was skipped |
| `W`     | wireframe                                                         |
| `N`     | world-space normals, as `n * 0.5 + 0.5`                           |
| `-` `=` | exposure, a third of a stop a press                               |
| `ESC`   | the menu: an exposure slider and the `SHELF` row                  |

**Re-export the file while it is open and the frame becomes the new document.**
The debug panel's `reloads` row is how you tell a reload that ran from one that
was never offered.

### The CLI

Everything the engine can do has to be reachable without a window.

```sh
cargo build -p crcbl-cli                  # builds target/debug/crcbl
export PATH="$PWD/target/debug:$PATH"     # fish: set -x PATH $PWD/target/debug $PATH

crcbl new mygame --path /tmp
cd /tmp/mygame && crcbl run --headless
```

`crcbl new` scaffolds a standalone project — its own workspace, depending on the
engine by path — that builds and runs immediately. **It has to find a Crucible
checkout**, either by being run inside one or through `--engine <DIR>`: the
engine is not on crates.io yet, so a generated project has to point at one.
`crcbl run` works on the project in the current directory, which is why the
binary wants to be on `PATH`.

The other commands: `build`, `screenshot` (offscreen render to a PNG), `replay`
(dump a `.crpl` replay's metadata), `crpix` (PNG frames into one sprite sheet),
`lod` (report or generate a glTF mesh's LOD chain), `import` (run the glTF
importer over one document and report what came out of it), `bench` (one fixed
workload, timed and reported as a distribution), `sim` (the determinism harness:
N ticks of a seed-generated world, and its state hash) and `settings` (`get`,
`set`, `list` and `preset` over a game's `settings.toml`, so a player setting —
or a whole quality tier — is scriptable and not only reachable from a settings
screen). Every one of them takes `--json`.

### The demo site

```sh
./web/build.sh --serve                    # http://localhost:8000
```

Builds the browser demos into `target/site` and serves them with the COOP/COEP
pair a threaded wasm build needs. `cargo` and `node` are the whole tool list —
no npm, no bundler, no `node_modules`, no `wasm-bindgen`. It is the same script
GitHub Pages runs, so "works in CI" and "works on my machine" are one claim. See
`web/README.md`.

## The samples

Each one exists to prove something, and is the exit criterion for the slice that
built it.

| Sample           | What it is for                                                         | In the browser |
| ---------------- | ---------------------------------------------------------------------- | -------------- |
| `sandbox`        | the dev playground — the first window lived here                       |                |
| `breakout`       | the first playable                                                     | ✓              |
| `flappy`         | the second game, and the test that nothing is breakout-shaped          | ✓              |
| `asteroids`      | churn: entities spawn and die every tick                               | ✓              |
| `horde`          | scale: thousands of agents, one broadphase, a job pool                 | ✓              |
| `hud`            | the UI system's living fixture — draw-list primitives and nothing else | ✓              |
| `orbit`          | the physics pillar's acceptance test, wearing a rocket costume         | ✓              |
| `bracket`        | matchmaking, rating and ranked flow, with no game attached             | ✓              |
| `options`        | the settings acceptance test: move a fader, and it is still there      | ✓              |
| `puppet`         | a character, a controller and a camera on a small shadowed map         | ✓              |
| `breach`         | a first-person firing range, on the controller `puppet` walks          | ✓              |
| `shard`          | a torch-lit interior zone, walked in an isometric-ish third person     | ✓              |
| `sparks`         | the VFX fixture: stock effects, a hostile one, and the budget for them | ✓              |
| `lantern`        | the lighting acceptance fixture: one room, every effect                | ✓              |
| `quarry`         | the geometry acceptance fixture: one dense scene on every path         | ✓              |
| `viewer`         | a glTF model viewer, and the asset pipeline's acceptance test          | ✓              |
| `bare`           | the engine as a plain library, with a hand-written loop                |                |
| `render-harness` | drives the golden scenes through a browser GPU for the parity gate     |                |

## Testing

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
```

All three, every time — a green clippy with a skipped `fmt` is a red CI run over
whitespace. As of 2026-09-03 `cargo test --workspace` reports **6,006 passing
tests** across 178 test binaries and doctest runs, with 21 ignored: six that
need a real device pinned with `CRCBL_GPU`, or a generator rerun, and are driven
by their own scripts, and fifteen doctests marked `ignore` because they are
illustrative fragments rather than runnable programs.

That command is the floor rather than the suite. The device-bound work lives in
the nine `crates/crcbl/tests/run-*-e2e.sh` harnesses, each of which pins a
backend and turns its own feature on — a bare `cargo test` compiles several of
them out entirely — alongside each sample's own golden script and the browser
gates under `web/`.

CI runs those on Linux, macOS and Windows and then goes further: a nested-sway
Wayland session, Xvfb with and without a window manager, a real Windows desktop,
lavapipe for Vulkan, WARP for D3D12, a Metal runner, headless Chromium for
WebGPU, golden-image comparison, a decoder fuzz job and coverage.

## Layout

```
crates/crcbl            the umbrella crate — what a game depends on
crates/crcbl-hal        the backend seam: traits and POD descriptors
crates/crcbl-vk         Vulkan            crates/crcbl-mtl     Metal
crates/crcbl-dx12       D3D12             crates/crcbl-webgpu  WebGPU
crates/crcbl-shell      Wayland, X11, AppKit, Win32, canvas, headless
crates/crcbl-render     render graph, frame loop, meshes, materials
crates/crcbl-scene      glTF import, meshlets, the cluster DAG
crates/crcbl-anim       skeletons, clips, pose sampling, joint palettes
crates/crcbl-ecs        crates/crcbl-phys     crates/crcbl-input
crates/crcbl-server     crates/crcbl-client   crates/crcbl-net
crates/crcbl-ui         crates/crcbl-sprite   crates/crcbl-audio
crates/crcbl-assets     crates/crcbl-store    crates/crcbl-jobs
crates/crcbl-console    the debug console's registry: variables, commands, the line
crates/crcbl-core       ids, handles, arenas, time, logging
crates/crcbl-shaders    Slang sources, and the SPIR-V, WGSL, MSL and DXIL built from them
crates/crcbl-wl-scanner the Wayland protocol code generator, run at build time
crates/crcbl-vfx        particle simulation: pooled effects, a fixed modifier menu
crates/crcbl-greybox    greybox prototyping primitives, sized in real-world metres
crates/crcbl-golden     golden-image comparison for the render tests
crates/crcbl-rand       the one randomness seam
crates/crcbl-cli        the `crcbl` binary
apps/                   the samples
web/                    the demo site and its hand-written ES modules
docs/plan/              the design docs and the canonical roadmap
```

## License

MIT
