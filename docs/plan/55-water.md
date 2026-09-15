# Topic 55 — Water: bodies, the surface pass, waves, foam and buoyancy

Written 2026-09-15, from a survey of the tree and three research briefs on how
shipped games and engines draw and simulate water. Nothing in this document is
built. Its place in the set is [18-render-features.md](18-render-features.md)'s
index for the render half and [05-physics.md](05-physics.md) for the buoyancy
half; the wind it reads is [56-wind.md](56-wind.md)'s; the fixture that proves
it is [sample/21-tide.md](sample/21-tide.md).

**Water is an engine system, not a sample's shader.** A body of water is data
the engine owns — its kind, its extent, its waves, its flow, its medium — and
three consumers read the same data: the renderer draws it, the physics world
floats things on it, and a server that links no renderer answers "how high is
the water here, and which way is it moving" for gameplay. That split decides the
crate layout below before it decides any shader.

## Where this is

**Rung 1's engine half landed 2026-09-15**: `crcbl-water` and
`ForwardRenderer::set_water`, with the `water-copy` and `water` passes after
`ssr-blur` in every view. It is held by `Scene::StillPool`'s golden, four band
relations — absorption with depth, the shoreline fade, grazing Fresnel and the
in-front rejection — and a claim that removing the body draws the frame never
given one bit for bit, each shown red by a sabotage. Priced at 1920×1080 by
`mesh_e2e`'s `the_price_of_the_water_passes`, p50: on radv (RX 7900 XTX)
`water-copy` 0.021 ms and `water` 0.132 ms, the frame 0.955 ms against 0.803 ms
with no body; on lavapipe 1.160 ms and 14.799 ms, the frame 82.6 ms against 67.0
ms. **The browser price is not taken**: the render harness has no pass timer, so
it waits for the tide sample, which is the rung's other half. The DFG table is
not read (Schlick); the sky prefilter, sky-view LUT and reflection block are
borrowed from the SSR and sky passes rather than uploaded twice; `Medium` has no
anisotropy until a rung reads one.

The rest of this section is the survey that preceded rung 1, and the consumers
it names are still missing what it says:

- **No surface a camera can see through.** `crates/crcbl-render/src/forward.rs`
  builds no mesh pipeline with a `BlendState`; `GpuMaterial`'s mode bits stop at
  `ALPHA_MODE_MASK` and `DOUBLE_SIDED` (`crates/crcbl-shaders/src/mesh.rs`).
  [53-transparency.md](53-transparency.md)'s sorted blended pass is designed and
  unbuilt.
- **No copy of the opaque frame for a material to read.** `View::add_passes`
  keeps its scene colour and scene depth internal, and the overlay seam
  (`ForwardOverlayTargets`) runs after the tonemap. No plan document mentions
  refraction.
- **No time in the frame.** `FrameUniforms` carries matrices, the eye, ambient,
  cascades and the cluster grid, and nothing that advances.
- **No caller-deformed geometry.** Mesh data is fixed once `with_scene` loads
  it; the only path that rewrites vertices is compute skinning, driven by joint
  palettes.
- **Reflections exist for opaque surfaces only.** SSR is a Hi-Z march
  (`crates/crcbl-shaders/shaders/ssr.slang`) with a probe and sky fallback, and
  it rebuilds the normal from depth (`normal_at`), so a normal-mapped ripple on
  a flat plane would reflect as a perfect mirror. Planar reflections are the
  ladder's unbuilt second rung ([47-reflections.md](47-reflections.md)).
  `ForwardRenderer::create_view` can draw a second camera over the same scene,
  but `Projection` offers only `Perspective` and `Orthographic`, so no reflected
  or oblique-clipped matrix reaches it.
- **Particles are CPU-simulated opaque meshes** (`crcbl-vfx`), so there is no
  spray, mist or soft splash.
- **Physics has drag and triggers, not buoyancy.**
  `crates/crcbl-phys/src/forces.rs` carries `ForceProvider`, `DragForce`,
  `DampingForce`, `GravityForce` and `ThrustForce`, and
  `crates/crcbl-phys/src/atmosphere.rs` carries `AtmosphericDrag`. Nothing knows
  a fluid volume.
- **Physics bodies do not rotate, and a force provider has no per-body
  parameters.** `RigidBody` (`crates/crcbl-phys/src/components.rs`) holds mass,
  inverse mass, velocity and a force accumulator — no inertia, no torque, no
  angular velocity — and `PhysicsSystem::step` applies every registered
  `ForceProvider` to every dynamic body alike, so a drag coefficient or a
  reference area lives on the provider, one per scene. A boat that pitches on a
  wave needs the first, and a crate and a hull that float differently need the
  second; decision 13 depends on both.
- **Audio has looping spatial voices** and no line or area emitter for a
  shoreline or a river, and `Listener` (`crates/crcbl-audio/src/spatial.rs`)
  holds a position and no orientation.

