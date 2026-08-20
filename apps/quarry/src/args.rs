//! Quarry's command line: the shared set, the camera, the two path forcings and
//! the two LOD controls.
//!
//! The shared half is [`crcbl::args::Common`] verbatim — `--headless`,
//! `--frames`, `--backend`, `--size` and the rest — so the flags this sample
//! adds are the ones a *geometry* fixture has and nothing else.

use crcbl::args::{Common, Consumed};
use crcbl::hal::{BindingModel, GeometryPath};
use crcbl::render::ForwardRenderer;

use crate::gpu::Forced;
use crate::menu::CameraMode;

/// The simulation rate. Nothing here integrates anything but a camera, so it is
/// the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How quarry was asked to run.
///
/// [`PartialEq`] and no [`Eq`], unlike the other samples': the LOD budget is a
/// pixel count and pixel counts are fractional.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which camera the run starts on.
    pub camera: CameraMode,
    /// Which selectors the run asks to be held below the device's own.
    pub forced: Forced,
    /// The screen-space error budget the cut is selected under, in pixels.
    ///
    /// Larger is coarser: a group projecting *over* the budget is expanded and
    /// its children drawn, so raising this stops the descent higher up the DAG.
    /// [`ForwardRenderer::LOD_ERROR_BUDGET`] is the default and the renderer's
    /// own.
    pub lod_budget: f32,
    /// Whether the run starts with each cluster tinted by its DAG level rather
    /// than shaded — `docs/plan/25-lod.md`'s overlay.
    pub lod_view: bool,
    /// Print the device-free report and exit, opening no shell and no adapter.
    ///
    /// Not a run: `src/main.rs` answers this before the front end starts, which
    /// is what keeps the counts reachable on a machine with no driver.
    pub report: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            common: Common::new(DEFAULT_TICK_HZ),
            camera: CameraMode::default(),
            forced: Forced::default(),
            lod_budget: ForwardRenderer::LOD_ERROR_BUDGET,
            lod_view: false,
            report: false,
        }
    }
}

