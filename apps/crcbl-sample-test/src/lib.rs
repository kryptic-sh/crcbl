//! The fixture every sample's golden suite drives its binary through.
//!
//! ```text
//! SampleRun { … }.screenshot(backend) ──▶ the compiled sample, --screenshot
//!                                            │
//!                                            ├─ its stdout: the frame count,
//!                                            │  the state, the tick count
//!                                            └─ the PNG it left behind ──▶ Image
//! ```
//!
//! # What a sample's golden suite actually is
//!
//! `apps/{asteroids,breakout,flappy,horde,hud}/tests/golden.rs` all do the same
//! thing and none of them owns a GPU: each runs the **compiled binary** with
//! `--screenshot`, reads back the file it wrote, makes a few ratio claims about
//! where the frame is bright and dark, and finally compares it against a
//! checked-in golden. `--screenshot` is an engine flag on `crcbl::args::Common`
//! rather than a per-sample one, which is the whole reason the frame under test
//! is the frame a player would have seen — menu, HUD and every pass the game
//! hung off the swapchain image included.
//!
//! Five copies of that machinery is duplicated **knowledge**: a fix to the
//! adapter-line parse or to the stale-file removal has to land five times, and
//! the copy that gets missed stays green while testing something slightly
//! different. It had already happened — see `docs/notes/samples.md`.
//!
//! # And the helpers a sample's own unit tests share
//!
//! [`ui_text`], [`row_value`] and [`headless_common`] are not about a golden at
//! all: they are what a `#[cfg(test)]` module in a sample's `src/app.rs` reads
//! a frame back with, and each is a fact about an *engine* surface — how a
//! frame's text comes off `Loop::gpu().draw_list()`, how the debug panel lays a
//! label/value pair out and that a duplicate label makes the reading
//! meaningless, and what a deterministic headless run is. Fifteen samples had
//! written them out, character for character.
//!
//! They live here because a dev-dependency is reachable from a crate's own test
//! module as well as from its `tests/`, and because this crate already exists
//! for exactly this reason. It costs the samples nothing: it is a
//! dev-dependency, so it reaches no shipped binary.
//!
//! # Why it is a crate rather than an include
//!
//! A test binary cannot reach another test binary's helper, so the only two
//! homes are a `#[path]`-included file and a crate. The crate wins because an
//! included file is compiled once per suite and each copy gets its own
//! `const`s, which is fine for five callers and surprising for the sixth.
//!
//! It cannot live in `crcbl-golden`: this spawns a sample binary and reads
//! [`crcbl::backend::BACKEND_ENV_VAR`], so it depends on `crcbl`, and
//! `crcbl-golden` is the leaf those suites already depend on. The arithmetic
//! that *doesn't* need `crcbl` went there instead — see
//! [`crcbl_golden::srgb`].

use std::path::PathBuf;
use std::process::Command;

use crcbl::args::Common;
use crcbl::backend::GpuBackend;
use crcbl::ui::draw_list::{DrawCommand, DrawList};
use crcbl_golden::{Golden, Image};

/// A deterministic headless run of `frames` frames at `tick_hz`.
///
/// The [`Common`] every sample's `#[cfg(test)]` module built for itself, and
/// the three fields on it are the whole of what makes a run a *test* run:
///
/// * **`headless`** — no window, so the suite runs on a machine with no
///   compositor.
/// * **`backend`** — [`GpuBackend::Null`], and not a detail. `headless` only
///   says "no window"; without a backend named here the loop picks the real one
///   and fails to start on any machine with no Vulkan driver, which is every
///   plain CI runner.
/// * **`frames`** — a budget, so the run terminates on its own rather than
///   being killed.
///
/// Everything else is [`Common::new`]'s, so a field added there reaches every
/// sample's tests without fifteen edits. What stays with the sample is
/// `tick_hz`, which is its own simulation rate, and the `Options` this becomes
/// the `common` of.
#[must_use]
pub fn headless_common(tick_hz: u32, frames: u64) -> Common {
    Common {
        headless: true,
        backend: Some(GpuBackend::Null),
        frames: Some(frames),
        ..Common::new(tick_hz)
    }
}