What water _can_ build on: `RenderGraph::add_compute_pass` and compute on every
backend including the browser; the Hi-Z pyramid's depth-writing full-screen pass
(`crates/crcbl-render/src/hiz.rs`); the Hillaire sky, the sky prefilter table
and the probe volume for reflection fallback; the cascade atlas walk that
`volumetric.slang` already copies out of `mesh.slang`; froxel volumetric fog;
`crcbl_shaders::fog`'s constructed exponential; and secondary views.

## The decisions

### 1. Three crates' worth of ownership, and the data half links no GPU

- **`crcbl-water`** (new) is pure data and CPU arithmetic: body descriptions,
  the wave field, flow, the shore cook, the ripple grid, and the query API. It
  depends on `crcbl-core` and nothing that opens a device, so `crcbl-server`
  links it — exactly as it links `crcbl-phys`.
- **`crcbl-render`** gains a `water` module that owns the surface pass, the
  underwater composite and their resources, reading `crcbl-water`'s data.
- **`crcbl-phys`** gains the buoyancy and immersion hooks as `ForceProvider`
  implementations over a `WaterQuery` trait that `crcbl-phys` defines and
  `crcbl-water` implements, so the dependency points from water to physics and
  never back.

`crcbl-shaders` carries the Slang, as every shader in the workspace does.

### 2. A body is one of six kinds, and they share one surface model

Unreal's water plugin (Ocean, Lake, River, Custom), Unity HDRP's surface types
(Ocean/Sea/Lake, River, Pool) and Crest's inputs converge on the same idea: a
body is a surface, a medium and a source of motion, and the kinds differ in
where the motion comes from.

| Kind      | Extent                                      | Motion source                                        |
| --------- | ------------------------------------------- | ---------------------------------------------------- |
| Ocean     | unbounded, camera-centred rings             | FFT cascades (decision 5), shore waves near a coast  |
| Lake      | a closed outline at one level               | trochoids plus detail normals                        |
| River     | a spline with per-point width, depth, speed | flow map (decision 8) plus advected detail           |
| Waterfall | a spline segment past a slope threshold     | flow along the fall, impact stamps on the body below |
| Pool      | a closed outline, small and clear           | ripple grid (decision 9), detail normals             |
| Custom    | a caller's mesh plus a height function      | caller-chosen from the above                         |

Ponds and swamps are lakes with different medium parameters (turbidity,
absorption, scattering); they need no kind. **Every kind carries the same medium
description** — per-channel absorption and scattering coefficients and a phase
asymmetry, the parameter set Unreal's Single Layer Water exposes — so an
underwater camera, a refracted view and a colour ramp read one model.

Bodies meet: a river's spline ends in a lake or an ocean, and a waterfall's
lower end names the body it falls into. Far Cry 5 blends the two materials in
screen space over an artist range (six metres there) and damps displacement at
the seam; that is the shape this takes.

### 3. The surface is its own pass after the opaque frame, not a blended material

Unreal's Single Layer Water is **opaque**: it is drawn after the lit base pass,
reads the lit scene and depth behind it, and composites transmission, scattering
and reflection itself. It is not in the translucency sort. That is the shape
taken here, for three reasons:

- **One surface per pixel is what water is.** A lake has no second layer of
  itself to sort against, so [53-transparency.md](53-transparency.md)'s per-slot
  sort buys it nothing and its per-object granularity would cost it.
- **Refraction needs the frame behind the surface as an input**, not a blend
  factor. A blend can darken what is behind; it cannot bend it.
- **It does not wait on 53.** The blended pass remains the right answer for
  glass and particles, and it is recorded after water so a translucent object
  above a lake is depth-tested against the lake.

**The pass is recorded after `volumetric-composite` and `ssr-blur`**, which is
where 53's decision 4 puts the blended pass, and before it: in
`View::add_passes` the frame runs `forward`, `sky`, the volumetric chain, the
Hi-Z levels, `ssr` and `ssr-blur`, then bloom, exposure, `debug-draw` and
`tonemap`, and water sits between `ssr-blur` and the bloom chain. It is recorded
per view, so a secondary view sees water too. It is two passes:

- **`water-copy`**, one full-screen draw that `Load`s the HDR scene colour and
  the opaque depth into two images of its own, writing the depth through
  `SV_Depth` under an always-pass compare — the Hi-Z pyramid's construction.
  **It is not `copy_image_to_image`** (corrected 2026-09-15, after a survey of
  the frame): the render graph refuses to attach and sample one image in one
  pass, so the opaque depth has to live in a second image; no depth
  image-to-image copy is exercised on any backend in this tree, and a browser is
  where it would first be tried; and a `PassKind::Copy` has no timer, where
  [sample/21-tide.md](sample/21-tide.md) needs per-pass cost. The render pass
  runs on all four backends today, because `hiz` does.
