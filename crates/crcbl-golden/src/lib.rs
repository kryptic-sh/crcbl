//! Golden-image comparison for render tests.
//!
//! ```text
//! offscreen render ──readback──▶ Image ──▶ Golden::check ──▶ pass
//!                                            │
//!                                            ├─ fail: actual + diff → target/golden-diff/
//!                                            └─ CRCBL_BLESS=1: overwrite the reference
//! ```
//!
//! `docs/plan/12-testing.md` schedules this for P1 — "lavapipe render e2e +
//! golden-image tooling (`--bless`)" — and specifies the shape: "`crcbl
//! screenshot` output vs checked-in references; compare with per-pixel
//! tolerance + SSIM-style metric (rasterizers differ slightly); regenerate via
//! `--bless` flag; diffs uploaded as CI artifacts on failure."
//!
//! # Two rasterisers is the requirement, not an accident
//!
//! The gate runs on **lavapipe** in CI and on **radv** on a developer's machine,
//! and those two do not agree bit for bit. That is the whole reason there is a
//! tolerance rather than an equality check, and getting the tolerance right is
//! most of the value here — too tight and every CI run is a false alarm, too
//! loose and the gate passes a blank frame.
//!
//! ## What the two actually differ by
//!
//! Measured on P1.2's triangle — 256×192, `Rgba8UnormSrgb`, the same committed
//! SPIR-V, reference blessed on radv (AMD RX 7900 XTX, Mesa 26.1.5) and
//! compared against lavapipe (LLVMpipe, LLVM 22.1.8):
//!
//! | | value |
//! | --- | --- |
//! | pixels differing at all | **1 882 of 49 152 — 3.83%** |
//! | max per-channel delta | **1** |
//! | pixels over a delta of 2 | 0 |
//! | mean absolute error | 0.0102 |
//! | RMSE | 0.1008 |
//! | block SSIM | 0.999933 |
//!
//! The shape of that is the interesting part, and it is not what a first guess
//! would predict. The difference is **not** confined to the triangle's edges:
//! 3.83% is far more than the perimeter, and it is spread across the whole
//! interior. Every one of those pixels is off by exactly **one** level in a
//! single channel, which is the two rasterisers rounding the interpolated
//! vertex colour differently on the way through the sRGB transfer function.
//! The edges, meanwhile, agree exactly — both implement Vulkan's top-left fill
//! rule, so with no MSAA there is no coverage to disagree about.
//!
//! In other words: a per-pixel bound of 0 would fail on 3.83% of the frame, and
//! a bound of 1 passes the whole thing. The naive tolerance — "allow a few
//! percent of pixels to be arbitrarily wrong" — would have been calibrated
//! against the wrong quantity entirely.
//!
//! ## P1.3 re-measured it against the HDR path, and the percentage exploded
//!
//! The same comparison on P1.3's lit-mesh frame — which renders to
//! `Rgba16Float`, samples that, tonemaps it and *then* hits the sRGB swapchain:
//!
//! | | triangle (P1.2) | lit mesh (P1.3) |
//! | --- | --- | --- |
//! | pixels differing at all | 1 882 — **3.83%** | 44 908 — **91.37%** |
//! | max per-channel delta | **1** | **1** |
//! | pixels over a delta of 2 | 0 | 0 |
//! | mean absolute error | 0.0102 | 0.2284 |
//! | block SSIM | 0.999933 | 0.999996 |
//!
//! 91% looks alarming and is not. Of the 44 908 differing pixels, **39 110 are
//! the flat background** — every single pixel of the clear colour, which radv
//! writes as `[29, 34, 48]` and lavapipe as `[29, 34, 49]`. A uniform clear
//! cannot disagree by rasterisation; what disagrees is *arithmetic*. The HDR
//! path adds a rounding step the triangle never had — linear clear → `f16`
//! store → sample → tonemap → sRGB encode — and the resulting value sits on a
//! quantisation boundary, so the two implementations land on opposite sides of
//! it across the whole frame at once.
//!
//! The lesson is that **`max_channel_delta` is the load-bearing number and
//! `max_failing_ratio` is not**. A tolerance built around "at most 2% of pixels
//! may differ" — the shape almost every golden-image harness ships with — would
//! have been correct for the triangle and would have failed the very first HDR
//! frame, on a difference of one level in one channel of the background. The
//! ratio here is a backstop for pixels that *exceed* the delta, and nothing has
//! ever reached it.
//!
//! [`Tolerance::RASTERISER`] is set from those numbers:
//!
//! * `max_channel_delta: 2` — twice the observed maximum, so a third driver
//!   that rounds a little differently still passes.
//! * `max_failing_ratio: 0.02` — the backstop for pixels that exceed that.
//!   Nothing reaches it on either frame (0 pixels differ by more than 1), and 2%
//!   of this frame is roughly twice the triangle's perimeter, so a future driver
//!   whose fill rule genuinely disagreed along an edge would still pass while a
//!   triangle that *moved* could not. Note that this ratio counts only pixels
//!   **over** `max_channel_delta` — if it counted pixels that differ at all, the
//!   HDR frame's 91% would blow straight through it for no defect at all.
//! * `min_ssim: 0.99` — four orders of magnitude below the observed 0.999933,
//!   and, as `compare`'s tests pin, still comfortably strict enough to reject a
//!   triangle shifted by six pixels.
//!
//! # Blessing
//!
//! `CRCBL_BLESS=1` rewrites the reference instead of comparing against it. The
//! harness scripts expose it as `--bless`, which is the spelling the plan asks
//! for; the environment variable is the mechanism because a test binary run
//! under `cargo nextest` has no argument of its own to read.
//!
//! Blessing is a deliberate act with a review consequence:
//! `docs/plan/12-testing.md`'s exit criterion is "A rendering change that shifts
//! output must touch a golden image (blessed intentionally) — unreviewed visual
//! drift is impossible."

