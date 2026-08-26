# Stage 6 — Assets + Scenes

Stop hardcoding cubes. Grounded open formats in, engine-native scene format,
stable asset ids, hot reload for the dev loop.

**Format decisions are LOCKED** (settled 2026-07-27, supersedes the earlier
"revisitable" flag):

## Format matrix

Pattern: **source formats = grounded open standards; cooked formats = ours**
(produced by `crcbl import` / `crcbl bake`, topic 11). We never invent a source
format where an open standard suffices — own formats only where we own the
semantics (scenes) or the runtime layout (cooked output).

| Asset     | Source (tracked in git)                   | Cooked (shipped/web)                       | Notes                                |
| --------- | ----------------------------------------- | ------------------------------------------ | ------------------------------------ |
| Mesh      | glTF 2.0 (.gltf/.glb)                     | packed binary matching GPU pool layout     | git LFS — mesh diffs are meaningless |
| Skeleton  | glTF skins                                | cooked joint tables                        | same file/pipeline as meshes         |
| Animation | glTF animation channels                   | cooked sampled/compressed curves           | curve compression = cook step        |
| Texture   | PNG                                       | KTX2/BasisU post-MVP (PNG passthrough MVP) | LFS; sRGB/linear correct from start  |
| Audio SFX | WAV (PCM)                                 | QOA (open one-page spec, own decoder)      | LFS for WAV; see topic 13            |
| Music     | WAV/FLAC                                  | Vorbis or Opus (post-MVP, decoder seam)    | streaming                            |
| Scene     | own: `.scn/` directory of RON chunk files | packed scene blob (`crcbl bake`)           | design below                         |
| Config    | TOML (`crcbl.toml`, tuning tables)        | as-is                                      | flat data, familiarity wins          |

RON vs TOML split: **RON for scene chunks + entity data** (nested, enum-heavy —
collider shapes, component variants map natively to Rust enums; TOML forces
stringly `type = "..."` conventions and verbose nesting). **TOML for flat
config** where ubiquity and editor support win. Both get comments.

`.gitattributes` in the workspace root exists and marks those types binary, but
**LFS is deliberately not enabled yet** and that file carries the reason:
everything binary through P8 is small, golden images are re-blessed often (which
LFS handles worse than plain git), and a `filter=lfs` line breaks `git commit`
outright on a clone without the git-lfs binary. It turns on when the P9 glTF
corpus lands — and when it does, every `actions/checkout` step in CI must gain
`lfs: true` in the same commit, or CI silently tests against pointer files.
Cooked outputs are build artifacts, never tracked.

## Scene format: directory of chunk files

A scene is a **directory with an extension**, chunks are files:

```
scenes/level1.scn/
  scene.ron          # header: format version, metadata, system manifest
  env.ron            # camera defaults, lighting, ambience
  sys/
    creeps.ron       # one file per system: that system's entity array
    towers.ron
    props.ron
```

This resolves the two constraints that fight inside any single file:

- **Chunk persistence**: the editor tracks dirty systems (same dirty-set
  machinery replication already needs). Save = rewrite only dirty chunk files,
  each small. A full-scene rewrite never happens.
- **Git-friendliness**: small text files, per-system diffs ("moved 3 towers" =
  3-line diff in `sys/towers.ron`), and two people editing different systems
  merge with zero conflicts.

Load semantics unchanged: entities as `(system → data)` arrays mirroring ECS
registration — scene ↔ ECS ↔ replication stay the same shape. Assets referenced
by canonical path (hashed to `AssetId` on load). Versioned header + serde
defaults for forward-compat; no migration machinery in MVP.

### Deterministic writer (90% of git-friendliness)

- Canonical field order; entities sorted by stable ID.
- **Stable entity IDs persisted in the file, never regenerated on save** (the
  classic Unity/Godot diff-noise bug — banned by test).
- Shortest-roundtrip float formatting (Rust f64 `Display` semantics):
  deterministic and exact.
- No timestamps, no editor-session state in scene files.
- Invariant, enforced as a topic-12 property test: load → save → byte-identical
  file; edit one entity → diff touches only its lines.

### Command journal (complement, not format)

Editor autosave = append-only log of edit commands (`.scn.autosave.log`,
gitignored — the stage 8 command stream serialized). Crash recovery replays the
journal; explicit save = flush dirty chunk files canonically + truncate journal.
Git only ever sees canonical form.

