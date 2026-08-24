//! Argument parsing for the orbit sample.
//!
//! ```text
//! orbit [--headless] [--frames N] [--tick-hz N] [--backend B]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns every flag orbit has, because orbit has none of
//! its own. There is no `--seed`: nothing in the flight draws a random number —
//! the planet, the rocket and the ascent script are all constants in
//! [`crate::game`] — so two runs of the same length are the same flight, and a
//! seed would be a knob wired to nothing.
//!
//! Still a wrapper rather than a bare alias for `Common`, because the *next*
//! flag orbit grows goes here without changing its callers' types.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and a
/// `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* every block byte for byte — including
/// [`crcbl::args::SCREENSHOT_HELP`], which is spliced where a sample's own flags
/// would go because orbit has wired `--screenshot` up.
pub const USAGE: &str = "\
orbit — the physics pillar's acceptance test, wearing a rocket costume

USAGE:
    orbit [OPTIONS]

OPTIONS:
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
    -h, --help           Print this help

CONTROLS:
    W, left shift        Open the throttle. Held, it keeps opening.
    S, left ctrl         Close it.
    A / D                Turn anticlockwise / clockwise in the orbital plane.
    . / ,                Timewarp one step up / down the ladder. A press, not a
                         held key. Only above the atmosphere with the engine
                         shut down; the flight panel says when.
    Space                Release the launch clamp, or fly again after a landing
                         or a crash.
    ESC                  Pause, and open the menu
    F3                   Debug panel      F11   Fullscreen

The ascent flies itself — a gravity turn and a circularisation burn — until the
first key. After that the rocket is yours and the script does not come back.";

/// What the command line asked orbit for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has, which for orbit is all of them.
    pub common: Common,
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
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            // Orbit claims nothing of its own, so anything the engine hands
            // back is an unknown argument.
            Consumed::No => return Invocation::BadUsage(format!("unknown argument: {arg}")),
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

    /// The engine tests the shared flags; what is orbit's to assert is that this
    /// sample's tick rate reaches them, because it comes from `game.rs` and the
    /// engine cannot know it.
    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run() {
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
        assert!(parsed(&["--debug-overlay"]).common.debug_overlay_visible());
        assert!(
            !parsed(&["--no-debug-overlay"])
                .common
                .debug_overlay_visible()
        );
        assert_eq!(
            parsed(&["--headless"]).common.frame_budget(),
            Some(crcbl::args::HEADLESS_FRAME_BUDGET)
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// Orbit claims no flag of its own, so **every** unknown argument is a
    /// rejection — including one another sample takes. A `--seed` quietly
    /// ignored here would be a run the caller believed was seeded, and this
    /// flight has nothing a seed could reach.
    #[test]
    fn an_argument_this_sample_does_not_claim_is_refused_including_another_samples() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
        assert!(rejected(&["--seed", "17"]).contains("--seed"));
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
        assert!(USAGE.contains("orbit — the physics pillar's acceptance test"));
    }
}
