//! The sun on its scripted clock: a pure function of a tick count.
//!
//! ```text
//!  tick ──▶ phase ──▶ (elevation, azimuth) ──▶ towards() ──▶ DirectionalLight
//!             │
//!  keys ──────┤  (pause, scrub, reset)
//!             │
//!  page ──────┘  ask_tick / ask_running / ask_reset ──▶ adopted by advance()
//! ```
//!
//! # Ticks, never a wall clock
//!
//! `docs/plan/sample/18-sundial.md` asks for "a sun on a scripted clock,
//! pausable, scrubbable", and its exit criteria ask for a scripted sun sweep
//! that "runs as a determinism check, not merely as a demo". Both of those want
//! the same thing: the sun at tick `k` is the same sun on every machine, in
//! every process, at every frame rate. So [`Sky::at`] takes a **tick count** and
//! reads no clock — `apps/lantern`'s lamp takes seconds off `Gpu::elapsed`, and
//! a fixture whose sun did that could not be asked to draw tick 40 twice.
//!
//! The fixture's own loop advances the count once per fixed step, which is what
//! makes it simulated time rather than real time: a run at `--tick-hz 5` sweeps
//! the sun at a twelfth of the speed and draws exactly the same frames.
//!
//! # The arc, and why the two halves are different shapes
//!
//! One sweep is [`SWEEP_TICKS`] long and wraps. Across it the sun's **azimuth**
//! runs straight from one end of its range to the other and its **elevation**
//! follows a sine arc, high in the middle:
//!
//! * A linear azimuth is what makes the shadows *rotate* at a constant rate,
//!   which is the motion `docs/plan/sample/18-sundial.md`'s "the edges do not
//!   swim" claim is read against.
//! * A sine elevation is what puts a grazing sun at both ends of the sweep and a
//!   high one in the middle, so one pass of the clock visits the whole range of
//!   shadow lengths the scene was laid out for.
//!
//! **The elevation never reaches the horizon.** [`MIN_ELEVATION`] is where the
//! arc bottoms out, and it is a few degrees above it: a sun at zero elevation
//! casts shadows of unbounded length, so the frame would be uniformly shadowed
//! and every claim below would be about two dark readings.
//!
//! # Which tick the fixture is at
//!
//! [`FIXTURE_TICK`], and it is not zero — [`GRAZING_TICK`] is. The pose the
//! goldens are taken from wants a sun low enough that acne would show and high
//! enough that most of the plaza is still lit; the sweep's own bottom is neither.
//! So the run *starts* at [`FIXTURE_TICK`], which is what `--sun-tick`'s default
//! is, and the sweep's grazing end is a tick a golden asks for by name.

use crcbl::math::Vec3;
use crcbl::render::DirectionalLight;

/// How many ticks one sweep of the sun takes.
///
/// Ten seconds at [`crate::DEFAULT_TICK_HZ`], which is slow enough to read as a
/// sun crossing a sky rather than a strobe and short enough that a person
/// watching a windowed run sees the whole arc without waiting.
pub const SWEEP_TICKS: u64 = 600;

/// The lowest the sun gets, in radians above the horizon.
///
/// Ten degrees. See this module's header: the arc stops here rather than at the
/// horizon because a sun at zero elevation casts shadows of unbounded length.
pub const MIN_ELEVATION: f32 = 10.0 * core::f32::consts::PI / 180.0;

/// The highest it gets, at the middle of the sweep.
///
/// Fifty-five degrees, which is a shadow a little under three quarters of its
/// caster's height — short enough that the counters'
/// [`crate::plaza::COUNTERS`] shadows do not run into each other and each is
/// still a shadow with a penumbra rather than a smudge under the caster.
pub const MAX_ELEVATION: f32 = 55.0 * core::f32::consts::PI / 180.0;

/// How far either side of the plaza's axis the sun swings, in radians.
///
/// Twenty-two degrees each way. Wider and the colonnade's shadows leave the
/// pavement at the ends of the sweep; narrower and the rotation the stability
/// claim is read against is too small to see.
pub const AZIMUTH_SWEEP: f32 = 22.0 * core::f32::consts::PI / 180.0;

/// The tick the fixed camera's goldens are taken at.
///
/// A tenth of the way into the sweep: elevation a little under twenty-four
/// degrees, which is the grazing regime where shadow acne shows on a large flat
/// plane, with the plaza still mostly lit. `the_fixture_sun_is_a_grazing_one` is
/// what holds it there rather than this sentence.
pub const FIXTURE_TICK: u64 = 60;

