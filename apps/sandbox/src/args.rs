//! Argument parsing, by hand.
//!
//! Four flags do not justify a dependency — see `crates/crcbl-cli/src/args.rs`,
//! which makes the same argument at the length it deserves and is the one that
//! had a real choice to make.

use crcbl::args::MAX_TICK_RATE;
use crcbl::backend::GpuBackend;

use crate::app::{CameraMode, Options};

/// `--help` text, and the definition of the flag set.
pub const USAGE: &str = "\
crcbl sandbox — the engine's development playground

USAGE:
    sandbox [OPTIONS]

OPTIONS:
        --headless        Run against HeadlessShell with a hand-driven clock.
                          Deterministic, needs no display, always terminates.
        --backend <NAME>  GPU backend: `vk` or `null`. Default: choose one,
                          which today means Vulkan. `null` renders nothing and
                          needs no driver, so it is never chosen automatically.
        --camera <MODE>   `perspective` or `ortho`. Default: perspective.
                          The 2D story is a projection-matrix swap and nothing
                          else, which is what this flag exists to demonstrate.
        --frames <N>      Stop after N presented frames.
                          Default: unlimited windowed, 120 headless.
        --tick-hz <N>     Simulation rate. Default: 60.
        --title <TITLE>   Window title. Default: \"Crucible sandbox\".
        --fullscreen      Open borderless instead of windowed. F11 still
                          toggles, and a window system may refuse both; the
                          summary reports what it actually did.
        --debug-overlay   Start with the debug panel visible. F3 toggles it.
        --no-debug-overlay
                          Start with it hidden. The default is `visible in a
                          debug build, hidden in a release build`.
    -h, --help            Print this text.

ENVIRONMENT:
    CRCBL_SHELL   Force a shell backend (wayland, x11, headless).
    CRCBL_GPU     Force a GPU backend (vk, null); --backend wins over it.
    CRCBL_LOG     Log filter; `debug` prints every shell event.

EXIT CODES:
    0  ran to completion   1  failed   2  bad invocation";

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    /// Run with these options.
    Run(Box<Options>),
    /// Print [`USAGE`].
    Help,
    /// The arguments do not make sense, for this reason.
    BadUsage(String),
}

/// Parses arguments, which must **not** include the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => options.headless = true,
            "--fullscreen" => options.fullscreen = true,
            "--debug-overlay" => options.debug_overlay = Some(true),
            "--no-debug-overlay" => options.debug_overlay = Some(false),
            "-h" | "--help" => return Invocation::Help,
            "--backend" => match args.next() {
                Some(name) => match GpuBackend::from_name(&name) {
                    Some(backend) => options.backend = Some(backend),
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown --backend `{name}`; try `vk` or `null`"
                        ));
                    }
                },
                None => return Invocation::BadUsage("--backend needs a value".to_string()),
            },
            "--camera" => match args.next() {
                Some(name) => match CameraMode::from_name(&name) {
                    Some(mode) => options.camera = mode,
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown --camera `{name}`; try `perspective` or `ortho`"
                        ));
                    }
                },
                None => return Invocation::BadUsage("--camera needs a value".to_string()),
            },
            "--frames" => match take_number(&mut args, "--frames") {
                Ok(frames) => options.frames = Some(frames),
                Err(message) => return Invocation::BadUsage(message),
            },
            "--tick-hz" => match take_number(&mut args, "--tick-hz") {
                Ok(0) => {
                    return Invocation::BadUsage("--tick-hz must be at least 1".to_string());
                }
                Ok(hz) => match u32::try_from(hz) {
                    Ok(hz) if hz <= MAX_TICK_RATE => options.tick_hz = hz,
                    Ok(hz) => {
                        return Invocation::BadUsage(format!(
                            "--tick-hz {hz} is out of range (max {MAX_TICK_RATE})"
                        ));
                    }
                    Err(_) => {
                        return Invocation::BadUsage(format!("--tick-hz {hz} is out of range"));
                    }
                },
                Err(message) => return Invocation::BadUsage(message),
            },
            "--title" => match args.next() {
                Some(title) => options.title = title,
                None => return Invocation::BadUsage("--title needs a value".to_string()),
            },
            other => {
                return Invocation::BadUsage(format!("unrecognized argument `{other}`"));
            }
        }
    }
    Invocation::Run(Box::new(options))
}