pub mod compare;
pub mod image;

use std::path::{Path, PathBuf};

pub use compare::{Comparison, Failure, Tolerance, compare, diff_image, ssim};
pub use image::{ChannelOrder, Image, ImageError};

/// The environment variable that turns a comparison into a regeneration.
pub const BLESS_ENV: &str = "CRCBL_BLESS";

/// Whether this process was asked to regenerate references.
///
/// Anything other than unset, empty or `0` counts, so `CRCBL_BLESS=1` and
/// `CRCBL_BLESS=true` both work and `CRCBL_BLESS=0` does not.
#[must_use]
pub fn blessing() -> bool {
    match std::env::var(BLESS_ENV) {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

/// What a check did.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// The image matched its reference.
    Match(Comparison),
    /// The reference was written, either because it did not exist or because
    /// blessing was asked for. Never a pass in CI — see [`Outcome::into_result`].
    Blessed {
        /// Where the reference now is.
        path: PathBuf,
        /// Whether the file had to be created rather than overwritten.
        created: bool,
    },
    /// The image did not match.
    Mismatch {
        /// The numbers.
        comparison: Box<Comparison>,
        /// Where the rendered image was written.
        actual: PathBuf,
        /// Where the diff was written.
        diff: PathBuf,
    },
}

/// A checked-in reference image and the rules for comparing against it.
#[derive(Clone, Debug)]
pub struct Golden {
    reference: PathBuf,
    diff_dir: PathBuf,
    tolerance: Tolerance,
    name: String,
}

impl Golden {
    /// A golden at `reference`, compared with [`Tolerance::RASTERISER`].
    ///
    /// Diffs are written under `target/golden-diff/`, which is inside the
    /// existing `/target` ignore and is therefore a path CI can upload without
    /// any new ignore rule.
    #[must_use]
    pub fn new(reference: impl Into<PathBuf>) -> Self {
        let reference = reference.into();
        let name = reference
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "golden".to_string());
        Self {
            reference,
            diff_dir: default_diff_dir(),
            tolerance: Tolerance::RASTERISER,
            name,
        }
    }

    /// Uses a different tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Writes diffs somewhere other than `target/golden-diff/`.
    #[must_use]
    pub fn with_diff_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.diff_dir = dir.into();
        self
    }

    /// The reference's path.
    #[must_use]
    pub fn reference_path(&self) -> &Path {
        &self.reference
    }

    /// Compares `actual` against the reference, or blesses it.
    ///
    /// A **missing** reference is blessed rather than failed, so adding a golden
    /// test is one commit rather than two — and the caller still finds out,
    /// because [`Outcome::Blessed`] is not a pass. It is
    /// [`Outcome::into_result`] that turns that into the CI-appropriate error.
    ///
    /// # Errors
    ///
    /// [`ImageError`] if the reference could not be read, or if the actual
    /// image or diff could not be written.
    pub fn check(&self, actual: &Image) -> Result<Outcome, ImageError> {
        let missing = !self.reference.exists();
        if blessing() || missing {
            actual.save_png(&self.reference)?;
            return Ok(Outcome::Blessed {
                path: self.reference.clone(),
                created: missing,
            });
        }

        let reference = Image::load_png(&self.reference)?;
        let comparison = compare(&reference, actual, &self.tolerance);
        if comparison.is_match() {
            return Ok(Outcome::Match(comparison));
        }

        // Written before returning, so a CI job that uploads the directory has
        // something in it even though the test is about to fail.
        let actual_path = self.diff_dir.join(format!("{}.actual.png", self.name));
        let diff_path = self.diff_dir.join(format!("{}.diff.png", self.name));
        actual.save_png(&actual_path)?;
        diff_image(&reference, actual).save_png(&diff_path)?;
        // The reference is copied beside them so the three files can be
        // compared without a checkout.
        reference
            .save_png(self.diff_dir.join(format!("{}.expected.png", self.name)))
            .ok();

        Ok(Outcome::Mismatch {
            comparison: Box::new(comparison),
            actual: actual_path,
            diff: diff_path,
        })
    }
}

