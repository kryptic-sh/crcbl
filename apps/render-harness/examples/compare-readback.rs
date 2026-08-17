//! `compare-readback` — the comparator half of the WebGPU parity gate.
//!
//! ```text
//! cargo run -p render-harness --example compare-readback -- <readback-dir> [options]
//! ```
//!
//! The browser half (`web/tools/render-harness-e2e.mjs`) drives every golden
//! scene through the browser GPU backend and writes each frame it read back into
//! one directory. This reads those files and compares each against the very
//! `crates/crcbl/tests/golden/<scene>.png` the native `vk`/`mtl`/`dx12`
//! `render_e2e` suite compares against, at the same
//! [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER) — so a
//! "matched" here means the same thing it means there, and a mismatch is
//! reported in the same numbers.
//!
//! # Why this is a native binary and not part of the driver
//!
//! The pixels are read back inside a headless browser, so the obvious place to
//! compare them is the JS that pulled them out. That would be a second pixel
//! comparator, tuned separately, drifting from the one every native golden test
//! already runs — and two comparators that disagree are worse than one that is
//! wrong, because neither is the answer. So the driver's job ends at writing the
//! bytes, and the comparison is `crcbl-golden`'s, unmodified.
//!
//! # The files it reads
//!
//! One per scene, named `<scene>.<width>x<height>.<order>.bin`, holding
//! `width * height * 4` bytes: row-major, top row first, in `<order>`'s memory
//! channel order (`rgba` or `bgra`). Everything needed to read the bytes is in
//! the name because the alternative is a sidecar file that can go missing, or a
//! bespoke header — and a readback whose dimensions are a guess is a comparison
//! against garbage. The harness picks the name from the swapchain format it
//! actually opened; see `apps/render-harness/src/lib.rs`.
//!
//! Every scene [`crcbl_render_harness::golden_names`] lists must have a file. A
//! scene that produced none is reported as a failure and not passed over: the
//! whole point of a gate is that it cannot go green by matching nothing.
//!
//! Exit codes: `0` every scene matched, `1` at least one did not (the table says
//! which and by how much), `2` bad usage or an unreadable input.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crcbl_golden::{ChannelOrder, Comparison, Image, Tolerance, compare, diff_image};

const USAGE: &str = "\
compare-readback — compare the browser harness's readbacks against the goldens

USAGE:
    compare-readback <READBACK-DIR> [OPTIONS]

OPTIONS:
        --golden-dir DIR    Where the reference PNGs are
                            (default crates/crcbl/tests/golden).
        --diff-dir DIR      Where to write the actual/expected/diff PNGs of a
                            mismatch (default target/golden-diff).
        --tolerance NAME    `rasteriser` (default) or `exact`.
    -h, --help              Print this text.";

/// What the arguments parsed to.
struct Args {
    readback_dir: PathBuf,
    golden_dir: PathBuf,
    diff_dir: PathBuf,
    tolerance: Tolerance,
    tolerance_name: String,
}

/// One scene's verdict.
struct Verdict {
    scene: &'static str,
    /// The numbers, or `None` when the comparison could not be made at all.
    comparison: Option<Comparison>,
    /// Why it could not be made, or what was written on a mismatch.
    detail: String,
}