### Scaling

If a system's array grows huge, shard its chunk by sector:
`sys/props/{sector}.ron` — same mechanism, finer chunks, and it aligns with
physics sector streaming (stage 5) so a sector's scene data and its physics load
unit coincide.

### Bake

`crcbl bake` packs a `.scn/` directory (+ referenced cooked assets) into one
binary blob for shipping and wasm (solves the many-small-fetches problem;
`FetchSource` serves the blob). Cooked output = build artifact.

## Asset model

- `AssetId` = **128 bits**, and the corrections below replace hash-of-path with
  a sidecar GUID as its source; `AssetId::from_path` survives as the CLI/debug
  lookup and `AssetId::from_bits` is where a GUID arrives without the type
  changing shape. The handle is `crcbl_core::Handle<Asset>`, not a second handle
  type — see the task-2 note below for why there is no `<T>` parameter.
- Load states: `Loading → Ready | Failed` — **async from day one**, because wasm
  has no blocking filesystem: all IO goes through an `AssetSource` trait
  (`DirSource` native; `FetchSource` wasm in stage 10; `PackSource` for baked
  blobs). Physics sector streaming (stage 5) rides the same trait.
- Dependency tracking: scene → meshes → materials → textures. Refcounted
  release; the GPU pools get retire calls through the stage 2 deletion queue.

## Import pipeline

- **glTF 2.0** (`gltf` crate): meshes (positions/normals/tangents/UVs),
  PBR-metallic-roughness materials, textures, node hierarchy flattened into
  instances. Skins/animations parsed but unused until the animation feature
  lands (post-MVP) — the format choice already covers them.
- Import happens at load time in MVP (no offline bake step required); the
  `AssetSource` seam is where `crcbl bake` output slots in without touching
  consumers.
- Textures: sRGB/linear handling correct from the start; mip generation on
  upload (compute pass — GPU-side, per the round-trip principle).

## Hot reload (dev builds)

- Native: notify-based file watcher → asset reimport → in-place GPU pool update
  (mesh range swap; stale ranges retire through deletion queue).
- Shaders: Slang recompile → pipeline rebuild keyed by shader hash (stage 2 left
  the runtime-recompile path open).
- Scene reload: chunk-file granularity — a changed `sys/towers.ron` tears down
  and re-instantiates only that system's scene entities (server-side;
  replication propagates). Editor "revert" reuses the same path.

## Tasks

1. `.gitattributes` + LFS setup (P0, foundations).
2. `AssetId`/handle/state machine + `AssetSource` trait + `DirSource`.
3. glTF import → GPU pools (meshes, materials, textures, mips-on-GPU).
4. Scene-dir format: deterministic RON writer, chunk load/save, dirty-chunk
   tracking, roundtrip property tests.
5. Watcher + reload paths (assets, shaders, per-chunk scene reload).
6. `crcbl import` CLI wiring — the **report** half is built; see the task-3 note
   below and [11-cli-headless.md](11-cli-headless.md). Writing waits on task 4,
   which has not landed, so there is no scene directory to write into.
   `crcbl bake` (may land later, pre-Pages-demo of a scene-heavy sample).
7. Sandbox: load a real glTF scene (Sponza or similar) via a `.scn/` dir, fly
   through it at stage 3 performance targets.

## Exit criteria

- Sponza-class scene loads through the full path (scene dir → server →
  replication → client → GPU pools) and renders at target perf.
- Deterministic-writer property tests green: save is byte-stable, single- entity
  edit produces minimal diff, editing one chunk file hot-reloads only that
  system.
- Editing a texture/shader/scene chunk on disk reflects in the running sandbox
  without restart.
- No synchronous IO anywhere in engine crates (CI: deny `std::fs` outside
  `DirSource` + tooling).

## Risks

- **Import scope explosion (full glTF spec).** MVP importer supports exactly
  what the sandbox scene needs; unsupported features log-and-skip loudly.
- **Hot reload edge cases eat time.** Reload is dev-only: correctness bar is
  "doesn't crash, usually works", not production-grade.
- **Writer determinism erosion.** Any new serialized type must keep the
  byte-stable-save property; the property test is the gate, not review
  vigilance.

## Corrections (design review, 2026-07-27)