- **`water`**, which draws the bodies into the HDR target and the depth
  attachment with depth writes on, reading the two copies.

A body is content, not an effect, so it follows the sky pass rather than a
`RenderEffects` bit: no body registered means no pass and the same frame bit for
bit. Consequences, stated:

- **Water fogs only the light it adds.** The transmitted term already carries
  the fog between the eye and the floor, because the opaque frame was fogged per
  fragment or by `volumetric-composite`; the reflected and scattered terms are
  fogged at the surface's own depth, sampling the integrated froxel volume when
  that effect ran and the analytic fog when it did not.
- **Water is not an SSR receiver through the shared pass** (SSR has already run,
  and its reconstructed normal could not see a ripple anyway). The surface
  computes its own reflection — decision 7.
- **Water writes depth**, so the blended pass, overlays and the underwater mask
  test against it; the Hi-Z pyramid and SSAO stay the opaque frame's, which is
  what they were written against.
- **Water receives the sun's cascades** through the same atlas walk
  `volumetric.slang` already copies, held to `mesh.slang` by the same
  letter-for-letter guard. It casts no shadow, for 53's decision 3 reason; the
  light it bends onto a floor is caustics (decision 10).
- **The forward pass's per-pixel write budget does not grow.** The surface
  writes into the targets that exist.

### 4. Waves are a pure function of the body, the seed and the tick

Uncharted 3's stated requirement for its ocean was "stateless, parametric,
deterministic (needed for cutscenes and multiplayer)", and Atlas says of its FFT
field that it "only depends on absolute time and spectrum parameters: can be
simulated independently on server and clients". That is this engine's rule
already, and it settles two questions at once: **nothing about the wave surface
is frame history**, and **a server and a client agree on it by construction** —
the wire carries body parameters and the tick, never heights.

The tick is the game's fixed-step clock, not wall time. The renderer receives
the tick and the fraction toward the next one, so a display faster than the
simulation still sees continuous motion (decision 6 is how the phase stays
continuous and exact).

### 5. The deep ocean is an FFT spectrum, split into non-overlapping cascades

Sea of Thieves' ocean is "an implementation of the FFT technique described in
[Tessendorf 2001]" (Ang et al., SIGGRAPH 2018 talk abstract), as are Atlas's,
Assassin's Creed IV's, War Thunder's and Unity HDRP's. It is what gives real
choppy displacement, slopes and a Jacobian to hang foam on.

- **Spectrum**: a JONSWAP spectrum with the TMA finite-depth factor, Donelan–
  Banner or Hasselmann directional spreading and Horvath's swell elongation
  (Horvath, DigiPro 2015; reference code EncinoWaves). Two JONSWAPs per ocean —
  local wind sea plus a long smooth swell — as Atlas and Acerola both sum.
