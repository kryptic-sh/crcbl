//! Argument parsing for the breach sample.
//!
//! ```text
//! breach [--map M] [--headless] [--frames N] [--size WxH] [--tick-hz N] …
//! ```
//!
//! # One flag of its own, and it is which map
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size`, `--screenshot` and the debug-overlay pair. What is
//! breach's is `--map`, because this sample has two of them —
//! `docs/plan/sample/11-breach.md`'s milestone 0 is a firing range **and** a bot
//! practice map — and everything else a player can change is a key rather than
//! an argument.
//!
//! # A page has no argv, so the browser's half is a static
//!
//! [`REQUESTED_MAP`] is `--map` reachable from a browser, and it is the shape
//! `apps/horde`'s `--prefill` already has: `crcbl::impl_web_pending!` opens the
//! sample with `<Options>::default()` and nothing crosses into wasm except
//! through an export, so `crate::web`'s `__crcbl_breach_map` parks the choice
//! here and [`Options::default`] reads it back. A native run parses `--map` onto
//! the options it is already holding and never touches it.
//!
//! `docs/backlog.md` records the one flag rule 12 still owes — a way to hold a
//! render path below what the device offers — which is where it will go.

use std::sync::atomic::{AtomicU8, Ordering};

use crcbl::args::{Common, Consumed};

use crate::map::MapChoice;

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and a
/// `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
breach — first person, one hitscan pistol, two maps: a firing range and a
         bot practice arena

USAGE:
    breach [OPTIONS]

CONTROLS:
    W/A/S/D              Walk, relative to where you are looking
    Mouse                Look. A browser reports no raw pointer motion, so
                         there the arrows are the look instead.
    Arrows               Look: left/right turns, up/down tilts
    SPACE                Fire. One pull is one shot.
    ESC                  Pause, F3 the debug panel, F11 fullscreen

OPTIONS:
    --map <M>            Which map to open: range (the firing range, default) or
                         practice (the bot practice map — cover, three bots on
                         authored patrols, and they shoot back)
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, mtl, metal, dx12, d3d12,
                         null, none, wgpu or webgpu
    --fullscreen         Open borderless instead of windowed. F11 still toggles.
                         A window system may refuse; the summary reports what
                         it actually did, not what was asked for.
    --pacing <P>         How frames are paced against the display: auto, vsync,
                         adaptive or off. Default: auto, which is adaptive sync
                         where the display is running it and vsync where it is
                         not. 'adaptive' is the one to ask for on a VRR panel.
    --fps <N>            Frame limit, in frames a second. Default: 1000, high
                         enough to be a runaway guard rather than a cap. 0 is
                         unlimited. Under vsync the display paces the loop and
                         this rarely fires.
    --size <WxH>         Window size in pixels, WxH (default 960x720). The
                         headless offscreen ring renders at exactly this extent,
                         which is what makes a scale measurement reproducible.
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// Which map a run built from [`Options::default`] opens on.
///
/// **This is the browser's `--map`, and it is a static because a page has no
/// other way in** — see the module docs. Stored as the index into
/// [`MapChoice::ALL`], because that is what an atomic can hold; anything outside
/// it reads back as the default, which is unreachable while
/// [`request_map`] is the only writer.
///
/// `Relaxed` because the value carries nothing with it and is written on the
/// same thread that later reads it: the browser's main thread, before `boot`.
static REQUESTED_MAP: AtomicU8 = AtomicU8::new(0);

/// Asks that the next run built from [`Options::default`] open on `map`.
///
/// Read once, when the options are taken, so it has to be set before the game is
/// built. `crate::web`'s `__crcbl_breach_map` is what enforces that ordering for
/// a page — it refuses once start-up has gone past the point where this would be
/// read — and it is the reason this exists.
pub fn request_map(map: MapChoice) {
    let index = MapChoice::ALL
        .iter()
        .position(|candidate| *candidate == map)
        .unwrap_or(0);
    REQUESTED_MAP.store(index as u8, Ordering::Relaxed);
}

