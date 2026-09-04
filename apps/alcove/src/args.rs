//! Alcove's command line: the shared set, the two path forcings, and the
//! occlusion state a run starts in.
//!
//! The shared half is [`crcbl::args::Common`] verbatim — `--headless`,
//! `--frames`, `--backend`, `--size` and the rest — so the flags this sample
//! adds are the ones an *occlusion fixture* has and nothing else.
//!
//! # The occlusion flags write console variables and keep nothing
//!
//! `--technique`, `--split` and `--ao-view` are a *starting state* rather than a
//! second copy of one: [`Options::apply`] writes them into the cells
//! [`crate::occlusion`] reads, and after that the keys, the pause panel and a
//! typed console line are all editing the same value. A flag that kept its own
//! field would be a fourth writer that the other three could not see.

use crcbl::args::{Common, Consumed};
use crcbl::hal::{BindingModel, GeometryPath};
use crcbl::render::{DebugView, RenderEffects};

use crate::gpu::Forced;
use crate::menu::CameraMode;
use crate::occlusion;

/// The simulation rate. Nothing here integrates anything but a camera, so it is
/// the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How alcove was asked to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which camera the run starts on.
    pub camera: CameraMode,
    /// Which selectors the run asks to be held below the device's own.
    pub forced: Forced,
    /// Which of `docs/plan/18-render-features.md`'s effects the run draws.
    ///
    /// `--no-ao` is the one flag that moves it, and it is the switch this
    /// fixture is about: with the occlusion pass out, the renderer binds its 1×1
    /// white image and every claim below turns into its own control.
    pub effects: RenderEffects,
    /// Which gather the run starts on, or `None` for the one that ships.
    pub technique: Option<&'static str>,
    /// Where the comparison seam starts, or `None` for a run comparing nothing.
    pub split: Option<f32>,
    /// Whether the run starts drawing the occlusion channel instead of shading.
    pub ao_view: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // `with_screenshot` is what makes `--screenshot` a flag this binary
            // has rather than an unknown argument: `crate::app::assemble` arms
            // the request on the context, and a sample that had not done that
            // must refuse the flag instead of writing nothing.
            #[cfg(not(target_arch = "wasm32"))]
            common: Common::new(DEFAULT_TICK_HZ).with_screenshot(),
            #[cfg(target_arch = "wasm32")]
            common: Common::new(DEFAULT_TICK_HZ),
            camera: CameraMode::default(),
            forced: Forced::default(),
            effects: RenderEffects::all(),
            technique: None,
            split: None,
            ao_view: false,
        }
    }
}