/// The tick the acne golden is taken at: the bottom of the arc.
///
/// [`MIN_ELEVATION`] exactly, which is the most grazing sun this clock ever
/// reaches and therefore the worst case for both of the artefacts a bias trades
/// against — acne where the offset is too small and peter-panning where it is
/// too large.
pub const GRAZING_TICK: u64 = 0;

/// The tick the sweep is highest at.
///
/// Half a sweep, where the sine arc peaks at [`MAX_ELEVATION`] and the azimuth
/// is square down the plaza's own axis.
pub const NOON_TICK: u64 = SWEEP_TICKS / 2;

/// How many ticks one press of the scrub keys moves the clock.
///
/// A sixtieth of the sweep, so ten presses walk a sixth of the arc — coarse
/// enough to see the shadows move and fine enough to stop on a pose.
pub const SCRUB_STEP: u64 = SWEEP_TICKS / 60;

/// How bright the sun is, before its colour.
///
/// Above one, like every other sun in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back.
///
/// **Set by the goldens' control point rather than by taste.** Open pavement is
/// what every darkness claim in `tests/golden.rs` is read against, and a control
/// that sits at 255 out of 255 is a control which cannot answer: it reads the
/// same whether the frame is correct or twice as bright, so an exposure that ran
/// away would darken nothing and be caught by nothing. Turned down until
/// `crate::plaza::OPEN_PAVEMENT` lands clear of the top of the range at the top
/// of the sun's arc, which is the brightest the fixture ever gets.
const INTENSITY: f32 = 1.4;

/// The sun's colour at the top of its arc.
const COLOR: Vec3 = Vec3::new(1.0, 0.97, 0.92);

/// The flat ambient term, and the whole of what lights a shadowed surface.
///
/// Small next to the sun, which is what makes a shadow legible: a plaza whose
/// ambient came close to its direct light would draw shadows a reader has to
/// look for. Large enough that a shadowed surface is not black — a shadow at
/// zero is one no filter can be compared inside.
const AMBIENT: Vec3 = Vec3::new(0.085, 0.093, 0.112);

/// Where the sun stands: an elevation above the horizon and an azimuth about the
/// plaza's axis.
///
/// A value rather than a bare vector so the panel and the `[HUD]` line can print
/// the two angles a person setting a sundial thinks in, and so
/// [`Sky::towards`] is the one place the two become a direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// Which tick of the clock this is.
    pub tick: u64,
    /// How far above the horizon the sun stands, in radians.
    pub elevation: f32,
    /// How far the sun stands off the plaza's `-Z` axis, in radians, positive
    /// towards `+X`.
    pub azimuth: f32,
}

impl Sky {
    /// The sun at `tick` of the clock.
    ///
    /// A pure function, and the whole of the script: no state, no clock, and the
    /// same answer in every process. Ticks past [`SWEEP_TICKS`] wrap, so a run
    /// left going overnight is a run that keeps drawing the same sweep.
    #[must_use]
    pub fn at(tick: u64) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tick index inside one sweep is a few hundred"
        )]
        let phase = (tick % SWEEP_TICKS) as f32 / SWEEP_TICKS as f32;
        let arc = core::f32::consts::PI * phase;
        Self {
            tick,
            elevation: MIN_ELEVATION + (MAX_ELEVATION - MIN_ELEVATION) * arc.sin(),
            // Linear in the phase rather than in the arc's cosine: see this
            // module's header. `1 - 2 * phase` runs from `+1` to `-1`.
            azimuth: AZIMUTH_SWEEP * 2.0f32.mul_add(-phase, 1.0),
        }
    }

    /// The unit vector **towards** the sun.
    ///
    /// [`DirectionalLight::direction`]'s convention, and the same one
    /// [`crcbl::render::Cascades::new`] takes. The azimuth is measured about `+Y`
    /// from the plaza's forward axis, which is `-Z` — Godot-style axes, as every
    /// scene in this workspace uses.
    #[must_use]
    pub fn towards(self) -> Vec3 {
        let (sin_elevation, cos_elevation) = self.elevation.sin_cos();
        let (sin_azimuth, cos_azimuth) = self.azimuth.sin_cos();
        Vec3::new(
            sin_azimuth * cos_elevation,
            sin_elevation,
            -cos_azimuth * cos_elevation,
        )
    }

    /// How long a shadow this sun throws, per metre of its caster's height.
    ///
    /// `1 / tan(elevation)`, which is what the plaza's constants are laid out
    /// against — [`crate::plaza::COUNTERS`]' spacing is the length of the tallest
    /// one's shadow at [`FIXTURE_TICK`], and a test rather than a comment is what
    /// holds it there.
    #[must_use]
    pub fn shadow_reach(self) -> f32 {
        self.elevation.tan().recip()
    }

    /// The light itself, as [`crcbl::render::ForwardRenderer::begin_frame`] takes
    /// it.
    ///
    /// The colour is constant across the sweep. A sun that reddened at the ends
    /// of its arc would be prettier and would put a second variable into every
    /// reading a golden takes — this fixture's subject is the shadow, so the
    /// light that casts it holds still in every way but its direction.
    #[must_use]
    pub fn light(self) -> DirectionalLight {
        DirectionalLight {
            direction: self.towards(),
            color: COLOR * INTENSITY,
            ambient: AMBIENT,
        }
    }

    /// What the panel and the `[HUD]` line call this pose.
    #[must_use]
    pub fn row(self) -> String {
        format!(
            "tick {} of {SWEEP_TICKS}, {:.1}° up, {:+.1}° across",
            self.tick,
            self.elevation.to_degrees(),
            self.azimuth.to_degrees(),
        )
    }
}