fn take_number(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u64, String> {
    let Some(value) = args.next() else {
        return Err(format!("{flag} needs a value"));
    };
    value
        .parse()
        .map_err(|_| format!("{flag} needs a number, not `{value}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Invocation {
        parse(args.iter().map(|arg| (*arg).to_string()))
    }

    fn options(args: &[&str]) -> Options {
        match parse_args(args) {
            Invocation::Run(options) => *options,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn no_arguments_is_a_windowed_run() {
        let options = options(&[]);
        assert!(!options.headless);
        assert_eq!(options.frames, None);
        assert_eq!(options.tick_hz, 60);
    }

    /// Switching the debug overlay on is one flag, and the default follows the
    /// build profile rather than a constant.
    #[test]
    fn the_debug_overlay_flags_override_the_build_profile_default() {
        assert_eq!(options(&[]).debug_overlay, None);
        assert_eq!(
            options(&[]).debug_overlay_visible(),
            cfg!(debug_assertions),
            "the default is 'on in a dev build'",
        );
        assert!(options(&["--debug-overlay"]).debug_overlay_visible());
        assert!(!options(&["--no-debug-overlay"]).debug_overlay_visible());
        // Last flag wins, so a wrapper script can append an override.
        assert!(!options(&["--debug-overlay", "--no-debug-overlay"]).debug_overlay_visible());
        assert!(USAGE.contains("--debug-overlay"));
    }

    #[test]
    fn every_flag_parses() {
        let options = options(&[
            "--headless",
            "--frames",
            "12",
            "--tick-hz",
            "30",
            "--title",
            "a b",
            "--backend",
            "null",
            "--fullscreen",
        ]);
        assert!(options.headless);
        assert_eq!(options.frames, Some(12));
        assert_eq!(options.tick_hz, 30);
        assert!(options.fullscreen);
        assert_eq!(
            options.display_mode(),
            crcbl::shell::DisplayMode::Borderless { monitor: None },
        );
        assert_eq!(options.title, "a b");
        assert_eq!(options.backend, Some(GpuBackend::Null));
        assert_eq!(options.camera, CameraMode::Perspective);
    }

    /// `MAX_TICK_RATE` is the largest rate the clock can express — `1e9 / 1e9`
    /// is a 1 ns period — and stays accepted.
    #[test]
    fn the_largest_expressible_tick_rate_is_still_accepted() {
        assert_eq!(options(&["--tick-hz", "1000000000"]).tick_hz, 1_000_000_000);
    }

    /// Milestone 5's flag. The mode reaches the renderer as a
    /// [`Projection`](crcbl::render::Projection) and changes nothing else — see
    /// `app::CameraMode`.
    #[test]
    fn the_camera_mode_parses_both_spellings_and_defaults_to_perspective() {
        assert_eq!(options(&[]).camera, CameraMode::Perspective);
        assert_eq!(
            options(&["--camera", "ortho"]).camera,
            CameraMode::Orthographic
        );
        assert_eq!(
            options(&["--camera", "orthographic"]).camera,
            CameraMode::Orthographic
        );
        assert_eq!(
            options(&["--camera", "perspective"]).camera,
            CameraMode::Perspective
        );
    }

    /// The default is "choose one", not "null": a sandbox that silently
    /// rendered nothing because a driver was missing would look like a black
    /// window, which is the same rule `crcbl-shell` applies to headless.
    #[test]
    fn no_backend_flag_means_choose_one() {
        assert_eq!(options(&[]).backend, None);
        assert_eq!(
            options(&["--backend", "vk"]).backend,
            Some(GpuBackend::Vulkan)
        );
        assert_eq!(
            options(&["--backend", "vulkan"]).backend,
            Some(GpuBackend::Vulkan),
            "the long spelling is accepted too"
        );
    }

    #[test]
    fn help_wins_wherever_it_appears() {
        assert_eq!(parse_args(&["--headless", "-h"]), Invocation::Help);
        assert_eq!(parse_args(&["--help"]), Invocation::Help);
    }

    /// Every bad-usage path, because exit code 2 is a contract and a flag that
    /// silently parsed as something else would break a CI script quietly.
    #[test]
    fn bad_usage_is_named_rather_than_guessed() {
        for args in [
            vec!["--frames"],
            vec!["--frames", "lots"],
            vec!["--tick-hz"],
            vec!["--tick-hz", "0"],
            vec!["--tick-hz", "1000000001"],
            vec!["--tick-hz", "5000000000"],
            vec!["--title"],
            vec!["--backend"],
            vec!["--backend", "metal"],
            vec!["--camera"],
            vec!["--camera", "isometric"],
            vec!["--nope"],
            vec!["stray"],
        ] {
            assert!(
                matches!(parse_args(&args), Invocation::BadUsage(_)),
                "{args:?} should be rejected"
            );
        }
    }
}
