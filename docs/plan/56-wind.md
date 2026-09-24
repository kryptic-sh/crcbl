# Topic 56 — Wind: the two-layer field, gusts, motors and every consumer

Written 2026-09-15, from a survey of the tree and a research brief on how
shipped games represent and consume wind. Nothing in this document is built. It
is an engine service rather than a render feature: its consumers are
[55-water.md](55-water.md), [57-grass.md](57-grass.md) (grass and trees),
[58-hair.md](58-hair.md), cloth, particles and rigid bodies in `crcbl-phys`. The
fixtures that prove it are [sample/22-meadow.md](sample/22-meadow.md) and
[sample/23-mane.md](sample/23-mane.md), with
[sample/21-tide.md](sample/21-tide.md) reading its speed for sea state.

**One field, read by everything that moves in air.** A gust that bends the
grass, lifts a character's hair, ruffles a lake and pushes a crate is one gust,
arriving at each of them at the moment its position says it should. That only
holds if every consumer samples the same field with the same formula, on the CPU
for anything gameplay reads and on the GPU for anything only drawn.

## Where this is

Absent. The nearest things in the tree:

- **Physics drag assumes still air.** `DragForce`
  (`crates/crcbl-phys/src/forces.rs`) is `−k·v` and `AtmosphericDrag`
  (`crates/crcbl-phys/src/atmosphere.rs`) is quadratic drag against the body's
  own velocity through a density that falls with altitude. Neither has a medium
  velocity, so neither can be pushed by moving air.
- **Physics has no per-body aerodynamic description and no rotation.**
  `RigidBody` (`crates/crcbl-phys/src/components.rs`) holds mass, velocity and a
  force accumulator; a provider is applied to every dynamic body alike, so a
  drag coefficient lives on the provider. [55-water.md](55-water.md)'s decision
  13 names both as prerequisites, and wind shares them.
- **Particles are CPU-simulated** (`ParticleSystem`,
  `crates/crcbl-vfx/src/system.rs`), which makes them a CPU wind consumer with
  no GPU half to agree with.
- **The renderer has no per-vertex motion for static meshes** other than compute
  skinning, so a tree cannot sway today.

## The decisions

### 1. The authored field is two texture layers: a fine intensity and a coarse direction

The user's model, and it generalises Ghost of Tsushima's: there, wind is one
direction vector times "time varying Perlin noise … scrolled in the direction
that the wind is blowing", sampled identically "on CPU and GPU" (Wohllaib, GDC
2021). Making the direction spatial too buys shelter behind cliffs, wind
channelled down a valley and turned around a mountain — without a fluid
simulation.

- **Direction layer** — larger scale, coarser: a unit horizontal vector per
  texel. The research suggests 8–16 m per texel, so a 256² layer covers two to
  four kilometres. **Stored as a vector, never an angle**: bilinear filtering of
  vectors is valid, an angle would need `atan2`, `sin` and `cos` to use, and the
  renormalise afterwards is a square root.
- **Intensity layer** — finer: a speed multiplier per texel, zero meaning calm.
  The research suggests 1–2 m per texel, paged per world tile like any other
  streamed texture.
- **Both are ordinary assets** in a standard image format, authored or cooked
  from terrain, loaded through `crcbl-assets`.

**Candidate extra channels, recorded and not decided**: an updraft bias and a
turbulence scale on the direction layer; a gust susceptibility and a shelter
term on the intensity layer. Each is cheap because the texel is already read;
each is added when a consumer needs it, not before.

### 2. The field's weather is a small state advanced on the tick

Over the two layers sits a weather state: a base direction and a base speed. God
of War's authors named five speeds from the Beaufort scale — still 0.5, calm 2,
breezy 5, strong 9, violent 15 m/s — which is the scale presets use. SpeedTree's
gust model puts rise, fall and response times on changes; here those are
properties of the **consumers'** springs (decision 7), so the field itself
changes the moment its state does and nothing in it carries history.

### 3. Gusts travel by one global scroll offset, in integer fixed point

A static intensity map does not move, and a gust that visibly rolls across a
field is most of what makes wind read as wind. Ghost of Tsushima scrolls noise
along the wind; God of War's vegetation talk (Feeley, Advances 2019) records the
trap in doing it per object: **each object scrolling its own offset diverges
from its neighbours as soon as direction or speed changes**, and they needed
flow-map flips with log-binned speeds to repair it.

So there is **one offset for the world**, accumulated on the tick as
`o ← o + direction · gust speed · Δt` in integer fixed point, so it never
drifts; a gust term is a baked tileable noise texture sampled at `(x − o) / L`
at one or two scales. The renderer receives the offset relative to the camera so
a float never holds a world-scale coordinate. Travelling gust fronts, where
wanted, are smooth triangle waves of `dot(x, direction) / λ` minus a phase —
`x·x·(3 − 2x)` over `abs(frac(x + 0.5)·2 − 1)`, the Crysis construction, which
is permitted arithmetic.

