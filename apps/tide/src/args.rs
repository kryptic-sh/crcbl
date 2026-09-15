//! Tide's command line: the shared set, the two path forcings, and the knobs a
//! run starts on.
//!
//! The shared half is [`crcbl::args::Common`] verbatim, so the flags this sample
//! adds are the ones a *water fixture* has and nothing else.
//!
//! # The knob flags write the cell and keep nothing
//!
//! `--scene`, `--medium` and `--camera` are a starting state, on
//! `apps/sundial/src/args.rs`' terms: [`Options::apply`] writes them into
//! [`crate::knobs`]' cell, and after that a key, a pause row and a page export
//! are all editing the same value.

use crcbl::args::{Common, Consumed, binding_from_name, geometry_from_name};
use crcbl::engine::ForcedPaths;

use crate::knobs::{self, Knobs};
use crate::medium::Preset;
use crate::menu::CameraMode;
use crate::scene::Scene;

/// The simulation rate. Nothing here integrates anything but a camera, so it is
/// the engine's ordinary 60.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How tide was asked to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// Which selectors the run asks to be held below the device's own.
    pub forced: ForcedPaths,
    /// Where the three knobs start.
    pub knobs: Knobs,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // `with_screenshot` is what makes `--screenshot` a flag this binary
            // has: `crate::app::assemble` arms the request on the context.
            #[cfg(not(target_arch = "wasm32"))]
            common: Common::new(DEFAULT_TICK_HZ).with_screenshot(),
            #[cfg(target_arch = "wasm32")]
            common: Common::new(DEFAULT_TICK_HZ),
            forced: ForcedPaths::default(),
            knobs: Knobs::default(),
        }
    }
}

impl Options {
    /// Writes the knobs this run asked for into the cell. Called once, before
    /// the first frame.
    pub fn apply(&self) {
        knobs::set(self.knobs);
    }
}

/// The `--help` text.
///
/// One literal, as every other sample spells its own:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` is what stops the
/// shared half drifting from the engine's.
pub const USAGE: &str = "\
tide — the water acceptance fixture: four scenes, and the courtyard pool drawn

USAGE:
    tide [OPTIONS]

Not a game. A gallery of four scenes — open sea, coast, valley and courtyard —
of which milestone 1 builds the courtyard: a tiled pool with a deep end and a
shallow end under a fixed sun, its water refracting, absorbing and reflecting
the frame behind it. The other three are empty rooms naming the milestone that
fills each. See docs/plan/sample/21-tide.md.

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
    --scene <S>          Which scene to open on: 'open-sea', 'coast', 'valley'
                         or 'courtyard'. Default: courtyard, the one built.
    --medium <M>         What the water is made of: 'clear-pool', 'lake',
                         'pond' or 'swamp'. Default: clear-pool.
    --camera <C>         Which camera to start on: 'fixed' (the pose the goldens
                         are taken from), 'orbit' (turning round the pool on the
                         fixed step) or 'free' (fly it with WASD, Space/Shift
                         and the arrow keys). Default: fixed.
    --force-geometry <P> Require 'mesh-shader', 'indirect-count' or
                         'indirect-per-batch'; unsupported paths fail startup.
                         Default: this device's preferred geometry path.
    --force-binding <B>  Request a 'bindless' or 'array-pages' capability ceiling.
                         This forward renderer always uses array-pages.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help

KEYS:
    N                    Move on to the next scene
    M                    Move on to the next medium preset
    C                    Move on to the next camera: fixed, orbit, free
    R                    Put every knob back where a run opens
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
            "--scene" => match args.next().as_deref().map(Scene::from_name) {
                Some(Some(scene)) => options.knobs.scene = scene,
                Some(None) => {
                    return Invocation::BadUsage(
                        "unknown scene — try `open-sea`, `coast`, `valley` or `courtyard`".into(),
                    );
                }
                None => return Invocation::BadUsage("--scene needs a value".into()),
            },
            "--medium" => match args.next().as_deref().map(Preset::from_name) {
                Some(Some(medium)) => options.knobs.medium = medium,
                Some(None) => {
                    return Invocation::BadUsage(
                        "unknown medium — try `clear-pool`, `lake`, `pond` or `swamp`".into(),
                    );
                }
                None => return Invocation::BadUsage("--medium needs a value".into()),
            },
            "--camera" => match args.next().as_deref().map(CameraMode::from_name) {
                Some(Some(camera)) => options.knobs.camera = camera,
                Some(None) => {
                    return Invocation::BadUsage(
                        "unknown camera — try `fixed`, `orbit` or `free`".into(),
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
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

#[cfg(test)]
mod tests {
    use crcbl::hal::{BindingModel, GeometryPath};

    use super::*;

    fn run(argv: &[&str]) -> Invocation {
        parse(argv.iter().map(|arg| (*arg).to_string()))
    }

    /// A bare invocation is the golden's courtyard on the device's own paths.
    #[test]
    fn a_bare_invocation_is_the_courtyard_on_the_devices_own_paths() {
        let Invocation::Run(options) = run(&[]) else {
            panic!("no arguments is a run");
        };
        assert_eq!(options.knobs, Knobs::default());
        assert_eq!(options.knobs.scene, Scene::Courtyard);
        assert_eq!(options.forced, ForcedPaths::default());
        assert_eq!(options.common.tick_hz, DEFAULT_TICK_HZ);
    }

    /// Every flag this sample adds parses and reaches the field it names, and
    /// the shared half still lands beside them.
    #[test]
    fn the_samples_own_flags_reach_their_fields() {
        let Invocation::Run(options) = run(&[
            "--scene",
            "valley",
            "--medium",
            "swamp",
            "--camera",
            "orbit",
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
        assert_eq!(
            options.knobs,
            Knobs {
                scene: Scene::Valley,
                medium: Preset::Swamp,
                camera: CameraMode::Orbit,
            }
        );
        assert_eq!(
            options.forced,
            ForcedPaths {
                geometry: Some(GeometryPath::IndirectPerBatch),
                binding: Some(BindingModel::ArrayPages),
            }
        );
        assert!(options.common.headless);
        assert_eq!(options.common.frames, Some(4));
    }

    /// A value outside the vocabulary is refused, and so is a flag with none.
    #[test]
    fn a_bad_value_and_a_missing_one_are_both_refused() {
        for argv in [
            vec!["--scene", "desert"],
            vec!["--scene"],
            vec!["--medium", "milk"],
            vec!["--medium"],
            vec!["--camera", "sideways"],
            vec!["--camera"],
            vec!["--force-geometry", "raytraced"],
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

    /// **The help offers every scene, preset and camera the sample has**, so a
    /// fifth preset added to [`Preset::ALL`] with the help still naming four
    /// fails here.
    #[test]
    fn the_help_offers_every_scene_preset_and_camera() {
        let offers = |name: &str| USAGE.contains(&format!("'{name}'"));
        for scene in Scene::ALL {
            assert!(
                offers(scene.label()),
                "the help does not offer {}",
                scene.label()
            );
        }
        for preset in Preset::ALL {
            assert!(
                offers(preset.label()),
                "the help does not offer {}",
                preset.label()
            );
        }
        for camera in CameraMode::ALL {
            let name = camera.label().to_lowercase();
            assert!(offers(&name), "the help does not offer {name}");
        }
    }

    /// The shared half of the usage text is the engine's, byte for byte.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        crcbl::args::assert_forced_path_help(USAGE);
    }
}