impl Verdict {
    fn matched(&self) -> bool {
        self.comparison.as_ref().is_some_and(Comparison::is_match)
    }
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("compare-readback: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    run(&args)
}

fn run(args: &Args) -> ExitCode {
    println!(
        "compare-readback: {} against {} at tolerance {} (max channel delta {}, max failing \
         ratio {}, gross channel delta {}, max gross ratio {}, min ssim {})",
        args.readback_dir.display(),
        args.golden_dir.display(),
        args.tolerance_name,
        args.tolerance.max_channel_delta,
        args.tolerance.max_failing_ratio,
        args.tolerance.gross_channel_delta,
        args.tolerance.max_gross_ratio,
        args.tolerance.min_ssim,
    );

    let verdicts: Vec<Verdict> = crcbl_render_harness::golden_names()
        .map(|scene| match check(args, scene) {
            Ok(verdict) => verdict,
            Err(detail) => Verdict {
                scene,
                comparison: None,
                detail,
            },
        })
        .collect();

    print_table(&verdicts);

    let matched = verdicts.iter().filter(|v| v.matched()).count();
    println!(
        "\ncompare-readback: {matched}/{} scene(s) match their golden",
        verdicts.len()
    );
    if matched == verdicts.len() {
        return ExitCode::SUCCESS;
    }
    for verdict in verdicts.iter().filter(|v| !v.matched()) {
        eprintln!("compare-readback: {} — {}", verdict.scene, verdict.detail);
    }
    ExitCode::FAILURE
}

/// Compares one scene, or says why it could not be compared.
fn check(args: &Args, scene: &'static str) -> Result<Verdict, String> {
    let (path, width, height, order) = find_readback(&args.readback_dir, scene)?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let actual = Image::from_readback(width, height, &bytes, order).map_err(|error| {
        format!(
            "{} is {} byte(s), which is not one {width}x{height} image: {error}",
            path.display(),
            bytes.len(),
        )
    })?;

    let reference_path = args.golden_dir.join(format!("{scene}.png"));
    let reference = Image::load_png(&reference_path).map_err(|error| {
        format!(
            "could not read the golden {}: {error}",
            reference_path.display()
        )
    })?;

    let comparison = compare(&reference, &actual, &args.tolerance);
    // The distinct-colour count is diagnostics rather than a gate here, unlike
    // `compare-png`'s: what makes two blank frames pass there is that *both*
    // sides could be blank, and here one side is a committed golden that is not.
    // A blank readback therefore fails the comparison on its own — but the count
    // is still the fastest way to tell "nothing drew" from "the wrong thing
    // drew", so it is printed.
    let colors = actual.distinct_colors(64);
    let detail = if comparison.is_match() {
        format!("{} ({colors} distinct colours)", comparison.summary())
    } else {
        let written = write_artifacts(args, scene, &reference, &actual);
        format!(
            "{} ({colors} distinct colours); {written}",
            comparison.summary()
        )
    };
    Ok(Verdict {
        scene,
        comparison: Some(comparison),
        detail,
    })
}

/// Finds `scene`'s readback and reads its dimensions and channel order out of
/// the filename.
///
/// The directory is scanned rather than a name being constructed, because the
/// extent and the order are the harness's to report and not this tool's to
/// assume — a backend that hands back a different extent has to be visible, and
/// it cannot be if the file it would be in is one this tool refused to look at.
fn find_readback(dir: &Path, scene: &str) -> Result<(PathBuf, u32, u32, ChannelOrder), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("could not list {}: {error}", dir.display()))?;
    let mut found: Option<(String, (u32, u32), ChannelOrder)> = None;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not list {}: {error}", dir.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((stem, extent, order)) = parse_name(name) else {
            continue;
        };
        if stem != scene {
            continue;
        }
        if let Some((earlier, ..)) = &found {
            return Err(format!(
                "two readbacks for `{scene}` in {}: {earlier} and {name}. Which one is the frame \
                 is not this tool's guess to make; clear the directory and re-run the harness.",
                dir.display(),
            ));
        }
        found = Some((name.to_string(), extent, order));
    }
    let Some((name, (width, height), order)) = found else {
        return Err(format!(
            "no readback for `{scene}` in {} — the harness drove the scene list but wrote nothing \
             for this one, so nothing was compared",
            dir.display(),
        ));
    };
    Ok((dir.join(name), width, height, order))
}

/// Splits `<scene>.<width>x<height>.<order>.bin` into its parts.
///
/// `None` for anything else, so the directory may hold the driver's own logs
/// without this tripping over them.
fn parse_name(name: &str) -> Option<(&str, (u32, u32), ChannelOrder)> {
    let rest = name.strip_suffix(".bin")?;
    let (rest, order) = rest.rsplit_once('.')?;
    let (scene, extent) = rest.rsplit_once('.')?;
    let (width, height) = extent.split_once('x')?;
    let order = match order {
        "rgba" => ChannelOrder::Rgba,
        "bgra" => ChannelOrder::Bgra,
        _ => return None,
    };
    Some((scene, (width.parse().ok()?, height.parse().ok()?), order))
}

