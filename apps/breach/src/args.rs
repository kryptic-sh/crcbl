//! Argument parsing for the breach sample.
//!
//! ```text
//! breach [--headless] [--frames N] [--size WxH] [--tick-hz N] …
//! ```
//!
//! # There is nothing left here but the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size`, `--screenshot` and the debug-overlay pair, and slice
//! 1 has no flag of its own to add to them: the range is fixed, the pistol is
//! the only weapon, and everything a player can change is a key rather than an
//! argument. This file is still a file rather than a call into the engine's
//! parser, because the usage prose is this sample's and because
//! `docs/backlog.md` records the one flag rule 12 still owes — a way to hold a
//! render path below what the device offers — which is where it will go.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and a
/// `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
breach — a first-person firing range: one hitscan pistol, three lanes

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

/// What the command line asked breach for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
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
///
/// Every argument is offered to the shared set; what comes back as
/// [`Consumed::No`] is the unknown-argument rejection, because breach claims
/// none of its own.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
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
        assert!(
            USAGE.contains("breach — a first-person firing range: one hitscan pistol, three lanes")
        );
    }
}