/// The clock itself: which tick the sun is at, and whether it is running.
///
/// The simulation state this fixture has, and it is a tick counter.
/// `crate::filter` is the other half of what a run can change and it lives in the
/// console's own cells; this does not, because a tick is not a setting — it is
/// where the simulation has got to. It lives on `crate::app::Sundial`, and
/// [`page_clock`] is the only other way to reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Which tick the sun is at.
    tick: u64,
    /// Whether [`Clock::advance`] moves it.
    running: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            tick: FIXTURE_TICK,
            running: true,
        }
    }
}

impl Clock {
    /// A clock stopped at `tick`, or running from it.
    #[must_use]
    pub const fn at(tick: u64, running: bool) -> Self {
        Self { tick, running }
    }

    /// Which tick the sun is at.
    #[must_use]
    pub const fn tick(self) -> u64 {
        self.tick
    }

    /// Whether the clock is running.
    #[must_use]
    pub const fn running(self) -> bool {
        self.running
    }

    /// The sun this clock is showing.
    #[must_use]
    pub fn sky(self) -> Sky {
        Sky::at(self.tick)
    }

    /// One fixed step, and the one place a page's request is taken up.
    ///
    /// The step is a no-op while the clock is stopped, which is what makes
    /// pausing the sun different from pausing the loop: the camera still flies.
    ///
    /// **A request from [`ask_tick`], [`ask_running`] or [`ask_reset`] is
    /// adopted first, whether or not the clock is running**, and where this
    /// step got to is published back for [`page_clock`] to read — a browser has
    /// no key to press, and this is the one method the fixed step already calls
    /// every tick.
    pub fn advance(&mut self) {
        let mut page = page();
        if let Some(asked) = page.asked.take() {
            *self = asked;
        }
        if self.running {
            self.tick = self.tick.wrapping_add(1);
        }
        page.seen = *self;
    }

    /// Starts or stops the clock.
    pub const fn toggle(&mut self) {
        self.running = !self.running;
    }

    /// Moves the clock one [`SCRUB_STEP`] forward or back, and stops it.
    ///
    /// **Scrubbing pauses**, which is the behaviour a person walking the sun to
    /// a pose wants: a scrub on a running clock would be a nudge the next tick
    /// undoes.
    pub const fn scrub(&mut self, forward: bool) {
        self.running = false;
        self.tick = if forward {
            self.tick.wrapping_add(SCRUB_STEP)
        } else {
            self.tick.wrapping_sub(SCRUB_STEP)
        };
    }

    /// Back to [`FIXTURE_TICK`], running.
    pub const fn reset(&mut self) {
        *self = Self {
            tick: FIXTURE_TICK,
            running: true,
        };
    }
}

// ---------------------------------------------------------------------------
// The channel a page drives the clock through
// ---------------------------------------------------------------------------