/// What the command line asked breach for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which map to open. See [`MapChoice`].
    pub map: MapChoice,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // `with_screenshot` is what makes `--screenshot` a flag this binary
            // has rather than an unknown argument: `crate::app::assemble` arms
            // the request on the context, and a sample that had not done that
            // must refuse the flag instead of writing nothing.
            #[cfg(not(target_arch = "wasm32"))]
            common: Common::new(crate::game::DEFAULT_TICK_HZ).with_screenshot(),
            #[cfg(target_arch = "wasm32")]
            common: Common::new(crate::game::DEFAULT_TICK_HZ),
            // The range unless a page asked otherwise — see [`REQUESTED_MAP`].
            // A command line sets `map` on the options this returns, so `parse`
            // overwrites whatever lands here.
            map: MapChoice::ALL
                .get(REQUESTED_MAP.load(Ordering::Relaxed) as usize)
                .copied()
                .unwrap_or_default(),
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Every map `--map` will answer to, for the rejection message.
fn map_names() -> String {
    MapChoice::ALL
        .iter()
        .map(|map| map.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; `--map` is the one this
/// sample claims for itself, and what comes back as [`Consumed::No`] after both
/// is the unknown-argument rejection.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => {}
        }
        if arg != "--map" {
            return Invocation::BadUsage(format!("unknown argument: {arg}"));
        }
        let Some(name) = args.next() else {
            return Invocation::BadUsage(format!("--map needs a map: {}", map_names()));
        };
        match MapChoice::from_name(&name) {
            Some(map) => options.map = map,
            None => {
                return Invocation::BadUsage(format!("--map {name} is not a map: {}", map_names()));
            }
        }
    }

    Invocation::Run(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(argv: &[&str]) -> Options {
        match parse(argv.iter().map(|s| (*s).to_string())) {
            Invocation::Run(options) => options,
            Invocation::Help => panic!("expected a run, got help"),
            Invocation::BadUsage(message) => panic!("expected a run, got: {message}"),
        }
    }

    fn rejected(argv: &[&str]) -> String {
        match parse(argv.iter().map(|s| (*s).to_string())) {
            Invocation::BadUsage(message) => message,
            _ => panic!("expected a rejection"),
        }
    }

    /// The engine tests the shared flags; what is breach's to assert is that
    /// this sample's default tick rate reaches them, which the engine cannot
    /// know.
    #[test]
    fn the_defaults_are_a_windowed_run_at_this_samples_own_rate() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.common.frame_budget(), None);
        assert_eq!(options.common.backend, None);
    }

    /// The shared flags still work *through this parser*, which is the join the
    /// engine's own tests cannot make: a sample that forgot to call `consume`
    /// would pass every test in `crcbl::args` and reject `--headless`.
    #[test]
    fn the_shared_flags_reach_the_common_set_through_this_parser() {
        assert!(parsed(&["--headless"]).common.headless);
        assert_eq!(parsed(&["--frames", "7"]).common.frames, Some(7));
        assert_eq!(parsed(&["--tick-hz", "30"]).common.tick_hz, 30);
        assert_eq!(
            parsed(&["--backend", "null"]).common.backend,
            Some(crcbl::backend::GpuBackend::Null)
        );
        assert_eq!(
            parsed(&["--size", "640x480"]).common.size,
            Some(crcbl::shell::PhysicalSize::new(640, 480))
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// `--screenshot` is a flag this sample answers to rather than one it
    /// refuses, which is `with_screenshot` on the default `Common` and nothing
    /// else — see [`crcbl::args::Common::can_screenshot`].
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_screenshot_path_is_accepted_and_forces_a_headless_run() {
        let options = parsed(&["--screenshot", "frame.png"]);
        assert_eq!(
            options.common.screenshot.as_deref(),
            Some(std::path::Path::new("frame.png"))
        );
        assert!(
            options.common.headless,
            "a windowed swapchain is not a surface every backend can copy back"
        );
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// **`--map` reaches the simulation, and a name that is not a map is
    /// refused rather than quietly opening the default one.**
    ///
    /// The rejection carries the list, because a flag whose valid values are
    /// only in `--help` is one a reader has to go and look up.
    #[test]
    fn the_map_flag_names_a_map_and_refuses_anything_else() {
        for map in MapChoice::ALL {
            assert_eq!(parsed(&["--map", map.name()]).map, map);
            assert!(
                USAGE.contains(map.name()),
                "{} is not in --help",
                map.name()
            );
        }
        let refused = rejected(&["--map", "carpark"]);
        assert!(refused.contains("carpark"), "{refused}");
        for map in MapChoice::ALL {
            assert!(refused.contains(map.name()), "{refused}");
        }
        assert!(rejected(&["--map"]).contains("--map"));
    }

    /// **A name and the map it spells are the same thing read both ways**,
    /// which is what lets `--map`, the wasm export and the page's `?map=` share
    /// one table instead of three.
    #[test]
    fn every_map_answers_to_its_own_name() {
        for map in MapChoice::ALL {
            assert_eq!(MapChoice::from_name(map.name()), Some(map));
        }
        assert_eq!(MapChoice::from_name("Range"), None, "names are exact");
        assert_eq!(MapChoice::from_name(""), None);
    }

    /// **A run nobody has asked anything of opens on the firing range**, which
    /// is what keeps the demo site's `/demos/breach/` the page it has always
    /// been: `?map=` is what asks for the other one.
    ///
    /// Asserted through the two facts [`REQUESTED_MAP`] is built on — it starts
    /// at zero, and zero is [`MapChoice::ALL`]'s first row — rather than by
    /// reading the static itself. It is process-wide and `request_map` is
    /// allowed to write it, so a test that read it would be a test that raced
    /// every other test in this binary. `apps/horde`'s `REQUESTED_PREFILL` is
    /// left alone for the same reason; what covers the write is the browser
    /// gate, which loads the page with `?map=practice` and reads the map back
    /// off the `[HUD]` line.
    #[test]
    fn a_run_nobody_asked_anything_of_opens_on_the_range() {
        assert_eq!(MapChoice::default(), MapChoice::Range);
        assert_eq!(MapChoice::ALL[0], MapChoice::default());
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        assert!(
            USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP),
            "the shared OPTIONS block has drifted from crcbl::args"
        );
        assert!(
            USAGE.contains(crcbl::args::COMMON_TAIL_HELP),
            "the shared tail has drifted from crcbl::args"
        );
        assert!(
            USAGE.contains(crcbl::args::SCREENSHOT_HELP),
            "the --screenshot block has drifted from crcbl::args"
        );
        assert!(USAGE.contains("breach — first person, one hitscan pistol, two maps"));
    }
}
