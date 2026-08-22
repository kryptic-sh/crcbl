//! Lantern's command line: the shared set, plus the camera and the two path
//! forcings.
//!
//! The shared half is [`crcbl::args::Common`] verbatim — `--headless`,
//! `--frames`, `--backend`, `--size` and the rest — so the flags this sample
//! adds are the ones a *lighting fixture* has and nothing else.

use crcbl::args::{Common, Consumed};
use crcbl::hal::{BindingModel, GeometryPath};
use crcbl::render::RenderEffects;

use crate::gpu::Forced;
use crate::menu::CameraMode;

/// The simulation rate. Nothing here integrates anything but a camera and a
/// lamp's orbit, so it is the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How lantern was asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which camera the run starts on.
    pub camera: CameraMode,
    /// Which selectors the run asks to be held below the device's own.
    pub forced: Forced,
    /// Which of topic 18's effects the run draws.
    ///
    /// The charter's "every effect toggles independently", reached from the
    /// command line: each `--no-*` flag clears one bit and the run drives the
    /// **programmatic** layer of the resolution order with what is left. The
    /// other two request layers have no source in this tree — see
    /// `crcbl::render::effects`.
    pub effects: RenderEffects,
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
            forced: Forced::default(),
            effects: RenderEffects::all(),
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
    --force-geometry <P> Hold the geometry path at 'mesh-shader',
                         'indirect-count' or 'indirect-per-batch' by opening a
                         device without the features that select a better one.
                         Default: whatever this device selects.
    --force-binding <B>  Hold the binding model at 'bindless' or 'array-pages',
                         on --force-geometry's terms. 'array-pages' is what
                         every browser and every Apple device runs.
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
            "--no-shadows" => options.effects.remove(RenderEffects::SHADOWS),
            "--no-ao" => options.effects.remove(RenderEffects::AMBIENT_OCCLUSION),
            "--no-reflections" => options.effects.remove(RenderEffects::REFLECTIONS),
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

/// A [`GeometryPath`] by the name `--force-geometry` takes.
///
/// Written here rather than on the enum because it is a *command line's*
/// vocabulary: `crcbl-hal` has no argument parsing in it and should not grow
/// any for one sample's flag.
#[must_use]
pub fn geometry_from_name(name: &str) -> Option<GeometryPath> {
    match name {
        "mesh-shader" | "mesh" => Some(GeometryPath::MeshShader),
        "indirect-count" | "count" => Some(GeometryPath::IndirectCount),
        "indirect-per-batch" | "per-batch" => Some(GeometryPath::IndirectPerBatch),
        _ => None,
    }
}

/// A [`BindingModel`] by the name `--force-binding` takes, on
/// [`geometry_from_name`]'s terms.
#[must_use]
pub fn binding_from_name(name: &str) -> Option<BindingModel> {
    match name {
        "bindless" => Some(BindingModel::Bindless),
        "array-pages" | "pages" => Some(BindingModel::ArrayPages),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(options.forced, Forced::default());
        assert_eq!(options.common.tick_hz, DEFAULT_TICK_HZ);
        assert_eq!(
            options.effects,
            RenderEffects::all(),
            "a bare run is the every-effect frame the golden is blessed from"
        );
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
        assert_eq!(options.effects, RenderEffects::REFLECTIONS);
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
            Forced {
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

    /// Both name tables round-trip through the paths they name, so a spelling
    /// added to one is a spelling the other can produce.
    #[test]
    fn every_path_this_sample_can_force_has_a_name() {
        for path in [
            GeometryPath::MeshShader,
            GeometryPath::IndirectCount,
            GeometryPath::IndirectPerBatch,
        ] {
            let name = format!("{path:?}");
            // The canonical spelling is the debug name in kebab case, which is
            // what the usage text prints.
            let kebab = name
                .chars()
                .enumerate()
                .flat_map(|(at, ch)| {
                    if ch.is_uppercase() && at > 0 {
                        vec!['-', ch.to_ascii_lowercase()]
                    } else {
                        vec![ch.to_ascii_lowercase()]
                    }
                })
                .collect::<String>();
            assert_eq!(geometry_from_name(&kebab), Some(path), "{kebab}");
            assert!(
                USAGE.contains(&kebab),
                "the usage text does not offer {kebab}"
            );
        }
        for model in [BindingModel::Bindless, BindingModel::ArrayPages] {
            let name = format!("{model:?}");
            let kebab = name
                .chars()
                .enumerate()
                .flat_map(|(at, ch)| {
                    if ch.is_uppercase() && at > 0 {
                        vec!['-', ch.to_ascii_lowercase()]
                    } else {
                        vec![ch.to_ascii_lowercase()]
                    }
                })
                .collect::<String>();
            assert_eq!(binding_from_name(&kebab), Some(model), "{kebab}");
            assert!(
                USAGE.contains(&kebab),
                "the usage text does not offer {kebab}"
            );
        }
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
        assert!(USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP));
        assert!(USAGE.contains(crcbl::args::COMMON_TAIL_HELP));
        assert!(
            USAGE.contains(crcbl::args::SCREENSHOT_HELP),
            "the --screenshot block has drifted from crcbl::args"
        );
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
