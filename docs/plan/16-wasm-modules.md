# Topic 16 — Game Modules via Wasm FFI

Game logic lives in **modules**: wasm binaries loaded by the engine host,
talking through a flat FFI over linear memory. Any language that compiles to
wasm (Rust, C/C++, Zig, TinyGo, AssemblyScript, C#/NativeAOT…) can write game
code. The engine is a host/runtime; a game is data (assets/scenes) + modules.

Prior art: Ambient engine (wasm game modules + server-authoritative multiplayer
— closest match to our architecture), Godot GDExtension (C ABI seam), Unity DOTS
(data-oriented logic separation). We combine the wasm sandbox with our
system-owned-array ECS.

## The load-bearing design decision: state lives in the engine

Modules are (approximately) **stateless logic over engine-owned SoA arrays**:

- A module _declares_ its systems + component schemas at init; the **engine
  allocates and owns the arrays** (`crcbl-ecs` storage, exactly as if the system
  were native).
- Each tick the host calls the module's `tick` export with views into those
  arrays (shared linear-memory windows, batch-oriented — one FFI crossing per
  system per tick, not per entity).
- Because state is engine-side: **hot reload** = swap the module, arrays
  survive; **saves** (topic 14 snapshots), **replication** (stage 4), and the
  **determinism hash** all work on module-defined components with zero module
  cooperation. The module is a pure-ish function; the engine is the database.
- Module-private scratch state is allowed (its linear memory persists between
  ticks) but anything that must survive reload/save/replicate belongs in
  components. `crcbl mod check` warns when a module's memory grows suspiciously
  (state-smuggling detector, best-effort).

## One API, two bindings: static and wasm

The module interface is **the** game API — defined once, consumed two ways:

| Binding    | What                                                       | Use                                                 |
| ---------- | ---------------------------------------------------------- | --------------------------------------------------- |
| **Static** | Rust trait impl compiled into the binary (no wasm runtime) | engine-internal systems, dev iteration, MVP samples |
| **Wasm**   | same interface over the FFI ABI, module loaded at runtime  | shipped game logic, mods, other languages           |

Samples are written against the module API from breakout onward (static binding
first) — **and that half is built**: `crcbl_ecs::game_module::GameModule` is the
static binding, with `name`, `register` and `tick`, and every sample implements
it. Nothing else in this document is: there is no wasm host, no `sdk/`, no ABI
and no `crcbl-abigen`. The wasm host later runs the _same_ breakout compiled to
`.wasm` and the determinism hash must match the static build — that equivalence
test is the ABI's acceptance criterion. Engine-internal systems (physics, render
feed, audio) stay native forever; the module seam is for _game_ logic.

## ABI sketch

> **Decision (2026-07-27): hand-rolled contract + own codegen; WIT/component
> model considered and rejected.** Rationale: (1) components in the browser
> require `jco` transpilation — breaks our own-JS-shim core-module symmetry; (2)
> the hot path (array views) bypasses interface types anyway, so WIT would only
> cover the cold surface while charging full toolchain tax; (3) the
> batch-oriented API is deliberately tiny (~dozens of functions, POD records) —
> per-language binding cost is small by construction; (4) foreign generated glue
> vs "we own all the bugs". The contract lives in `sdk/abi/` as data; **our own
> generator (`crcbl-abigen`) emits complete native SDKs** from it —
> programmatic, drift-proof, zero foreign toolchain. ABI stays WIT-shaped (flat
> funcs, POD records, no callbacks) so migration remains cheap if the calculus
> ever flips.

### `crcbl-abigen`: full native SDKs from the spec

Not just extern declarations — each language backend emits the **whole idiomatic
SDK**: typed wrappers over the raw ABI (safe array-view slices, builder-style
component declaration, event enums), the module entrypoint scaffold, and the
`hello-module` example. What stays hand-written per SDK is only what codegen
can't know: docs prose and language-taste review of the templates themselves.

- One backend per language inside `crcbl-abigen` (Rust, C header (+`.hpp`), Zig,
  Lua glue for the VM template, C# later). A backend is a template module over
  the spec's IR — tractable because the IR is tiny (POD records, flat funcs,
  versioned capability groups) and **only needs to cover our features**, never
  general-IDL completeness.
- Generated SDKs are committed (reviewable diffs) and **CI regenerates + diffs**
  — drift between spec and any SDK fails the build. ABI change = edit spec,
  regenerate, conformance suite validates every language in one commit.