/// Writes the rendered frame, the reference and the diff where CI can upload
/// them, and says where they went.
fn write_artifacts(args: &Args, scene: &str, reference: &Image, actual: &Image) -> String {
    if let Err(error) = std::fs::create_dir_all(&args.diff_dir) {
        return format!("could not create {}: {error}", args.diff_dir.display());
    }
    let mut written = Vec::new();
    for (image, suffix) in [(actual, "actual"), (reference, "expected")] {
        let path = args.diff_dir.join(format!("{scene}.{suffix}.png"));
        match image.save_png(&path) {
            Ok(()) => written.push(path.display().to_string()),
            Err(error) => written.push(format!("could not write {}: {error}", path.display())),
        }
    }
    // Only where the two correspond pixel for pixel: a diff of two different
    // sizes is not a picture of anything.
    if reference.width() == actual.width() && reference.height() == actual.height() {
        let path = args.diff_dir.join(format!("{scene}.diff.png"));
        match diff_image(reference, actual).save_png(&path) {
            Ok(()) => written.push(path.display().to_string()),
            Err(error) => written.push(format!("could not write {}: {error}", path.display())),
        }
    }
    format!("wrote {}", written.join(", "))
}

fn print_table(verdicts: &[Verdict]) {
    let name_width = verdicts
        .iter()
        .map(|verdict| verdict.scene.len())
        .max()
        .unwrap_or(5)
        .max(5);
    println!("\n{:name_width$}  matched  detail", "scene");
    println!("{}", "-".repeat(name_width + 2 + 7 + 2 + 40));
    for verdict in verdicts {
        println!(
            "{:name_width$}  {:7}  {}",
            verdict.scene,
            if verdict.matched() { "yes" } else { "no" },
            verdict.detail,
        );
    }
}

/// Parses the command line. `Ok(None)` means `--help` was asked for.
fn parse(argv: Vec<String>) -> Result<Option<Args>, String> {
    let mut positional: Vec<String> = Vec::new();
    let mut golden_dir: Option<PathBuf> = None;
    let mut diff_dir: Option<PathBuf> = None;
    let mut tolerance_name = "rasteriser".to_string();

    let mut argv = argv.into_iter();
    while let Some(argument) = argv.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--golden-dir" => golden_dir = Some(PathBuf::from(value(&mut argv, "--golden-dir")?)),
            "--diff-dir" => diff_dir = Some(PathBuf::from(value(&mut argv, "--diff-dir")?)),
            "--tolerance" => tolerance_name = value(&mut argv, "--tolerance")?,
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other => positional.push(other.to_string()),
        }
    }

    let [readback_dir] = positional.as_slice() else {
        return Err(format!(
            "expected one readback directory, got {}",
            positional.len()
        ));
    };
    let tolerance = match tolerance_name.as_str() {
        "rasteriser" | "rasterizer" => Tolerance::RASTERISER,
        "exact" => Tolerance::EXACT,
        other => return Err(format!("unknown tolerance `{other}`")),
    };

    Ok(Some(Args {
        readback_dir: PathBuf::from(readback_dir),
        golden_dir: golden_dir.unwrap_or_else(default_golden_dir),
        diff_dir: diff_dir.unwrap_or_else(default_diff_dir),
        tolerance,
        tolerance_name,
    }))
}

fn value(argv: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    argv.next().ok_or_else(|| format!("{flag} needs a value"))
}

/// `crates/crcbl/tests/golden`, the directory the native suite compares
/// against, found relative to this crate rather than to the working directory.
fn default_golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/crcbl/tests/golden")
}

/// `target/golden-diff`, the directory CI already uploads.
fn default_diff_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-diff"),
        |target| PathBuf::from(target).join("golden-diff"),
    )
}