/// Every string the UI pass will draw this frame, in the order it draws them.
///
/// A frame's text is the [`DrawCommand::Text`] payloads of the list the bundle
/// handed over and nothing else, which is what lets a test read a HUD, a menu
/// or the debug panel back without a device. The list is a sample's
/// `engine.gpu().draw_list()` — the `#[cfg(test)]` accessor every bundle has.
#[must_use]
pub fn ui_text(list: &DrawList) -> Vec<String> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The rectangle of every image quad the UI pass will draw this frame, as
/// `[min.x, min.y, max.x, max.y]` in screen pixels, in the order it draws them.
///
/// A menu's art is image quads — its scrim, its window frame's nine and nine per
/// button — so this is how a test reads a menu's picture back without a device,
/// beside [`ui_text`] for its words.
#[must_use]
pub fn ui_images(list: &DrawList) -> Vec<[f32; 4]> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Image { min, max, .. } => Some([min.x, min.y, max.x, max.y]),
            _ => None,
        })
        .collect()
}

/// Asserts that a paused frame's list carries the menu's **art above the overlay
/// cut and under the menu's title**: the scrim and the frames are image quads
/// pushed after `DrawList::begin_overlay` and before the `title` string, and
/// nothing below the cut is one.
///
/// The picture half of "the pause menu covers the HUD and its words cover the
/// picture", read off the one list the UI pass draws — a menu whose art never
/// went in draws its labels over the bare game, and one whose art went in after
/// its title paints the frame over its own words.
///
/// # Panics
///
/// When any of the three does not hold, naming which.
pub fn assert_menu_art_above_the_cut_and_under(list: &DrawList, title: &str) {
    let is_image = |command: &DrawCommand| matches!(command, DrawCommand::Image { .. });
    assert!(
        !list.base_commands().iter().any(is_image),
        "menu art landed below the overlay cut, under the game's HUD"
    );
    let overlay = list.overlay_commands();
    let first_image = overlay
        .iter()
        .position(is_image)
        .expect("the paused frame's overlay holds no menu art");
    let title_at = overlay
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { text, .. } if text == title))
        .unwrap_or_else(|| panic!("the overlay holds no {title:?}"));
    assert!(
        first_image < title_at,
        "the menu's art starts at overlay command {first_image}, after its title at {title_at}"
    );
}

/// Every render pass the frame declared, in declaration order.
///
/// A pass line of the graph dump reads `[i] <kind> pass "<label>"`, and the
/// labels are the reading that separates "this was drawn" from "this was
/// composited": a renderer whose geometry is empty declares **no pass at all**,
/// so a pass missing from this list is art that never reached the frame. The
/// dump is a sample's `engine.gpu().last_dump()`, which is recorded only for a
/// bundle built with `recording_graph_dumps`.
#[must_use]
pub fn pass_labels(dump: &str) -> Vec<&str> {
    dump.lines()
        .filter_map(|line| line.split(" pass ").nth(1))
        .filter_map(|rest| rest.split('"').nth(1))
        .collect()
}

/// The value drawn immediately after the row labelled `label`, which is how the
/// debug panel lays a row out: label then value, in one draw list.
///
/// # Panics
///
/// When no row carries `label`, when nothing follows it, and — the reason this
/// is not a `find` — when **two** rows do. Row labels share one namespace
/// across every section of the panel, and two have collided already:
/// `crcbl-render`'s frame timings draw a `pending` row, and a sample's first
/// draft named one of its own the same. A reader tells them apart by the
/// heading above them; a search through the flat draw list cannot, and would
/// read whichever came first for ever after.
#[must_use]
pub fn row_value(drawn: &[String], label: &str) -> String {
    let mut matches = drawn
        .iter()
        .enumerate()
        .filter(|(_, text)| *text == label)
        .map(|(at, _)| at);
    let at = matches
        .next()
        .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
    assert!(
        matches.next().is_none(),
        "more than one {label} row in {drawn:?}, so this reads whichever the panel \
         happened to draw first"
    );
    drawn
        .get(at + 1)
        .unwrap_or_else(|| panic!("no value after {label} in {drawn:?}"))
        .clone()
}