impl Outcome {
    /// Turns an outcome into a pass or an explanation.
    ///
    /// **Blessing is not a pass.** A run that regenerated a reference has not
    /// checked anything, and a CI job that treated it as green would be a gate
    /// that any missing file switches off. The error says what to do next.
    ///
    /// # Errors
    ///
    /// A `String` with every number in it, ready to hand to `panic!` or
    /// `expect`.
    pub fn into_result(self) -> Result<Comparison, String> {
        match self {
            Self::Match(comparison) => Ok(comparison),
            Self::Blessed { path, created } => Err(format!(
                "golden image {} was {} rather than compared, so this run proved nothing. \
                 Review the image, commit it, and re-run without {BLESS_ENV}.",
                path.display(),
                if created { "created" } else { "re-blessed" },
            )),
            Self::Mismatch {
                comparison,
                actual,
                diff,
            } => Err(format!(
                "golden image mismatch.\n  {}\n  rendered: {}\n  diff:     {}\n\n\
                 If this change is intentional, re-run with {BLESS_ENV}=1 and commit the new \
                 reference — docs/plan/12-testing.md requires that a rendering change which \
                 shifts output touches a golden image deliberately.",
                comparison.summary(),
                actual.display(),
                diff.display(),
            )),
        }
    }
}

/// `target/golden-diff/`, found by walking up from this crate.
///
/// `CARGO_TARGET_DIR` wins when it is set, because that is what a caller with a
/// throwaway target directory expects — the `crcbl new` scaffold suite sets one.
fn default_diff_dir() -> PathBuf {
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target).join("golden-diff");
    }
    // `crates/crcbl-golden` → the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map_or_else(
            || PathBuf::from("target/golden-diff"),
            |root| root.join("target/golden-diff"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "crcbl-golden-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn picture(shift: u32) -> Image {
        let mut image = Image::filled(32, 32, [5, 10, 20, 255]);
        for y in 8..24 {
            for x in (8 + shift)..(24 + shift) {
                image.set_pixel(x, y, [200, 100, 50, 255]);
            }
        }
        image
    }

    #[test]
    fn a_missing_reference_is_created_and_reported_as_not_a_pass() {
        let dir = scratch("missing");
        let golden = Golden::new(dir.join("first.png")).with_diff_dir(dir.join("diff"));

        let outcome = golden.check(&picture(0)).expect("writes");
        assert!(matches!(outcome, Outcome::Blessed { created: true, .. }));
        assert!(
            outcome.into_result().is_err(),
            "creating a reference must not count as having checked one"
        );
        assert!(golden.reference_path().exists());

        // And the second run really does compare.
        let outcome = golden.check(&picture(0)).expect("compares");
        assert!(matches!(outcome, Outcome::Match(_)));
        outcome.into_result().expect("a match");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_mismatch_writes_the_rendered_image_and_a_diff_where_ci_can_find_them() {
        let dir = scratch("mismatch");
        let diff_dir = dir.join("diff");
        let golden = Golden::new(dir.join("shape.png")).with_diff_dir(&diff_dir);
        golden
            .check(&picture(0))
            .expect("blesses")
            .into_result()
            .ok();

        let outcome = golden.check(&picture(5)).expect("compares");
        let Outcome::Mismatch {
            comparison,
            actual,
            diff,
        } = &outcome
        else {
            panic!("a shifted square must not match: {outcome:?}");
        };
        assert!(actual.exists(), "the rendered image must be written");
        assert!(diff.exists(), "the diff must be written");
        assert!(
            diff_dir.join("shape.expected.png").exists(),
            "the reference is copied beside them so the three can be compared"
        );
        assert!(!comparison.is_match());

        let error = outcome.into_result().expect_err("a mismatch");
        assert!(error.contains("CRCBL_BLESS"), "{error}");
        assert!(error.contains("ssim"), "the numbers must be in it: {error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Blessing must overwrite, and must still refuse to report a pass — a
    /// blessed run has checked nothing.
    #[test]
    fn blessing_overwrites_and_is_still_not_a_pass() {
        let dir = scratch("bless");
        let golden = Golden::new(dir.join("shape.png")).with_diff_dir(dir.join("diff"));
        golden.check(&picture(0)).expect("creates");

        // Simulated rather than by setting the environment variable, which is
        // process-global and would race every other test in this binary.
        let replacement = picture(5);
        replacement
            .save_png(golden.reference_path())
            .expect("bless writes the new reference");
        let outcome = golden.check(&replacement).expect("compares");
        assert!(
            matches!(outcome, Outcome::Match(_)),
            "the blessed image is now the reference"
        );

        let created = Outcome::Blessed {
            path: golden.reference_path().to_path_buf(),
            created: false,
        };
        let error = created.into_result().expect_err("blessing is not a pass");
        assert!(error.contains("re-blessed"), "{error}");
        assert!(error.contains("proved nothing"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_bless_flag_reads_the_documented_values() {
        // The parsing, without touching the process environment.
        for (value, expected) in [("1", true), ("true", true), ("0", false), ("", false)] {
            let parsed = !value.is_empty() && value != "0";
            assert_eq!(parsed, expected, "{value:?}");
        }
        assert_eq!(BLESS_ENV, "CRCBL_BLESS");
    }

    #[test]
    fn diffs_default_to_a_path_inside_the_ignored_target_directory() {
        let dir = default_diff_dir();
        assert!(
            dir.ends_with("golden-diff"),
            "{} must be the diff directory",
            dir.display()
        );
        assert!(
            dir.to_string_lossy().contains("target"),
            "{} must be under target/, which .gitignore already covers",
            dir.display()
        );
    }
}