- **Cascades**: three or four at 256², split **by lattice index** so each keeps
  one octave and no energy is counted twice (Crest's `ShapeFFT` rule). Tile
  lengths are non-commensurate — Robert Ryan relates them by the golden ratio
  "to prevent common factors causing visible tiling". **Acerola's shipped
  defaults sample every cascade over the full wavenumber range**, which counts
  energy up to four times; that is the pitfall this rule exists to avoid.
- **Channels per cascade**: horizontal and vertical displacement, the two
  slopes, and the three displacement derivatives the Jacobian needs — four
  complex pairs, as in Acerola's and gasgiant's compute.
- **The FFT runs in compute over storage buffers**, not storage textures. WebGPU
  allows four storage textures per stage and 256 invocations per workgroup with
  16 KiB of workgroup storage, so the 1024-wide `groupshared` kernel Acerola's
  repository uses does not port; a per-row in-place butterfly over `array<f32>`
  does, and every operation in it is an add or a multiply. Results are assembled
  into mipmapped `Rgba16Float` textures, which are filterable on every backend
  including the browser.
- **Sea states** (Sea of Thieves' calm, normal and stormy; Assassin's Creed IV's
  Beaufort keyframes) are baked spectra, and a region's state is a blend of two
  amplitude sets over the same random phases: `A = sqrt(lerp(S1, S2))`, which is
  permitted arithmetic. Wind speed from [56-wind.md](56-wind.md) selects the
  blend; it does not re-cook a spectrum at run time.

Measured elsewhere, for pricing: four 256² cascades simulated in 0.08–0.11 ms on
an RTX 4070 (Ryan, 2026), with **rendering the surface, not the FFT,
dominating** at 0.97–3.92 ms; Atlas's wind waves 0.5 ms on an RTX 2080. The
engine's own numbers replace these before the rung counts as built.

### 6. No transcendental reaches a pixel: constructed trigonometry and an integer phase

The workspace rule ([44-lighting.md](44-lighting.md)) is that no platform `sin`,
`cos`, `exp` or `pow` reaches a colour, because IEEE-754 specifies those to no
precision and four backends' implementations differ in the last place. The math
is not banned; the platform's library is. Every published water model is written
in trigonometry, so the mapping is decided here rather than per shader:

- **`crcbl_shaders::trig` — `sin` and `cos` built from exactly specified
  operations**, on `crcbl_shaders::fog::exp_neg`'s pattern: reduce the argument
  by multiples of π/2 against a two-part constant so the subtraction is exact,
  keep the quadrant as an integer, evaluate a short Taylor polynomial in Horner
  form on a remainder within π/4, and pick sign and function by quadrant. One
  body in Rust and one in Slang, held to each other by a guard, and tested
  against `f64` over the whole domain as `exp_neg` is. This is the default for
  every trigonometric term below, so Gerstner and trochoid waves, Acerola's sum
  of sines and the FFT's phase rotation are written as their sources write them.
- **What "deterministic" means for it.** On the CPU it is bit-identical on every
  target: Rust never contracts a multiply and an add on its own, and
  `crcbl-phys` already bans fast-math and `mul_add`. On the GPU it is equal
  within a known bound rather than bit-identical, because a shader compiler may
  still contract a multiply-add — the fog construction measured within two
  last-place units of `f64::exp`. That bound is the one every shading path
  already meets, and it is far below anything a floating body could show.
- **The phase stays an integer**, for precision rather than for the rule.
  Tessendorf's looping, quantised dispersion: `ω₀ = 2π/T` and each wavenumber's
  frequency is rounded to an integer multiple `n_k` of it, so the phase at tick
  `t` is `(n_k · t) mod M` steps of `2π/M`, plus `n_k` times the fraction toward
  the next tick. A float time grows without bound and `ω·t` in `f32` loses
  precision over a long session, which shows as waves that judder; a phase
  reduced modulo `M` is exact however long the game runs. Longer `T` tracks the
  true dispersion more closely.
- **Spectrum, spreading and Gaussian amplitudes** are cooked at build time,
  where `f64` and any function are available. The Gaussian draws come from an
  integer hash (HDRP's precedent), so a seed reproduces the sea.
- **Per-channel absorption `exp(−σd)` and Beckmann's `exp`** use `exp_neg`
  directly.
- **Schlick's `(1 − c)⁵`** is an integer power and permitted as written.
- **Snell's window** — `cos(asin(n sin(acos c)))` in Crest — is
  `sqrt(max(0, 1 − n²(1 − c²)))`, cheaper than any construction.
- **Baked tables remain where they win**: functions of two or more variables
  with no cheap construction, like the roughness-aware Fresnel with a
  non-integer exponent (Atlas, Bruneton), and the `erf` in whitecap coverage if
  its construction measures slower than a fetch. A table is also the right
  answer when a construction would be evaluated millions of times per frame and
  a fetch is cheaper; that is priced per rung, not assumed.

**The CPU side runs the same constructions**, so physics needs neither platform
libm nor a decision on [05-physics.md](05-physics.md)'s open
`libm`-versus-tables question to evaluate a wave.

### 7. Reflection: sky and probes always, a surface march next, planar only where flat

Far Cry used planar reflections until Far Cry 5 needed sloped rivers and
waterfalls, then moved to screen-space reflections with an environment map;
Unreal's Single Layer Water uses SSR, then captures, then the sky; Unreal's own
documentation calls SSR "much more efficient" and "much less reliable" than
planar. The ladder here is the same:

1. **Sky and probe reflection** with Fresnel, always on, and the fallback at
   every screen edge.
2. **A surface march**: the water fragment marches the opaque frame's Hi-Z
   pyramid along the reflection of **its own** normal, so ripples reflect as
   ripples. Single-frame, no temporal accumulation.
3. **Planar**, for a flat bounded body only (a pool, a still lake), capped at
   one plane per frame. It is [47-reflections.md](47-reflections.md)'s planar
   rung, and water is its second client beside the mirror
   ([sample/17-mirrors.md](sample/17-mirrors.md)). It needs `Projection` to
   accept a reflected view with Lengyel's oblique near-plane clip, re-derived
   for this engine's infinite reversed-Z (the published derivation is OpenGL's),
   and the reflected winding flipped. WebGPU's optional `clip-distances` feature
   exists but is optional, so the oblique matrix is the portable form. Unreal's
   planar documentation budgets "half your frame time" for it.

### 8. Rivers and waterfalls flow by two-phase flow maps, with no trigonometry

Valve's flow maps (Vlachos, SIGGRAPH 2010) distort a detail normal map along a
per-texel flow vector in two layers half a phase apart. The blend weight is a
triangle wave `1 − |1 − 2·frac(t + phase)|` and a noise texture offsets the
phase to hide pulsing: frac, abs and multiplies, all permitted. Uncharted 4
added advected wave-particle displacement on top; Far Cry 5 generated its flow
maps offline by a spline flood fill guided by a signed distance field.

- **The river mesh is built from its spline** in `crcbl-water`, with UVs along
  the flow, and **the flow map is cooked from the same spline** (velocity along
  the tangent, falling off toward the banks, bending around obstacles by their
  distance field). Unreal's river body writes per-point velocity into a flow map
  the same way.
- **Foam** rises with flow speed, with nearness to rocks and banks from the
  distance field, and with shallowness.
- **A waterfall is a spline segment past a slope threshold**: a fall mesh whose
  scroll speed stretches along the fall, foam density rising toward the base,
  and alpha-masked edges. Its impact is a foam ring and a displacement stamp on
  the body below — Far Cry 5's projected box decals, max-blended into a buffer —
  and Horizon Forbidden West's flipbook deformation meshes are the richer rung
  after that.
- **Mist and spray wait on blended particles**
  ([53-transparency.md](53-transparency.md) and
  [20-particles.md](20-particles.md)). Without them, mist is a local fog volume;
  alpha-to-coverage needs MSAA. Stochastic dither is refused: without a temporal
  resolve it reads as noise.
- **The CPU samples the same flow texture**, so a floating body drifts
  downstream by the flow the surface draws (Far Cry 5: "everything we do on GPU
  we replicate on CPU" for flow and physics).

### 9. Interactive ripples are simulation state stepped on the tick, and the CPU owns the gameplay copy

A ripple or wake simulation carries state from one step to the next, which looks
like history and is not: history is last frame's picture reprojected into this
one, and a wave-equation grid stepped at a fixed `dt` is game state, a pure
function of the previous state and the inputs, exactly as a rigid body is.

- **The gameplay copy is a CPU grid** (128² to 256² around each interactive
  region) stepped on the physics tick: the four-neighbour wave equation of
  Müller-Fischer's GDC 2008 course, symplectic Euler, damping, an absorbing edge
  feather, and depth-zero cells reflecting. Every operation is an add or a
  multiply in fixed order with no FMA contraction, which is the condition for
  bit-identical results across targets. It uploads to an `Rgba16Float` texture
  each tick.
- **Bodies push into it and it pushes back**: a submerged column displaces water
  and the grid returns `−Δu·h²·ρ·g` per column. A moving obstruction's wake
  falls out of the equation (Tessendorf's iWave makes the Kelvin wedge this way;
  its convolution kernel is a build-time constant table).
- **A GPU cascade that follows the camera** (Crest's dynamic waves) is a later,
  visual-only rung; it is deterministic per GPU and not across vendors, so
  nothing gameplay reads may come from it.
- **Frostbite's prototype chose the opposite**: the simulation was client-only
  and server objects used the resting height. That is a stated trade-off, not an
  oversight; this engine's server-authoritative rule is why the CPU copy exists.

### 10. Foam is history-free

Sea of Thieves "progressively blur[s] the result of the foam buffer with
feedback"; Atlas, Crest, gasgiant's and Acerola's oceans all accumulate foam
across frames. Every one of them is refused by the no-history rule, and the
replacement is published:

- **Whitecap coverage** from Dupuy and Bruneton 2012:
  `W = ½ + ½·erf((ε − µ_J) / sqrt(2σ_J²))`, with the Jacobian's mean and
  variance read from mipmapped per-cascade `J` and `J²`. The mip chain is the
  spatial blur the feedback approximated, and hardware filtering antialiases it.
  `erf` is constructed from `exp_neg` or baked, whichever prices cheaper.
- **A lifetime look without a lifetime**: HDRP's stateless foam drives a baked
  erosion texture by coverage, which reads as foam dissolving.
- **Intersection foam** from the depth difference between the surface and the
  opaque frame, every frame, plus analytic foam around hulls whose transforms
  the CPU already knows.
- **Wake foam** from a deterministic list of wake stamps (position, tick,
  heading) held as simulation state and evaluated analytically, which is what
  Uncharted's stateless wave particles demonstrate.
- **Artist foam textures** blended by coverage: Sea of Thieves' stylisation
  step, and the part of its look that costs least.

Caustics are the light side of the same field: HDRP's area-ratio method
rasterises a grid refracted through the wave normals onto a virtual plane and
measures how each texel's area changed, with no trigonometry, into a tileable
texture projected along the refracted sun onto everything below the surface.

### 11. The Sea of Thieves look is a colour model on top of a physical surface

The talk abstract is the only primary source, and it is specific about what
makes the look: a blend "between a deep water colour and a sub-surface water
colour based on a combination of view angle, sun direction and a wave peak
mask", where the mask comes from "the FFT choppiness vertex offsets" — the
translucent crest is an artistic blend keyed off horizontal displacement, not a
light-transport model. Add a large soft sun (Karis's closest-point sphere
light), stylised foam textures, and a Snell's window underwater.

So the shading is: Schlick Fresnel at `F0 = 0.02` over the sky and probes,
Karis's area sun over a Beckmann distribution with Walter's masking term, the
deep-to-subsurface colour blend by view, sun and peak mask (or Atlas's four
scatter terms, whose powers are integers), per-channel absorption, and foam.
**Distant waves dissolve into roughness rather than tiling**: Bruneton, Neyret
and Holzschuch 2010 add the slope variance of every wave the mesh cannot resolve
to the BRDF, which is what keeps glints stable with no temporal filter.

### 12. The ocean mesh is camera-centred rings with geomorphing

No tessellation stage exists on WebGPU. Crest's clipmap rings, CDLOD's
geomorphing quads and Unreal's water quadtree all answer "horizon to feet" the
same way: rings of doubling spacing, each ring's origin snapped to twice its own
spacing, odd vertices sliding toward the coarser layout across a transition
band, displacement sampled at the snapped **rest** position and added, and a
flat skirt to the horizon. Each cascade fades out at about thirty of its own
periods (Atlas). The rings are generated from the vertex index into one index
buffer per ring shape, so the ocean has no vertex buffer that grows with its
area, and plain indexed draws carry it on every backend.

Rendering dominated the FFT in every measured budget above, so the rings' vertex
count is what each quality tier prices.

### 13. Buoyancy is a CPU query against the same field, in two strengths

Every shipped answer that is deterministic evaluates the water on the CPU: War
Thunder runs a 128² physics FFT "on CPU … server + each client … fixed timestep,
48 ticks/second", judged "close enough! < 5 cm @ 3 m amplitude" against its
128²–512² graphics grid; Unreal sums its Gerstner waves on the CPU; HDRP re-runs
its FFT at half resolution in Burst. GPU readback (Crest's default, Acerola's
`AsyncGPUReadback`) lags a frame and differs by vendor, and is refused for
physics.

**Two physics prerequisites come first, and they are `crcbl-phys` work shared
with [56-wind.md](56-wind.md)**: rigid-body rotation — an inertia tensor per
shape, a torque accumulator beside `force_accum`, and angular velocity
integrated on the same semi-implicit step — because buoyancy applied off the
centre of mass is what makes a hull right itself and a crate roll; and
**per-body medium properties** — a component the providers read, holding the
pontoons or the hull mesh for water and the drag coefficient and reference area
for air — because a global provider cannot float a crate and a boat differently.

- **The query** — `WaterQuery` in `crcbl-phys`, implemented by `crcbl-water` —
  answers height, surface normal, surface velocity, flow velocity, depth to the
  floor and body identity at a world point. Under choppy waves a point's height
  needs the horizontal displacement inverted: `x ← p − D(x)`, three or four
  iterations (Crest, War Thunder, gasgiant), or Newton with step halving using
  the derivative fields the FFT already produces. The iteration contracts only
  where the Jacobian's eigenvalues are positive and fails at a fold, so the
  query reports convergence rather than returning a guess.
- **The ocean's CPU field** runs the swell cascades only, through the same
  integer phase and `crcbl_shaders::trig`, and a hand-written radix-2 FFT in
  fixed operation order; the fine cascades are visual only.
- **Pontoon buoyancy** for props: sample spheres, `−ρ g V_submerged` per sphere,
  first- and second-order vertical damping, a force cap and linear plus
  quadratic drag (the parameter set of Unreal's `BuoyancyComponent`).
- **Hull buoyancy** for vessels: Jacques Kerner's model from Just Cause 3 — clip
  the hull's triangles against a water patch sampled around the body,
  hydrostatic force at each submerged triangle's centre of pressure, viscous
  resistance, pressure and suction drag, and a slamming force. It keeps one step
  of submerged area per triangle, which is physics state. `log₁₀` in the viscous
  coefficient becomes a per-vessel constant; `f = 0.5` is a square root.
- **Low server tick rates** get Atlas's answer as an option: fit a plane to the
  samples and spring the vessel toward it.
- **Immersion events** — enter and exit with velocity (for splash size), a
  signed immersion depth per shape, a swim state when a head sample is under,
  medium drag scaled by submerged fraction, and the river current force from the
  flow map.

### 14. Underwater is a mask, a medium and the surface seen from below

Crest's approach ports to every backend; HDRP's waterline buffer relies on
`InterlockedMax` and `GroupMemoryBarrierWithGroupSync`, which WebGPU does not
have, and is refused.

- **The mask**: the displaced surface drawn with culling off into a mask target,
  front-facing meaning above and back-facing below; a horizon pass classifying
  what no tile covered; a neighbour fill for single-pixel holes, as a render
  pass rather than an in-place read-write compute.
- **The medium**: a full-screen composite applying per-channel extinction by
  distance through the water, the sun's transmittance by depth below the
  surface, and caustics. Light shafts under the surface come from the froxel
  volume with a water medium where the camera is submerged
  ([51-volumetrics.md](51-volumetrics.md)'s density-field rung), or from a
  radial post pass, which is multiplies only.
- **The meniscus**: the mask sampled a few pixels along the horizon normal,
  darkening where the classification changes.
- **The underside**: refract with a relative index of 1.333; past the critical
  angle the refracted vector is zero and total internal reflection shows the
  water below (Evan Wallace's WebGL water) — Snell's window with no
  trigonometry.

### 15. Shores are cooked, not simulated

War Thunder builds a 4096² distance field at load for a 65 km world into one
RGBA8 texture — depth, distance to shore and the field's gradient — and drives
shore waves as Gerstner waves whose **phase is the distance to shore**, with
amplitude falling with depth, crests shifting forward as they shoal, and a
sawtooth for foam. An "openness" channel, summed over directions, keeps ocean
waves off lakes and out of the lee of islands automatically.

- **The cook** lives in `crcbl-water`: from the floor's height (a heightfield or
  the scene's static geometry rasterised from above) and the water level, write
  depth, distance to shore, gradient and openness per body.
- **Shallow-water attenuation**: a wave counts as deep past half its wavelength
  and ramps down linearly below that (Crest), with shoreline foam rising as
  depth falls.
- **Breaking waves** as Horizon Forbidden West draws them — one hand-animated
  cross-section baked to a texture, deformed along wavefront curves in compute —
  are a late rung; they need dense tessellation near the wavefront.
- **Wet sand** needs a wetness term in `mesh.slang` that a body writes into; it
  is a material hook and is recorded as gated rather than planned here.

## The rungs

Each rung is priced on the desktop adapter, lavapipe and the browser before it
counts as built, per [43-render-standards.md](43-render-standards.md), and every
rung runs on the WebGPU backend and publishes in the fixture's browser demo.

| Rung | What it buys                                                                                                                                                                                                                                                                               | What it costs                                                                  | Needs                                                      |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| 1    | `crcbl-water` bodies and medium; the surface pass after `ssr-blur`; `water-copy` (colour and depth by a full-screen pass); refraction with the in-front rejection; per-channel absorption; soft shoreline fade; sky and probe reflection with Fresnel; sun cascades received; a still lake | one HDR and one depth image per frame; one pass over water pixels              | none; the tick waits for rung 2                            |
| 2    | `crcbl_shaders::trig`; trochoid waves for lakes and pools; hex-tiled detail normals (Mikkelsen 2022); camera-centred rings; `WaterQuery` height, normal and velocity; pontoon buoyancy; immersion events                                                                                   | the trigonometry construction and its guard; ring vertices per tier            | rung 1; rigid-body rotation and per-body medium properties |
| 3    | The FFT ocean: cooked spectrum, integer phase, storage-buffer FFT in compute, cascades, sea-state blend, the Sea of Thieves colour model, Bruneton variance, whitecap coverage and erosion foam; the CPU swell field for physics                                                           | compute per cascade; mipmapped cascade textures; a CPU FFT at the physics tick | rung 2; wind speed from [56-wind.md](56-wind.md)           |
| 4    | Rivers and waterfalls: spline meshes, cooked flow maps, two-phase flow, flow-driven foam, fall meshes, impact stamps, CPU flow drift; body transitions                                                                                                                                     | flow textures; spline cook                                                     | rung 2                                                     |
| 5    | Shores: the distance-field cook, shore waves, shallow attenuation, shore foam, openness                                                                                                                                                                                                    | one RGBA8 field per body                                                       | rung 3                                                     |
| 6    | The surface's own Hi-Z march; planar reflection for one flat body                                                                                                                                                                                                                          | the march per water pixel; planar is a second scene draw                       | rung 1; 47's planar rung for the second half               |
| 7    | Underwater: mask, horizon, fill, medium composite, meniscus, Snell's window, caustics                                                                                                                                                                                                      | a mask target; a full-screen composite; the caustics texture                   | rungs 2 and 5                                              |
| 8    | Interaction: the CPU ripple grid, wake stamps, Kerner hull buoyancy                                                                                                                                                                                                                        | a CPU grid step per tick; an upload per tick                                   | rung 2; rigid-body rotation                                |
| 9    | Pools and fountains: clear medium, floor caustics, a planar candidate, jets as meshes with parabolic paths, ripples from rung 8                                                                                                                                                            | as its parts                                                                   | rungs 6, 7, 8                                              |

**Gated elsewhere and recorded, not planned here**: mist, spray and splash
particles ([53-transparency.md](53-transparency.md),
[20-particles.md](20-particles.md)); wet surfaces and rain ripples (a material
wetness hook); underwater light shafts through the froxel volume
([51-volumetrics.md](51-volumetrics.md)'s density field); surf, river and
waterfall audio along a line or area ([13-audio.md](13-audio.md)); a GPU ripple
cascade; breaking-wave deformation.

**Refused, with the reason**: foam and ripple feedback buffers read across
frames (the no-history rule — decision 10 replaces them); GPU readback for
physics (lags and differs by vendor — decision 13); FLIP, SPH or any volumetric
fluid solver (a different product; no shipped game in the research used one at
run time); HDRP's waterline buffer (no WebGPU equivalent — decision 14);
stochastic dithered transparency for mist (reads as noise without a temporal
resolve).

## What each rung is checked by

- **Determinism**: two CPU evaluations of the same body, seed and tick are
  bit-identical, and a replay of a floating body lands in the same pose; a
  sabotage that swaps `crcbl_shaders::trig` for `f32::sin` goes red on a
  cross-target comparison.
- **Agreement**: the CPU height query against the rendered displacement read
  back from the GPU, at sample points, within a stated tolerance — the claim War
  Thunder's "< 5 cm" is the shape of.
- **No double counting**: the summed variance of the cascades equals the
  spectrum's integral over the range they cover.
- **Goldens** per rung and per body kind, blessed under `CRCBL_BLESS` and drawn
  by the fixture's CI step on lavapipe, with claims written as relations between
  bands (the refracted floor is darker with depth; the crest is brighter than
  the trough against the sun) rather than absolute colours.
- **The browser**: every rung's scene in the fixture's browser demo, gated by
  `pages.yml` like every other demo.
- **The buoyancy model**: a box of known density floats at the waterline its
  density predicts, to a tolerance; a vessel on flat water settles without
  oscillating.

## Sources

Primary unless marked. Research briefs, 2026-09-15.

- Ang, Catling, Cifariello Ciardi, Kozin — _The Technical Art of Sea of
  Thieves_, SIGGRAPH 2018 talk abstract (the only primary source on its water).
- Tessendorf — _Simulating Ocean Water_, course notes (2001, 2004 edition);
  _Interactive Water Surfaces_ (iWave, 2004).
- Horvath — _Empirical Directional Wave Spectra for Computer Graphics_, DigiPro
  2015, read through its reference code, EncinoWaves.
- Mihelich, Tcheblokov — _Wakes, Explosions and Lighting: Interactive Water
  Simulation in Atlas_, GDC 2019 slides.
- NVIDIA WaveWorks in War Thunder, CGDC 2015 slides.
- Bruneton, Neyret, Holzschuch — _Real-time Realistic Ocean Lighting using
  Seamless Transitions from Geometry to BRDF_, 2010; Dupuy, Bruneton —
  _Real-time Animation and Rendering of Ocean Whitecaps_, 2012.
- Crest ocean (Unity), source and SIGGRAPH 2017 and 2019 talks.
- Unity HDRP water system, source and documentation.
- Unreal Engine water plugin and Single Layer Water documentation.
- Garrett Gunnell (Acerola) — the `Water` repository (sum of sines with domain
  warping, FFT with dual JONSWAP over four cascades, the Atlas lighting model).
- gasgiant — `FFT-Ocean`; Robert Ryan — _Ocean rendering_, parts 1 and 2 (2025,
  2026).
- Vlachos — _Water Flow in Portal 2_, SIGGRAPH 2010.
- _Rendering Rapids in Uncharted 4_, SIGGRAPH 2016; _Water Technology of
  Uncharted_, GDC 2012.
- Grujic — _Water Rendering in Far Cry 5_, GDC 2018.
- Malan — _Water in Horizon Forbidden West_, SIGGRAPH 2022.
- Kerner — _Water interaction model for boats in video games_, parts 1 and
  2, 2015.
- Müller-Fischer — _Fast Water Simulation for Games Using Height Fields_,
  GDC 2008.
- Yuksel — _Wave Particles_, 2007.
- Ottosson — _Water interaction_ (Frostbite thesis).
- Mikkelsen — _Practical Real-Time Hex-Tiling_, JCGT 2022; Heitz, Neyret —
  histogram-preserving blending, 2018.
- Lengyel — _Oblique View Frustum Depth Projection and Clipping_.
- Evan Wallace — _WebGL Water_ (caustics by area ratio, Snell's window by
  `refract`).
- W3C WebGPU candidate recommendation, 2026-09-01 (storage and workgroup
  limits).

**Not reachable, and so not claimed**: the Sea of Thieves talk video and any
slides (none are published); Acerola's videos (his repository was read instead);
Unreal's water C++ source; public ocean talks for Ghost of Tsushima, Frostbite,
Skull and Bones and later Assassin's Creed titles; public waterfall breakdowns
for God of War, Uncharted and Zelda.
