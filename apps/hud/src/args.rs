//! Argument parsing for the hud sample.
//!
//! ```text
//! hud [--headless] [--frames N] [--tick-hz N] [--backend B] [--seed N]
//!     [--styles DIR]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size` and the debug-overlay pair, because those are the
//! *engine's* vocabulary. This file is hud's own: its usage prose, its `--seed`
//! and the default seed that goes with it, and `--styles`, the directory the
//! stylesheet is read and re-read from.

use std::path::{Path, PathBuf};

use crcbl::args::{Common, Consumed};
use crcbl::assets::{AssetSource, DirSource};

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and a
/// `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
hud — the UI system's living fixture

USAGE:
    hud [OPTIONS]

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
    --seed <N>           Ticker seed. The same seed is the same damage numbers.
    --styles <DIR>       Read the stylesheet from DIR/hud.css instead of the
                         copy compiled in, and re-read it while the run is
                         live: saving it restyles the HUD. A save that does not
                         parse keeps the last good sheet and shows the error.
                         apps/hud/assets is the committed sheet's directory.
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked hud for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The ticker seed. The same seed is the same damage numbers.
    pub seed: u64,
    /// The directory `--styles` named, whose `hud.css` the HUD is styled by
    /// and polled from; `None` for the copy compiled in.
    pub styles: Option<PathBuf>,
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
            seed: crate::game::DEFAULT_SEED,
            styles: None,
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is hud's to claim, and what hud does not claim either is the
/// unknown-argument rejection.
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
            "--seed" => match crcbl::args::seed_u64(&mut args) {
                Ok(seed) => options.seed = seed,
                Err(message) => return Invocation::BadUsage(message),
            },
            "--styles" => match args.next() {
                Some(dir) => match readable_sheet(&dir) {
                    Ok(dir) => options.styles = Some(dir),
                    Err(message) => return Invocation::BadUsage(message),
                },
                None => return Invocation::BadUsage("--styles needs a value".into()),
            },
            other => return Invocation::BadUsage(format!("unknown argument: {other}")),
        }
    }

    Invocation::Run(options)
}

/// `dir`, if a [`DirSource`] over it can read the stylesheet, or the message
/// to refuse the run with.
///
/// Only that it can be read: a sheet that does not parse is the case the live
/// reload exists for — drawn in the last good sheet, with the error on screen
/// until a save fixes it — so refusing the run over it would refuse the edit
/// loop's first step.
fn readable_sheet(dir: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(dir);
    DirSource::at(root.clone())
        .read(Path::new(crate::sheet::SHEET_KEY))
        .map(|_| root)
        .map_err(|error| format!("--styles {dir}: {error}"))
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

    /// The engine tests the shared flags; what is hud's to assert is that this
    /// sample's defaults reach them — the tick rate and the seed, both of which
    /// come from `game.rs` and neither of which the engine can know.
    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run_on_the_published_seed() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.seed, crate::game::DEFAULT_SEED);
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
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    #[test]
    fn a_seed_can_be_named_so_two_runs_can_be_compared() {
        assert_eq!(parsed(&["--seed", "17"]).seed, 17);
        assert_ne!(parsed(&["--seed", "17"]).seed, parsed(&[]).seed);
        assert!(rejected(&["--seed", "kittens"]).contains("seed"));
    }

    /// **`--styles` names a directory whose sheet can be read**, and one whose
    /// sheet cannot is refused at the command line rather than drawn in the
    /// built-in sheet with nothing said.
    #[test]
    fn the_styles_flag_takes_a_directory_holding_the_sheet_and_refuses_one_without() {
        assert_eq!(parsed(&[]).styles, None, "the built-in sheet by default");

        let dir = std::env::temp_dir().join(format!("hud-styles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temporary directory is made");
        let path = dir.to_str().expect("the temporary directory is UTF-8");
        assert!(
            rejected(&["--styles", path]).contains(crate::sheet::SHEET_KEY),
            "a directory with no sheet in it must be refused, naming the sheet",
        );

        std::fs::write(dir.join(crate::sheet::SHEET_KEY), "#hud { }")
            .expect("the temporary sheet is written");
        assert_eq!(parsed(&["--styles", path]).styles, Some(dir.clone()));
        assert!(rejected(&["--styles"]).contains("--styles needs a value"));

        // A leftover directory in the temporary directory is harmless; a panic
        // here would hide nothing, but a failed clean-up is worth a line.
        if let Err(error) = std::fs::remove_dir_all(&dir) {
            eprintln!("{}: not removed: {error}", dir.display());
        }
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        assert!(USAGE.contains("hud — the UI system's living fixture"));
        for flag in ["--seed", "--styles"] {
            assert!(USAGE.contains(flag), "this sample's own {flag} is missing");
        }
    }
}