- Adding a language = writing one backend (mechanical — LLM-amenable against the
  spec + existing backends as reference) and passing conformance. The suite
  judges, not the author.

Flat C-style, versioned, no host-side codegen required of guests (language SDKs
are sugar, Rust SDK first):

Guest exports:

```
crcbl_abi_version() -> u32
crcbl_init(ctx) -> ()            // declare systems, components, event subs
crcbl_tick(sys_id, dt_fixed) -> ()      // server tick, per declared system
crcbl_event(sys_id, ptr, len) -> ()     // replicated events, commands
crcbl_alloc(size) / crcbl_free(ptr)     // guest allocator for host writes
```

Host imports (capability-scoped — a module only links what it's granted):

- ECS: component array views (`borrow(sys, comp) -> (ptr, len, stride)`),
  spawn/despawn queues, entity queries by id.
- Physics L0: raycast/sweep/overlap batches.
- Events/net: emit server events, read input commands.
- Audio: play/emit spatial events (server-event path from topic 13).
- UI (client modules): block/span builder calls into the stage 7 tree.
- Log/diagnostics; deterministic RNG handle; fixed-point time. **No clock, no
  filesystem, no sockets** — determinism and sandbox by omission.

Schemas: components declared with a compact type description (POD layouts only)
so the engine can hash/replicate/save them and the editor inspector can render
them generically.

## Runtime

- **Host = `wasmtime` behind a `WasmHost` seam** (pragmatic exception to
  from-scratch, stated openly: a correct JIT is a multi-year project and not
  this project's learning goal — the _seam_ is ours, so a from-scratch
  interpreter can replace it later as its own learning exercise, post-MVP).
  Config: NaN canonicalization ON (cross-module float determinism), fuel or
  epoch limits (a buggy mod can't hang the server tick), no WASI.
- **Browser target symmetry**: in the browser the _browser_ is the wasm runtime
  — modules are instantiated via the stage 15 JS shim
  (`WebAssembly.instantiate`) with the same import surface; no wasmtime shipped
  to wasm builds. Engine-as-wasm hosting game-as-wasm nests cleanly.
- Sandbox = modding story: capability-granted imports, memory-isolated,
  server-authoritative user code is safe by construction (a mod cannot reach the
  filesystem, network, or other modules' memory).

## Language support tiers

| Tier                             | Languages                                                                                                                     | Path                                                       |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 1 — compiled, no GC              | **Rust** (SDK #1, P6A), **C/C++** (C header = ABI reference), **Zig**                                                         | direct wasm32 targets, near-zero runtime                   |
| 2 — scripting-feel, compiled     | **AssemblyScript** (TS syntax, wasm-first — best modder on-ramp), **TinyGo**                                                  | small runtimes, easy SDKs                                  |
| 3 — big-audience, runtime-heavy  | **C#** (Mono-wasm/NativeAOT — Unity-refugee audience), **Kotlin/Wasm**                                                        | ship GC runtime in-module; WasmGC host support when stable |
| 4 — interpreted via VM-in-module | **Lua** (official Lua-VM module template; scripts = hot-reloadable assets), JS (QuickJS), Python (MicroPython, demand-driven) | one template per VM, zero new ABI work                     |

**Priority (LOCKED)**: Rust SDK (P6A) → **C header** (proves
language-neutrality; unlocks the whole C-FFI class — C/C++/Nim/D/Odin — in one
move) → **Zig SDK** (first-class citizen: idiomatic bindings + `build.zig`
template, not just `@cImport` — Zig's wasm story is excellent and it deserves
better than header consumption) → **Lua VM template** (the modding masses) →
**C#** (Unity-refugee audience, when NativeAOT-wasm settles). Covers the bases:
native gamedev (C class + Zig), scripting/modding (Lua), managed mainstream
(C#). Other tiers remain possible by construction (flat C ABI) — just no
first-party SDK until demanded. WasmGC tracked as future-proofing for tier 3.

## SDK layout: in-repo, one dir per language

SDKs live in the engine repo — versioned with the ABI they bind, tested in the
same CI, released together:

```
sdk/
  README.md          # ABI overview, versioning policy, how to add a language
  abi/               # the single source of truth: ABI types/ids/version
  rust/              # crcbl-sdk crate (guest-side; workspace member, wasm32 target)
  c/                 # crcbl.h (generated from abi/, CI-checked in sync; extern "C"
                     #   guarded — C++ consumes it directly) + make example;
                     #   optional crcbl.hpp RAII/span wrapper when demanded
  zig/               # first-class idiomatic Zig bindings (comptime-generated from
                     #   the ABI, not just @cImport of crcbl.h) + build.zig template
  lua/               # Lua VM module template + engine-API lua glue
  csharp/            # .NET project template (lands when NativeAOT-wasm settles)
```

Rules:

- **`sdk/abi/` is the single source of truth** (Rust crate with the type/id/
  version definitions); the host imports it, `sdk/rust` re-exports it, and
  `crcbl.h` is generated from it — a drifted header fails CI, not a user.
- **Every SDK ships a `hello-module` example** (spawn an entity, tick it, emit
  an event) that CI compiles to `.wasm` and runs through the **shared
  conformance suite**: host loads it, runs N ticks, asserts exports, schema
  registration, and state hash. One suite, N languages — the ABI's
  cross-language regression net. A host ABI change that breaks any SDK breaks
  the build immediately, not a modder three weeks later.
- **Binding production is mechanical by design** — POD-only surface + the
  `sdk/abi/` spec means new-language bindings are brute-forceable (own codegen,
  or LLM-generated against the spec); the conformance suite is the acceptance
  gate either way, so correctness never rests on the producer.
- Each SDK dir is self-contained for its users (copy dir / add dep, build to
  wasm, done) — engine repo checkout not required to _use_ a released SDK
  (published per ecosystem: crates.io, header download, LuaRocks-style template
  zip, NuGet — release job grows per SDK).
- `crcbl new --lang rust|c|lua|csharp` scaffolds from the matching SDK template
  (topic 11).

## Consequences elsewhere (kept honest)

- **`crcbl new` scaffolds a module project**, not an engine fork; `crcbl run`
  hosts it. The engine binary + game modules + assets = a shipped game.
- Editor play mode loads the game's modules; the inspector renders
  module-declared components from their schemas.
- Replication carries module component data as schema'd blobs — protocol
  unchanged.
- Perf rule: FFI crossings are per-system-per-tick, array-batch granularity. If
  a module needs per-entity host calls in a hot loop, the API is wrong — add a
  batch call.

## Delivery (interleaved — see ROADMAP)

| Slice                                                                    | Roadmap phase                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Module API (trait) + static binding — samples use it from the start      | **Built** — `crcbl_ecs::game_module`'s `GameModule`. Implemented by the gameplay samples; `apps/lantern` and `apps/quarry` carry none and claim no exemption for it, and `apps/viewer`'s absence is sanctioned in its own docs. This row said "every sample" until 2026-08-23. |
| Component schema declaration + generic inspect/save/replicate            | P2–P4                                                                                                                                                                                                                                                                          |
| `sdk/` scaffold: `abi/` source of truth + conformance suite + `sdk/rust` | **P6A**                                                                                                                                                                                                                                                                        |
| `crcbl-abigen` core + Rust backend (full-SDK generation, CI drift check) | **P6A**                                                                                                                                                                                                                                                                        |
| Wasm host (`wasmtime` seam, NaN canon, fuel), Rust guest SDK             | **P6A**                                                                                                                                                                                                                                                                        |
| breakout-as-`.wasm` equivalence gate (hash == static build)              | P6A                                                                                                                                                                                                                                                                            |
| Browser nested-module instantiation via JS shim                          | P7–P10 window                                                                                                                                                                                                                                                                  |
| `crcbl mod` CLI (build/check/sign-later), hot reload of modules          | P9–P10                                                                                                                                                                                                                                                                         |
| C header (ABI reference — unlocks C/C++/Nim/D/Odin class)                | post-MVP #1                                                                                                                                                                                                                                                                    |
| Zig SDK (idiomatic bindings + build.zig, conformance-tested)             | post-MVP #2                                                                                                                                                                                                                                                                    |
| Lua VM module template (scripts = hot-reloadable assets)                 | post-MVP #3                                                                                                                                                                                                                                                                    |
| C# SDK (NativeAOT-wasm when it settles)                                  | post-MVP #4                                                                                                                                                                                                                                                                    |
| Modding polish (capability manifests, version negotiation, mod packs)    | post-MVP                                                                                                                                                                                                                                                                       |

## Exit criteria (MVP)

- breakout compiled as wasm module runs bit-identical (state hash) to its static
  build, native and in-browser.
- Hot-swapping a module mid-session preserves world state (arrays engine-side —
  demonstrated in the editor).
- A deliberately hostile module (infinite loop, OOB, huge allocs) cannot crash
  or hang the server — fuel/limits tests in CI.
- Module component data round-trips through save/load and replication with no
  module-specific engine code.

## Risks

- **ABI churn**: every engine API addition now has an FFI shape. Contained:
  additive-only after P6A, version negotiation at load, the static binding keeps
  dev friction near zero.
- **Perf cliffs at the boundary**: mitigated by batch-only design + the
  equivalence benchmark (static vs wasm breakout perf delta recorded; budget set
  there).
- **wasmtime dependency weight**: seam-isolated; interpreter replacement stays
  possible; wasm builds don't carry it at all.

## Correction: the guest memory model (design review, 2026-07-27)

The original text said modules get "views into engine-owned arrays (shared
linear-memory windows)" — **wasm cannot do that for host-owned buffers.** A
guest can only address its own linear memory, so there were exactly two
implementations and the doc had silently assumed a third that doesn't exist.

**Decision: host-created imported memory.**

- The host creates the `Memory` and the module **imports** it (rather than
  defining its own). Component arrays for module-declared components are
  allocated _inside that memory_ by the host's ECS storage allocator.
- Module `tick` therefore receives real `(offset, len, stride)` triples into its
  own address space — **genuinely zero-copy**, which is what the "one FFI
  crossing per system per tick" claim requires.
- **Hot reload survives**: memory is a separate object from the instance, so
  swapping the module instance while keeping the memory preserves all component
  state — the property the whole design rests on.
- **Cross-module access**: one memory per module (isolation preserved); a module
  reading another's components goes through a host batch call that copies — cold
  path by design, and the perf asymmetry makes the intended architecture the
  fast one.
- **Native/static binding**: unchanged (direct slices) — the equivalence gate
  compares behavior, not mechanism.
- **Browser**: an imported `WebAssembly.Memory` needs no `SharedArrayBuffer` as
  long as it isn't shared _across threads_ — which the single-threaded wasm
  build (10) already is. This is why the COOP/COEP limitation on GitHub Pages
  doesn't block modules.
- Rejected alternative: copy-in/copy-out per system per tick (the Ambient
  approach) — correct and simpler, but 2× memcpy of every component array every
  tick contradicts the engine's whole data-movement discipline. Recorded so the
  choice isn't relitigated blindly.

## Correction: module state and re-simulation (design review)

"Approximately stateless" is fine for hot reload but **not** for rollback (26)
or `replay verify` (22): both re-simulate from restored engine arrays while
guest linear memory still holds _current_ values, so any scratch- dependent
logic diverges while looking deterministic.

**Rule:** a system declared `predicted` or participating in verification must be
**strictly stateless** — all state in components. Enforced, not trusted: the
verifier re-runs it in a **freshly instantiated module** and requires an
identical state hash. Systems that want scratch may keep it, but they are
ineligible for prediction and are skipped by the verifier (declared at
registration, so the restriction is visible in code).

## Correction (browser runtime gaps, 2026-08-09)

**The browser is the wasm runtime, and it provides neither of the two guarantees
this topic's runtime section relies on.**

The runtime section configures `wasmtime` with **NaN canonicalization ON** for
cross-module float determinism and **fuel or epoch limits** so a buggy module
cannot hang the server tick. In the browser, modules are instantiated with
`WebAssembly.instantiate` — which has **no NaN canonicalization and no fuel**.

Two consequences, neither previously stated:

- **The equivalence gate is unprotected against NaN divergence.** The exit
  criterion is "breakout compiled as wasm module runs bit-identical (state hash)
  to its static build, native **and in-browser**". Basic IEEE operations agree
  across targets; NaN _payloads_ are exactly what canonicalization exists to
  normalise, and the determinism hash is where a difference would surface. The
  gate can pass for a long time and then not.
- **Hostile-module containment has no browser equivalent.** "A deliberately
  hostile module (infinite loop, OOB, huge allocs) cannot crash or hang the
  server" holds natively because fuel holds it. In a browser there is no fuel:
  an infinite loop in a module hangs the tab. This is survivable in practice
  because untrusted modules run **server-side**, and servers are native — but a
  browser-hosted single-player game with mods has no containment at all, and the
  modding story should say so.

What is genuinely solved and should not be re-litigated: **imported
`WebAssembly.Memory` needs no `SharedArrayBuffer`** while it is not shared
across threads, so the COOP/COEP limitation does not block modules. That half of
the memory-model correction holds.
