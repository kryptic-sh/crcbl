# Stage 5 — Assets + Scenes

Stop hardcoding cubes. glTF meshes/materials in, engine-native scene format,
stable asset ids, hot reload for the dev loop.

> Scene-format choice (glTF for meshes + own scene format) was flagged
> revisitable in the overview. This stage starts with a short design doc PR
> before implementation — final call happens there.

## Goals

- `crcbl-scene`: load a scene file → entities registered into ECS systems +
  meshes/materials resident in the stage 3 GPU pools.
- Asset identity + lifetime model that works for editor (mutable, reloadable)
  and shipped game (packed, immutable) — and wasm (fetched, async).
- Hot reload: shaders and assets, dev builds only.

## Asset model

- `AssetId` = 64-bit hash of canonical path (stable across machines);
  `AssetHandle<T>` = runtime generational handle (from `crcbl-core`).
- Load states: `Unloaded → Loading → Ready | Failed` — **async from day one**,
  because wasm has no blocking filesystem: all IO goes through an `AssetSource`
  trait (`DirSource` native; `FetchSource` wasm in stage 9; `PackSource`
  post-MVP for shipped builds).
- Dependency tracking: scene → meshes → materials → textures. Refcounted
  release; the GPU pools get retire calls through the stage 2 deletion queue.

## Import pipeline

- **glTF 2.0** (`gltf` crate): meshes (positions/normals/tangents/UVs),
  PBR-metallic-roughness materials, textures (KTX2/basis post-MVP; PNG/JPEG
  decode for MVP), node hierarchy flattened into instances. Skins/animation
  parsed but unused (post-MVP).
- Import happens at load time in MVP (no offline bake step). The `AssetSource`
  seam is where a bake/pack pipeline slots in later without touching consumers.
- Textures: sRGB/linear handling correct from the start; mip generation on
  upload (compute pass — GPU-side, per the round-trip principle).

## Scene format

- Own format, human-diffable RON (`.scn.ron`) in MVP: entities as
  `(system → data)` maps mirroring the ECS registration model — a scene file
  literally lists, per system, the array entries to create. This keeps scene ↔
  ECS ↔ replication all the same shape.
- References assets by canonical path (hashed to `AssetId` on load).
- Versioned header + serde defaults for forward-compat; no migration machinery
  in MVP.
- The scene loader is a _server-side_ concern (scenes are simulation state); the
  client learns of the result through normal replication. Render-only data
  (mesh/material assignment) replicates as component data.

## Hot reload (dev builds)

- Native: notify-based file watcher → asset reimport → in-place GPU pool update
  (mesh range swap; stale ranges retire through deletion queue).
- Shaders: Slang recompile → pipeline rebuild keyed by shader hash (stage 2 left
  the runtime-recompile path open).
- Scene reload: tear down scene-owned entities, re-instantiate — server-side
  operation, replication propagates it. This same path is the editor's "revert
  scene" later.

## Tasks

1. Design-doc PR settling the scene format question (½ day, then locked).
2. `AssetId`/handle/state machine + `AssetSource` trait + `DirSource`.
3. glTF import → GPU pools (meshes, materials, textures, mips-on-GPU).
4. RON scene format: serialize/deserialize, server-side instantiation.
5. Watcher + reload paths (assets, shaders, scene).
6. Sandbox: load a real glTF scene (Sponza or similar) via a `.scn.ron`, fly
   through it at stage 3 performance targets.

## Exit criteria

- Sponza-class scene loads through the full path (scene file → server →
  replication → client → GPU pools) and renders at target perf.
- Editing a texture/shader/scene file on disk reflects in the running sandbox
  without restart.
- No synchronous IO anywhere in engine crates (CI: deny `std::fs` outside
  `DirSource` + tooling).

## Risks

- **Import scope explosion (full glTF spec).** MVP importer supports exactly
  what the sandbox scene needs; unsupported features log-and-skip loudly.
- **Hot reload edge cases eat time.** Reload is dev-only: correctness bar is
  "doesn't crash, usually works", not production-grade.