/// The `--help` text.
///
/// One literal rather than a `concat!` of [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and this sample's own flags, exactly as the other samples spell theirs: help
/// text is read, not parsed, and the alignment is part of it.
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` is what stops the
/// two copies drifting.
pub const USAGE: &str = "\
quarry — the geometry acceptance fixture: one dense scene, every geometry path

USAGE:
    quarry [OPTIONS]

Not a game. One quarry face receding 180 metres, drawn from a cluster DAG so
that near and far parts of the *same* mesh are selected at different levels.
The debug panel names the geometry path the frame took and how much of the
reduction was cluster culling — see docs/plan/sample/14-quarry.md.

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
    --camera <C>         Which camera to start on: 'fixed' (the dolly's start
                         pose, the one the goldens are taken from, held still)
                         or 'free' (fly it with WASD, Space/Shift and the arrow
                         keys). Default: fixed. ENTER on the pause menu's CAMERA
                         row swaps them.
    --force-geometry <P> Hold the geometry path at 'mesh-shader',
                         'indirect-count' or 'indirect-per-batch' by opening a
                         device without the features that select a better one.
                         Default: whatever this device selects. This is the
                         sample the flag exists for: the mesh path selects a
                         level per cluster and the other two per instance.
    --force-binding <B>  Hold the binding model at 'bindless' or 'array-pages',
                         on --force-geometry's terms. 'array-pages' is what
                         every browser and every Apple device runs.
    --lod-budget <PX>    Screen-space error budget in pixels (default 1). Larger
                         is coarser: a group projecting over the budget is
                         expanded and its children drawn, so raising this stops
                         the descent higher up the DAG. Must be positive and
                         finite.
    --lod-view           Start with each cluster tinted by the DAG level it was
                         decimated to, instead of shaded. ENTER on the pause
                         menu's LOD VIEW row toggles it. The two indirect paths
                         select one level per instance and draw one flat tint,
                         which is the comparison rather than a shortfall.
    --report             Print the face's counts, the meshlet total and the
                         per-level DAG breakdown, then exit. Opens no window and
                         no adapter, so it runs on a machine with no driver.
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
            "--lod-budget" => match args.next() {
                Some(value) => match budget_from_str(&value) {
                    Ok(budget) => options.lod_budget = budget,
                    Err(message) => return Invocation::BadUsage(message),
                },
                None => return Invocation::BadUsage("--lod-budget needs a value".into()),
            },
            "--lod-view" => options.lod_view = true,
            "--report" => options.report = true,
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

/// A screen-space error budget by the value `--lod-budget` takes.
///
/// **Zero, a negative and a NaN are all refused by name.** The budget is
/// compared against a group's projected error, so a non-positive one expands
/// every group to the bottom of the DAG and a NaN makes every comparison false —
/// either way the run draws a cut nobody can reason about while the panel goes
/// on printing the number that was asked for.
fn budget_from_str(value: &str) -> Result<f32, String> {
    let budget: f32 = value
        .parse()
        .map_err(|_| format!("--lod-budget wants a number of pixels, and got {value:?}"))?;
    if !budget.is_finite() {
        return Err(format!(
            "--lod-budget must be finite, and {value:?} is not — a cut selected against it is one \
             nobody can reason about"
        ));
    }
    if budget <= 0.0 {
        return Err(format!(
            "--lod-budget must be positive, and {value:?} is not — a budget at or below zero \
             expands every group to the bottom of the DAG"
        ));
    }
    Ok(budget)
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
    fn a_bare_invocation_is_the_dolly_pose_on_the_devices_own_paths() {
        let Invocation::Run(options) = run(&[]) else {
            panic!("no arguments is a run");
        };
        assert_eq!(options.camera, CameraMode::Fixed);
        assert_eq!(options.forced, Forced::default());
        assert_eq!(options.common.tick_hz, DEFAULT_TICK_HZ);
        assert_eq!(
            options.lod_budget,
            ForwardRenderer::LOD_ERROR_BUDGET,
            "a bare run selects under the renderer's own budget, not a copy of it",
        );
        assert!(!options.lod_view, "the tint is an overlay, not the picture");
        assert!(!options.report);
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
            "--lod-budget",
            "16",
            "--lod-view",
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
        assert_eq!(options.lod_budget, 16.0);
        assert!(options.lod_view);
        // And the shared half still landed, which is what a parser that
        // consumed its own flags first would break.
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// `--report` is a run's option and not a run: `src/main.rs` reads it and
    /// never opens a shell.
    #[test]
    fn the_report_flag_reaches_its_field() {
        let Invocation::Run(options) = run(&["--report"]) else {
            panic!("--report parses");
        };
        assert!(options.report);
    }

    /// **A budget that cannot select a cut is refused, by name.**
    ///
    /// Zero and a negative expand every group to the bottom of the DAG; a NaN
    /// makes every comparison against it false. All three would draw a frame and
    /// report the value they were given, which is the failure worth catching —
    /// there is no picture that says the budget was nonsense.
    #[test]
    fn a_budget_that_selects_nothing_is_refused() {
        for value in ["0", "0.0", "-1", "-0.5", "nan", "NaN", "inf", "sixteen", ""] {
            let outcome = run(&["--lod-budget", value]);
            let Invocation::BadUsage(message) = outcome else {
                panic!("--lod-budget {value:?} was accepted: {outcome:?}");
            };
            assert!(
                message.contains("--lod-budget"),
                "the refusal must name the flag: {message}"
            );
        }
        // And the ones either side of the boundary are accepted, so this is a
        // guard rather than a flag that refuses everything.
        for value in ["1", "0.5", "16", "4096"] {
            assert!(
                matches!(run(&["--lod-budget", value]), Invocation::Run(_)),
                "--lod-budget {value:?} was refused"
            );
        }
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
            vec!["--lod-budget"],
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
    /// added to one is a spelling the other can produce — and the usage text
    /// offers every one of them.
    #[test]
    fn every_path_this_sample_can_force_has_a_name() {
        /// The canonical spelling: the debug name in kebab case, which is what
        /// the usage text prints.
        fn kebab(name: &str) -> String {
            name.chars()
                .enumerate()
                .flat_map(|(at, ch)| {
                    if ch.is_uppercase() && at > 0 {
                        vec!['-', ch.to_ascii_lowercase()]
                    } else {
                        vec![ch.to_ascii_lowercase()]
                    }
                })
                .collect()
        }

        for path in [
            GeometryPath::MeshShader,
            GeometryPath::IndirectCount,
            GeometryPath::IndirectPerBatch,
        ] {
            let name = kebab(&format!("{path:?}"));
            assert_eq!(geometry_from_name(&name), Some(path), "{name}");
            assert!(
                USAGE.contains(&name),
                "the usage text does not offer {name}"
            );
        }
        for model in [BindingModel::Bindless, BindingModel::ArrayPages] {
            let name = kebab(&format!("{model:?}"));
            assert_eq!(binding_from_name(&name), Some(model), "{name}");
            assert!(
                USAGE.contains(&name),
                "the usage text does not offer {name}"
            );
        }
    }

    /// **Every flag this sample adds appears in its own usage text.**
    ///
    /// A flag the parser accepts and the help never mentions is a flag nobody
    /// finds. Written out here rather than read off the parser, which would only
    /// ask whether the file agrees with itself.
    #[test]
    fn every_flag_this_sample_adds_is_in_the_usage_text() {
        for flag in [
            "--camera",
            "--force-geometry",
            "--force-binding",
            "--lod-budget",
            "--lod-view",
            "--report",
        ] {
            assert!(USAGE.contains(flag), "the usage text does not offer {flag}");
        }
    }

    /// The shared half of the usage text is the engine's, byte for byte.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        assert!(USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP));
        assert!(USAGE.contains(crcbl::args::COMMON_TAIL_HELP));
    }
}
