//! `crcbl crpix` — PNG frames in, one `.crpix` sheet out.
//!
//! The conversion itself is [`crcbl_sprite::trace`], which is written and
//! tested; this is the half that knows about *files*. That division is the
//! reason several checks live here rather than being left to the library:
//!
//! * **The library names frames, not files.** `TraceError::SizeMismatch` and
//!   `TraceError::DuplicateName` carry the frame names it was given, because
//!   names are all it has. A person who typed six paths needs the path, so the
//!   size and stem checks are done here, over `(path, name)` pairs, and their
//!   messages name the file. The library's own checks remain as the backstop
//!   for every other caller.
//! * **A frame is named after its file stem.** `art/bird-up.png` becomes the
//!   frame `bird-up`. It is the only naming anyone would have to look up, it
//!   makes `crcbl crpix up.png down.png` produce a sheet whose clip lines can
//!   be written by hand, and it is what makes a stem collision — `a/bird.png`
//!   and `b/bird.png` — an error rather than a frame nobody can address.
//! * **The generated text is parsed before it is written.** `trace` returns a
//!   `String` and cannot check its own output; writing a `.crpix` this
//!   workspace's own parser refuses would be the worst possible failure, since
//!   it only shows up in whatever loads the sheet later. Parsing is also where
//!   the header numbers reported to `--json` come from, so it is not a cost
//!   paid for the check alone.

use std::path::Path;

use crcbl_sprite::load::{Rgba8, decode_png};
use crcbl_sprite::trace::{Image, Options, trace};
use crcbl_sprite::{Rect, crpix};

use crate::args::{CrpixArgs, check_sheet_name};
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// One input: where it came from, what its frame is called, and its pixels.
struct Frame<'a> {
    path: &'a Path,
    name: String,
    image: Rgba8,
}

/// Runs `crcbl crpix`.
///
/// # Errors
///
/// [`Failure`] if the output exists and `--force` was not given, if an input
/// cannot be read or decoded, if the inputs disagree about their size, if two
/// stems collide, or if the art has more colours than a palette can hold.
pub fn run(args: &CrpixArgs) -> Result<Outcome, Failure> {
    // Refused before a single PNG is read. Checking after the decode would
    // spend the whole conversion to reach the same answer, and checking after
    // the *write* is not a check at all.
    if args.output.exists() && !args.force {
        return Err(Failure::new(format!(
            "{} already exists; pass --force to overwrite it",
            args.output.display()
        )));
    }

    let mut frames = Vec::with_capacity(args.inputs.len());
    for path in &args.inputs {
        let name = frame_name(path)?;
        let bytes = std::fs::read(path)
            .map_err(|error| Failure::new(format!("cannot read {}: {error}", path.display())))?;
        let image = decode_png(&bytes)
            .map_err(|error| Failure::new(format!("{}: {error}", path.display())))?;
        frames.push(Frame { path, name, image });
    }

    let Some(first) = frames.first() else {
        // Unreachable through the parser, which requires an input. Kept as a
        // failure rather than an `expect` so the one caller that is not the
        // parser — a test — gets an error instead of a panic.
        return Err(Failure::new("`crpix` needs at least one PNG"));
    };

    for (index, frame) in frames.iter().enumerate() {
        if let Some(earlier) = frames[..index]
            .iter()
            .find(|other| other.name == frame.name)
        {
            return Err(Failure::new(format!(
                "{} and {} would both be a frame called `{}`; a frame is named after its \
                 file stem, and a clip could not tell the two apart",
                earlier.path.display(),
                frame.path.display(),
                frame.name
            )));
        }
        if (frame.image.width, frame.image.height) != (first.image.width, first.image.height) {
            return Err(Failure::new(format!(
                "{} is {}x{} and {} is {}x{}; every frame of a sheet is the same size, and \
                 padding one silently would move the art on that frame alone",
                frame.path.display(),
                frame.image.width,
                frame.image.height,
                first.path.display(),
                first.image.width,
                first.image.height
            ))
            .with("file", Json::string(frame.path.display().to_string())));
        }
    }

    // Insets that overlap leave the centre a negative size, which the geometry
    // builder turns into inside-out quads that render as nothing — a sprite
    // that disappears at one size and not another. There is no frame to check
    // against until the first PNG is decoded, so the check is here.
    if let Some(nine) = args.nine
        && !nine.fits_in(Rect::new(0, 0, first.image.width, first.image.height))
    {
        return Err(Failure::new(format!(
            "a nine-slice of {} {} {} {} does not fit in a {}x{} frame; the fixed edges \
             would overlap and the centre would draw as nothing",
            nine.left, nine.right, nine.top, nine.bottom, first.image.width, first.image.height
        )));
    }

    let images: Vec<Image<'_>> = frames
        .iter()
        .map(|frame| Image {
            name: &frame.name,
            width: frame.image.width,
            height: frame.image.height,
            rgba: &frame.image.pixels,
        })
        .collect();

    let options = Options {
        nine: args.nine,
        sample: args.sample,
        clip: args.clip.as_deref().map(leak),
        hold: args.hold,
    };

    let text = trace(&images, &options)
        .map_err(|error| Failure::new(format!("cannot convert these images: {error}")))?;

    // See the module docs: what was generated has to parse, and this is also
    // where the reported header comes from.
    let art = crpix::parse(&text).map_err(|error| {
        Failure::new(format!(
            "the generated .crpix does not parse, which is a bug in `crcbl crpix`: {error}"
        ))
    })?;

    std::fs::write(&args.output, &text).map_err(|error| {
        Failure::new(format!("cannot write {}: {error}", args.output.display()))
    })?;

    let names: Vec<&str> = frames.iter().map(|frame| frame.name.as_str()).collect();
    let human = format!(
        "wrote {} ({}×{}, {} frame{}, {} colours)\n  frames: {}",
        args.output.display(),
        art.header.width,
        art.header.height,
        art.header.frames,
        if art.header.frames == 1 { "" } else { "s" },
        art.header.colours,
        names.join(" ")
    );

    Ok(Outcome {
        human,
        json: vec![
            ("path", Json::string(args.output.display().to_string())),
            ("width", Json::Number(i64::from(art.header.width))),
            ("height", Json::Number(i64::from(art.header.height))),
            ("frames", Json::Number(i64::from(art.header.frames))),
            ("colours", Json::Number(i64::from(art.header.colours))),
            ("names", Json::strings(names)),
        ],
    })
}

