//! The command line: the shared set, plus the one thing this application is
//! about — the file to open.
//!
//! # A positional argument, not a flag
//!
//! `docs/plan/sample/05-viewer.md`'s V-F5 is "path argument natively, drop
//! target in the browser", and the natively half is `viewer model.glb` because
//! that is what a file manager's "open with" and a shell's tab completion both
//! produce. A `--model` flag would be a second spelling of the same fact and
//! neither of those would use it.
//!
//! The first argument that is not a flag is the model; a second one is an error
//! rather than a silently ignored file, because a user who typed two paths
//! meant something this application cannot do.
//!
//! # And it is optional, because there is a shelf
//!
//! `viewer` with no path opens [`crate::shelf`]'s Suzanne — milestone 4's
//! second item, which says in as many words that Suzanne is what opens when
//! nothing is asked for, on both hosts. So [`Options::model`] is an `Option`
//! and `None` is not a placeholder: it is "the shelf's default", resolved
//! where the document is read rather than by inventing a path here that names
//! a file on the machine the binary was built on.

use std::path::PathBuf;

use crcbl::args::{Common, Consumed};

/// The fixed-timestep rate this application does not have.
///
/// [`Common`] requires one because every other sample steps a simulation on it.
/// A viewer steps nothing — it is the sanctioned client-only sample, see
/// [`crate`] — so this is the engine's ordinary 60 and nothing reads it.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How the viewer was asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The `.gltf` or `.glb` to open, or `None` for [`crate::shelf`]'s
    /// default — see the [module docs](self).
    pub model: Option<PathBuf>,
}