/// Which backend must draw, from the environment.
///
/// **Required, with no default.** Every backend draws a sample's frame
/// identically by construction, so a run that fell back to another one produces
/// a frame that passes and proves nothing about the one that was wanted —
/// `crcbl::backend::open` would otherwise answer the question for you. The same
/// argument each `run-*-golden.sh` makes, made where it can be enforced.
///
/// `harness` is the script that names one, quoted back at whoever ran the suite
/// by hand.
///
/// # Panics
///
/// When [`crcbl::backend::BACKEND_ENV_VAR`] is unset.
#[must_use]
pub fn required_backend(harness: &str) -> String {
    std::env::var(crcbl::backend::BACKEND_ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{} is not set, so nothing would pin the backend and a fallback would pass. \
             Run {harness}, which names one.",
            crcbl::backend::BACKEND_ENV_VAR
        )
    })
}

/// The adapter the binary opened, read out of its own log.
///
/// From the run rather than from the environment the test exported: a variable
/// that never reached the process and a pin that was honoured look identical
/// from outside. Each `run-*-golden.sh` reads this line back out of its suite
/// for the same reason.
///
/// # Panics
///
/// When the log names no adapter, which means the run never opened one.
#[must_use]
pub fn adapter_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains(" adapter \""))
        .map(|line| line[line.find("hal: ").map_or(0, |at| at + "hal: ".len())..].to_string())
        .unwrap_or_else(|| panic!("the run never said which adapter it opened:\n{stderr}"))
}

/// `web/tools/browser-e2e.mjs` — the browser gate's driver, pulled in whole so
/// a sample can hold the game constants it writes out against its own.
///
/// `include_str!` resolves against this file, so the crate is coupled to the
/// repository's layout on purpose: move the driver and this stops compiling,
/// which is a failure nobody can read as a pass. The same argument
/// `crates/crcbl-webgpu/src/js_mirror.rs` makes about `web/engine`.
const BROWSER_E2E_MJS: &str = include_str!("../../../web/tools/browser-e2e.mjs");

/// What the browser gate's `EXPECTATIONS` writes for `field` inside `block`,
/// as the JavaScript spells it.
///
/// **The gate carries game constants nothing enforces.** Its `EXPECTATIONS`
/// tree writes numbers beside a comment naming the Rust symbol they came from —
/// how much experience a kind of foe is worth, how far a pickup reaches, how
/// many bots a map posts — and a changed constant on the Rust side reddens a
/// browser row with "the number is not the one the rules give" rather than a
/// compile error. Worse, a changed *control* (a count, a starting health) makes
/// the gate's own baseline wrong while it still passes. This is what lets a
/// sample's unit test read the driver's copy and say so without a browser.
///
/// `block` is an `EXPECTATIONS` sub-block — `loot`, `save`, `practice` — and
/// `field` one of its keys. The value comes back as the rest of that line with
/// a trailing comma removed, so `reach: 2.5,` answers `2.5` and
/// `kill: { husk: 20, adept: 35, warden: 60 },` answers the braces and all: a
/// caller compares against a string it formats from its own constants, which
/// pins the spelling and the order as well as the numbers.
///
/// Trimmed rather than folded for CRLF: `.gitattributes` leaves `*.mjs` on
/// `text=auto`, so a Windows checkout hands this file `\r\n` and an untrimmed
/// value would carry the `\r` into every comparison.
///
/// # Panics
///
/// When the driver has no such block, has more than one, or the block has no
/// such field — each of which means the gate was restructured and the mirror
/// this reads for is no longer where it was.
#[must_use]
pub fn browser_gate_expectation(block: &str, field: &str) -> String {
    let opening = format!("\n    {block}: {{");
    let mut blocks = BROWSER_E2E_MJS.match_indices(&opening).map(|(at, _)| at);
    let at = blocks
        .next()
        .unwrap_or_else(|| panic!("web/tools/browser-e2e.mjs has no {block} block"));
    assert!(
        blocks.next().is_none(),
        "web/tools/browser-e2e.mjs has more than one {block} block, so this reads whichever \
         comes first"
    );
    let key = format!("\n      {field}: ");
    let rest = &BROWSER_E2E_MJS[at..];
    let value = rest
        .find(&key)
        .map(|found| &rest[found + key.len()..])
        .unwrap_or_else(|| panic!("the {block} block has no {field}"));
    let line = value.lines().next().unwrap_or_default().trim();
    line.strip_suffix(',').unwrap_or(line).to_string()
}

