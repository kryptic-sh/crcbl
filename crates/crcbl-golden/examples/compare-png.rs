//! `compare-png` — compare two PNGs and say *where* and *by how much* they
//! differ.
//!
//! ```text
//! cargo run -p crcbl-golden --example compare-png -- <left.png> <right.png> [options]
//! ```
//!
//! This is the comparator half of the cross-backend gate
//! (`crates/crcbl/tests/run-cross-backend-e2e.sh`), and it is a *binary* rather
//! than a `#[test]` for one reason: the two frames it compares are rendered by
//! two separate processes. Which Vulkan ICD a process gets is decided by
//! `VK_DRIVER_FILES`/`VK_ICD_FILENAMES` when the instance is created, so
//! "Vulkan on radv versus wgpu on lavapipe" — the case CI's runner and a
//! developer's machine disagree about — is not expressible inside one test
//! binary at all.
//!
//! It is also the tool a human points at a CI artifact after a red build, which
//! is why it prints the same numbers on a pass as on a failure.
//!
//! Exit codes: `0` the images match, `1` they do not (or one of them is too
//! nearly blank to be evidence of anything), `2` bad usage or unreadable input.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crcbl_golden::{
    Comparison, Image, Tolerance, compare, diff_image, differing_bounds, worst_pixels,
};

const USAGE: &str = "\
compare-png — compare two PNGs with crcbl-golden's tolerance

USAGE:
    compare-png <LEFT.png> <RIGHT.png> [OPTIONS]

OPTIONS:
        --label NAME        Name for the diff files and the log lines.
        --tolerance NAME    `rasteriser` (default) or `exact`.
        --min-colors N      Refuse to pass unless both images have at least N
                            distinct colours (default 8). Two blank frames match
                            perfectly; this is what stops that counting as a pass.
        --worst N           How many differing pixels to print (default 8).
        --diff-dir DIR      Where to write the diff on failure
                            (default target/golden-diff).
    -h, --help              Print this text.";