/// The `--help` text.
///
/// One literal rather than a `concat!` of [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and this application's own lines, exactly as every other sample spells
/// theirs: help text is read, not parsed, and the alignment is part of it.
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` is what stops the
/// two copies drifting.
pub const USAGE: &str = "\
viewer — open a glTF model, look at it

USAGE:
    viewer [MODEL] [OPTIONS]

ARGUMENTS:
    [MODEL]              The .gltf or .glb file to open. Its own directory
                         becomes the asset root, so a .gltf finds the .bin and
                         the textures beside it. Asset keys hold only letters,
                         digits, '.', '_' and '-', so a file name with a space
                         in it has to be renamed.

                         Left out, the shelf's Suzanne opens. The shelf is the
                         Khronos CC0 models the ESC panel's SHELF row steps
                         through; only Suzanne is in this repository, and
                         tools/fetch-shelf.sh fetches the rest. CRCBL_SHELF
                         moves the directory they are read from.

CONTROLS:
    left drag            Orbit — the model follows the pointer
    middle/right drag    Pan
    wheel                Zoom
    F                    Frame the model again
    I                    Listing panel — what the document holds, and every
                         feature the conversion could not bring in. Off to
                         begin with.
    W                    Wireframe — draw the model's triangles as lines. Off to
                         begin with. Needs a device with a line fill mode, which
                         a browser has not got; the debug panel's 'wireframe' row
                         is what the frame is actually drawn with.
    N                    Normals — colour each surface by its world-space normal
                         instead of shading it, as n * 0.5 + 0.5: +X red, +Y
                         green, +Z blue, and an inverted face reads as the
                         complement of the colour it should have. World space, so
                         a face keeps its colour while you orbit. Off to begin
                         with; the debug panel's 'normals' row says which.
    - / =                Exposure down / up, a third of a stop a press. Held,
                         they keep going. The range is five stops either side of
                         1.00x; the listing panel's 'exposure' row is what the
                         frame is actually drawn with, so it says when an end of
                         that range has been reached.
    ESC                  Open the menu — the only way to reach the two below,
                         or the model shelf, without a keyboard
    F11                  Fullscreen
    F3                   Debug panel

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
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help

This sample simulates nothing, so --tick-hz paces the only thing its tick does:
looking at the document on disk, so a re-export is picked up. The look itself is
on a fixed interval, so the rate a save is noticed at does not move with the
flag. The shared block is printed whole rather than edited, so that what it says
about a flag is what the engine says about it.";

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat argument iterator.
///
/// # Errors
///
/// Reported as [`Invocation::BadUsage`], never as a panic: an unknown flag, a
/// flag with a missing value, and a second model path. **No path at all is not
/// one of them** — that is the shelf's default, see the [module docs](self).
#[must_use]
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut common = Common::new(DEFAULT_TICK_HZ);
    let mut model: Option<PathBuf> = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => {}
        }
        // A leading dash is a flag nobody claimed. Reported as one rather than
        // taken as a path, so `viewer --frame 4 model.glb` names the typo
        // instead of trying to open a file called `--frame`.
        if arg.starts_with('-') {
            return Invocation::BadUsage(format!("unknown argument: {arg}"));
        }
        if model.is_some() {
            return Invocation::BadUsage(format!(
                "this viewer opens one model at a time, and `{arg}` is a second one"
            ));
        }
        model = Some(PathBuf::from(arg));
    }

    Invocation::Run(Options { common, model })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(argv: &[&str]) -> Invocation {
        parse(argv.iter().map(|arg| (*arg).to_string()))
    }

    /// The path reaches the field, and the shared flags still land beside it —
    /// which a parser that consumed its own argument first would break.
    #[test]
    fn the_model_path_and_the_shared_flags_both_arrive() {
        let Invocation::Run(options) = run(&["--headless", "meshes/helmet.glb", "--frames", "4"])
        else {
            panic!("that invocation is a run");
        };
        assert_eq!(options.model, Some(PathBuf::from("meshes/helmet.glb")));
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// **A path is optional, and its absence is not a placeholder**: no
    /// argument at all is a run with nothing named, which is what
    /// `crate::shelf`'s default answers. It used to be a usage error, and the
    /// shelf is what changed.
    #[test]
    fn an_invocation_with_no_model_opens_the_shelf() {
        let Invocation::Run(options) = run(&[]) else {
            panic!("no model is a run now");
        };
        assert_eq!(options.model, None);

        let Invocation::Run(options) = run(&["--headless", "--frames", "2"]) else {
            panic!("flags with no model are a run");
        };
        assert_eq!(options.model, None);
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(2));
    }

    /// A misspelled flag is refused by name rather than opened as a file, and a
    /// second path is refused rather than ignored.
    #[test]
    fn a_bad_flag_and_a_second_path_are_both_refused() {
        let Invocation::BadUsage(message) = run(&["--frame", "4", "model.glb"]) else {
            panic!("`--frame` is not a flag");
        };
        assert!(message.contains("--frame"), "{message}");

        let Invocation::BadUsage(message) = run(&["one.glb", "two.glb"]) else {
            panic!("two models is not a run");
        };
        assert!(message.contains("two.glb"), "{message}");
        assert!(matches!(run(&["--help"]), Invocation::Help));
    }

    /// A file whose name begins with a dash is reachable, which is the escape
    /// every argument parser owes and the one nobody remembers to check.
    #[test]
    fn a_path_that_looks_like_a_flag_is_still_refused_rather_than_guessed() {
        // Not supported, and it says so by name instead of opening something
        // else: this parser has no `--` separator, so a leading dash is always
        // a flag. `./-weird.glb` is the way in, and it parses.
        assert!(matches!(run(&["-weird.glb"]), Invocation::BadUsage(_)));
        let Invocation::Run(options) = run(&["./-weird.glb"]) else {
            panic!("a relative path is a run");
        };
        assert_eq!(options.model, Some(PathBuf::from("./-weird.glb")));
    }

    /// The shared half of the usage text is the engine's, byte for byte.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        assert!(USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP));
        assert!(USAGE.contains(crcbl::args::COMMON_TAIL_HELP));
    }
}
