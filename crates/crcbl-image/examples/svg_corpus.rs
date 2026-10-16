//! Measures `crcbl_image::svg` against real icon themes: every SVG under
//! each theme directory is rendered by this crate and by the resvg
//! command-line tool at the same size, and the two are compared.
//!
//! ```text
//! cargo run --release -p crcbl-image --features svg,png --example svg_corpus -- \
//!     [--size 48] [--resvg resvg] [--resvg-arg ARG]... [--cache DIR] \
//!     [--jobs N] [--show N] [--dump DIR] [--floor NAME=FRACTION]... NAME=DIR...
//! ```
//!
//! Per theme it reports the files that render and match, those that render
//! but differ, those this crate refuses (with the reasons, most common
//! first), and those resvg itself cannot render, which are left out of the
//! fractions. `--floor` makes it exit non-zero when a theme's matching
//! fraction falls below the floor, which is how CI holds the line.
//! `--cache` keeps resvg's renders between runs, keyed by size, theme and
//! file only, so a run with other `--resvg-arg`s wants a cache of its own;
//! `--resvg-arg` passes an argument to every resvg run, such as the generic
//! font families text falls back on (`--resvg-arg --serif-family
//! --resvg-arg 'DejaVu Serif'`), so that with `svg-text` built the
//! reference draws text in the faces this crate finds rather than in none;
//! `--show` lists that many of
//! the worst differing files and an example of each refusal; `--dump` writes
//! both renders of every differing file there as PNGs, to look at.
//!
//! resvg is a tool on the machine running this, not a dependency of any
//! crate, and the themes are fetched there: neither is in the repository
//! (Papirus is GPL-3.0 and Adwaita's icons CC-BY-SA).
//!
//! # The match metric
//!
//! Two rasterisers never agree bit for bit: resvg supersamples its coverage
//! and this crate integrates it exactly, and both round to eight bits at
//! each composite. So exact equality would call every icon different. A
//! pixel differs when any premultiplied channel is more than
//! [`PIXEL_THRESHOLD`] apart, which antialiasing alone stays well inside;
//! an icon matches when at most [`DIFFERING_FRACTION`] of its pixels
//! differ, which tolerates a few edge pixels of two curves flattened
//! differently and still fails a missing shape, a wrong colour or a
//! misplaced gradient, each of which moves far more pixels than that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crcbl_image::{ImageError, Limits, Rgba8Image, png, svg};

/// How far apart, out of 255, two premultiplied channels may be before
/// their pixel counts as differing.
const PIXEL_THRESHOLD: u8 = 32;

/// The fraction of an icon's pixels that may differ while it still matches.
const DIFFERING_FRACTION: f64 = 0.01;

struct Options {
    size: u32,
    resvg: String,
    resvg_args: Vec<String>,
    cache: Option<PathBuf>,
    jobs: usize,
    show: usize,
    dump: Option<PathBuf>,
    floors: Vec<(String, f64)>,
    themes: Vec<(String, PathBuf)>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        size: 48,
        resvg: "resvg".to_owned(),
        resvg_args: Vec::new(),
        cache: None,
        jobs: std::thread::available_parallelism().map_or(1, usize::from),
        show: 0,
        dump: None,
        floors: Vec::new(),
        themes: Vec::new(),
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| arguments.next().ok_or(format!("{name} needs a value"));
        match argument.as_str() {
            "--size" => {
                options.size = value("--size")?
                    .parse()
                    .map_err(|_| "--size takes a number")?
            }
            "--resvg" => options.resvg = value("--resvg")?,
            "--resvg-arg" => options.resvg_args.push(value("--resvg-arg")?),
            "--cache" => options.cache = Some(PathBuf::from(value("--cache")?)),
            "--dump" => options.dump = Some(PathBuf::from(value("--dump")?)),
            "--jobs" => {
                options.jobs = value("--jobs")?
                    .parse()
                    .map_err(|_| "--jobs takes a number")?
            }
            "--show" => {
                options.show = value("--show")?
                    .parse()
                    .map_err(|_| "--show takes a number")?
            }
            "--floor" => {
                let floor = value("--floor")?;
                let (name, fraction) =
                    floor.split_once('=').ok_or("--floor takes NAME=FRACTION")?;
                let fraction = fraction
                    .parse()
                    .map_err(|_| "--floor's fraction is a number")?;
                options.floors.push((name.to_owned(), fraction));
            }
            theme => {
                let (name, dir) = theme
                    .split_once('=')
                    .ok_or(format!("{theme:?} is not NAME=DIR"))?;
                options.themes.push((name.to_owned(), PathBuf::from(dir)));
            }
        }
    }
    if options.themes.is_empty() {
        return Err("name at least one theme as NAME=DIR".to_owned());
    }
    Ok(options)
}