/// What the arguments parsed to.
struct Args {
    left: PathBuf,
    right: PathBuf,
    label: String,
    tolerance: Tolerance,
    tolerance_name: String,
    min_colors: usize,
    worst: usize,
    diff_dir: PathBuf,
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("compare-png: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("compare-png: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let left = load(&args.left)?;
    let right = load(&args.right)?;

    println!(
        "compare-png[{}]: {} vs {} — tolerance {} (max channel delta {}, max failing ratio {}, \
         gross channel delta {}, max gross ratio {}, min ssim {})",
        args.label,
        args.left.display(),
        args.right.display(),
        args.tolerance_name,
        args.tolerance.max_channel_delta,
        args.tolerance.max_failing_ratio,
        args.tolerance.gross_channel_delta,
        args.tolerance.max_gross_ratio,
        args.tolerance.min_ssim,
    );

    let comparison = compare(&left, &right, &args.tolerance);
    println!("compare-png[{}]: {}", args.label, comparison.summary());

    // The anti-vacuity check, run *before* the verdict is reported: a tolerance
    // is only a tolerance if the thing it is applied to is a picture. Two
    // cleared frames agree to the byte.
    let ceiling = args.min_colors.max(1);
    let left_colors = left.distinct_colors(ceiling);
    let right_colors = right.distinct_colors(ceiling);
    println!(
        "compare-png[{}]: distinct colours (counted to {ceiling}): left {left_colors}, \
         right {right_colors}",
        args.label,
    );
    let vacuous = left_colors < args.min_colors || right_colors < args.min_colors;
    if vacuous {
        eprintln!(
            "compare-png[{}]: FAIL — a frame with fewer than {} distinct colours is not evidence \
             of anything. Either the render produced a blank frame or the scene changed; a \
             comparison of two blank frames passes every tolerance ever written.",
            args.label, args.min_colors,
        );
    }

    if comparison.is_match() && !vacuous {
        println!("compare-png[{}]: MATCH", args.label);
        return Ok(true);
    }

    if !comparison.is_match() {
        eprintln!(
            "compare-png[{}]: FAIL — {}",
            args.label,
            comparison
                .failures()
                .map(|failure| format!("{failure:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        report_where(args, &left, &right, &comparison);
    }

    // Written whichever half failed: a vacuous pair is exactly as worth looking
    // at as a mismatching one, and the CI job uploads this directory either way.
    match write_artifacts(args, &left, &right) {
        Ok(paths) => {
            for path in paths {
                eprintln!("compare-png[{}]: wrote {}", args.label, path.display());
            }
        }
        Err(message) => eprintln!("compare-png[{}]: {message}", args.label),
    }

    Ok(false)
}

/// The "where and by how much" half of the failure output.
fn report_where(args: &Args, left: &Image, right: &Image, comparison: &Comparison) {
    if left.width() != right.width() || left.height() != right.height() {
        eprintln!(
            "compare-png[{}]: sizes differ — {}x{} vs {}x{}; no pixel of one corresponds to a \
             pixel of the other, so nothing else was computed.",
            args.label,
            left.width(),
            left.height(),
            right.width(),
            right.height(),
        );
        return;
    }

    // Where, at each threshold the tolerance scores: everything that differs at
    // all, everything that drifted past the tolerance, and everything that is
    // grossly wrong. A box that covers the frame at the first threshold and
    // eight pixels at the last is a completely different bug from one that
    // covers the frame at all three.
    for (threshold, what) in [
        (0u8, "differing at all"),
        (args.tolerance.max_channel_delta, "over tolerance"),
        (args.tolerance.gross_channel_delta, "grossly wrong"),
    ] {
        match differing_bounds(left, right, threshold) {
            Some((x, y, width, height)) => eprintln!(
                "compare-png[{}]: pixels {what}: bounding box {width}x{height} at ({x},{y}) \
                 of {}x{}",
                args.label,
                left.width(),
                left.height(),
            ),
            None => eprintln!("compare-png[{}]: no pixels {what}", args.label),
        }
    }

    let worst = worst_pixels(left, right, args.worst);
    eprintln!(
        "compare-png[{}]: worst {} of {} differing pixel(s):",
        args.label,
        worst.len(),
        comparison.differing_pixels,
    );
    for pixel in worst {
        eprintln!("compare-png[{}]:   {pixel}", args.label);
    }
}

/// Copies both inputs and the rendered diff into the diff directory.
fn write_artifacts(args: &Args, left: &Image, right: &Image) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(&args.diff_dir)
        .map_err(|error| format!("could not create {}: {error}", args.diff_dir.display()))?;

    let mut written = Vec::new();
    for (image, suffix) in [(left, "left"), (right, "right")] {
        let path = args.diff_dir.join(format!("{}.{suffix}.png", args.label));
        image
            .save_png(&path)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        written.push(path);
    }
    if left.width() == right.width() && left.height() == right.height() {
        let path = args.diff_dir.join(format!("{}.diff.png", args.label));
        diff_image(left, right)
            .save_png(&path)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

fn load(path: &Path) -> Result<Image, String> {
    Image::load_png(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

/// Parses the command line. `Ok(None)` means `--help` was asked for.
fn parse(argv: Vec<String>) -> Result<Option<Args>, String> {
    let mut positional: Vec<String> = Vec::new();
    let mut label: Option<String> = None;
    let mut tolerance_name = "rasteriser".to_string();
    let mut min_colors = 8usize;
    let mut worst = 8usize;
    let mut diff_dir: Option<PathBuf> = None;

    let mut argv = argv.into_iter();
    while let Some(argument) = argv.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--label" => label = Some(value(&mut argv, "--label")?),
            "--tolerance" => tolerance_name = value(&mut argv, "--tolerance")?,
            "--min-colors" => min_colors = number(&mut argv, "--min-colors")?,
            "--worst" => worst = number(&mut argv, "--worst")?,
            "--diff-dir" => diff_dir = Some(PathBuf::from(value(&mut argv, "--diff-dir")?)),
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other => positional.push(other.to_string()),
        }
    }

    let [left, right] = positional.as_slice() else {
        return Err(format!("expected two PNG paths, got {}", positional.len()));
    };

    let tolerance = match tolerance_name.as_str() {
        "rasteriser" | "rasterizer" => Tolerance::RASTERISER,
        "exact" => Tolerance::EXACT,
        other => return Err(format!("unknown tolerance `{other}`")),
    };

    let left = PathBuf::from(left);
    let right = PathBuf::from(right);
    let label = label.unwrap_or_else(|| {
        left.file_stem().map_or_else(
            || "compare".to_string(),
            |stem| stem.to_string_lossy().into_owned(),
        )
    });

    Ok(Some(Args {
        left,
        right,
        label,
        tolerance,
        tolerance_name,
        min_colors,
        worst,
        diff_dir: diff_dir.unwrap_or_else(default_diff_dir),
    }))
}

fn value(argv: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    argv.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn number(argv: &mut impl Iterator<Item = String>, flag: &str) -> Result<usize, String> {
    let raw = value(argv, flag)?;
    raw.parse()
        .map_err(|_| format!("{flag} needs a number, got `{raw}`"))
}

/// `target/golden-diff`, the directory CI already uploads.
fn default_diff_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from("target/golden-diff"),
        |target| PathBuf::from(target).join("golden-diff"),
    )
}
