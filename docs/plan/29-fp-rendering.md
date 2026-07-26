# Topic 29 — First-Person Rendering

The camera/viewmodel layer for first-person games: viewmodel passes, the
one-entity/two-models split, ADS camera mechanics, and picture-in-picture
magnified optics. FPS-project era; specced now because every piece is a
render-graph/camera-pipeline consumer whose seams (data-driven cameras — 18,
sockets — 17, replicated anim/weapon state — 4/17) already exist.

## One entity, two presentations (the structural rule)

A first-person character is **one server entity with two client meshes**:

| Model           | Contents           | Who renders it                                                        | Driven by                                                           |
| --------------- | ------------------ | --------------------------------------------------------------------- | ------------------------------------------------------------------- |
| **World-model** | full body + weapon | everyone else; shadows (always); PiP/mirrors; spectator 3P; kill-cams | replicated anim state (17) — the only truth                         |
| **Viewmodel**   | arms + weapon      | owning client (and 1P spectating) only                                | same replicated weapon/anim state + client-local procedural offsets |

- **Derivability rule (LOCKED)**: the viewmodel pose must be a pure function of
  replicated state + deterministic cosmetics — never client-local secrets. This
  is what makes 1P spectating, kill-cams, and replay POV (topic 22) render a
  _credible_ viewmodel for someone else's session.
- Self-shadow: in 1P, the world-model still renders **depth-only into shadow
  passes** (you see your own shadow, correctly full-bodied) while the viewmodel
  casts nothing (classic rule — viewmodel FOV lies would make its shadow lie
  too).
- Full-body 1P (Tarkov-style: look down, see legs) is a supported mode: render
  the world-model in 1P with head-bone collapsed + camera on the head socket;
  the arms viewmodel then renders only when holding weapons (or not at all —
  per-game choice). CS2-style arms-only is the other mode. Both are
  configurations of the same two-model machinery, not separate systems.

## Viewmodel pass (render graph)

A dedicated pass after world opaque, before transparency:

- **Own projection**: fixed viewmodel FOV (~54°) regardless of world FOV (arms
  proportions immune to the player's FOV setting — competitive standard).
- **Depth-slice trick**: viewmodel depth remapped into a reserved near slice of
  the depth range — always in front of world geometry (arms never clip into
  walls) while self-occluding correctly within the pass. No second depth clear
  needed; the graph declares the range.
- Lit like world content: same sun/CSM sampling at the entity's world position
  (viewmodel receives world shadows — standing in shade looks like shade),
  HDR/post stack applies (18) — it's in the scene, just projected specially.
- Weapon-mounted VFX (muzzle flash, ejected brass) attach to viewmodel sockets
  and render in the viewmodel pass; their light (flash) applies in world space.

## Camera pipeline

- Camera = head/eye **socket** (17) on the world-model skeleton + game offsets;
  camera state (FOV, shake, kick) is component data — replicated where
  gameplay-relevant (ADS state), client-local where cosmetic (bob).
- **ADS**: camera lerps to the weapon's **sight socket** (every weapon declares
  aligned sight sockets); world FOV zooms per-weapon; viewmodel FOV stays fixed;
  transition curves = game data. Sight alignment is socket math, not hand-tuned
  screen offsets — swapping optics re-aligns automatically.
- Recoil/sway/bob: procedural offset layers on the camera + viewmodel (17's
  additive machinery), patterns = game data (28's kick pairs with it). Server
  knows aim direction truth; the cosmetic kick is client presentation reconciled
  by 26's smoothing rules.
- **Ballistic origin rule (LOCKED)**: server truth fires from the **eye trace**
  (26/28 validate against it); tracers/VFX render from the muzzle socket and
  blend onto the true path within a few meters — the classic eye-vs-muzzle
  mismatch resolved by rule: _truth from the eye, theater from the muzzle_.

## PiP magnified optics

- Magnified scope while ADS = **second camera** through the existing data-driven
  camera pipeline (18): narrow zoomed frustum, renders to an offscreen target;
  the scope lens material samples it + reticle overlay, eyebox/parallax shader
  (off-axis blur/cutoff), vignette.
- **Budgeted honestly** — it's a second world render:
  - RTT resolution knob (512–1024², quality setting — topic 14);
  - narrow frustum = its own cull dispatch (cheap — tiny frustum, same
    GPU-driven path) + LOD bias knob (+1 typical, 25);
  - shadow maps reused (same CSMs — no second shadow pass);
  - post stack minimal preset on the PiP camera (tonemap yes, bloom/FXAA
    optional per quality);
  - active only while ADS with a magnified optic; one PiP max.
- Non-magnified sights (red dots/holo) are **shader fakes** (collimator reticle
  at infinity) — no PiP, no cost. The 1×–4× gray zone is a per-optic game
  choice.
- **Relevance interaction** (stated for the anti-wallhack filter): a magnified
  optic extends what the client can legitimately see — the visibility-culling
  filter must use the client's _maximum potential_ FOV/ range (optic-aware), or
  scoped players see enemies pop in. Recorded here so the vis-culling slice
  inherits the requirement.

## Full-sync guarantees: feet, cosmetics, sound (LOCKED)

The two-model split must never become two _truths_. Three guarantees keep 1P,
3P, spectators, and audio all describing the same body:

### One skeleton, one pose

- The world-model skeleton pose (from replicated anim state, 17) is **the**
  pose. Full-body-1P renders it directly (head collapsed for the camera — a
  render detail, not a pose change); the arms viewmodel is a _view_ of the same
  arm chains + weapon state; 3P is the pose unmodified.
- **Feet are therefore identical by construction**: foot placement, stride
  phase, and leg IK (applied to the shared pose _before_ any view renders) are
  the same world-space positions whether you look down in 1P, watch in 3P,
  spectate, or scrub a replay. Looking down at your feet shows exactly what an
  enemy sees — no "1P legs animation set" divergence, ever.
- Pose parity is test-enforced (below), same pattern as viewmodel derivability.

### Cosmetics: declared once, rendered everywhere

- A player's **cosmetic loadout is a replicated component** (entitlement claims
  arrive via topic 27 tokens; the loadout itself is ordinary synced state —
  spectators and replays get it free).