/// What the browser gate's `EXPECTATIONS` writes for one demo, at `path` under
/// that demo's own block, as the JavaScript spells it.
///
/// [`browser_gate_expectation`] finds a sub-block by its name alone, which is
/// only an answer while one demo has a block of that name. `knobs` is not such
/// a name — alcove, sundial and tide each carry one — and a demo's own
/// top-level fields (`beatMs`) sit in no sub-block at all. So this starts from
/// the demo and walks down: `("puppet", &["walk", "highStep"])` reads
/// `EXPECTATIONS.puppet.walk.highStep`, and `("shard", &["beatMs"])` reads
/// `EXPECTATIONS.shard.beatMs`.
///
/// **The search is bounded by each block's closing brace**, so a field this
/// demo does not have panics rather than answering with the next demo's. The
/// value comes back as [`browser_gate_expectation`] returns it: the rest of the
/// line, trimmed, without its trailing comma.
///
/// # Panics
///
/// When `path` is empty, when the demo or a block on the path is missing,
/// appears more than once, or is written on one line, or when the last block
/// has no such field — each of which means the gate was restructured and the
/// mirror this reads for is no longer where it was.
#[must_use]
pub fn browser_gate_demo_expectation(demo: &str, path: &[&str]) -> String {
    demo_expectation_in(BROWSER_E2E_MJS, demo, path)
}

/// [`browser_gate_demo_expectation`] over any text, so its bounds can be held
/// against a driver written for the purpose.
fn demo_expectation_in(driver: &str, demo: &str, path: &[&str]) -> String {
    let (field, blocks) = path
        .split_last()
        .expect("a path names at least the field to read");
    let mut indent = "  ".to_string();
    let mut scope = gate_block(driver, &indent, demo);
    for block in blocks {
        indent.push_str("  ");
        scope = gate_block(scope, &indent, block);
    }
    indent.push_str("  ");
    let key = format!("\n{indent}{field}: ");
    let mut found = scope.match_indices(&key).map(|(at, _)| at);
    let at = found
        .next()
        .unwrap_or_else(|| panic!("EXPECTATIONS.{demo} has no {}", path.join(".")));
    assert!(
        found.next().is_none(),
        "EXPECTATIONS.{demo} writes {} more than once",
        path.join(".")
    );
    let line = scope[at + key.len()..]
        .lines()
        .next()
        .unwrap_or_default()
        .trim();
    line.strip_suffix(',').unwrap_or(line).to_string()
}

/// The body of the one block called `name` at `indent` inside `text`, from its
/// opening line to the line before its closing brace.
///
/// Matched on the newline and the opening brace only, never on the line ending
/// after it, for the CRLF reason [`browser_gate_expectation`] gives.
fn gate_block<'a>(text: &'a str, indent: &str, name: &str) -> &'a str {
    let opening = format!("\n{indent}{name}: {{");
    let mut blocks = text.match_indices(&opening).map(|(at, _)| at);
    let at = blocks
        .next()
        .unwrap_or_else(|| panic!("web/tools/browser-e2e.mjs has no {name} block here"));
    assert!(
        blocks.next().is_none(),
        "web/tools/browser-e2e.mjs has more than one {name} block here, so this would read \
         whichever comes first"
    );
    let body = &text[at + opening.len()..];
    assert!(
        body.lines().next().unwrap_or_default().trim().is_empty(),
        "the {name} block is written on one line, so it has no closing line to stop at"
    );
    let closing = format!("\n{indent}}}");
    let end = body
        .find(&closing)
        .unwrap_or_else(|| panic!("the {name} block never closes at its own indentation"));
    &body[..end]
}