impl Options {
    /// Writes the occlusion state this run asked for into the console's own
    /// cells.
    ///
    /// Called once, before the first frame. Everything it does not name is left
    /// at the engine's default, which is what every golden in this workspace was
    /// blessed at.
    pub fn apply(&self) {
        if let Some(technique) = self.technique {
            let value = crcbl::console::Value::Enum(technique);
            if let Err(fault) = occlusion::var(occlusion::TECHNIQUE).set(&value) {
                crcbl::log::error!("alcove: --technique {technique} was refused: {fault}");
            }
        }
        if let Some(at) = self.split {
            let value = crcbl::console::Value::Float(at);
            if let Err(fault) = occlusion::var(occlusion::SPLIT).set(&value) {
                crcbl::log::error!("alcove: --split {at} was refused: {fault}");
            }
        }
        if self.ao_view {
            crcbl::debug_view::set(DebugView::AmbientOcclusion);
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
alcove — the ambient-occlusion acceptance fixture: one court, every technique

USAGE:
    alcove [OPTIONS]

Not a game. One walled court of nothing but occlusion geometry: an alcove, a
flight of cantilevered treads, boxes resting on a floor, a deep slot the sun
runs down, and a sphere against a far wall. Every vertical surface the fixed
camera sees carries no direct light at all, so what models them is the ambient
term — which is the term ambient occlusion scales, and the whole subject of
this sample. See docs/plan/sample/19-alcove.md.

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
    --no-ao              Draw with no ambient occlusion. The ambient term is
                         scaled by the renderer's 1x1 white instead, which is
                         the control every claim this fixture makes is read
                         against. The pause menu's AO row toggles it live.
    --ao-view            Start drawing the occlusion channel as grey instead of
                         the shaded picture — the AO VIEW row, and the console's
                         'debug_view ambient occlusion'. V toggles it.
    --technique <T>      Which gather the near side of the seam runs: 'gtao' or
                         'hemisphere'. Default: whatever r_ssao_technique ships.
                         T cycles it, and the pause menu's TECHNIQUE row shows
                         the one in force.
    --split [AT]         Draw the console's occlusion against the shipped one,
                         seam at AT across the frame (0..1, default 0.5). The
                         near side runs --technique and the far side what ships;
                         the panel's NEAR SIDE and FAR SIDE rows say which. X
                         raises and lowers it, ',' and '.' move it.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help

KEYS:
    V                    Draw the occlusion channel as grey instead of the court
    N                    Draw the bent direction the gather reported instead
    T                    Cycle the gather the near side runs
    B                    Gather a bent direction beside the scalar, or do not
    X                    Raise the comparison seam at the centre, or drop it
    , .                  Walk the seam a step left or right
    [ ]                  Narrow and widen the radius the horizons sweep
    - =                  Weaken and strengthen how hard the occlusion is applied
    R                    Put every occlusion knob back where a run opens
    ESC                  Pause, and the panel every row above is on";

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
            "--no-ao" => options.effects.remove(RenderEffects::AMBIENT_OCCLUSION),
            "--ao-view" => options.ao_view = true,
            "--technique" => match args.next().as_deref().map(technique_from_name) {
                Some(Some(technique)) => options.technique = Some(technique),
                Some(None) => {
                    return Invocation::BadUsage(format!(
                        "unknown technique — the engine declares {}",
                        occlusion::names(occlusion::TECHNIQUE).join(", ")
                    ));
                }
                None => return Invocation::BadUsage("--technique needs a value".into()),
            },
            // **The value is optional and that is what `peek` is for.** `--split`
            // alone means the middle of the frame, which is what a person asking
            // for a comparison almost always wants; `--split 0.7` moves the
            // seam. A following argument that is not a fraction — another flag,
            // most often — is left for the loop to parse rather than swallowed.
            "--split" => {
                let peeked = args.peek().and_then(|next| seam_from_name(next));
                let at = match peeked {
                    Some(at) => {
                        args.next();
                        at
                    }
                    None => occlusion::SEAM_CENTRE,
                };
                options.split = Some(at);
            }
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

/// The technique `name` names, **as the engine spells it**.
///
/// Matched against `r_ssao_technique`'s own declared set rather than against a
/// list here, which is what makes a third gather landing in `crcbl-render` a
/// value this flag takes without a change in this file.
#[must_use]
pub fn technique_from_name(name: &str) -> Option<&'static str> {
    occlusion::names(occlusion::TECHNIQUE)
        .iter()
        .copied()
        .find(|declared| *declared == name)
}

/// A seam position by the name `--split` takes, or `None` for anything that is
/// not a fraction inside the variable's own range.
#[must_use]
pub fn seam_from_name(name: &str) -> Option<f32> {
    let at: f32 = name.parse().ok()?;
    match occlusion::var(occlusion::SPLIT).kind() {
        crcbl::console::Kind::Float { min, max } => (at >= min && at <= max).then_some(at),
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
        assert_eq!(options.technique, None, "a bare run takes what ships");
        assert_eq!(options.split, None, "a bare run compares nothing");
        assert!(!options.ao_view, "a bare run draws the shaded picture");
    }

    /// Every flag this sample adds parses, and reaches the field it names.
    #[test]
    fn the_samples_own_flags_reach_their_fields() {
        let Invocation::Run(options) = run(&[
            "--camera",
            "free",
            "--force-geometry",
            "indirect-per-batch",
            "--force-binding",
            "array-pages",
            "--no-ao",
            "--ao-view",
            "--technique",
            "hemisphere",
            "--split",
            "0.25",
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
        assert_eq!(
            options.effects,
            RenderEffects::all().difference(RenderEffects::AMBIENT_OCCLUSION),
            "--no-ao clears the occlusion bit and no other"
        );
        assert!(options.ao_view);
        assert_eq!(options.technique, Some("hemisphere"));
        assert_eq!(options.split, Some(0.25));
        // And the shared half still landed, which is what a game parser that
        // consumed its own flags first would break.
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// **`--split` with no value is the middle of the frame, and it does not eat
    /// the flag after it.**
    ///
    /// The second half is the one worth guarding: a `--split` that consumed
    /// whatever followed would swallow `--headless` and leave the run windowed,
    /// which on a CI runner is a run that never starts.
    #[test]
    fn a_bare_split_is_the_centre_and_leaves_the_next_flag_alone() {
        let Invocation::Run(options) = run(&["--split", "--headless"]) else {
            panic!("--split with no value is a run");
        };
        assert_eq!(options.split, Some(occlusion::SEAM_CENTRE));
        assert!(
            options.common.headless,
            "--split swallowed the flag that followed it"
        );

        let Invocation::Run(last) = run(&["--headless", "--split"]) else {
            panic!("--split at the end of an argv is a run");
        };
        assert_eq!(last.split, Some(occlusion::SEAM_CENTRE));
    }

    /// A value that is not one of the vocabulary is refused **by name**, and a
    /// flag with no value after it is refused too.
    #[test]
    fn a_bad_value_and_a_missing_one_are_both_refused() {
        for argv in [
            vec!["--camera", "sideways"],
            vec!["--camera"],
            vec!["--force-geometry", "raytraced"],
            vec!["--force-geometry"],
            vec!["--force-binding", "descriptors"],
            vec!["--force-binding"],
            vec!["--technique", "hbao"],
            vec!["--technique"],
            vec!["--nonsense"],
        ] {
            assert!(
                matches!(run(&argv), Invocation::BadUsage(_)),
                "{argv:?} was accepted"
            );
        }
        assert!(matches!(run(&["--help"]), Invocation::Help));
    }

    /// Both path name tables round-trip, so a spelling added to one is a
    /// spelling the other can produce, and the usage text offers it.
    #[test]
    fn every_path_this_sample_can_force_has_a_name() {
        let kebab = |name: &str| {
            name.chars()
                .enumerate()
                .flat_map(|(at, ch)| {
                    if ch.is_uppercase() && at > 0 {
                        vec!['-', ch.to_ascii_lowercase()]
                    } else {
                        vec![ch.to_ascii_lowercase()]
                    }
                })
                .collect::<String>()
        };
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

    /// **The help offers every technique the engine declares**, and takes each
    /// of them.
    ///
    /// The check that catches a gather landing in `crcbl-render` with this
    /// sample's help still naming two: `--technique` would accept the new name
    /// and the text a person reads would not mention it.
    #[test]
    fn the_help_offers_every_technique_the_engine_declares() {
        let names = occlusion::names(occlusion::TECHNIQUE);
        assert!(names.len() > 1, "a selector over one name selects nothing");
        for name in names {
            assert_eq!(technique_from_name(name), Some(*name));
            assert!(
                USAGE.contains(name),
                "the usage text does not offer the {name} technique"
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
        assert!(options.common.screenshot_request().is_some());
    }
}
