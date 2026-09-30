//! Argument parsing for the towers sample.
//!
//! ```text
//! towers [--headless] [--frames N] [--size WxH] [--tick-hz N] [--scene DIR] …
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size`, `--screenshot` and the debug-overlay pair, and
//! `--scene` is the one flag that is this sample's: it names a `.scn/` directory
//! to read the map out of instead of the committed `assets/scenes/field.scn/`.
//! The shape is `apps/breakout/src/args.rs`'s and `apps/puppet/src/args.rs`'s —
//! the directory is read *here*, while there is still an exit code to refuse the
//! run with, and [`Options`] carries the parsed map rather than the path.
//! Everything else a player can change is a key.
//! `docs/plan/sample/07-towers.md` records the one flag rule 12 still owes
//! every sample, which is a way to hold a render path below what the device
//! offers.

use crcbl::args::{Common, Consumed};

use crate::map::Map;

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and
/// a `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
towers — co-op tower defense; this slice is the solo loop on one map

USAGE:
    towers [OPTIONS]

CONTROLS:
    LEFT/RIGHT           Pick a build plot
    B                    Build a tower on it. The server refuses a plot that is
                         taken and a purse that is short.
    N                    Send the next wave now rather than waiting out the
                         build phase. Waves arrive on their own either way.
    R                    Restart the run
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
    --exec <LINE>        Run a console line before the first frame, after the
                         player's autoexec.cfg. Repeatable; the lines run in
                         the order given. The one way to set a console variable
                         on a --headless run, which reads no autoexec.cfg.
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --scene <DIR>        Read the map from a .scn/ scene directory instead of
                         the committed apps/towers/assets/scenes/field.scn.
                         DIR is the scene directory itself, the one holding
                         scene.ron. A directory that is not a scene is refused
                         by key, line and column, and a layout the field cannot
                         hold is refused by the waypoint, leg or plot at fault.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked towers for.
///
/// **Not `Eq`.** A [`Map`] is a list of world-space coordinates, and a float has
/// no total equality; nothing compares two invocations for anything but a test's
/// `assert_eq!`, which [`PartialEq`] serves.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The field the run is played on: the committed one unless `--scene`
    /// named another directory.
    pub map: Map,
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
            map: Map::built_in(),
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is this sample's own to claim or to reject.
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
        match arg.as_str() {
            "--scene" => match args.next() {
                // Refused here rather than fallen back on: a run that quietly
                // kept the committed field when the directory it was pointed at
                // would not parse is one that played the map it always played
                // and reported nothing.
                Some(path) => match Map::read_dir(&path) {
                    Ok(map) => options.map = map,
                    Err(message) => return Invocation::BadUsage(message),
                },
                None => return Invocation::BadUsage("--scene needs a value".into()),
            },
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
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

    /// The engine tests the shared flags; what is towers' to assert is that
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

    /// **`--scene` reads a directory at run time, and the map in it reaches the
    /// options** — and one that is not a towers map is refused rather than
    /// swapped for the committed field.
    ///
    /// The one-leg scene is what makes that assertable: a parser that accepted
    /// the flag and kept the committed field would pass any check that only
    /// counted a successful parse.
    #[test]
    fn the_scene_flag_reads_a_directory_and_refuses_one_that_is_not_a_map() {
        let dir = std::env::temp_dir().join(format!("towers-scene-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"one\", systems: [\"waypoints\", \"plots\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("env.ron"),
            "Env(camera: (position: (0.0, 32.0, 23.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.0, 0.0, 0.0))",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("waypoints.ron"),
            "Chunk(system: \"waypoints\", entities: [\n\
             (0, (order: 0, position: (-10.0, 0.0, 0.0))),\n\
             (1, (order: 1, position: (10.0, 0.0, 0.0))),\n\
             ])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("plots.ron"),
            "Chunk(system: \"plots\", entities: [(2, (label: \"north\", \
             position: (0.0, 0.0, -3.0)))])",
        )
        .expect("the temp dir is writable");

        let path = dir.to_str().expect("utf-8");
        let map = parsed(&["--scene", path]).map;
        assert_eq!(map.path().legs(), 1, "the file's path must reach the run");
        assert_eq!(map.plots().len(), 1);
        assert_eq!(map.plots()[0].label, "north");

        // A plot moved onto the lane is refused by the rule it breaks, not
        // absorbed as a map with one plot fewer.
        std::fs::write(
            dir.join("sys").join("plots.ron"),
            "Chunk(system: \"plots\", entities: [(2, (label: \"north\", \
             position: (0.0, 0.0, -0.5)))])",
        )
        .expect("the temp dir is writable");
        let message = rejected(&["--scene", path]);
        assert!(message.contains("north"), "{message}");
        assert!(message.contains("lane"), "{message}");

        assert!(
            matches!(
                parse(["--scene".to_string()].into_iter()),
                Invocation::BadUsage(_)
            ),
            "--scene at the end of an argv is a run that silently kept the built-in map"
        );
    }

    /// With no `--scene`, the map is the committed one — the browser's only path,
    /// since a wasm build has no directory to point the flag at.
    #[test]
    fn the_default_map_is_the_committed_scene_directory() {
        assert_eq!(parsed(&[]).map, Map::built_in());
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        assert!(USAGE.contains("towers — co-op tower defense"));
    }
}
