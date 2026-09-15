//! Lantern's command line: the shared set, plus the camera and the two path
//! forcings.
//!
//! The shared half is [`crcbl::args::Common`] verbatim — `--headless`,
//! `--frames`, `--backend`, `--size` and the rest — so the flags this sample
//! adds are the ones a *lighting fixture* has and nothing else.

use crcbl::args::{Common, Consumed, binding_from_name, geometry_from_name};
use crcbl::render::{CameraStack, RenderEffects};

use crate::menu::CameraMode;
use crcbl::engine::ForcedPaths;

/// The simulation rate. Nothing here integrates anything but a camera and a
/// lamp's orbit, so it is the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// The render stack the room's own view draws through, as it is committed.
///
/// `include_str!` rather than a read at run time, for the reason every other
/// asset this sample ships is compiled in: a browser has no filesystem, and a
/// binary that could fail to find its own camera is one whose golden depends on
/// the working directory it was run from. `--stack` is the run-time door, and
/// it is a door onto a *different* file.
const BUILT_IN_STACK_RON: &str = include_str!("../assets/camera.ron");

/// What `BUILT_IN_STACK_RON` — `apps/lantern/assets/camera.ron` — says, parsed.
///
/// # Panics
///
/// If the committed file is not a camera stack, naming the line, the column and
/// the field. It is compiled into this binary, so that is a tree in which
/// `the_committed_stack_is_the_frame_the_room_already_drew` is also red — the
/// panic is what keeps a run from starting on a stack nobody could read.
#[must_use]
pub fn built_in_stack() -> CameraStack {
    CameraStack::from_ron(BUILT_IN_STACK_RON)
        .unwrap_or_else(|error| panic!("apps/lantern/assets/camera.ron: {error}"))
}

/// How lantern was asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which camera the run starts on.
    pub camera: CameraMode,
    /// Which selectors the run asks to be held below the device's own.
    pub forced: ForcedPaths,
    /// Which of topic 18's effects the run draws.
    ///
    /// The charter's "every effect toggles independently", reached from the
    /// command line: each `--no-*` flag clears one bit and the run drives the
    /// **programmatic** layer of the resolution order with what is left. The
    /// other three request layers have their own sources — [`Self::stack`] for
    /// the camera one, the player's settings file for the two below it; see
    /// `crcbl::render::effects`.
    pub effects: RenderEffects,
    /// The **camera** layer of the resolution order for the room's own view:
    /// the render stack it asks for, read from a file.
    ///
    /// [`built_in_stack`] unless `--stack` named another one. The monitor's
    /// view keeps `crate::room::MONITOR_STACK`, which is a fact about a
    /// render-to-texture camera rather than something a run tunes.
    pub stack: CameraStack,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // `with_screenshot` is what makes `--screenshot` a flag this binary
            // has rather than an unknown argument: `crate::app::assemble` arms
            // the request on the context, and a sample that had not done that
            // must refuse the flag instead of writing nothing. lantern wants it
            // for the reason every other sample does and one of its own — the
            // in-scene monitor is fed at the tail of `crate::gpu::Gpu::frame`,
            // so the only picture that has a live screen in it is one this
            // binary presented.
            #[cfg(not(target_arch = "wasm32"))]
            common: Common::new(DEFAULT_TICK_HZ).with_screenshot(),
            #[cfg(target_arch = "wasm32")]
            common: Common::new(DEFAULT_TICK_HZ),
            camera: CameraMode::default(),
            forced: ForcedPaths::default(),
            effects: RenderEffects::all(),
            stack: built_in_stack(),
        }
    }
}