- **`AssetId` = hash-of-path breaks on rename/move.** Renaming `props/crate.glb`
  silently orphans every scene chunk, material instance, and effect referencing
  it — the exact problem Unity's `.meta` GUIDs and Godot's `.import` UIDs exist
  to solve. **Corrected model**: every asset gets a **sidecar meta file**
  (`crate.glb.meta.ron`) carrying a stable random 128-bit GUID, created on first
  import and committed to git; references use the GUID. Path hashing survives
  only as a CLI/debug convenience lookup. Cheap now, painful after P9 content
  exists.
- **The sidecar also carries import settings** that a bare file can't express:
  texture `color_space` (sRGB vs linear — a standalone normal-map PNG has no way
  to declare itself), `usage`, compression target, LOD overrides (25), ragdoll
  asset link (35).
- **sRGB mip generation**: Vulkan has **no sRGB storage-image format**, so
  "mipgen in a compute pass" cannot write sRGB directly. Corrected: create a
  `UNORM` **image view alias** over the sRGB image and do the encode/decode
  manually in the compute shader (the standard approach), or fall back to
  render-pass downsampling. Stated so it isn't discovered at P9.

## Landed: task 2 (2026-08-11)

`crates/crcbl-assets` — a new workspace member, not `crcbl-scene`. Assets are
read-only content shipped with the game; `crcbl-store` is data the player
produces; a scene _references_ assets, so the dependency runs `crcbl-scene` →
`crcbl-assets` and putting the registry in the scene crate would invert it.
Nothing depends on the crate yet, which it shares with the `crcbl-scene` stub
and for the same reason.

> **Both have consumers now, 2026-08-15.** `crates/crcbl-cli/Cargo.toml` depends
> on `crcbl-scene` and `crcbl-assets` together: `crcbl lod` reads a glTF and
> builds its cluster DAG through the scene crate, and the asset crate comes with
> it as the IO seam underneath. The dependency direction is the one this note
> argued for.

- **`AssetId`** — 128 bits, `Display`/`Debug` as 32 hex digits, derived by
  `AssetId::from_path` as the leading 16 bytes of the SHA-256 of the canonical
  key (`crcbl_shaders::sha256`, already NIST-vector-tested and already reused by
  `crcbl-store`). 128 rather than the 64 the asset-model section says, because
  the corrections section replaces path hashing with a sidecar GUID and
  `AssetId::from_bits` is where such a GUID arrives without the type changing
  shape. The sidecars themselves need the importer and are not here.
- **The handle is `crcbl_core::Handle<Asset>`**, issued by a `crcbl_core::Pool`.
  No second handle type, and no `<T>` parameter: `Mesh`/`Texture` arrive with
  the importers, and a phantom parameter with one instantiation checks nothing.
- **States: `Loading | Ready | Failed`.** No `Unloaded` — an asset nobody
  requested has no entry and a released one is removed, so nothing can hold that
  state and no test could reach it. It returns when hot reload (task 5) or the
  deletion-queue retire path can produce it.
- **`AssetSource`** — one method,
  `read(&Path) -> Result<Vec<u8>, StorageError>`, defined never to block; a
  source without the bytes answers `StorageError::Pending` and the caller polls.
  That is `crcbl_store::web::FetchSource`'s existing contract verbatim, so the
  stage-10 browser source is a delegating wrapper and no caller changes.
  Deliberately not a blanket impl over `StorageSource`: that would claim the
  trait for every storage backend and leave `PackSource` unable to implement it
  on its own terms.
- **`DirSource`** — `crcbl_store::NativeStorage` narrowed to a read. Keys go
  through `crcbl_store::web::canonical_key` first, so `DirSource` and a future
  `FetchSource` accept exactly one key set and an asset tree that loads from a
  directory is one that can be served over HTTP.
- **`AssetRegistry`** — dedupes by `AssetId`, refcounts, `poll()` advances every
  `Loading` entry and returns how many are still waiting. The refcount is the
  plan's "refcounted release" minus the GPU retire calls, which need the stage 2
  deletion queue and a GPU-resident asset to retire.

## Landed: task 3, first half — glTF parsing (2026-08-11)