/// A frame, read in blocks of a fixed half-extent.
///
/// A block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the picture: a glyph edge or a nine-slice seam
/// landing a pixel either way moves it. The half-extent is bound once because
/// every suite reads its whole frame at one size — passing it per call would be
/// an argument that never varies.
#[derive(Debug)]
pub struct Block<'a> {
    image: &'a Image,
    half: (u32, u32),
    sample: &'a str,
}

impl<'a> Block<'a> {
    /// Reads `image` in blocks reaching `half` pixels either side of a centre.
    ///
    /// `sample` is what the suite calls itself — `"breakout"`. Every line the
    /// claims below print is prefixed `<sample> golden: `, which is the label
    /// `tools/run-sample-golden.sh` reads its own checks out of.
    #[must_use]
    pub fn new(image: &'a Image, half: (u32, u32), sample: &'a str) -> Self {
        Self {
            image,
            half,
            sample,
        }
    }

    /// The mean brightness of the block around `centre`, out of 255.
    #[must_use]
    pub fn brightness(&self, centre: (u32, u32)) -> f32 {
        self.mean(centre, None)
    }

    /// The mean of one channel over the same block, out of 255.
    #[must_use]
    pub fn channel(&self, centre: (u32, u32), index: usize) -> f32 {
        self.mean(centre, Some(index))
    }

    /// **The frame has more than one thing on it.**
    ///
    /// The claim every suite's `inspect` opens with, and the one that makes the
    /// golden comparison after it mean anything: two blank frames compare
    /// perfectly, and so do two uniformly dark ones. `subject` is what this
    /// sample calls its frame, **with its article** — `"a board"`, `"an
    /// arena"` — because that is what the sentence reads back as.
    ///
    /// # Panics
    ///
    /// When fewer than `min` distinct colours are in the frame.
    pub fn distinct_enough(&self, subject: &str, min: usize) {
        let colors = self.image.distinct_colors(min);
        assert!(
            colors >= min,
            "{subject} with {colors} distinct colour(s) (counted to {min}) is not \
             evidence — nothing drew, or only the clear did"
        );
    }

    /// **Something was drawn here at all.**
    ///
    /// A floor rather than a ratio, and the half of a
    /// [`over`](Self::over) pair that stops it being satisfied by a frame that
    /// lost the thing behind: `bright > dark * ratio` holds for `dark == 0`,
    /// which is what a pass that never ran looks like. `verdict` finishes the
    /// sentence "…so " and names the pass that must have reached this point —
    /// `"the sprite pass reached nothing"`.
    ///
    /// # Panics
    ///
    /// When the block's mean brightness is not above `floor`.
    pub fn drew(&self, noun: &str, at: (u32, u32), floor: f32, verdict: &str) {
        let level = self.brightness(at);
        assert!(
            level > floor,
            "the {noun} is at {level:.1}/255, so {verdict}"
        );
    }

    /// **The bright thing is on top of the dark thing.**
    ///
    /// A ratio between two blocks rather than a level at one, because a level
    /// is a second golden written in numbers and moves whenever the art does.
    /// Each side is `(what it is, where it is)`; `verdict` finishes the
    /// sentence and is this frame's own — `"the panel is not on top of the
    /// board"`. What every suite says after it is the same, so it is added
    /// here.
    ///
    /// Both means are printed whether or not the claim holds, so the next
    /// person sizing `ratio` does not have to re-derive it.
    ///
    /// # Panics
    ///
    /// When the bright block is not at least `ratio` times the dark one.
    pub fn over(
        &self,
        bright: (&str, (u32, u32)),
        dark: (&str, (u32, u32)),
        ratio: f32,
        verdict: &str,
    ) {
        let (bright_noun, bright_at) = bright;
        let (dark_noun, dark_at) = dark;
        let lit = self.brightness(bright_at);
        let unlit = self.brightness(dark_at);
        eprintln!(
            "{} golden: {bright_noun} {lit:.1}/255, {dark_noun} {unlit:.1}/255",
            self.sample
        );
        assert!(
            lit > unlit * ratio,
            "the {bright_noun} is {lit:.1} and the {dark_noun} is {unlit:.1} — {verdict}, or \
             the whole frame has been flattened"
        );
    }