/// What a page has asked the clock for, and where the clock has got to.
///
/// # Why the clock needs one and the filter does not
///
/// [`crate::filter`]'s knobs are console variables: a browser export writes the
/// same cell a key and a typed line write, so there is one copy of the state and
/// no channel is needed. **A tick is not a setting.** It lives on
/// `crate::app::Sundial`, the fixed step is its only writer, and a page reaching
/// it has nothing to write to — which is why `crate::web`'s sun exports go
/// through this and the filter exports go straight at the console.
///
/// This is [`crcbl::debug_view`]'s shape and it is the smallest thing that
/// works: one cell, written by whoever is driving, read back by whoever draws.
/// [`Clock::advance`] is both ends of it — it takes `asked` up on the next fixed
/// step and leaves `seen` behind — so the sun still moves on the fixed step and
/// on nothing else, and the determinism claim
/// `the_clock_is_a_pure_function_of_its_tick` makes is untouched: a page moves
/// *which* tick is drawn, never what tick `k` looks like.
///
/// A [`std::sync::Mutex`] rather than a pair of atomics, so a request is one
/// indivisible `(tick, running)` rather than two stores a reader can land
/// between. It is taken once per fixed step and by nothing else.
#[derive(Clone, Copy, Debug)]
struct Page {
    /// Where the last fixed step left the clock.
    seen: Clock,
    /// What a page has asked for and no step has adopted yet.
    asked: Option<Clock>,
}

/// The one cell of it. See [`Page`].
static PAGE: std::sync::Mutex<Page> = std::sync::Mutex::new(Page {
    seen: Clock::at(FIXTURE_TICK, true),
    asked: None,
});

/// [`PAGE`], with a poisoned lock taken anyway.
///
/// A panic while this is held would have to come from `Option::take` or a `u64`
/// add, so a poisoned lock here means the process is already over; refusing to
/// answer would turn that into a second, less legible failure inside the export
/// a page called.
fn page() -> std::sync::MutexGuard<'static, Page> {
    PAGE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The clock as a page sees it: the request it made, or where the run has got
/// to.
///
/// The request wins while one is outstanding, so a control reads back what it
/// just asked for rather than the tick the last step happened to leave behind —
/// which on a stopped clock is the same value and on a running one is a flicker
/// backwards.
#[must_use]
pub fn page_clock() -> Clock {
    let page = page();
    page.asked.unwrap_or(page.seen)
}

/// Puts the sun at `tick`, **and stops the clock**, from the next fixed step.
///
/// Stopping is [`Clock::scrub`]'s rule rather than a second one: a tick written
/// onto a running clock is a position the next step moves off, so a page's
/// slider would fight the sun it is trying to place.
pub fn ask_tick(tick: u64) -> Clock {
    let mut page = page();
    let asked = Clock::at(tick, false);
    page.asked = Some(asked);
    asked
}

/// Starts or stops the clock from the next fixed step, leaving the tick alone.
pub fn ask_running(running: bool) -> Clock {
    let mut page = page();
    let asked = Clock::at(page.asked.unwrap_or(page.seen).tick, running);
    page.asked = Some(asked);
    asked
}