/// The `--help` text.
///
/// One literal rather than a `concat!` of [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and this sample's own flags, exactly as every other sample spells its own:
/// help text is read, not parsed, and the alignment is part of it.
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` is what stops the
/// two copies drifting.
pub const USAGE: &str = "\
lantern — the lighting acceptance fixture: one room, every effect

USAGE:
    lantern [OPTIONS]

Not a game. One indoor scene chosen for lighting rather than for geometry: a
window, a mirror-grade panel, a rough metal block, a coloured wall and a moving
light. The two metals have no ambient term and are lit by reflection alone —
see the debug panel's 'unbuilt' section, and docs/plan/sample/13-lantern.md.

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
    --camera <C>         Which camera to start on: 'fixed' (the pose the goldens
                         are taken from, held still) or 'free' (fly it with
                         WASD, Space/Shift and the arrow keys). Default: fixed.
                         ENTER on the pause menu's CAMERA row swaps them.
    --stack <PATH>       Read the room view's render stack from a RON file
                         instead of the committed apps/lantern/assets/camera.ron.
                         This is the camera layer: it says which passes the view
                         asks for, and the --no-* flags below still clear them
                         on top. A file that does not parse is refused by line
                         and column.
    --force-geometry <P> Require 'mesh-shader', 'indirect-count' or
                         'indirect-per-batch'; unsupported paths fail startup.
                         Default: this device's preferred geometry path.
    --force-binding <B>  Request a 'bindless' or 'array-pages' capability ceiling.
                         This forward renderer always uses array-pages.
    --no-shadows         Draw with no shadow atlas: no cascade cull, no light
                         tile, and every comparison against the cleared atlas
                         reads as fully lit.
    --no-ao              Draw with no ambient occlusion. The ambient term is
                         scaled by the renderer's 1x1 white instead.
    --no-reflections     Draw with no screen-space reflections. The frame is the
                         forward pass's own scene colour, bit for bit.
                         Each of the three has a pause-menu row — SHADOWS, AO
                         and REFLECTIONS — and ENTER toggles it, so a flag is
                         the starting state rather than the only way in. A row
                         reading 'unavailable' is an effect this device clamped
                         off, and pressing it does nothing on purpose.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat argument iterator.
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
            "--camera" => match args.next().as_deref().map(CameraMode::from_name) {
                Some(Some(camera)) => options.camera = camera,
                Some(None) => {
                    return Invocation::BadUsage("unknown camera — try `fixed` or `free`".into());
                }
                None => return Invocation::BadUsage("--camera needs a value".into()),
            },
            "--force-geometry" => match args.next().as_deref().map(geometry_from_name) {
                Some(Some(path)) => options.forced.geometry = Some(path),
                Some(None) => {
                    return Invocation::BadUsage(
                        "unknown geometry path — try `mesh-shader`, `indirect-count` or \
                         `indirect-per-batch`"
                            .into(),
                    );
                }
                None => return Invocation::BadUsage("--force-geometry needs a value".into()),
            },
            "--force-binding" => match args.next().as_deref().map(binding_from_name) {
                Some(Some(model)) => options.forced.binding = Some(model),
                Some(None) => {
                    return Invocation::BadUsage(
                        "unknown binding model — try `bindless` or `array-pages`".into(),
                    );
                }
                None => return Invocation::BadUsage("--force-binding needs a value".into()),
            },
            "--stack" => match args.next() {
                Some(path) => match read_stack(&path) {
                    Ok(stack) => options.stack = stack,
                    Err(message) => return Invocation::BadUsage(message),
                },
                None => return Invocation::BadUsage("--stack needs a value".into()),
            },
            "--no-shadows" => options.effects.remove(RenderEffects::SHADOWS),
            "--no-ao" => options.effects.remove(RenderEffects::AMBIENT_OCCLUSION),
            "--no-reflections" => options.effects.remove(RenderEffects::REFLECTIONS),
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

/// The stack `--stack` names, or the message to refuse the run with.
///
/// Both failures read the same way — the path, then what went wrong with it —
/// because to a person fixing it "no such file" and "line 3, column 5" are the
/// same kind of answer about the same argument.
fn read_stack(path: &str) -> Result<CameraStack, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    CameraStack::from_ron(&text).map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use crcbl::hal::{BindingModel, GeometryPath};

    use super::*;

    fn run(argv: &[&str]) -> Invocation {
        parse(argv.iter().map(|arg| (*arg).to_string()))
    }

    /// The defaults, and what a bare invocation means.
    #[test]
    fn a_bare_invocation_is_the_golden_pose_on_the_devices_own_paths() {
        let Invocation::Run(options) = run(&[]) else {
            panic!("no arguments is a run");
        };
        assert_eq!(options.camera, CameraMode::Fixed);
        assert_eq!(options.forced, ForcedPaths::default());
        assert_eq!(options.common.tick_hz, DEFAULT_TICK_HZ);
        assert_eq!(
            options.effects,
            RenderEffects::all(),
            "a bare run is the every-effect frame the golden is blessed from"
        );
        assert_eq!(
            options.stack,
            built_in_stack(),
            "a bare run draws through the committed camera file"
        );
    }

    /// **The committed `assets/camera.ron` is the stack the room was already
    /// drawing**, bit for bit.
    ///
    /// The whole safety of turning a constant into a file: `room::View::Main`'s
    /// stack is what every golden in `apps/lantern/tests` was blessed from, and
    /// a file that compiled to anything else would move all of them at once
    /// while still parsing, still opening and still drawing a plausible room.
    /// This is what makes the file a second spelling of that constant rather
    /// than a second opinion about it.
    #[test]
    fn the_committed_stack_is_the_frame_the_room_already_drew() {
        assert_eq!(built_in_stack().compile(), crate::room::View::Main.stack());
    }

    /// **`--stack` reads a file at run time, and a file that is not a stack is
    /// refused the way every other bad value is.**
    ///
    /// The refusal is the half worth asserting: a sample that fell back to the
    /// built-in stack when the file it was pointed at did not parse would draw
    /// the frame it always drew and report nothing, which is an A/B measurement
    /// silently comparing a stack against itself.
    #[test]
    fn the_stack_flag_reads_a_file_and_refuses_one_that_is_not_a_stack() {
        let dir = std::env::temp_dir();
        let good = dir.join(format!("lantern-stack-{}.ron", std::process::id()));
        let bad = dir.join(format!("lantern-stack-bad-{}.ron", std::process::id()));
        std::fs::write(&good, "(bloom: Some(()))").expect("the temp dir is writable");
        std::fs::write(&bad, "(\n    shadows: Some(()),\n    smaa: Some(()),\n)")
            .expect("the temp dir is writable");

        let Invocation::Run(options) = run(&["--stack", good.to_str().expect("utf-8")]) else {
            panic!("--stack with a readable stack is a run");
        };
        assert_eq!(
            options.stack.compile(),
            crcbl::render::RenderEffects::BLOOM,
            "the file's stack has to reach the field, not the built-in one"
        );

        let Invocation::BadUsage(message) = run(&["--stack", bad.to_str().expect("utf-8")]) else {
            panic!("a file that is not a stack is a bad usage");
        };
        assert!(message.contains("line 3, column 5"), "{message}");
        assert!(message.contains("smaa"), "{message}");

        assert!(
            matches!(run(&["--stack"]), Invocation::BadUsage(_)),
            "--stack at the end of an argv is a run that silently kept the built-in stack"
        );
        let missing = dir.join(format!("lantern-stack-absent-{}.ron", std::process::id()));
        assert!(
            matches!(
                run(&["--stack", missing.to_str().expect("utf-8")]),
                Invocation::BadUsage(_)
            ),
            "a path that names no file is refused rather than ignored"
        );

        std::fs::remove_file(&good).expect("the file this test wrote");
        std::fs::remove_file(&bad).expect("the file this test wrote");
    }

    /// **Each `--no-*` flag clears one effect and leaves the others alone**, and
    /// they compose.
    ///
    /// One arm per flag rather than one assertion over all three, because the
    /// failure worth catching is a flag wired to the wrong bit — which a test
    /// passing every flag at once cannot see, since the answer is empty either
    /// way.
    #[test]
    fn every_effect_flag_clears_its_own_effect_and_no_other() {
        for (flag, cleared) in [
            ("--no-shadows", RenderEffects::SHADOWS),
            ("--no-ao", RenderEffects::AMBIENT_OCCLUSION),
            ("--no-reflections", RenderEffects::REFLECTIONS),
        ] {
            let Invocation::Run(options) = run(&[flag]) else {
                panic!("{flag} is a run");
            };
            assert_eq!(
                options.effects,
                RenderEffects::all().difference(cleared),
                "{flag}"
            );
            assert!(USAGE.contains(flag), "the usage text does not offer {flag}");
        }

        let Invocation::Run(options) = run(&["--no-shadows", "--no-ao"]) else {
            panic!("two flags is a run");
        };
        assert_eq!(
            options.effects,
            RenderEffects::all()
                .difference(RenderEffects::SHADOWS)
                .difference(RenderEffects::AMBIENT_OCCLUSION),
            "two flags compose, and neither touches an effect this sample has no flag for"
        );
    }

    /// Every flag this sample adds parses, and reaches the field it names.
    #[test]
    fn the_sample_s_own_flags_reach_their_fields() {
        let Invocation::Run(options) = run(&[
            "--camera",
            "free",
            "--force-geometry",
            "indirect-per-batch",
            "--force-binding",
            "array-pages",
            "--headless",
            "--frames",
            "4",
        ]) else {
            panic!("that invocation is a run");
        };
        assert_eq!(options.camera, CameraMode::Free);
        assert_eq!(
            options.forced,
            ForcedPaths {
                geometry: Some(GeometryPath::IndirectPerBatch),
                binding: Some(BindingModel::ArrayPages),
            }
        );
        // And the shared half still landed, which is what a game parser that
        // consumed its own flags first would break.
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// A value that is not one of the vocabulary is refused **by name**, and a
    /// flag with no value after it is refused too.
    ///
    /// The second is the one that matters: `--force-geometry` at the end of an
    /// argv would otherwise take `None` and leave the run on the device's own
    /// path, which is a run that silently did not force anything.
    #[test]
    fn a_bad_value_and_a_missing_one_are_both_refused() {
        for argv in [
            vec!["--camera", "sideways"],
            vec!["--camera"],
            vec!["--force-geometry", "raytraced"],
            vec!["--force-geometry"],
            vec!["--force-binding", "descriptors"],
            vec!["--force-binding"],
            vec!["--nonsense"],
        ] {
            assert!(
                matches!(run(&argv), Invocation::BadUsage(_)),
                "{argv:?} was accepted"
            );
        }
        assert!(matches!(run(&["--help"]), Invocation::Help));
    }

    /// **The help names the pause-menu rows, and it names the ones that exist.**
    ///
    /// The three effect flags used to describe themselves and stop there, so a
    /// reader who found `--no-ao` had no way to learn that AO is also a row
    /// ENTER toggles — `--camera`'s entry has said so all along. Prose is
    /// decoration unless something checks it, and what makes this checkable is
    /// that the row labels are `crate::menu::EFFECT_ROWS`' own third column:
    /// renaming a row fails here rather than leaving the help describing a row
    /// nobody can find.
    #[test]
    fn the_help_names_every_effect_row_the_pause_menu_has() {
        for (_, _, label) in crate::menu::EFFECT_ROWS {
            assert!(
                USAGE.contains(label),
                "the pause menu has a {label} row and the usage text never mentions it",
            );
        }
    }

    /// The shared half of the usage text is the engine's, byte for byte.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        crcbl::args::assert_forced_path_help(USAGE);
    }

    /// **`--screenshot` is a flag this binary answers**, and it forces headless.
    ///
    /// The golden suite's live-monitor arm runs this binary and reads the file
    /// it leaves behind, so a run that accepted the flag and wrote nothing —
    /// which is what a `Common` without `with_screenshot` produces — would be a
    /// suite comparing the previous run's picture forever.
    #[test]
    fn the_screenshot_flag_is_accepted_and_names_a_file() {
        let Invocation::Run(options) = run(&["--screenshot", "shot.png"]) else {
            panic!("--screenshot is a run");
        };
        assert_eq!(
            options.common.screenshot.as_deref(),
            Some(std::path::Path::new("shot.png"))
        );
        assert!(
            options.common.headless,
            "--screenshot reads back off the offscreen ring, so it forces headless"
        );
        assert!(
            options.common.screenshot_request().is_some(),
            "the request the context is armed with has to exist"
        );
    }
}
