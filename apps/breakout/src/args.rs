//! Argument parsing for the breakout sample.
//!
//! ```text
//! breakout [--headless] [--frames N] [--tick-hz N] [--backend B] [--scene DIR]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns all but one of breakout's flags, and `--scene`
//! is the one that is this game's: it names a `.scn/` directory to read the
//! brick grid out of instead of the committed `assets/scenes/board.scn/`. The
//! shape is `apps/lantern/src/args.rs`'s `--stack` — the file is read *here*,
//! while there is still an exit code to refuse the run with, and `Options`
//! carries the parsed value rather than the path.

use crcbl::args::{Common, Consumed};

use crate::scene::Board;

/// The `--help` text.
///
/// Cannot drift from [`crcbl::args::COMMON_OPTIONS_HELP`] silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts
/// this string contains both shared blocks byte for byte — and
/// [`crcbl::args::SCREENSHOT_HELP`], which is spliced between them because
/// breakout is the sample that has wired `--screenshot` up.
pub const USAGE: &str = "\
breakout — the first playable Crucible sample

USAGE:
    breakout [OPTIONS]

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
    --set <KEY=VALUE>    Override one setting for this run, the value spelled
                         as in settings.toml: engine.video.shadows=false,
                         game.speed=1.5, text in quotes. Repeatable; the later
                         of two for one key wins. Never saved to the file.
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --scene <DIR>        Read the brick grid from a .scn/ scene directory
                         instead of the committed
                         apps/breakout/assets/scenes/board.scn. DIR is the
                         scene directory itself, the one holding scene.ron. A
                         directory that is not a scene is refused by key, line
                         and column.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// **Not `Eq`.** [`Board`] is a list of world-space coordinates, and a float
/// has no total equality; nothing compares two invocations for anything but a
/// test's `assert_eq!`, which [`PartialEq`] serves.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has, which for breakout is all but one.
    pub common: Common,
    /// The brick layout the run opens with: the committed board unless
    /// `--scene` named another directory.
    pub board: Board,
}

/// The shared half, for [`crcbl::args::run_front_end`].
impl AsRef<Common> for Options {
    fn as_ref(&self) -> &Common {
        &self.common
    }
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
            board: Board::built_in(),
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
            Consumed::No => {}
        }
        match arg.as_str() {
            "--scene" => match args.next() {
                // Refused here rather than fallen back on: a run that quietly
                // kept the built-in board when the directory it was pointed at
                // would not parse is one that drew the picture it always drew
                // and reported nothing.
                Some(path) => match Board::read_dir(&path) {
                    Ok(board) => options.board = board,
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

    /// What is breakout's to assert is that this game's tick rate reaches the
    /// shared set; the flags themselves are tested in `crcbl::args`.
    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.common.frame_budget(), None);
        assert_eq!(options.common.backend, None);
    }

    /// The join the engine's own tests cannot make: a game that forgot to call
    /// `consume` would pass every test in `crcbl::args` and reject `--headless`
    /// here.
    #[test]
    fn the_shared_flags_reach_the_common_set_through_this_parser() {
        assert!(parsed(&["--headless"]).common.headless);
        assert_eq!(parsed(&["--frames", "7"]).common.frames, Some(7));
        assert_eq!(parsed(&["--tick-hz", "30"]).common.tick_hz, 30);
        assert_eq!(
            parsed(&["--backend", "vk"]).common.backend,
            Some(crcbl::backend::GpuBackend::Vulkan)
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
        assert!(rejected(&["--frames", "0"]).contains("frame count"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// Breakout claims one flag of its own, so every **other** unknown
    /// argument is a rejection — including one another sample takes. A `--seed`
    /// silently ignored here would be a run the caller believed was seeded.
    #[test]
    fn an_argument_this_game_does_not_claim_is_refused_including_another_games() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
        assert!(rejected(&["--seed", "17"]).contains("--seed"));
    }

    /// **`--scene` reads a directory at run time, and the board in it reaches
    /// the field.**
    ///
    /// The one-brick scene is what makes that assertable: a parser that
    /// accepted the flag and kept the committed grid would pass any check
    /// that only counted a successful parse.
    #[test]
    fn the_scene_flag_reads_a_directory_and_refuses_one_that_is_not_a_scene() {
        let dir = std::env::temp_dir().join(format!("breakout-scene-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"one\", systems: [\"bricks\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("env.ron"),
            "Env(camera: (position: (0.0, 0.0, -1.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.0, 0.0, 0.0))",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("bricks.ron"),
            "Chunk(system: \"bricks\", entities: [\n    (0, (position: (1.0, 2.0, 0.0), \
             half_extents: (1.2, 0.4, 0.5))),\n])",
        )
        .expect("the temp dir is writable");

        let path = dir.to_str().expect("utf-8");
        let board = parsed(&["--scene", path]).board;
        assert_eq!(
            board.bricks().len(),
            1,
            "the file's board must reach the field"
        );
        assert_eq!(
            board.bricks()[0].position(),
            crcbl::math::DVec3::new(1.0, 2.0, 0.0)
        );

        // A chunk that is not this system's is refused by the file it is in,
        // not absorbed as an empty board.
        std::fs::write(
            dir.join("sys").join("bricks.ron"),
            "Chunk(system: \"walls\", entities: [])",
        )
        .expect("the temp dir is writable");
        let message = rejected(&["--scene", path]);
        assert!(message.contains("sys/bricks.ron"), "{message}");
        assert!(message.contains("walls"), "{message}");

        assert!(
            matches!(
                parse(["--scene".to_string()].into_iter()),
                Invocation::BadUsage(_)
            ),
            "--scene at the end of an argv is a run that silently kept the built-in board"
        );
    }

    /// With no `--scene`, the board is the committed one — the browser's only
    /// path, since a wasm build has no directory to point the flag at.
    #[test]
    fn the_default_board_is_the_committed_scene_directory() {
        assert_eq!(parsed(&[]).board, crate::scene::Board::built_in());
    }

    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        assert!(USAGE.contains("breakout — the first playable Crucible sample"));
    }
}