### 4. Local wind is an analytic motor list, identical on CPU and GPU

Ghost of Tsushima's "vorticles" — spheres of wind with a position, orientation,
radius and a vector in a frame on the sphere's surface, giving vortices, linear
gusts and blasts from one description, summed brute force "even up to like
hundreds" — and God of War's motors (sphere, cylinder and cone shapes;
directional, omni, vortex and wake emissions, wake motors by far the most used)
are the same idea. A helicopter, an explosion, a sword swing and a passing
character are motors.

- **A capped list** in a uniform array, so it spends no storage-buffer slot on
  WebGPU. The CPU keeps the authoritative list and the nearest entries go to the
  GPU.
- **Falloff by smoothstep polynomials**, one function body, compiled into both
  the CPU sampler and the Slang.
- **Aliasing** of small motors against a coarse grid (God of War's note) does
  not arise for an analytic list; it does for decision 5's grid, which scales a
  motor's contribution by how much of each texel it covers.

### 5. Persistent wakes are a later rung: a CPU grid stepped on the tick

An analytic motor stops the moment it is removed; a real wake lingers. God of
War ran a 32×16×32 grid at one metre following the player — Stam's stable
fluids, pressure switched off studio-wide because "nobody complained", about 0.1
ms on async compute. It also found that a huge directional motor inside the box
produced a gradient, strong downwind and weak upwind, because nothing pushed in
from outside, and fixed it by using the simple global-plus-noise wind as the
baseline everywhere with the grid adding only local detail. That is the layering
here: **the two layers are always the baseline, and the grid adds.**

- **A 2D grid first**, 64² at one metre around the camera: semi-Lagrangian
  advection and a few diffusion passes on the tick, re-centred by whole texels,
  uploaded as `Rgba16Float` each tick. It is simulation state, not render
  history, and on the CPU it is deterministic in fixed operation order.
- **A 3D grid** (God of War's shape) when a consumer needs vertical structure.

### 6. Sampling is one formula, CPU-authoritative

```
v(x) = I(x) · S · D(x) · (1 + gust(x − o)) + Σ motors(x) + grid(x)
```

with `I` the intensity, `D` the renormalised direction, `S` the weather speed
and `o` the scroll offset. A terrain-following vertical component
(`k · (v · ∇h)`, Ghost of Tsushima's particles rising over hills) and an updraft
channel are candidates. **Consumers may subtract their own velocity** — God of
War's counter-wind — so an object moving with the wind looks unaffected.

- **The CPU copy is authoritative** and is what physics, audio and CPU particles
  read. `crcbl-wind` (new) is pure data and CPU arithmetic, depends on
  `crcbl-core`, and links into `crcbl-server`.
- **The GPU copy** is one bind group — a uniform block (weather, offset, motors,
  grid origin), the two layers, the gust noise, the grid texture and a sampler.
  Every texture is sampled, not a storage texture, so WebGPU's four storage
  textures per stage are untouched. The GPU result is not bit-identical to the
  CPU's, and nothing gameplay reads comes from it.
- **`crcbl-phys` defines a `WindQuery` trait** that `crcbl-wind` implements, the
  same arrangement as [55-water.md](55-water.md)'s `WaterQuery`, so physics does
  not depend on the wind crate.

### 7. Response lives in each consumer, as state stepped on the tick

The field answers "what is the air doing here, now". How a thing responds — lag,
overshoot, a branch still swinging after the gust has passed — is the consumer's
own state: a spring per tree instance (God of War's sway spring and damping,
computed per instance), a spring per shell-fur object, constraints per hair
chain, a tip state per grass blade. That keeps the field stateless and every
consumer's history honest about being simulation state rather than render
history.

### 8. The consumers, and what each reads

| Consumer         | Reads                                           | Responds by                                                                | Owner                              |
| ---------------- | ----------------------------------------------- | -------------------------------------------------------------------------- | ---------------------------------- |
| Rigid bodies     | CPU sample at the body                          | quadratic drag on `v_rel = w − v` with a per-body `C_d` and projected area | `crcbl-phys`                       |
| Trees and plants | GPU sample per instance, per branch at rung T3  | per-instance springs; baked per-vertex hierarchy                           | [57-grass.md](57-grass.md)         |
| Grass            | GPU sample per blade or card                    | bend at the tip; blade tip state at the simulated rung                     | [57-grass.md](57-grass.md)         |
| Hair and fur     | CPU sample per chain segment or object          | XPBD chains, springs                                                       | [58-hair.md](58-hair.md)           |
| Water            | CPU and GPU regional speed; intensity per texel | sea-state blend; ripple and detail-normal strength                         | [55-water.md](55-water.md)         |
| Cloth and flags  | sample per triangle or vertex                   | per-triangle drag and lift on relative velocity                            | `crcbl-phys`                       |
| Particles        | CPU sample per particle                         | drag in a moving frame (God of War's particles)                            | [20-particles.md](20-particles.md) |
| Audio            | CPU speed at the listener                       | a wind bed's gain                                                          | [13-audio.md](13-audio.md)         |

**The rigid-body force, stated so it is written once.** Projected area of a
sphere is `πr²`; of a box with extents `L` against a unit relative direction `d`
in body space, `|d_x| L_y L_z + |d_y| L_x L_z + |d_z| L_x L_y`; the mean over
all directions of any convex body is its surface area over four (Cauchy).
Explicit quadratic drag overshoots when `k |v_rel| Δt ≥ 1` with
`k = ½ρ C_d A / m`, so the provider limits the change to the linearised implicit
update `v_rel' = v_rel / (1 + k |v_rel| Δt)`: it never reverses the relative
velocity, and it is algebraic. `AtmosphericDrag` becomes the special case of
still air.

### 9. No transcendental reaches a pixel

Every shipped wind animation in the research is written with `sin`: Ghost of
Tsushima's blade bob, Horizon's ambient motion, SpeedTree's oscillators, Pivot
Painter's rotations, Acerola's `cos²` and marble noise. The rule bans the
platform's library, not the math, so the mapping is decided here once:

- **Oscillation**: `sin` and `cos` from `crcbl_shaders::trig`, the construction
  [55-water.md](55-water.md)'s decision 6 defines — bit-identical on the CPU,
  within a known bound on the GPU — so a source's sine bob is written as
  published. The phase comes from the tick in integer arithmetic, so it stays
  exact over a long session. Crysis's smoothed triangle wave remains the cheaper
  choice where its shape is enough.
- **Noise**: baked tileable textures.
- **Rotation about a pivot**: translate and restore length toward the pivot, God
  of War's "avoids slow trigonometric functions" construction.
- **Bend exponents**: integers by repeated multiplication; a non-integer
  exponent through the same constructions or a baked table, priced.
- **Direction**: vectors, not angles (decision 1).

## The rungs

| Rung | What it buys                                                                                                                                                                                                              | What it costs                                    | Needs                          |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ | ------------------------------ |
| W1   | `crcbl-wind`: weather state, the two layers, CPU and GPU sampling, the bind group, the `WindQuery` trait — **built 2026-09-16**, with decision 3's smoothed-triangle gust front and the integer scroll offset it rides on | two taps per sample; two textures                | —                              |
| W2   | Travelling gusts: the integer scroll offset and baked gust noise                                                                                                                                                          | one or two taps                                  | W1                             |
| W3   | The motor list: vorticle frames and motor shapes                                                                                                                                                                          | per sample, linear in the list's cap             | W1                             |
| W4   | Rigid-body wind drag with per-body coefficient and projected area                                                                                                                                                         | per body per tick                                | W1; per-body medium properties |
| W5   | The CPU 2D motor grid for lingering wakes                                                                                                                                                                                 | a grid step and an upload per tick               | W3                             |
| W6   | A 3D grid for vertical structure                                                                                                                                                                                          | a larger step, or a GPU step that is visual-only | W5                             |

Every rung runs on the WebGPU backend; none needs a storage texture.

## What each rung is checked by

- **CPU determinism**: two samplings of the same field at the same tick are
  bit-identical; a replay of a body pushed by wind lands in the same pose.
- **CPU–GPU agreement**: the GPU sample read back at fixed points against the
  CPU sample, within a stated tolerance, on every backend.
- **Gust coherence**: two consumers a known distance apart along the wind see
  the same gust arrive separated by distance over gust speed — the property
  per-object scrolling loses.
- **The drag model**: a body released in steady wind approaches the wind's
  velocity and never overshoots it at the tick rate the fixture runs.
- **Calm means calm**: a zero-intensity texel moves nothing, in every consumer's
  golden.

## Sources

Research brief, 2026-09-15.

- Rockenbeck — _Blowing from the West: Simulating Wind in Ghost of Tsushima_,
  GDC 2021 slides and talk transcript.
- Wohllaib — _Procedural Grass in Ghost of Tsushima_, GDC 2021, transcript.
- Renard — _Wind Simulation in God of War_, GDC 2019, transcript; Feeley —
  _Interactive Wind and Vegetation in God of War_, Advances in Real-Time
  Rendering 2019, slides and speaker notes.
- Sanders — _Between Tech and Art: The Vegetation of Horizon Zero Dawn_,
  GDC 2018.
- Stam — _Real-Time Fluid Dynamics for Games_, GDC 2003.
- Sousa — _Vegetation Procedural Animation and Shading in Crysis_, GPU Gems 3
  chapter 16.
- SpeedTree wind documentation; Unreal `WindDirectionalSource` and Nanite
  Foliage documentation; Unity `WindZone` documentation.
- NASA Glenn — the drag equation.

**Not found**: Far Cry 5's wind runtime; any primary source for God of War's
motors pushing rigid debris.
