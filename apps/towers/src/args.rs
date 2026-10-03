//! Argument parsing for the towers sample.
//!
//! ```text
//! towers [--headless] [--frames N] [--size WxH] [--tick-hz N] [--scene DIR] …
//!        [--resume] [--host [PORT] [--record FILE] | --join IP:PORT | --browse]
//! towers --serve [PORT] [--tick-hz N] [--scene DIR] [--record FILE | --resume]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size`, `--screenshot` and the debug-overlay pair, and
//! `--scene` is the one flag that is this sample's alone: it names a `.scn/`
//! directory to read the map out of instead of the committed
//! `assets/scenes/field.scn/`. The three LAN flags are
//! `crcbl::lan::LanMode::consume`'s, native builds only, as `apps/sandbox`
//! reads them too. `--serve [PORT]` is this sample's own, native only too: a
//! dedicated server with no window, renderer or player, which takes the tick
//! rate, the map and a recording and refuses every flag that only means
//! something to a window or a frame — see `crate::lan::serve` for why it is
//! not `--headless --host`. `--record <FILE>` is
//! `crcbl::replay_record::consume`'s, as the sandbox reads it, and records a
//! session this process hosts — `--host`'s or `--serve`'s. `--resume` is
//! this sample's too, native only: it opens on the run the last session saved
//! between waves (`crate::save`) — solo, `--host`'s or `--serve`'s — and
//! refuses to start without one.
//! The shape is `apps/breakout/src/args.rs`'s and `apps/puppet/src/args.rs`'s —
//! the directory is read *here*, while there is still an exit code to refuse the
//! run with, and [`Options`] carries the parsed map rather than the path.
//! Everything else a player can change is a key.
//! `docs/plan/sample/07-towers.md` records the one flag rule 12 still owes
//! every sample, which is a way to hold a render path below what the device
//! offers.

use crcbl::args::{Common, Consumed};

use crate::map::Map;

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and
/// a `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
towers — co-op tower defense on one map, solo or over a LAN

USAGE:
    towers [OPTIONS]

    With none of --host, --serve, --join, --browse, --scene, --resume,
    --headless, --frames or --screenshot, towers opens on a lobby: continue
    the saved run, play solo, host, join a host on the local network, or type
    an IP:PORT to connect to. Any of them skips it. The browser build has no
    lobby, is single player, and opens on the run it saved last.

CONTROLS:
    LEFT/RIGHT           Pick a build plot
    B                    Build a tower on it. The server refuses a plot that is
                         taken and a purse that is short.
    N                    Send the next wave now rather than waiting out the
                         build phase. Waves arrive on their own either way.
    R                    Restart the run
    S                    Save the run. Taken between waves, with the field
                         clear; refused while a wave is on it. Each wave's end
                         is saved on its own too.
    ESC                  Pause, F3 the debug panel, F11 fullscreen

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
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --scene <DIR>        Read the map from a .scn/ scene directory instead of
                         the committed apps/towers/assets/scenes/field.scn.
                         DIR is the scene directory itself, the one holding
                         scene.ron. A directory that is not a scene is refused
                         by key, line and column, and a layout the field cannot
                         hold is refused by the waypoint, leg or plot at fault.
    --host [PORT]        Host a co-op session and play in it: listen for players
                         on UDP PORT and announce the session to --browse on
                         the local network. Default: any free port, printed at
                         start.
    --serve [PORT]       Run a dedicated server on UDP PORT: the --host session
                         with no window, no renderer and no player of its own,
                         announced to --browse, on the wall clock. With nobody
                         in it the run holds still. Prints a status line as
                         players come and go and every 10 seconds. Reads a
                         console on stdin: status prints the line now, save
                         saves the run between waves, load puts the saved run
                         back, quit tells every player and stops; stdin
                         closing does not stop it. Saves each wave's end on
                         its own. Takes --tick-hz, --scene, --record and
                         --resume and no other option.
    --record <FILE>      Record the session --host or --serve runs to a new
                         .crpl file FILE, which `crcbl replay` reads: every
                         tick's state hash and every player's input, enough
                         for a host built on the same map to re-simulate it.
                         Written when the session ends — quit at the console,
                         or the window closing. A FILE that exists is refused:
                         a recording never overwrites.
    --resume             Open on the run the last session saved, solo, with
                         --host or with --serve (each keeps its own), and
                         refuse to start without one, or with one played on
                         another map. Not with --join or --browse, whose run
                         is their host's; not with --record, whose recording
                         replays from a fresh run; not with --headless, which
                         keeps no saves.
    --join <IP:PORT>     Join the co-op session at IP:PORT directly. A joiner
                         plays on the host's map, whatever --scene says.
    --browse             Look for co-op sessions on the local network, print
                         what answers, and join the first one this build can
                         play with. --host, --serve, --join and --browse
                         exclude each other, and are native builds only: the
                         browser build is single player.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked towers for.
