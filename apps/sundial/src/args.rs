//! Sundial's command line: the shared set, the two path forcings, and the
//! shadow state a run starts in.
//!
//! The shared half is [`crcbl::args::Common`] verbatim — `--headless`,
//! `--frames`, `--backend`, `--size` and the rest — so the flags this sample adds
//! are the ones a *shadow fixture* has and nothing else.
//!
//! # The filter flags write console variables and keep nothing
//!
//! `--filter` and `--split` are a *starting state* rather than a second copy of
//! one: [`Options::apply`] writes them into the cells [`crate::filter`] reads,
//! and after that the keys, the pause panel and a typed console line are all
//! editing the same value. A flag that kept its own field would be a fourth
//! writer the other three could not see.
//!
//! **The clock is the exception, and it is not one of those.** `--sun-tick` and
//! `--sun-paused` reach [`crate::sun::Clock`], which the loop owns, because a
//! tick is where the simulation has got to rather than a setting a console line
//! could change under it.

use crcbl::args::{Common, Consumed};
use crcbl::hal::{BindingModel, GeometryPath};
use crcbl::render::RenderEffects;

use crate::filter;
use crate::gpu::Forced;
use crate::menu::CameraMode;
use crate::sun;

/// The simulation rate. Nothing here integrates anything but a camera and a sun,
/// so it is the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How sundial was asked to run.
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
    /// `--no-shadows` is the one flag that moves it, and it is the switch this
    /// fixture is about: with the shadow passes out, every surface is lit and
    /// every claim below turns into its own control.
    pub effects: RenderEffects,
    /// Which filter the run starts on, or `None` for the one that ships.
    pub filter: Option<&'static str>,
    /// Where the comparison seam starts, or `None` for a run comparing nothing.
    pub split: Option<f32>,
    /// Which tick of [`crate::sun::Clock`] the run starts at.
    pub sun_tick: u64,
    /// Whether the clock starts stopped.
    pub sun_paused: bool,
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
            filter: None,
            split: None,
            sun_tick: sun::FIXTURE_TICK,
            sun_paused: false,
        }
    }
}

impl Options {
    /// Writes the shadow state this run asked for into the console's own cells.
    ///
    /// Called once, before the first frame. Everything it does not name is left
    /// at the engine's default, which is what every golden in this workspace was
    /// blessed at.
    pub fn apply(&self) {
        if let Some(name) = self.filter {
            let value = crcbl::console::Value::Enum(name);
            if let Err(fault) = filter::var(filter::FILTER).set(&value) {
                crcbl::log::error!("sundial: --filter {name} was refused: {fault}");
            }
        }
        if let Some(at) = self.split {
            let value = crcbl::console::Value::Float(at);
            if let Err(fault) = filter::var(filter::SPLIT).set(&value) {
                crcbl::log::error!("sundial: --split {at} was refused: {fault}");
            }
        }
    }

    /// The clock this run starts with.
    #[must_use]
    pub const fn clock(&self) -> sun::Clock {
        sun::Clock::at(self.sun_tick, !self.sun_paused)
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
sundial — the shadow acceptance fixture: one plaza, a moving sun, every filter

USAGE:
    sundial [OPTIONS]

Not a game. One open plaza laid out so that every named shadow artefact has a
surface it would appear on: a large pavement at a grazing sun where acne shows,
a colonnade whose shadow crosses a cascade boundary, a plinth resting on the
pavement whose contact point peter-panning would light, and three counters
hanging at graded heights so a contact-hardening penumbra is a thing you can
look at. The sun runs on a scripted clock — tick-driven, never wall-clock — so
any two runs draw the same frame. See docs/plan/sample/18-sundial.md.

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
                         are taken from), 'counters' (the close pose the
                         penumbra ladder is read at) or 'free' (fly it with
                         WASD, Space/Shift and the arrow keys). Default: fixed.
                         ENTER on the pause menu's CAMERA row cycles them.
    --force-geometry <P> Hold the geometry path at 'mesh-shader',
                         'indirect-count' or 'indirect-per-batch' by opening a
                         device without the features that select a better one.
                         Default: whatever this device selects.
    --force-binding <B>  Hold the binding model at 'bindless' or 'array-pages',
                         on --force-geometry's terms. 'array-pages' is what
                         every browser and every Apple device runs.
    --no-shadows         Draw with no shadows at all. Every surface is lit, which
                         is the control every claim this fixture makes is read
                         against.
    --filter <F>         Which filter the near side of the seam runs: 'pcss',
                         'disc' or 'box'. Default: whatever r_shadow_filter
                         ships. F cycles it, and the pause menu's FILTER row
                         shows the one in force.
    --split [AT]         Draw the console's filter against the shipped one, seam
                         at AT across the frame (0..1, default 0.5). The near
                         side runs --filter and the far side what ships; the
                         panel's NEAR SIDE and FAR SIDE rows say which. X raises
                         and lowers it, ',' and '.' move it.
    --sun-tick <N>       Which tick of the scripted clock the run starts at.
                         Default: the fixture pose the goldens are taken at. The
                         sweep is 600 ticks long and wraps; '-' and '=' scrub it
                         and P starts and stops it.
    --sun-paused         Start with the clock stopped, on the tick --sun-tick
                         names. What a headless run wants when it means to draw
                         one pose rather than a sweep.
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
                    return Invocation::BadUsage(
                        "unknown camera — try `fixed`, `counters` or `free`".into(),
                    );
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
            "--filter" => match args.next().as_deref().map(filter::filter_from_name) {
                Some(Some(name)) => options.filter = Some(name),
                Some(None) => {
                    return Invocation::BadUsage(format!(
                        "unknown filter — the engine declares {}",
                        filter::names(filter::FILTER).join(", ")
                    ));
                }
                None => return Invocation::BadUsage("--filter needs a value".into()),
            },
            // **The value is optional and that is what `peek` is for.** `--split`
            // alone means the middle of the frame, which is what a person asking
            // for a comparison almost always wants; `--split 0.7` moves the seam.
            // A following argument that is not a fraction — another flag, most
            // often — is left for the loop to parse rather than swallowed.
            "--split" => {
                let peeked = args.peek().and_then(|next| filter::seam_from_name(next));
                let at = match peeked {
                    Some(at) => {
                        args.next();
                        at
                    }
                    None => filter::SEAM_CENTRE,
                };
                options.split = Some(at);
            }
            "--sun-tick" => match args.next().as_deref().map(str::parse::<u64>) {
                Some(Ok(tick)) => options.sun_tick = tick,
                Some(Err(why)) => {
                    return Invocation::BadUsage(format!("--sun-tick wants a tick count: {why}"));
                }
                None => return Invocation::BadUsage("--sun-tick needs a value".into()),
            },
            "--sun-paused" => options.sun_paused = true,
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

/// A [`GeometryPath`] by the name `--force-geometry` takes.
///
/// Written here rather than on the enum because it is a *command line's*
/// vocabulary: `crcbl-hal` has no argument parsing in it and should not grow any
/// for one sample's flag.
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
    fn a_bare_invocation_is_the_fixture_pose_on_the_devices_own_paths() {
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
        assert_eq!(options.filter, None, "a bare run takes what ships");
        assert_eq!(options.split, None, "a bare run compares nothing");
        assert_eq!(options.sun_tick, sun::FIXTURE_TICK);
        assert!(!options.sun_paused, "a bare run's sun moves");
        assert_eq!(options.clock(), sun::Clock::default());
    }

