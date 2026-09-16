//! The command line: the shared flags every binary in this workspace takes,
//! plus one positional argument.
//!
//! Modelled on `apps/bare`'s parser, which is the smallest one here, with
//! `apps/viewer`'s positional shape: a path is a path, not a flag, because
//! naming the document is what a tool is for.

use crcbl::args::{Common, Consumed};

/// The simulation rate the shared clock is set to.
///
/// The editor ticks nothing — a scene being edited is not a scene being
/// simulated — but the flag is shared and the clock still paces the loop, so
/// the number has to be something. 60, like every other binary here.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The half every binary in this workspace shares.
    pub common: Common,
    /// The `.scn/` directory to open, or [`None`] for the compiled-in scene.
    pub scene: Option<std::path::PathBuf>,
}

/// What [`parse`] hands back.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat argument iterator.
///
/// One positional argument, and a second one is an error rather than a silent
/// replacement of the first: a tool that opened whichever path came last would
/// be one that opened the wrong document on a shell-glob typo.
///
/// # A path that starts with `-` is not reachable, and that is deliberate
///
/// The shared parser is asked first, so anything beginning with a dash is
/// either a flag it knows or an unknown argument. There is no `--` separator
/// because a scene directory whose name starts with a dash is not a thing that
/// happens, and the flag to introduce one would be a flag with no user.
#[must_use]
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut common = Common::new(DEFAULT_TICK_HZ);
    let mut scene = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => {
                if arg.starts_with('-') {
                    return Invocation::BadUsage(format!("unknown argument: {arg}"));
                }
                if let Some(first) = &scene {
                    return Invocation::BadUsage(format!(
                        "one scene directory at a time: already opening {}, then {arg}",
                        std::path::Path::new(first).display(),
                    ));
                }
                scene = Some(std::path::PathBuf::from(arg));
            }
        }
    }

    Invocation::Run(Options { common, scene })
}

/// The `--help` text.
pub const USAGE: &str = "\
editor — the Crucible scene editor

USAGE:
    editor [OPTIONS] [SCENE_DIR]

ARGS:
    <SCENE_DIR>          A .scn/ scene directory to open. Saving writes back
                         over it. Its systems must be ones this build
                         registers; one it does not is refused by name. Without
                         a directory the editor opens the greybox scene compiled
                         into it, which has nowhere to save — Ctrl+S on it says
                         so rather than guessing where to write.

PANELS:
    The scene's entities are listed on the left, grouped by the system whose
    chunk file they came out of, with the selected one's fields under them.
    Selecting a row and picking in the viewport are the same selection. Drag a
    divider to move a panel's edge; where they were left is remembered between
    runs. The rest of the window is the viewport.

EDITING:
    Left click           In the viewport, pick the entity under the cursor; on
                         an outliner row, select what it names
    Ctrl / Shift click   Add a row to the selection, or take a run of them
    Drag a field         Edit it, one step a pixel — an undoable command like
                         every other edit
    Arrow keys           Nudge the selection along X (left/right) and Y
                         (up/down), or walk the rows once a panel has the
                         keyboard
    Page Up / Page Down  Nudge the selection along Z
    Ctrl+Z / Ctrl+Y      Undo and redo. Ctrl+Shift+Z redoes too
    Ctrl+S               Save the scene back over the directory it came from

    A click in the viewport takes the keyboard back from the panels, and none
    of the keys above fire while a field is being typed into.

VIEW:
    Right drag           Orbit (from inside the viewport)
    Middle drag          Pan
    Wheel                Zoom in the viewport, scroll a panel over one
    F                    Frame the whole scene

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
    -h, --help           Print this help";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Invocation {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    /// No argument at all opens the compiled-in scene.
    #[test]
    fn no_positional_argument_opens_the_built_in_scene() {
        let Invocation::Run(options) = run(&[]) else {
            panic!("an empty command line is a run");
        };
        assert_eq!(options.scene, None);
    }

    /// The positional argument is the scene directory, and the shared flags
    /// still parse around it.
    #[test]
    fn a_positional_argument_is_the_scene_directory() {
        let Invocation::Run(options) = run(&["--headless", "scenes/board.scn", "--frames", "4"])
        else {
            panic!("that is a run");
        };
        assert_eq!(
            options.scene.as_deref(),
            Some(std::path::Path::new("scenes/board.scn")),
        );
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// Two directories is a refusal rather than a silent choice of one.
    #[test]
    fn a_second_scene_directory_is_refused_and_names_both() {
        let Invocation::BadUsage(message) = run(&["one.scn", "two.scn"]) else {
            panic!("two documents is not a run");
        };
        assert!(
            message.contains("one.scn") && message.contains("two.scn"),
            "{message}"
        );
    }

    /// Unknown flags are refused by name, and help still prints.
    #[test]
    fn an_unknown_flag_is_refused_by_name_and_help_still_prints() {
        assert!(
            matches!(run(&["--nonsense"]), Invocation::BadUsage(message) if message.contains("nonsense")),
        );
        assert!(matches!(run(&["--help"]), Invocation::Help));
    }

    /// The shared half of the usage text is the engine's, byte for byte — the
    /// same assertion every other binary here makes.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
    }
}
