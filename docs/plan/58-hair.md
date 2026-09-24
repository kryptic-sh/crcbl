# Topic 58 — Hair and fur: shells, cards on simulated chains, strands, and the motion that drives them

Written 2026-09-15, from a survey of the tree and a research brief on how
shipped games and Acerola's fur work render and simulate hair. Nothing in this
document is built. Its place in the set is
[18-render-features.md](18-render-features.md)'s index for the drawing and
[17-animation.md](17-animation.md) for the motion; the wind it reads is
[56-wind.md](56-wind.md)'s; the shell technique it shares with grass is
[57-grass.md](57-grass.md)'s; the fixture that proves it is
[sample/23-mane.md](sample/23-mane.md).

**Hair moves because what it is attached to moves.** A ponytail that sways only
in the wind, and not when its owner turns, is animation rather than physics. The
subject of this topic is that coupling — the parent's velocity and acceleration
feeding the simulation, with the wind added on top — and every rung below is
judged by it before it is judged by how the strands look.

## Where this is

Absent. What it builds on, and what constrains it:

- **Skeletal animation ends at a palette.** `crcbl-anim` samples clips into a
  `Pose` and composes a `Palette` (`crates/crcbl-anim/src/palette.rs`); the
  compute skinning dispatch is `crcbl-render`'s, fed through
  `ForwardRenderer::reserve_skinned` and `add_skinned_instance`. Nothing runs
  between the pose and the palette, which is exactly where secondary motion
  goes.