/// The frame name an input file gets: its stem.
fn frame_name(path: &Path) -> Result<String, Failure> {
    let Some(stem) = path.file_stem() else {
        return Err(Failure::new(format!(
            "{} has no file name to take a frame name from",
            path.display()
        )));
    };
    let Some(stem) = stem.to_str() else {
        return Err(Failure::new(format!(
            "{}: a frame name is written into the sheet as text, and this file's stem is \
             not valid UTF-8",
            path.display()
        )));
    };
    check_sheet_name(stem).map_err(|reason| {
        Failure::new(format!(
            "{}: `{stem}` cannot be a frame name because {reason}",
            path.display()
        ))
    })?;
    Ok(stem.to_string())
}

/// A clip name with the `'static` lifetime [`Options::clip`] requires.
///
/// The field is `Option<&'static str>` and a clip name typed on a command line
/// is not static, so something has to bridge the two. Leaking is the honest
/// bridge for *this* caller: it is one small allocation, at most once, in a
/// process whose whole job is to convert one sheet and exit — the same lifetime
/// a `&'static str` would have had. Widening `Options` to carry a lifetime is
/// the library-side fix and is a breaking change to a crate outside this
/// slice's scope; `docs/backlog.md` is where that belongs.
fn leak(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_frame_is_named_after_its_file_stem() {
        assert_eq!(
            frame_name(Path::new("art/sprites/bird-up.png")).expect("a usable stem"),
            "bird-up"
        );
        // The extension is what is dropped, not everything after the first dot.
        assert_eq!(
            frame_name(Path::new("bird.up.png")).expect("a usable stem"),
            "bird.up"
        );
        // The keyword check is exact-token, not substring: these contain `loop`
        // and `@` without being either keyword.
        assert_eq!(
            frame_name(Path::new("art/looped.png")).expect("a usable stem"),
            "looped"
        );
        assert_eq!(
            frame_name(Path::new("art/a@b.png")).expect("a usable stem"),
            "a@b"
        );
    }

    /// A stem the format cannot spell back is refused before anything is
    /// written, rather than producing a `.crpix` its own parser rejects.
    ///
    /// Every fixture carries a leading directory because of the colon one.
    /// Windows reads a `:` as a drive separator, but only in the first two
    /// bytes of the whole path — `parse_prefix` matches `[drive, b':', ..]`
    /// against the path's own bytes and is called once, on the path, never on a
    /// later component. So `a:b.png` is drive `A:` there and its stem is `b`,
    /// while `art/a:b.png` has no prefix, two ordinary components, and the stem
    /// `a:b` on every platform. The stem each path is expected to yield is
    /// written down beside it so the fixture asserts what it *parsed*, not only
    /// that something was refused: a platform that split these differently
    /// would fail here rather than quietly testing another string.
    #[test]
    fn a_stem_the_format_cannot_spell_is_refused() {
        for (path, stem) in [
            ("art/my art.png", "my art"),
            ("art/a:b.png", "a:b"),
            ("art/a#b.png", "a#b"),
            ("art/loop.png", "loop"),
            ("art/reverse.png", "reverse"),
            ("art/pingpong.png", "pingpong"),
            ("art/@.png", "@"),
        ] {
            let failure = match frame_name(Path::new(path)) {
                Ok(name) => panic!("`{path}` must not yield a frame name, and gave `{name}`"),
                Err(failure) => failure,
            };
            assert!(
                failure.message.contains(path),
                "the refusal must name the file: {}",
                failure.message
            );
            // The backticks matter: every stem here is also a substring of its
            // own path, so a bare `contains(stem)` would be satisfied by the
            // line above and could never fail on its own.
            assert!(
                failure.message.contains(&format!("`{stem}`")),
                "`{path}` must have been refused for the stem `{stem}`: {}",
                failure.message
            );
        }
    }

    /// The output is not touched when it exists and `--force` was not given —
    /// and the check happens before any input is read, so a run that refuses
    /// cannot have half-decoded a sheet first.
    #[test]
    fn an_existing_output_is_left_alone_without_force() {
        let directory = std::env::temp_dir().join(format!(
            "crcbl-crpix-unit-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let output = directory.join("taken.crpix");
        std::fs::write(&output, "do not tread on me").expect("the file exists");

        let args = CrpixArgs {
            // Deliberately absent: if the existence check did not come first,
            // this would fail with "cannot read" instead.
            inputs: vec![PathBuf::from("nowhere.png")],
            output: output.clone(),
            force: false,
            nine: None,
            sample: crcbl_sprite::SampleMode::default(),
            clip: None,
            hold: 1,
            json: false,
        };
        let failure = run(&args).expect_err("the output is occupied");
        assert!(failure.message.contains("--force"), "{}", failure.message);
        assert_eq!(
            std::fs::read_to_string(&output).expect("still there"),
            "do not tread on me"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }
}