`crates/crcbl-scene`, which stops being a placeholder. Not `crcbl-assets`: that
crate is the IO seam and its own docs say decoding belongs to whoever owns the
format, which is how PNG ended up in `crcbl-sprite` and WAV in `crcbl-audio`.
glTF's owner is the crate whose package description has said "scene format and
glTF import" since the workspace skeleton, that [12-testing.md](12-testing.md)'s
anchor list assigns the glTF corpus to, and whose dependency direction
(`crcbl-scene` → `crcbl-assets`) the task-2 note above already states. A third
crate would have been a new name for the same responsibility beside a crate that
had already claimed it.

**Parsing only.** No GPU pool upload, no textures, no mip generation, no RON
scene format, no hot reload, no `crcbl import`. Skins and animations are in the
file and are not read: a type nothing fills is worse than no type, and the
format choice already covers them.

> **The crate outgrew that sentence, 2026-08-15.** Parsing is `gltf_import` and
> `gltf_check`, and beside them the crate now carries a mesh **simplifier**
> (`simplify`), a **meshlet** builder (`meshlet`), a **cluster DAG**
> (`cluster_dag`) and LOD selection and resolution (`lod`, `lod_resolve`) — the
> geometry pipeline topic 25 owns, reached from `crcbl lod`. What the list above
> says is still absent is still absent: no GPU pool upload, no textures, no mip
> generation, no RON scene format, no hot reload, and skins and animations are
> still deliberately unread.
>
> **`crcbl import` landed on 2026-08-23**, as the reporting half alone:
> `crcbl import <gltf> [--json]` runs `import_gltf` and prints what came out —
> meshes, primitives, materials, images, nodes, instances — with the importer's
> own skip warnings beside them. It writes nothing, and its `--out <dir>` is
> refused by name for the reason the list above still gives: there is no on-disk
> scene format in this tree to write one to.

- **`import_gltf(&dyn AssetSource, &Path) -> Result<GltfScene, StorageError>`.**
  The document and every external `.bin` it names go through the seam, so a
  browser source answering `Pending` makes the import `Pending` and the caller
  retries — the "no synchronous IO anywhere in engine crates" exit criterion,
  met by not enabling the `gltf` crate's `import` feature, which does its own
  blocking `std::fs` reads and drags in a second image decoder.
- **Buffer URIs resolve relative to the document's key and through the source**,
  so `crcbl_store::web::canonical_key` governs them: a `.bin` outside the asset
  root, a percent-encoded name, a Windows path are all `InvalidPath` rather than
  reads. `data:` URI buffers are `Unsupported` — decoding base64 would have
  meant the `gltf` crate's `base64` feature, which only exists as part of
  `import`.
- **`GltfScene::materials` is `[crcbl_shaders::mesh::GpuMaterial]`**, not a
  parallel material type. glTF's `pbrMetallicRoughness.baseColorFactor` is
  linear RGBA by specification and `GpuMaterial::base_color` is documented as
  linear RGBA, so the mapping is an assignment; both defaults are `[1.0; 4]`
  too. The colour-space question the texture half will have is a different one —
  factors are linear, base-colour _textures_ are sRGB.
- **`GltfInstance::transform` is column-major `[f32; 16]`**, the layout
  `GpuInstance::transform` holds, and is **not** guaranteed rigid: glTF nodes
  carry scale and this preserves it. That field takes any affine matrix — the
  mesh shaders build the normal transform out of it — so a scaled node needs no
  decision at the upload step. What it still costs is the per-cluster back-face
  cull, which `crcbl_scene::gltf_render` reports as a `scale` skip; see
  `docs/backlog.md`.
- **The importer validates the document itself** (`crcbl_scene::gltf_check`) and
  parses with `Gltf::from_slice_without_validation`, because `gltf` 1.4.1's own
  validation **panics on inputs it exists to reject** — reproduced, not reasoned
  about: an out-of-range `POSITION` accessor index indexes `root.accessors`
  directly in `primitive_validate_hook`, and a `.glb` whose header declares a
  length under 12 subtracts with overflow in `Glb::from_slice`. Everything the
  importer reads is bounds-checked before the typed API — which is full of
  `unwrap`, `unreachable!` and `debug_assert` reachable from file contents —
  sees it.
- **`StorageError` is reused rather than joined by a second enum**; malformed
  files are `Other` with the key and the reason. `docs/backlog.md` records what
  a dedicated variant would buy and why nothing needs it yet.