- **Physics has no constraint solver for chains.** `crcbl-phys` integrates point
  masses under force providers and does not rotate bodies (see
  [55-water.md](55-water.md)'s "Where this is"). Hair does not need rigid-body
  rotation — a chain is particles and constraints — but it needs a solver, and
  none exists.
- **Alpha-to-coverage needs MSAA, which is off by default**
  ([rendering notes](../notes/rendering.md)), and temporal accumulation is
  refused. Every strand renderer in the research that is not opaque relies on
  one or the other: Unreal's grooms on temporal AA, Horizon Zero Dawn's alpha on
  its two-sample temporal resolve.
- **`mesh.slang` has one material model**, the GGX lobe
  ([44-lighting.md](44-lighting.md)); hair's anisotropic lobes are a second
  model and belong to their own pass.

## The decisions

### 1. Secondary motion is a stage between the pose and the palette

Unreal's AnimDynamics and Kawaii Physics, Unity's Dynamic Bone and Ghost of
Tsushima's hair and horse manes (1D chains in its cloth system) all simulate a
short chain of joints after the animation has posed the character and before
skinning. That is the engine's own seam: `crcbl-anim` gains a **secondary motion
stage** that reads the posed parent joint, steps each chain on the fixed tick,
and writes the chain's joints back into the `Pose` the palette composes. Hair
cards, ponytails, tails, ears, cloaks' tassels and a horse's mane then ride the
skinning path that exists, with no new render pass for the motion.

### 2. The solver is XPBD with small substeps, on the CPU tick

Macklin, Müller and Chentanez's XPBD (2016) makes a constraint's stiffness
independent of the iteration count and the step, which is what Ghost of Tsushima
lacked: its spring solve moved each spring a fraction toward its rest length,
lost time-step invariance, and broke under changed clock speeds and the PS5's
doubled frame rate. "Small Steps in Physics Simulation" (Macklin et al. 2019)
shows many substeps of one iteration beat many iterations of one step.

- **Constraints**: distance (length), bending between consecutive segments, a
  maximum distance from the skinned rest position (Ghost of Tsushima's anchor
  constraint, which stops stretching), and collision against capsules on the
  character's joints.
- **Deterministic**: a chain of tens of particles in fixed order with no FMA
  contraction is bit-identical across targets, so a replay and a server agree —
  which matters for a cape a gameplay raycast can hit, and costs nothing for a
  ponytail that nobody can.
- **Unreal's grooms use XPBD** too, in Niagara, with Cosserat-rod or
  angular-spring models over guides resampled to 4, 8, 16 or 32 points.

### 3. The parent's motion enters as inertia, scaled

Simulating in world space gives full inertia: a character who sprints leaves
their hair behind. Simulating in the parent's space gives none: an elevator's
passengers' hair floats. Kawaii Physics exposes "World Damping Location and
Rotation" as "influence from movement in world coordinate system"; Dynamic
Bone's "Inert" is "how much character's position change is ignored"; TressFX
propagates the root segment's frame-to-frame rotation and translation down the
strand ("velocity shock propagation"), forced fully on above an acceleration
threshold so a teleport does not whip the hair across the room.

So each chain carries an **inertia scale** between zero and one: every tick the
chain's previous positions move with the parent by `(1 − inertia)` of the
parent's displacement, which is the whole trick and costs one vector add per
particle. A teleport threshold resets the chain to the pose.

### 4. Wind is a drag on each segment, relative to the segment's own velocity

Each segment samples [56-wind.md](56-wind.md)'s CPU field and applies drag on
`v_rel = w(x) − v_segment`, so hair moving with the wind feels nothing and hair
held still in it streams. TressFX's form, `−cross(cross(v, w), v)` per segment
`v`, applies only the wind component across the segment, which is what makes a
strand stream rather than stretch; its developer guide recommends varying the
wind cyclically, which the field's travelling gusts already do.

### 5. Five rungs from fur to strands, and the browser tier is the chain

| Rung | Drawn as                                                                                                             | Moved by                                                                                                  | Deterministic   | WebGPU                           |
| ---- | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------- | -------------------------------- |
| H1   | Shells with fins ([57-grass.md](57-grass.md)'s decision 3) over a skinned surface                                    | one damped spring per object on the parent's acceleration plus wind, displacing shells by height          | yes, CPU tick   | yes; overdraw scales with shells |
| H2   | Rigid cards on a stateless wind mesh: pivot at the nearest scalp point, mask by distance from it (God of War)        | a per-instance sway spring                                                                                | yes, CPU tick   | yes                              |
| H3   | Cards skinned to simulated chains                                                                                    | decisions 1–4: XPBD chains in the secondary motion stage                                                  | yes, CPU tick   | yes — the best browser tier      |
| H4   | Guide strands in compute with interpolated follow strands, drawn as camera-facing instanced quads widened to a pixel | TressFX-style shape constraints and length constraints in compute on the tick; velocity shock propagation | no, visual only | yes, at modest counts            |
| H5   | Full strands with a visibility buffer                                                                                | a grid and point simulation (Frostbite)                                                                   | no, visual only | not for the browser              |

**Acerola's fur is H1**, and his repository's motion is not a spring: it
subtracts the movement direction and, with no input, lowers the displacement by
gravity — "rudimentary state blending" in his own words. It is replaced by a
damped spring on the parent's acceleration plus the wind sample, which reads the
same and responds to a turn.

**Frostbite's measured strand costs** (PS4, 900p, no MSAA) set H5's price: long
hair of 10,000 strands and 240,000 points spent 4.7 ms in physics and 4.5 ms
rendering; simulating a tenth of the strands and interpolating was about five
times faster, which is H4's premise.

### 6. Shading is Marschner's lobes, and Kajiya-Kay below it

- **Kajiya-Kay** (Scheuermann, ATI 2004): two shifted specular highlights along
  the tangent and a wrapped diffuse. Its `pow(sinTH, exp)` is a power-of-two
  exponent by repeated squaring, or a non-integer one as `exp_neg` of a
  constructed logarithm, which does not exist yet and is priced against a table
  when a consumer needs it.
- **Marschner R, TT and TRT** as Karis (Unreal 2016) approximates them, and
  Frostbite's LUT of Gaussian fit parameters for the transmission lobe: the
  longitudinal and azimuthal lobes are functions of angle and roughness with
  Gaussians in them, so they are baked into build-time tables where a
  two-variable construction would cost more per fragment than a fetch — the case
  [55-water.md](55-water.md)'s decision 6 keeps tables for. Karis's `cos(φ/2)`
  term is `sqrt((1 + cos φ) / 2)`, which is algebraic.
- **Multiple scattering** is Karis's wrapped Lambert with absorption over a path
  length taken from the shadow; Frostbite's deep opacity maps are a later rung.
- **Hair has its own pass**, lit by a guarded copy of the light walk as
  [57-grass.md](57-grass.md) is.

### 7. Coverage without temporal accumulation

Strands and cards are thinner than a pixel, and every shipped answer that blends
them either sorts (Scheuermann's static inside-to-outside card order with a
depth prime), keeps per-pixel lists (TressFX's order-independent path, which
needs fragment-stage storage writes and per-pixel memory), or leans on temporal
AA. The permitted set, in order of preference:

1. **Opaque cutout, widened to a pixel** — Frostbite's and Jahrmann and Wimmer's
   width correction, so no fragment is sub-pixel and nothing needs blending.
2. **Alpha-to-coverage with sharpening**, on a view that pays for MSAA.
3. **Scheuermann's sorted card passes** for a hero character's cards.
4. **A hashed per-strand `sample_mask`** under MSAA, which decorrelates
   overlapping strands without a temporal resolve.

Per-pixel linked lists are refused for the browser tier and left unscheduled
elsewhere.

## The rungs

Each rung is priced on the desktop adapter, lavapipe and the browser before it
counts as built, per [43-render-standards.md](43-render-standards.md).

| Rung | What it buys                                                                                                                                                | What it costs                                | Needs                                                         |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- | ------------------------------------------------------------- |
| H1   | Shell fur with fins; the parent spring; wind displacement                                                                                                   | overdraw per shell                           | [57-grass.md](57-grass.md) G3; W1 of [56-wind.md](56-wind.md) |
| H2   | Wind-mesh cards with a sway spring; Kajiya-Kay shading pass                                                                                                 | a spring per instance                        | W1                                                            |
| H3   | The secondary motion stage in `crcbl-anim`; XPBD chains with distance, bend, anchor and capsule constraints; inertia scale; teleport reset; chain wind drag | particles × substeps per tick                | H2                                                            |
| H4   | Guide strands in compute, follow strands, pixel-wide instanced quads; Marschner tables                                                                      | compute per tick; a storage buffer per groom | H3                                                            |
| H5   | Full strands with a visibility buffer and deep opacity maps                                                                                                 | Frostbite's figures above                    | H4; native tiers only                                         |

**Refused, with the reason**: dithered or stochastic hair transparency (needs a
temporal resolve); GPU readback of simulated strands for gameplay (visual-only
arithmetic); per-pixel linked lists in the browser (fragment storage and memory
per pixel); a spring that moves a fraction toward rest per step (Ghost of
Tsushima's recorded failure under a changed tick rate — decision 2).

## What each rung is checked by

- **Parent coupling**: a chain on a parent that accelerates trails it by an
  amount that grows as the inertia scale falls, and a chain whose inertia scale
  is zero moves exactly with the parent — the property the whole topic is for.
- **Tick invariance**: the same motion stepped at two tick rates settles to the
  same rest shape within a stated tolerance, against a sabotage that swaps XPBD
  for a fractional spring.
- **Determinism**: two runs of a chain on the CPU are bit-identical.
- **Wind**: a chain at rest in steady wind streams downwind, and a chain moving
  at the wind's velocity hangs as if in still air.
- **Collision**: no particle ends a tick inside a capsule.
- **Goldens** per rung, with claims as relations — the tangent highlight sits
  where the strand direction predicts, and a shell field's silhouette is
  continuous where fins are on.

## Sources

Research brief, 2026-09-15.

- Macklin, Müller, Chentanez — _XPBD: Position-Based Simulation of Compliant
  Constrained Dynamics_, 2016; Macklin et al. — _Small Steps in Physics
  Simulation_, 2019.
- AMD TressFX 4 — `TressFXSimulation.hlsl` and the developer guide.
- Tafuri — _Strand-based Hair Rendering in Frostbite_, Advances 2019.
- Karis — _Physically Based Hair Shading in Unreal_, 2016; Scheuermann — _Hair
  Rendering and Shading_, GDC 2004.
- Rockenbeck — _Blowing from the West_, GDC 2021 (hair and mane chains, the
  fractional-spring failure).
- Feeley — _Interactive Wind and Vegetation in God of War_, Advances 2019 (hair
  as a wind mesh).
- Garrett Gunnell (Acerola) — _How Do Games Render Fur?_ and the
  `Shell-Texturing` repository.
- Lengyel et al. — _Real-Time Fur over Arbitrary Surfaces_, 2001; NVIDIA — _Fur
  (with Shells and Fins)_, 2007.
- Unreal groom physics documentation; Kawaii Physics parameters; Dynamic Bone.

**Recalled, not read**: Unreal's groom cards following their guides; Karis's
exact constants.