    /// **One channel of a block beats the others, in that order.**
    ///
    /// The claim a channel-order mistake fails and nothing else does: a BGRA
    /// readback written as RGBA leaves every brightness ratio happy, and the
    /// structural half of a golden comparison is computed on luma and barely
    /// moves for a swap.
    ///
    /// `reads` is `(name, index)` per channel with the **winner first**; every
    /// other named channel must be at least `ratio` below it. `floor`, where a
    /// suite gives one, is the drew-at-all bound on the winner — the same
    /// `bright > dark * ratio` hole [`drew`](Self::drew) exists for. `verdict`
    /// finishes the sentence.
    ///
    /// # Panics
    ///
    /// When the first channel does not beat every other by `ratio`, or falls
    /// below `floor`.
    pub fn channel_beats(
        &self,
        subject: &str,
        at: (u32, u32),
        reads: &[(&str, usize)],
        ratio: f32,
        floor: Option<f32>,
        verdict: &str,
    ) {
        let levels: Vec<(&str, f32)> = reads
            .iter()
            .map(|&(name, index)| (name, self.channel(at, index)))
            .collect();
        let ((_, winner), rest) = levels.split_first().expect("a channel to compare");
        let reading = levels
            .iter()
            .map(|(name, level)| format!("{name} {level:.1}"))
            .collect::<Vec<_>>();
        eprintln!("{} golden: {subject} {}", self.sample, reading.join(", "));
        assert!(
            floor.is_none_or(|floor| *winner > floor)
                && rest.iter().all(|(_, level)| *winner > level * ratio),
            "the {subject} reads {} — {verdict}",
            reading.join(" / ")
        );
    }

    /// `index` names a channel, or `None` averages the three colour channels.
    fn mean(&self, centre: (u32, u32), index: Option<usize>) -> f32 {
        let mut total = 0.0f32;
        let mut count = 0u32;
        let last = (self.image.width() - 1, self.image.height() - 1);
        for y in centre.1.saturating_sub(self.half.1)..=(centre.1 + self.half.1).min(last.1) {
            for x in centre.0.saturating_sub(self.half.0)..=(centre.0 + self.half.0).min(last.0) {
                let pixel = self.image.pixel(x, y).expect("inside the frame");
                total += match index {
                    Some(index) => f32::from(pixel[index]),
                    None => (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0,
                };
                count += 1;
            }
        }
        total / count as f32
    }
}

/// One sample binary's screenshot run, described.
///
/// Every field is required and there is no [`Default`], on purpose: a new field
/// has to be a compile error at all five call sites, because the alternative is
/// a suite that silently stops checking whatever the field was added for.
#[derive(Debug)]
pub struct SampleRun<'a> {
    /// What the sample calls itself in failure messages — `"breakout"`.
    pub name: &'a str,
    /// The compiled binary, which a caller spells `env!("CARGO_BIN_EXE_…")`.
    ///
    /// Passed in rather than derived from [`name`](Self::name): that macro
    /// resolves in the caller's own test target and nowhere else.
    pub binary: &'a str,
    /// Where the frame is written, which a caller spells
    /// `env!("CARGO_TARGET_TMPDIR")`.
    ///
    /// Cargo gives an integration test that directory for exactly this, and it
    /// is already inside the `/target` ignore — so there is no new ignore rule
    /// and a reviewer has a path to open.
    pub tmp_dir: &'a str,
    /// The file's name inside [`tmp_dir`](Self::tmp_dir) — `"board.png"`.
    pub file: &'a str,
    /// How many frames the run presents before the one that gets written.
    pub frames: u32,
    /// The extent the checked-in golden was blessed at.
    pub extent: (u32, u32),
    /// Arguments this sample needs and the others do not — `apps/horde`'s
    /// `--prefill`, which is what puts a field on its frame at all.
    pub args: &'a [&'a str],
    /// Substrings the run's summary must contain.
    ///
    /// The state the golden was blessed on, named rather than assumed:
    /// `"WaitingToStart"` for a title screen, `"Playing"` for a prefilled run.
    /// A frame drawn from another state is a picture of a different game.
    pub stdout_contains: &'a [&'a str],
    /// Whether the summary's *simulated* tick count must have moved.
    ///
    /// Off for a sample whose summary carries no such count. Where it is on it
    /// is load-bearing — see [`screenshot`](Self::screenshot).
    pub simulation_advanced: bool,
}