/// Every regular `.svg` file under `dir`, symbolic links left out so an
/// icon a theme links under several names counts once.
fn svg_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            svg_files(&path, out)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "svg") {
            out.push(path);
        }
    }
    Ok(())
}

enum Outcome {
    Matching,
    Differing { differing: usize, worst: u8 },
    Refused(String),
    ReferenceFailed(String),
}

/// A refusal's reason with its particulars blanked, so reasons of one kind
/// tally together.
fn reason_kind(error: &ImageError) -> String {
    let text = error.to_string();
    let mut out = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                if quoted {
                    out.push_str("\"…");
                } else {
                    out.push('"');
                }
            }
            _ if quoted => {}
            c if c.is_ascii_digit() => {
                if !out.ends_with('#') {
                    out.push('#');
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// A path in the temporary directory no other render is using.
fn scratch_path(extension: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let index = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "svg_corpus_{}_{index}.{extension}",
        std::process::id()
    ))
}

/// Runs resvg on `input`, writing `out`.
#[expect(
    clippy::disallowed_methods,
    reason = "a measuring tool run from a terminal, which has its console already; \
              crcbl_spawn::command would be this crate's only tie to crcbl-spawn"
)]
fn run_resvg(
    options: &Options,
    arguments: &[&str],
    input: &Path,
    out: &Path,
) -> Result<(), String> {
    let output = Command::new(&options.resvg)
        .args(&options.resvg_args)
        .args(arguments)
        .arg(input)
        .arg(out)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", options.resvg))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Text made safe inside a double-quoted XML attribute.
fn xml_attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// resvg's render of `file` into a `size`-square, from the cache if it is
/// there.
///
/// Asked for a size, resvg fits a drawing that is not square into a box of
/// whole pixels and stretches it to that box, which puts it up to half a
/// pixel from where a centred drawing would be. So such a drawing is drawn
/// again as an `<image>` inside a square SVG of that size, which centres it
/// exactly, as `svg::decode` does.
fn reference(
    options: &Options,
    theme: &str,
    root: &Path,
    file: &Path,
) -> Result<Rgba8Image, String> {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let out = match &options.cache {
        Some(cache) => {
            let mut out = cache
                .join(options.size.to_string())
                .join(theme)
                .join(relative);
            out.set_extension("png");
            out
        }
        None => scratch_path("png"),
    };
    let decode = |out: &Path| -> Result<Rgba8Image, String> {
        let bytes = std::fs::read(out).map_err(|error| error.to_string())?;
        png::decode(&bytes, &Limits::DEFAULT).map_err(|error| error.to_string())
    };
    let square =
        |image: &Rgba8Image| (image.width(), image.height()) == (options.size, options.size);
    // A cached render that is not square predates the wrapping below.
    if !(out.exists() && decode(&out).is_ok_and(|image| square(&image))) {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let size = options.size.to_string();
        run_resvg(options, &["-w", &size, "-h", &size], file, &out)?;
        let first = decode(&out)?;
        if !square(&first) {
            let absolute = file.canonicalize().map_err(|error| error.to_string())?;
            let wrapper = scratch_path("svg");
            let text = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{size}" height="{size}"><image width="{size}" height="{size}" xlink:href="{}"/></svg>"#,
                xml_attribute(&absolute.to_string_lossy())
            );
            std::fs::write(&wrapper, text).map_err(|error| error.to_string())?;
            let result = run_resvg(options, &[], &wrapper, &out);
            std::fs::remove_file(&wrapper).map_err(|error| error.to_string())?;
            result?;
        }
    }
    let image = decode(&out)?;
    if options.cache.is_none() {
        std::fs::remove_file(&out).map_err(|error| error.to_string())?;
    }
    Ok(image)
}

fn premultiplied(pixel: &[u8; 4]) -> [u8; 4] {
    let alpha = u32::from(pixel[3]);
    [
        ((u32::from(pixel[0]) * alpha + 127) / 255) as u8,
        ((u32::from(pixel[1]) * alpha + 127) / 255) as u8,
        ((u32::from(pixel[2]) * alpha + 127) / 255) as u8,
        pixel[3],
    ]
}