/// Back to [`FIXTURE_TICK`], running — [`Clock::reset`] asked for from a page.
pub fn ask_reset() -> Clock {
    let mut page = page();
    let mut asked = page.seen;
    asked.reset();
    page.asked = Some(asked);
    asked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises every check that drives [`PAGE`], and empties it afterwards.
    ///
    /// **Every check that calls [`Clock::advance`] takes it, and that is the
    /// point.** The cell is process-global by design and `cargo test` runs a
    /// crate's tests as threads of one process, so two checks that ask the clock
    /// for something are two writers to one cell — which shows up as a flake
    /// rather than as a failure anybody can read. `crate::filter`'s own `Held` is
    /// the same shape for the same reason.
    struct Held {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    /// What an empty channel holds: the pose a fresh run opens on, and nothing
    /// asked for.
    fn empty() -> Page {
        Page {
            seen: Clock::default(),
            asked: None,
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            *page() = empty();
        }
    }

    /// The lock [`Held`] takes.
    static CLOCK_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn held() -> Held {
        let guard = CLOCK_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *page() = empty();
        Held { _guard: guard }
    }

    /// **A page can stop the sun, place it and put it back**, and every request
    /// reaches the clock through the fixed step rather than around it.
    ///
    /// The four claims, in order: a step publishes where it got to; a request is
    /// read back the instant it is made, before any step has run; a request is
    /// adopted **while the clock is stopped**, which is the state a page places a
    /// tick from; and a reset leaves the fixture pose running again.
    ///
    /// The last clause of the third is what makes this more than a getter pair: a
    /// channel whose requests were only taken up by a *running* clock would place
    /// nothing at all, because [`ask_tick`] stops it.
    #[test]
    fn a_page_can_stop_the_sun_place_it_and_put_it_back() {
        let _held = held();
        let mut clock = Clock::default();
        clock.advance();
        assert_eq!(page_clock(), clock, "a step publishes where it got to");

        assert!(!ask_running(false).running());
        assert!(!page_clock().running(), "a page reads its own request back");
        assert!(clock.running(), "and no step has run yet, so nothing moved");
        let stopped_at = clock.tick();
        clock.advance();
        assert_eq!(clock.tick(), stopped_at, "the step adopted the stop");
        assert_eq!(page_clock(), clock);

        let asked = ask_tick(NOON_TICK);
        assert_eq!(asked.tick(), NOON_TICK);
        assert!(!asked.running(), "placing a tick stops the clock");
        clock.advance();
        assert_eq!(
            clock.tick(),
            NOON_TICK,
            "a stopped clock must still adopt what the page placed"
        );
        assert_eq!(page_clock(), clock);
        clock.advance();
        assert_eq!(clock.tick(), NOON_TICK, "and the request is taken up once");

        assert_eq!(ask_reset(), Clock::default());
        clock.advance();
        assert_eq!(
            clock.tick(),
            FIXTURE_TICK + 1,
            "a reset run is a running one"
        );
        assert!(clock.running());
    }

    /// **The sun at a tick is the same sun every time**, and a different one at a
    /// different tick.
    ///
    /// The host half of the determinism claim
    /// `docs/plan/sample/18-sundial.md`'s exit criteria ask for; the GPU half is
    /// `tests/golden.rs`'s `the_scripted_sweep_redraws_byte_for_byte`, which
    /// checks that the frames follow. Without the second clause this passes for
    /// a clock that never moves.
    #[test]
    fn the_clock_is_a_pure_function_of_its_tick() {
        for tick in [0u64, 1, FIXTURE_TICK, NOON_TICK, SWEEP_TICKS - 1] {
            assert_eq!(Sky::at(tick), Sky::at(tick), "tick {tick}");
            assert_eq!(
                Sky::at(tick).towards(),
                Sky::at(tick + SWEEP_TICKS).towards(),
                "the sweep must wrap at {SWEEP_TICKS}"
            );
        }
        assert_ne!(
            Sky::at(FIXTURE_TICK).towards(),
            Sky::at(GRAZING_TICK).towards(),
            "the fixture and grazing poses are the same sun, so nothing moved"
        );
        assert_ne!(
            Sky::at(FIXTURE_TICK).towards(),
            Sky::at(NOON_TICK).towards(),
            "the sun does not move between the fixture pose and the top of its arc"
        );
    }

    /// **The sun stays above the horizon across the whole sweep**, and reaches
    /// both ends of its declared range.
    ///
    /// A sun that dipped under would light nothing, and every claim about a
    /// shadow would be a claim about two ambient readings —
    /// `apps/alcove/src/court.rs`'s crease test guards the same thing for a
    /// fixed sun.
    #[test]
    fn the_sun_stays_above_the_horizon_and_sweeps_its_whole_range() {
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        let (mut leftmost, mut rightmost) = (f32::MAX, f32::MIN);
        for tick in 0..SWEEP_TICKS {
            let sky = Sky::at(tick);
            assert!(
                sky.towards().y > 0.0,
                "at tick {tick} the sun points at {:?}, which is below the horizon",
                sky.towards()
            );
            assert!(
                (sky.towards().length() - 1.0).abs() < 1e-5,
                "at tick {tick} the sun's direction is not a unit vector"
            );
            lowest = lowest.min(sky.elevation);
            highest = highest.max(sky.elevation);
            leftmost = leftmost.min(sky.azimuth);
            rightmost = rightmost.max(sky.azimuth);
        }
        assert!((lowest - MIN_ELEVATION).abs() < 1e-5, "lowest {lowest}");
        assert!(
            highest > MAX_ELEVATION - 1e-3,
            "the arc peaks at {highest} and declares {MAX_ELEVATION}"
        );
        assert!(rightmost > AZIMUTH_SWEEP - 1e-5, "rightmost {rightmost}");
        // One tick short of the far end, because the sweep is a cycle: tick
        // `SWEEP_TICKS` is tick 0 again, so the last tick drawn stops a step
        // before `-AZIMUTH_SWEEP` and never stands on it.
        #[expect(
            clippy::cast_precision_loss,
            reason = "SWEEP_TICKS is six hundred and the step is only a tolerance"
        )]
        let step = 2.0 * AZIMUTH_SWEEP / SWEEP_TICKS as f32;
        // The two sides reach the same angle by different routes — one through
        // `Sky::at`'s phase, one through the step above — and land about 1e-8
        // rad apart, so the comparison is on the step and not on the last bit.
        const SLOP: f32 = 1e-6;
        assert!(
            (leftmost + AZIMUTH_SWEEP).abs() <= step + SLOP,
            "the sweep stops at {leftmost} rad, more than one {step}-rad step short of the \
             {AZIMUTH_SWEEP} it declares"
        );
    }

    /// **The fixture pose is a grazing sun**, which is what puts acne where a
    /// golden can see it, and the grazing pose is lower still.
    ///
    /// The numbers rather than the words: a fixture tick that drifted to the top
    /// of the arc would draw a pleasant frame in which no bias artefact could
    /// appear, and nothing else in this crate would notice.
    #[test]
    fn the_fixture_sun_is_a_grazing_one() {
        /// The steepest sun the fixture pose may stand at, in degrees. Past
        /// thirty a shadow is shorter than its caster is tall and the grazing
        /// regime this fixture is about is gone.
        const GRAZING_CEILING: f32 = 30.0;

        let fixture = Sky::at(FIXTURE_TICK);
        let grazing = Sky::at(GRAZING_TICK);
        assert!(
            fixture.elevation.to_degrees() < GRAZING_CEILING,
            "the fixture sun stands {:.2}° up, past the {GRAZING_CEILING}° a grazing pose means",
            fixture.elevation.to_degrees()
        );
        assert!(
            grazing.elevation < fixture.elevation,
            "the grazing pose stands {:.2}° up and the fixture pose {:.2}°, so the acne golden \
             is taken at the higher sun of the two",
            grazing.elevation.to_degrees(),
            fixture.elevation.to_degrees()
        );
        assert!(
            fixture.shadow_reach() > 2.0,
            "the fixture sun throws {:.2} m of shadow per metre of caster, which is not a \
             grazing sun",
            fixture.shadow_reach()
        );
    }

    /// **The clock runs, pauses, scrubs and resets**, and a scrub stops it.
    #[test]
    fn the_clock_runs_pauses_scrubs_and_resets() {
        let _held = held();
        let mut clock = Clock::default();
        assert_eq!(clock.tick(), FIXTURE_TICK);
        assert!(clock.running());

        clock.advance();
        assert_eq!(clock.tick(), FIXTURE_TICK + 1, "a running clock advances");

        clock.toggle();
        assert!(!clock.running());
        clock.advance();
        assert_eq!(clock.tick(), FIXTURE_TICK + 1, "a stopped clock holds");

        clock.toggle();
        clock.scrub(true);
        assert_eq!(clock.tick(), FIXTURE_TICK + 1 + SCRUB_STEP);
        assert!(!clock.running(), "a scrub stops the clock");
        clock.scrub(false);
        assert_eq!(clock.tick(), FIXTURE_TICK + 1);

        clock.reset();
        assert_eq!(clock.tick(), FIXTURE_TICK);
        assert!(clock.running(), "a reset run is a running one");
    }

    /// **A scrub back from tick zero wraps rather than panicking.**
    ///
    /// The clock's tick is a `u64` and `GRAZING_TICK` is zero, so the first
    /// thing a person scrubbing backwards through the sweep's start does is
    /// subtract past it. `Sky::at` takes the remainder, so a wrapped count is a
    /// pose rather than an arithmetic overflow — in a debug build the
    /// subtraction itself would be the panic.
    #[test]
    fn scrubbing_back_past_the_start_of_the_sweep_wraps() {
        let mut clock = Clock::at(0, false);
        clock.scrub(false);
        assert_eq!(clock.tick(), u64::MAX - (SCRUB_STEP - 1));
        let sky = clock.sky();
        assert!(sky.towards().y > 0.0, "{:?}", sky.towards());
        assert!(sky.row().contains("across"), "{}", sky.row());
    }

    /// **The row names the tick and both angles**, which is what a person
    /// reading a headless log has instead of a picture.
    #[test]
    fn the_row_names_the_tick_and_both_angles() {
        let row = Sky::at(FIXTURE_TICK).row();
        assert!(row.contains(&format!("tick {FIXTURE_TICK}")), "{row}");
        assert!(row.contains("up"), "{row}");
        assert!(row.contains("across"), "{row}");
    }
}