///
/// **Not `Eq`.** A [`Map`] is a list of world-space coordinates, and a float has
/// no total equality; nothing compares two invocations for anything but a test's
/// `assert_eq!`, which [`PartialEq`] serves.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The field the run is played on: the committed one unless `--scene`
    /// named another directory. A joiner plays on the host's instead, which
    /// the host sends at join — see `crate::lan`.
    pub map: Map,
    /// Host, join or look for a co-op session — see `crate::lan`. Native
    /// builds only: web builds have no networking.
    #[cfg(not(target_arch = "wasm32"))]
    pub lan: crcbl::lan::LanMode,
    /// Serve a dedicated session on this UDP port — 0 for any free one —
    /// instead of playing: see `crate::lan::serve`. Native builds only.
    #[cfg(not(target_arch = "wasm32"))]
    pub serve: Option<u16>,
    /// Record the hosted or served session to this new `.crpl` file — see
    /// `crcbl::replay_record`. Native builds only.
    #[cfg(not(target_arch = "wasm32"))]
    pub record: Option<std::path::PathBuf>,
    /// Open on the run the last session saved — see `crate::save` — and
    /// refuse to start without one. Native builds only: the browser opens on
    /// its saved run whenever there is one.
    #[cfg(not(target_arch = "wasm32"))]
    pub resume: bool,
    /// Open on the lobby rather than on the field — see `crate::lobby`.
    ///
    /// [`parse`] sets it for a command line that chose nothing: no session
    /// flag, no `--scene`, and no flag that makes the run a script
    /// (`--headless`, `--frames`, `--screenshot`), so every script, CI run
    /// and test starts where it always did. **`false` by default**, so an
    /// `Options` built in code rather than parsed opens on the field as it
    /// always has. Native builds only: the browser has no networking.
    #[cfg(not(target_arch = "wasm32"))]
    pub lobby: bool,
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
            map: Map::built_in(),
            #[cfg(not(target_arch = "wasm32"))]
            lan: crcbl::lan::LanMode::Off,
            #[cfg(not(target_arch = "wasm32"))]
            serve: None,
            #[cfg(not(target_arch = "wasm32"))]
            record: None,
            #[cfg(not(target_arch = "wasm32"))]
            resume: false,
            #[cfg(not(target_arch = "wasm32"))]
            lobby: false,
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is this sample's own to claim or to reject.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();
    // Whether the map was chosen, which a lobby would otherwise ask about:
    // the parsed map equal to the committed one cannot tell `--scene` of the
    // committed directory from no flag.
    #[cfg(not(target_arch = "wasm32"))]
    let mut scene_given = false;

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => {}
        }
        #[cfg(not(target_arch = "wasm32"))]
        match options.lan.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::Help | Consumed::No => {}
        }
        #[cfg(not(target_arch = "wasm32"))]
        match crcbl::replay_record::consume(&mut options.record, &arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::Help | Consumed::No => {}
        }
        match arg.as_str() {
            #[cfg(not(target_arch = "wasm32"))]
            "--serve" => {
                if options.serve.is_some() {
                    return Invocation::BadUsage(SERVE_EXCLUDES.into());
                }
                // Optional, as `--host`'s is: taken only when it is a port.
                let port = match args.peek().map(|value| value.parse::<u16>()) {
                    Some(Ok(port)) => {
                        args.next();
                        port
                    }
                    Some(Err(_)) | None => 0,
                };
                options.serve = Some(port);
            }
            #[cfg(not(target_arch = "wasm32"))]
            "--resume" => options.resume = true,
            "--scene" => match args.next() {
                // Refused here rather than fallen back on: a run that quietly
                // kept the committed field when the directory it was pointed at
                // would not parse is one that played the map it always played
                // and reported nothing.
                Some(path) => match Map::read_dir(&path) {
                    Ok(map) => {
                        options.map = map;
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            scene_given = true;
                        }
                    }
                    Err(message) => return Invocation::BadUsage(message),
                },
                None => return Invocation::BadUsage("--scene needs a value".into()),
            },
            _ => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    if options.serve.is_some() {
        if options.lan != crcbl::lan::LanMode::Off {
            return Invocation::BadUsage(SERVE_EXCLUDES.into());
        }
        // Everything but the tick rate as it was before parsing: a flag a
        // server has no use for is refused rather than silently dropped.
        let untouched = Common {
            tick_hz: options.common.tick_hz,
            ..Options::default().common
        };
        if options.common != untouched {
            return Invocation::BadUsage(
                "--serve takes --tick-hz, --scene, --record and --resume and no other option: \
                 a dedicated server has no window, no renderer and no frames"
                    .into(),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    if options.record.is_some()
        && options.serve.is_none()
        && !matches!(options.lan, crcbl::lan::LanMode::Host { .. })
    {
        return Invocation::BadUsage(
            "--record records a session this process hosts: it needs --host or --serve".into(),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    if options.resume {
        if matches!(
            options.lan,
            crcbl::lan::LanMode::Join(_) | crcbl::lan::LanMode::Browse
        ) {
            return Invocation::BadUsage(
                "--resume resumes a run this process serves: a joiner plays its host's".into(),
            );
        }
        if options.record.is_some() {
            return Invocation::BadUsage(
                "--resume and --record exclude each other: a recording re-simulates from a \
                 fresh run"
                    .into(),
            );
        }
        if options.common.headless && options.serve.is_none() {
            return Invocation::BadUsage(
                "--resume reads the save a windowed run keeps, and a --headless run keeps none"
                    .into(),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        options.lobby = options.lan == crcbl::lan::LanMode::Off
            && options.serve.is_none()
            && !options.resume
            && !scene_given
            && !options.common.headless
            && options.common.frames.is_none();
    }

    Invocation::Run(options)
}

/// Why `--serve` beside another session is refused.
#[cfg(not(target_arch = "wasm32"))]
const SERVE_EXCLUDES: &str = "--host, --serve, --join and --browse exclude each other";

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

    /// The engine tests the shared flags; what is towers' to assert is that
    /// this sample's default tick rate reaches them, which the engine cannot
    /// know.
    #[test]
    fn the_defaults_are_a_windowed_run_at_this_samples_own_rate() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
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
        assert_eq!(
            parsed(&["--size", "640x480"]).common.size,
            Some(crcbl::shell::PhysicalSize::new(640, 480))
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// `--screenshot` is a flag this sample answers to rather than one it
    /// refuses, which is `with_screenshot` on the default `Common` and nothing
    /// else — see [`crcbl::args::Common::can_screenshot`].
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_screenshot_path_is_accepted_and_forces_a_headless_run() {
        let options = parsed(&["--screenshot", "frame.png"]);
        assert_eq!(
            options.common.screenshot.as_deref(),
            Some(std::path::Path::new("frame.png"))
        );
        assert!(
            options.common.headless,
            "a windowed swapchain is not a surface every backend can copy back"
        );
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// **`--scene` reads a directory at run time, and the map in it reaches the
    /// options** — and one that is not a towers map is refused rather than
    /// swapped for the committed field.
    ///
    /// The one-leg scene is what makes that assertable: a parser that accepted
    /// the flag and kept the committed field would pass any check that only
    /// counted a successful parse.
    #[test]
    fn the_scene_flag_reads_a_directory_and_refuses_one_that_is_not_a_map() {
        let dir = std::env::temp_dir().join(format!("towers-scene-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"one\", systems: [\"waypoints\", \"plots\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("env.ron"),
            "Env(camera: (position: (0.0, 32.0, 23.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.0, 0.0, 0.0))",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("waypoints.ron"),
            "Chunk(system: \"waypoints\", entities: [\n\
             (0, (order: 0, position: (-10.0, 0.0, 0.0))),\n\
             (1, (order: 1, position: (10.0, 0.0, 0.0))),\n\
             ])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("plots.ron"),
            "Chunk(system: \"plots\", entities: [(2, (label: \"north\", \
             position: (0.0, 0.0, -3.0)))])",
        )
        .expect("the temp dir is writable");

        let path = dir.to_str().expect("utf-8");
        let map = parsed(&["--scene", path]).map;
        assert_eq!(map.path().legs(), 1, "the file's path must reach the run");
        assert_eq!(map.plots().len(), 1);
        assert_eq!(map.plots()[0].label, "north");

        // A plot moved onto the lane is refused by the rule it breaks, not
        // absorbed as a map with one plot fewer.
        std::fs::write(
            dir.join("sys").join("plots.ron"),
            "Chunk(system: \"plots\", entities: [(2, (label: \"north\", \
             position: (0.0, 0.0, -0.5)))])",
        )
        .expect("the temp dir is writable");
        let message = rejected(&["--scene", path]);
        assert!(message.contains("north"), "{message}");
        assert!(message.contains("lane"), "{message}");

        assert!(
            matches!(
                parse(["--scene".to_string()].into_iter()),
                Invocation::BadUsage(_)
            ),
            "--scene at the end of an argv is a run that silently kept the built-in map"
        );
    }

    /// **The three LAN flags reach this sample's options through its own
    /// parser**, beside the shared flags and `--scene` rather than instead of
    /// them — the join `crcbl::lan`'s own tests cannot make — and solo is the
    /// default.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lan_flags_reach_the_options_beside_the_rest() {
        use crcbl::lan::LanMode;

        assert_eq!(parsed(&[]).lan, LanMode::Off);
        let hosting = parsed(&["--host", "--headless"]);
        assert_eq!(hosting.lan, LanMode::Host { port: 0 });
        assert!(
            hosting.common.headless,
            "the flag after --host is still read"
        );
        assert_eq!(
            parsed(&["--tick-hz", "30", "--host", "27015"]).lan,
            LanMode::Host { port: 27_015 }
        );
        let joining = parsed(&["--join", "192.168.1.20:27015", "--tick-hz", "30"]);
        assert_eq!(
            joining.lan,
            LanMode::Join("192.168.1.20:27015".parse().unwrap())
        );
        assert_eq!(joining.common.tick_hz, 30);
        assert_eq!(parsed(&["--browse"]).lan, LanMode::Browse);
        for flag in ["--host [PORT]", "--join <IP:PORT>", "--browse"] {
            assert!(USAGE.contains(flag), "USAGE lists {flag}");
        }
    }

    /// **Two sessions, or an address that is not one, are bad usage** — exit
    /// code 2, never a guess at which was meant.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn two_lan_modes_or_a_bad_address_are_refused() {
        assert!(rejected(&["--host", "--browse"]).contains("exclude each other"));
        assert!(rejected(&["--join", "127.0.0.1:1", "--host"]).contains("exclude each other"));
        assert!(rejected(&["--join"]).contains("--join needs a value"));
        assert!(rejected(&["--join", "localhost"]).contains("IP:PORT"));
        // Not a port, so not `--host`'s: read as an argument of its own.
        assert!(rejected(&["--host", "99999"]).contains("unknown argument: 99999"));
    }

    /// **`--serve` reaches the options with its port or without one**, beside
    /// the tick rate and the map it takes — and nothing else runs a server.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_serve_flag_reaches_the_options_with_or_without_a_port() {
        assert_eq!(parsed(&[]).serve, None);
        assert_eq!(parsed(&["--serve"]).serve, Some(0));
        let serving = parsed(&["--serve", "27015", "--tick-hz", "30"]);
        assert_eq!(serving.serve, Some(27_015));
        assert_eq!(
            serving.common.tick_hz, 30,
            "the flag after the port is read"
        );
        assert_eq!(serving.lan, crcbl::lan::LanMode::Off);
        // Not a port, so not `--serve`'s: read as a flag of its own.
        assert_eq!(parsed(&["--serve", "--tick-hz", "20"]).serve, Some(0));
        assert!(rejected(&["--serve", "99999"]).contains("unknown argument: 99999"));
        assert!(USAGE.contains("--serve [PORT]"), "USAGE lists --serve");
        let every = format!(
            "every {} seconds",
            crate::lan::serve::STATUS_INTERVAL.as_secs()
        );
        assert!(
            USAGE.contains(&every),
            "USAGE says the status line comes {every}"
        );
    }

    /// **`--serve` beside another session, or beside a flag only a window or
    /// a frame has a use for, is bad usage** — never a server that quietly
    /// dropped the flag.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_serve_flag_refuses_another_session_and_every_window_flag() {
        for argv in [
            &["--serve", "--host"][..],
            &["--host", "--serve"],
            &["--serve", "--join", "127.0.0.1:1"],
            &["--browse", "--serve"],
            &["--serve", "--serve"],
        ] {
            assert!(
                rejected(argv).contains("exclude each other"),
                "{argv:?}: {}",
                rejected(argv)
            );
        }
        for argv in [
            &["--serve", "--headless"][..],
            &["--serve", "--frames", "10"],
            &["--serve", "--backend", "null"],
            &["--serve", "--size", "640x480"],
            &["--serve", "--screenshot", "frame.png"],
            &["--serve", "--exec", "echo hi"],
            &["--debug-overlay", "--serve"],
        ] {
            assert!(
                rejected(argv).contains("--serve takes --tick-hz, --scene, --record and --resume"),
                "{argv:?}: {}",
                rejected(argv)
            );
        }
    }

    /// **`--record` takes a new file beside `--host` or `--serve`**, and is
    /// refused by name for a file that exists — a recording never overwrites
    /// — and without a session this process hosts.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_record_flag_needs_a_hosted_session_and_a_new_file() {
        let new = format!("{}/target-never-made.crpl", env!("CARGO_MANIFEST_DIR"));
        let hosting = parsed(&["--host", "--record", &new]);
        assert_eq!(hosting.record, Some(std::path::PathBuf::from(&new)));
        assert_eq!(
            parsed(&["--record", &new, "--serve"]).record.as_deref(),
            Some(std::path::Path::new(&new))
        );
        assert_eq!(parsed(&["--host"]).record, None);

        let existing = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        let refusal = rejected(&["--host", "--record", &existing]);
        assert!(refusal.contains("Cargo.toml"), "{refusal}");
        assert!(refusal.contains("never overwrites"), "{refusal}");

        for argv in [
            &["--record", &new][..],
            &["--join", "127.0.0.1:1", "--record", &new],
            &["--browse", "--record", &new],
        ] {
            assert!(
                rejected(argv).contains("it needs --host or --serve"),
                "{argv:?}: {}",
                rejected(argv)
            );
        }
        assert!(USAGE.contains("--record <FILE>"), "USAGE lists --record");
    }

    /// **A command line that chose nothing opens on the lobby, and every
    /// flag that chose something skips it** — a session, a map, or a run
    /// that is a script. A display flag chooses nothing, so it keeps the
    /// lobby.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_command_line_that_chose_nothing_opens_the_lobby_and_every_choice_skips_it() {
        assert!(parsed(&[]).lobby, "a bare towers opens on the lobby");
        assert!(parsed(&["--fullscreen", "--size", "640x480"]).lobby);
        let field = format!(
            "{}/assets/scenes/{}",
            env!("CARGO_MANIFEST_DIR"),
            crate::scene::FIELD
        );
        for argv in [
            &["--host"][..],
            &["--join", "127.0.0.1:27015"],
            &["--browse"],
            &["--serve"],
            &["--scene", &field],
            &["--headless"],
            &["--frames", "10"],
            &["--screenshot", "frame.png"],
            &["--resume"],
        ] {
            assert!(!parsed(argv).lobby, "{argv:?} opened the lobby");
        }
        assert!(
            !Options::default().lobby,
            "options built in code open on the lobby"
        );
    }

    /// **`--resume` reaches the options solo, hosting and serving**, and is
    /// refused by name beside a join, a recording or a headless run — each a
    /// run with no save it could resume into, or one a resume would break.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_resume_flag_reaches_a_run_this_process_serves_and_no_other() {
        assert!(!parsed(&[]).resume);
        assert!(parsed(&["--resume"]).resume);
        assert!(parsed(&["--host", "--resume"]).resume);
        assert!(parsed(&["--resume", "--serve", "27015"]).resume);
        let new = std::env::temp_dir()
            .join("crcbl-towers-resume-record.crpl")
            .display()
            .to_string();
        for (argv, why) in [
            (&["--join", "127.0.0.1:1", "--resume"][..], "a joiner plays"),
            (&["--resume", "--browse"], "a joiner plays"),
            (
                &["--host", "--record", &new, "--resume"],
                "exclude each other",
            ),
            (
                &["--serve", "--resume", "--record", &new],
                "exclude each other",
            ),
            (&["--resume", "--headless"], "keeps none"),
            (&["--screenshot", "frame.png", "--resume"], "keeps none"),
        ] {
            let refusal = rejected(argv);
            assert!(refusal.contains(why), "{argv:?}: {refusal}");
        }
        assert!(USAGE.contains("--resume"), "USAGE lists --resume");
    }

    /// With no `--scene`, the map is the committed one — the browser's only path,
    /// since a wasm build has no directory to point the flag at.
    #[test]
    fn the_default_map_is_the_committed_scene_directory() {
        assert_eq!(parsed(&[]).map, Map::built_in());
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        crcbl::args::assert_shared_help(USAGE);
        crcbl::args::assert_screenshot_help(USAGE);
        assert!(USAGE.contains("towers — co-op tower defense"));
    }
}