fn measure(options: &Options, theme: &str, root: &Path, file: &Path) -> Outcome {
    let ours = std::fs::read(file)
        .map_err(|error| ImageError::Malformed(error.to_string()))
        .and_then(|bytes| svg::decode(&bytes, options.size, 1, &Limits::DEFAULT));
    let theirs = match reference(options, theme, root, file) {
        Ok(theirs) => theirs,
        Err(why) => return Outcome::ReferenceFailed(why),
    };
    let ours = match ours {
        Ok(ours) => ours,
        Err(error) => return Outcome::Refused(reason_kind(&error)),
    };
    if (theirs.width(), theirs.height()) != (options.size, options.size) {
        return Outcome::ReferenceFailed(format!(
            "resvg drew {}x{}",
            theirs.width(),
            theirs.height()
        ));
    }
    let mut differing = 0;
    let mut worst = 0;
    for (a, b) in ours
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(theirs.pixels().as_chunks::<4>().0)
    {
        let (a, b) = (premultiplied(a), premultiplied(b));
        let distance = a
            .iter()
            .zip(&b)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        worst = worst.max(distance);
        if distance > PIXEL_THRESHOLD {
            differing += 1;
        }
    }
    let pixels = options.size as usize * options.size as usize;
    if differing as f64 <= DIFFERING_FRACTION * pixels as f64 {
        return Outcome::Matching;
    }
    if let Some(dump) = &options.dump {
        let name = file
            .strip_prefix(root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace(['/', '\\'], "_");
        let write = |suffix: &str, image: &Rgba8Image| {
            let path = dump.join(format!("{theme}_{name}.{suffix}.png"));
            match png::encode(image) {
                Ok(bytes) => {
                    if let Err(error) = std::fs::write(&path, bytes) {
                        eprintln!("svg_corpus: cannot write {}: {error}", path.display());
                    }
                }
                Err(error) => eprintln!("svg_corpus: cannot encode {}: {error}", path.display()),
            }
        };
        write("ours", &ours);
        write("resvg", &theirs);
    }
    Outcome::Differing { differing, worst }
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(why) => {
            eprintln!("svg_corpus: {why}");
            return ExitCode::from(2);
        }
    };
    let mut below_floor = false;
    for (theme, root) in &options.themes {
        let mut files = Vec::new();
        if let Err(error) = svg_files(root, &mut files) {
            eprintln!("svg_corpus: cannot read {}: {error}", root.display());
            return ExitCode::from(2);
        }
        files.sort();
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::with_capacity(files.len()));
        std::thread::scope(|scope| {
            for _ in 0..options.jobs.max(1) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(index) else { break };
                        let outcome = measure(&options, theme, root, file);
                        results
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((index, outcome));
                    }
                });
            }
        });
        let results = results
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut matching, mut differing, mut refused, mut failed) =
            (0usize, Vec::new(), BTreeMap::new(), Vec::new());
        for (index, outcome) in results {
            match outcome {
                Outcome::Matching => matching += 1,
                Outcome::Differing {
                    differing: count,
                    worst,
                } => differing.push((count, worst, index)),
                Outcome::Refused(reason) => {
                    refused.entry(reason).or_insert_with(Vec::new).push(index)
                }
                Outcome::ReferenceFailed(why) => failed.push((index, why)),
            }
        }
        let refused_total: usize = refused.values().map(Vec::len).sum();
        let measured = matching + differing.len() + refused_total;
        let percent = |count: usize| 100.0 * count as f64 / measured.max(1) as f64;
        println!(
            "{theme}: {measured} measured — {matching} matching ({:.1}%), {} differing ({:.1}%), {refused_total} refused ({:.1}%); {} resvg could not render",
            percent(matching),
            differing.len(),
            percent(differing.len()),
            percent(refused_total),
            failed.len()
        );
        let mut reasons: Vec<_> = refused.iter().collect();
        reasons.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        for (reason, indices) in &reasons {
            if options.show > 0 {
                println!(
                    "  refused {:>5}  {reason}  e.g. {}",
                    indices.len(),
                    files[indices[0]].display()
                );
            } else {
                println!("  refused {:>5}  {reason}", indices.len());
            }
        }
        differing.sort_by(|a, b| b.cmp(a));
        for (count, worst, index) in differing.iter().take(options.show) {
            println!(
                "  differs {count:>5} px (worst {worst:>3})  {}",
                files[*index].display()
            );
        }
        for (index, why) in failed.iter().take(options.show) {
            println!("  resvg failed  {}: {why}", files[*index].display());
        }
        let fraction = matching as f64 / measured.max(1) as f64;
        for (name, floor) in &options.floors {
            if name == theme && fraction < *floor {
                println!(
                    "  {theme}: matching fraction {fraction:.4} is below its floor of {floor}"
                );
                below_floor = true;
            }
        }
    }
    if below_floor {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