impl SampleRun<'_> {
    /// Runs the real binary with `--screenshot` and hands back the frame it
    /// wrote, with the adapter line it drew on.
    ///
    /// **The stale file is removed first**, and its absence is what makes the
    /// assertion here mean anything: a `--screenshot` that quietly did nothing
    /// would otherwise pass on the previous run's picture forever.
    ///
    /// **The simulated tick count is the other half**, where
    /// [`simulation_advanced`](Self::simulation_advanced) asks for it. The
    /// pictures these suites guard are of menus over still fields, so a build
    /// whose `Game::tick` did nothing presented its frames, wrote a
    /// byte-identical image and passed every pixel claim — measured, by
    /// emptying `tick`. It has to be the *simulated* count and not the loop's:
    /// the loop counts the times it called `tick` and reads the same either
    /// way, while `sim_ticks` comes from `Game::ticks_run` and goes to zero.
    /// `apps/flappy`'s golden asserted the loop's number first, passed the
    /// frozen build, and was the same defect it was written to catch. Half of
    /// [`frames`](Self::frames) rather than the exact figure, because the exact
    /// one is the accumulator's business; zero is the case that matters.
    ///
    /// # Panics
    ///
    /// When the binary fails to run, exits non-zero, writes nothing, writes a
    /// frame at another extent, or reports a summary that does not match what
    /// this run asked for.
    #[must_use]
    pub fn screenshot(&self, backend: &str) -> (Image, String) {
        let name = self.name;
        let path = PathBuf::from(self.tmp_dir).join(self.file);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("could not clear {}: {error}", path.display()),
        }

        let frames = self.frames.to_string();
        let output = Command::new(self.binary)
            .args(["--backend", backend, "--frames", &frames])
            .args(self.args)
            .args([
                // Not because a headless run needs saying — `--screenshot`
                // turns it on — but because saying it is how these suites record
                // that the picture is of the offscreen ring and not of a window.
                "--headless",
                // It is on by default in a debug build and it draws frame times.
                // A golden of a frame with `0.007 ms` written on it is a golden
                // that fails on the next machine.
                "--no-debug-overlay",
                "--screenshot",
            ])
            .arg(&path)
            .output()
            .unwrap_or_else(|error| panic!("the {name} binary runs: {error}"));

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name} exited {:?} on {backend}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status.code()
        );
        // The binary's log, re-emitted whether or not it passed. `.output()`
        // keeps the child's stderr, and a green run would otherwise show none of
        // it — so a `vk validation:` line the layer wrote there would reach
        // nothing. Each `run-*-golden.sh` reads its suite's log for exactly that
        // line and for the messenger's own announcement, and both live in the
        // child's stderr, not this process's.
        eprint!("{stderr}");

        // The run has to have played the game, not merely started and stopped.
        // A frame written by a run that never reached the simulation is a
        // picture of start-up, and the summary is the only thing that can tell
        // the two apart from out here.
        assert!(
            stdout.contains(&format!("{} frames", self.frames)),
            "the summary does not say the run presented {} frames:\n{stdout}",
            self.frames
        );
        for wanted in self.stdout_contains {
            assert!(
                stdout.contains(wanted),
                "the summary does not say `{wanted}`, so this is not the state the golden was \
                 blessed on:\n{stdout}"
            );
        }
        if self.simulation_advanced {
            let simulated: u32 = stdout
                .split_once(" simulated)")
                .and_then(|(before, _)| before.rsplit('(').next())
                .and_then(|word| word.parse().ok())
                .unwrap_or_else(|| panic!("the summary names no simulated tick count:\n{stdout}"));
            assert!(
                simulated >= self.frames / 2,
                "the simulation advanced {simulated} times over {} frames, so it was not \
                 running and this image is of a game that never started:\n{stdout}",
                self.frames
            );
        }

        assert!(
            path.exists(),
            "{name} exited 0 and wrote no {} — `--screenshot` did nothing",
            path.display()
        );
        let image = Image::load_png(&path).expect("the screenshot is a readable PNG");
        assert_eq!(
            (image.width(), image.height()),
            self.extent,
            "the binary wrote a {}x{} frame, which is not the extent the golden was blessed at",
            image.width(),
            image.height()
        );
        (image, adapter_line(&stderr))
    }

    /// **The frame, against the golden checked in beside the suite.**
    ///
    /// The last assertion each suite makes, after its own claims about where
    /// the frame is bright and dark: those say the picture is of the right
    /// thing, and this says it is the same picture as last time.
    ///
    /// The reference is `tests/golden/` + [`file`](Self::file) under
    /// `manifest_dir`, which a caller spells `env!("CARGO_MANIFEST_DIR")` — the
    /// macro resolves in the caller's own test target and nowhere else, which
    /// is why it is passed rather than read here. `subject` is what this suite
    /// calls the picture in its log — `apps/horde` writes `horde.png` and calls
    /// it the field.
    ///
    /// # Panics
    ///
    /// When the reference is unreadable, when the frame does not match it, and
    /// — through [`Outcome::into_result`](crcbl_golden::Outcome::into_result) —
    /// when `CRCBL_BLESS` rewrote it, because a blessed run is never a pass.
    pub fn compare_to_golden(
        &self,
        image: &Image,
        backend: &str,
        manifest_dir: &str,
        subject: &str,
    ) {
        let reference = PathBuf::from(manifest_dir)
            .join("tests/golden")
            .join(self.file);
        let comparison = Golden::new(reference)
            .check(image)
            .expect("the reference is readable")
            .into_result()
            .unwrap_or_else(|message| panic!("on {backend}: {message}"));
        eprintln!(
            "{} golden: {subject} on {backend} — {}",
            self.name,
            comparison.summary()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::demo_expectation_in;

    /// Two demos in the shape `web/tools/browser-e2e.mjs` writes them: each
    /// with a `knobs` block, and only the second with a `beatMs`.
    const DRIVER: &str = "const EXPECTATIONS = {\n  first: {\n    key: null,\n    knobs: {\n      \
                          centre: '0.50',\n    },\n  },\n  second: {\n    beatMs: 500,\n    \
                          knobs: {\n      centre: '0.25',\n    },\n  },\n};\n";

    #[test]
    fn a_path_reads_the_named_demos_block_and_not_a_same_named_one_elsewhere() {
        assert_eq!(
            demo_expectation_in(DRIVER, "first", &["knobs", "centre"]),
            "'0.50'"
        );
        assert_eq!(
            demo_expectation_in(DRIVER, "second", &["knobs", "centre"]),
            "'0.25'"
        );
        assert_eq!(demo_expectation_in(DRIVER, "second", &["beatMs"]), "500");
    }

    /// **The bound is the point.** An unbounded search from `first` finds
    /// `second`'s `beatMs` and answers with it, which is a mirror test passing
    /// against the wrong demo's number.
    #[test]
    #[should_panic(expected = "EXPECTATIONS.first has no beatMs")]
    fn a_field_the_demo_does_not_write_is_not_read_from_the_next_demo() {
        let _ = demo_expectation_in(DRIVER, "first", &["beatMs"]);
    }

    /// The same text with CRLF line endings, which is what a Windows checkout
    /// hands `include_str!`.
    #[test]
    fn a_crlf_driver_reads_the_same() {
        let crlf = DRIVER.replace('\n', "\r\n");
        assert_eq!(
            demo_expectation_in(&crlf, "second", &["knobs", "centre"]),
            "'0.25'"
        );
        assert_eq!(demo_expectation_in(&crlf, "second", &["beatMs"]), "500");
    }
}