    /// Every flag this sample adds parses, and reaches the field it names.
    #[test]
    fn the_samples_own_flags_reach_their_fields() {
        let Invocation::Run(options) = run(&[
            "--camera",
            "counters",
            "--force-geometry",
            "indirect-per-batch",
            "--force-binding",
            "array-pages",
            "--no-shadows",
            "--filter",
            "box",
            "--split",
            "0.25",
            "--sun-tick",
            "123",
            "--sun-paused",
            "--headless",
            "--frames",
            "4",
        ]) else {
            panic!("that invocation is a run");
        };
        assert_eq!(options.camera, CameraMode::Counters);
        assert_eq!(
            options.forced,
            Forced {
                geometry: Some(GeometryPath::IndirectPerBatch),
                binding: Some(BindingModel::ArrayPages),
            }
        );
        assert_eq!(
            options.effects,
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "--no-shadows clears the shadow bit and no other"
        );
        assert_eq!(options.filter, Some("box"));
        assert_eq!(options.split, Some(0.25));
        assert_eq!(options.sun_tick, 123);
        assert!(options.sun_paused);
        assert_eq!(options.clock(), sun::Clock::at(123, false));
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
        assert_eq!(options.split, Some(filter::SEAM_CENTRE));
        assert!(
            options.common.headless,
            "--split swallowed the flag that followed it"
        );

        let Invocation::Run(last) = run(&["--headless", "--split"]) else {
            panic!("--split at the end of an argv is a run");
        };
        assert_eq!(last.split, Some(filter::SEAM_CENTRE));
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
            vec!["--filter", "poisson"],
            vec!["--filter"],
            vec!["--sun-tick", "noon"],
            vec!["--sun-tick"],
            vec!["--nonsense"],
        ] {
            assert!(
                matches!(run(&argv), Invocation::BadUsage(_)),
                "{argv:?} was accepted"
            );
        }
        assert!(matches!(run(&["--help"]), Invocation::Help));
    }

    /// Both path name tables round-trip, so a spelling added to one is a spelling
    /// the other can produce, and the usage text offers it.
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

    /// **The help offers every filter the engine declares**, and takes each of
    /// them.
    ///
    /// The check that catches a rung landing in `crcbl-render` with this sample's
    /// help still naming three: `--filter` would accept the new name and the text
    /// a person reads would not mention it.
    #[test]
    fn the_help_offers_every_filter_the_engine_declares() {
        let names = filter::names(filter::FILTER);
        assert!(names.len() > 1, "a selector over one name selects nothing");
        for name in names {
            assert_eq!(filter::filter_from_name(name), Some(*name));
            assert!(
                USAGE.contains(name),
                "the usage text does not offer the {name} filter"
            );
        }
    }

    /// **The usage text names the sweep's own length**, which is what a person
    /// reaching for `--sun-tick` needs and the one number in it that could drift.
    #[test]
    fn the_help_names_the_length_of_the_sweep() {
        assert!(
            USAGE.contains(&format!("{} ticks long", sun::SWEEP_TICKS)),
            "the usage text does not say how long one sweep of the sun is"
        );
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