- Each cosmetic slot declares its representations once:
  `CosmeticSlot { world_mesh, viewmodel_mesh?, materials, sockets }` —
  gloves/sleeves define the arms the viewmodel shows, the jacket defines what 3P
  and full-body-1P show, weapon skins apply to both weapon instances by shared
  material reference. One data entry, every view.
- Rule: **there is no view-only cosmetic state** — if it's visible anywhere,
  it's in the loadout component. Kill-cams and spectators render your outfit
  correctly because there is nothing else to know.

### Sound: positions from the body, grammar for everyone

- Every gameplay sound is an event **at a world position derived from the shared
  skeleton**: footsteps fire from anim events (17) at the foot socket's world
  position; gunshots at the muzzle socket; foley (reload, gear rustle) at their
  sockets. Since the pose is shared, the _owner's_ footstep and the _enemy's
  perception_ of that footstep are the same event at the same coordinates — the
  esports audio grammar (13) spatializes it per listener, and a spectator hears
  exactly what proximity dictates.
- Own-sound presentation: the owner hears their own actions through the same
  grammar (distance ≈ 0 → effectively the unprocessed reference, rule 1 of topic
  13 — no special "1P sound set" needed); cosmetic-only local layers (breathing)
  are client-side but positioned on the same sockets, so switching to
  3P/spectator never relocates a sound source.
- Surface-dependent footsteps: the anim event carries the foot socket; the audio
  system resolves surface material from the ground collider under that foot (the
  property block again) — identical resolution for every listener because it
  runs from replicated state.

## Spectating / replay / kill-cam (consumers, mostly free)

1P spectate + kill-cam = render the target's POV from replicated state: eye
socket camera + derived viewmodel (the derivability rule is what makes this
work) + their ADS/PiP state. Replay scrub (22) gets the same. The rewind
visualizer (26) and shot traces (28) overlay in these views — the disputed-kill
review shows what the shooter's screen credibly showed.

## Testing (topic 12)

- Golden frames: viewmodel over world (depth-slice correctness — arms vs wall
  corner cases), ADS transitions at keyframes, PiP scope with reticle (per
  quality tier), full-body-1P look-down.
- Property: viewmodel derivability — render a viewmodel from replicated state
  alone (spectator path) vs owner path → identical pose hash (cosmetic offsets
  seeded).
- Perf: PiP budget recorded (frame cost at each RTT tier, native + Tier B); the
  FPS sample's budget gates include scoped combat.
- Sight alignment: socket-math property — any optic on any weapon centers its
  reticle on the eye trace at ADS rest.
- **Pose parity**: full-body-1P pose hash == world-model pose hash from the same
  replicated state (feet/legs identical across views by test, not intent).
- **Cosmetic parity**: a loadout renders to golden frames in 1P, 3P, and
  spectator views — one data entry, three matching appearances; no view-only
  cosmetic state compiles (schema-level check).
- **Audio-position parity**: footstep/gunshot event positions derived owner-side
  vs spectator-side from the same tick state are identical (socket-position
  hash); surface-material resolution matches for every listener.

## Delivery (FPS-project era)

1. Two-model split + viewmodel pass (depth slice, fixed FOV) + self-shadow rule.
2. Camera pipeline: eye socket, ADS socket lerp, FOV zoom, procedural offset
   layers.
3. Muzzle-vs-eye rule wiring (tracer blend, VFX sockets).
4. PiP camera + scope materials + budgets + relevance requirement handoff.
5. Full-body-1P mode; spectator/kill-cam POV assembly.
6. Golden/property suites above.

## Risks

- **PiP cost on low-end/Tier B**: the knobs exist from day one (res, LOD bias,
  post preset) and the budget is a recorded number, not a hope; worst case =
  magnified optics fall back to zoom-without-PiP (world FOV zoom + overlay) as a
  quality floor — decided per game, mechanism ships.
- **Depth-slice artifacts** (viewmodel vs near-field transparency, particle
  sorting): the pass ordering (after opaque, before transparents) is the
  standard resolution; golden frames pin the corner cases.
- **Derivability erosion** (client-only viewmodel state creeping in): the
  spectator-vs-owner pose-hash property test is the structural guard — same
  pattern as every other "it's a rule because a test enforces it" in this plan.
