//! hud's stylesheet, read through an [`AssetSource`] and re-read while the run
//! is live.
//!
//! `assets/hud.css` is compiled in, and a run reads it back through a
//! [`MemorySource`] — so the golden and the browser demo depend on no file on
//! disk, for the reason `apps/breakout/src/scene.rs` gives about its board.
//! `--styles <DIR>` swaps in a [`DirSource`](crcbl::assets::DirSource) over a
//! directory holding a `hud.css`, and that is the source an edit reaches.
//!
//! # Polled, through the reload the tree already has
//!
//! Every [`STYLESHEET_POLL_INTERVAL`] of the frame clock [`LiveSheet::poll`]
//! reads the sheet's bytes from the source, and when they differ from the last
//! bytes it offered, hands them to [`Ui::replace_stylesheet`] — which parses
//! them and, **when the parse has an error, keeps the last good sheet** and
//! returns the errors. Those errors are kept here until the next change, which
//! is what puts them on screen.
//!
//! [`Ui::load_stylesheet`] and [`Ui::poll_stylesheets`] are the other reload
//! the tree has, and they are not used, for two reasons. They stat and read
//! the path with `std::fs`, which bypasses the asset source and has no answer
//! in a browser; and the poll returns how many sheets it replaced, not why one
//! was refused, so a refused save would be a log line and nothing on screen.
//!
//! An [`AssetSource`] has no modification stamp to compare, so a change is a
//! change of bytes: a read four times a second of a file a few kilobytes long,
//! on no budget. A source still answering [`StorageError::Pending`] is asked
//! again at the next interval and changes nothing meanwhile.

use std::path::Path;
use std::time::Duration;

use crcbl::assets::{AssetSource, MemorySource, StorageError};
use crcbl::ui::style::{STYLESHEET_POLL_INTERVAL, SheetId};
use crcbl::ui::tree::Ui;

/// The sheet's key in every source, and its name in a diagnostic.
pub const SHEET_KEY: &str = "hud.css";

/// `assets/hud.css`, as it is committed: the sheet a run starts on, and the
/// last good sheet until a better one is read.
pub const BUILT_IN_CSS: &str = include_str!("../assets/hud.css");

/// The committed sheet, as a source with no filesystem under it, keyed under
/// [`SHEET_KEY`].
#[must_use]
pub fn built_in_source() -> MemorySource {
    let mut source = MemorySource::new();
    source
        .insert(Path::new(SHEET_KEY), BUILT_IN_CSS.as_bytes().to_vec())
        .expect("the sheet's key is a legal asset key");
    source
}

/// One stylesheet a [`Ui`] holds, kept in step with its source.
#[derive(Debug)]
pub struct LiveSheet {
    source: Box<dyn AssetSource>,
    sheet: SheetId,
    /// The bytes last handed to the reload, parsed or refused. A refused save
    /// is not offered again: it would fail again, and the next change is what
    /// tries again.
    offered: Vec<u8>,
    /// The frame clock at the last read, `None` before the first.
    last_poll: Option<Duration>,
    /// Why the sheet showing is not the one in the source, while it is not.
    error: Option<String>,
}

impl LiveSheet {
    /// Adds [`BUILT_IN_CSS`] to `ui` and watches `source` for a different one.
    ///
    /// The first [`poll`](Self::poll) reads `source` at once, so a `--styles`
    /// directory's sheet is showing from the first frame; the built-in source's
    /// bytes are the ones already offered, so reading them changes nothing.
    pub fn new(ui: &mut Ui, source: Box<dyn AssetSource>) -> Self {
        Self {
            source,
            sheet: ui.add_stylesheet(SHEET_KEY, BUILT_IN_CSS),
            offered: BUILT_IN_CSS.as_bytes().to_vec(),
            last_poll: None,
            error: None,
        }
    }

    /// Reads the sheet again if [`STYLESHEET_POLL_INTERVAL`] has passed on
    /// `now`, and replaces it in `ui` if its bytes changed. A replacement takes
    /// effect at the next [`Ui::begin_frame`].
    pub fn poll(&mut self, ui: &mut Ui, now: Duration) {
        if self
            .last_poll
            .is_some_and(|last| now.saturating_sub(last) < STYLESHEET_POLL_INTERVAL)
        {
            return;
        }
        self.last_poll = Some(now);

        let bytes = match self.source.read(Path::new(SHEET_KEY)) {
            Ok(bytes) => bytes,
            Err(StorageError::Pending(_)) => return,
            Err(error) => {
                self.error = Some(format!(
                    "{SHEET_KEY}: cannot be read ({error}); keeping the last good sheet"
                ));
                // Whatever arrives next is a change, even the bytes that were
                // showing before the file went away.
                self.offered.clear();
                return;
            }
        };
        if bytes == self.offered {
            return;
        }
        self.error = match std::str::from_utf8(&bytes) {
            Err(error) => Some(format!(
                "{SHEET_KEY}: is not UTF-8 ({error}); keeping the last good sheet"
            )),
            Ok(css) => match ui.replace_stylesheet(self.sheet, css) {
                Ok(()) => None,
                Err(errors) => Some(refusal(&errors)),
            },
        };
        self.offered = bytes;
    }

    /// Why the sheet showing is not the one in the source, while it is not:
    /// `None` once a good sheet has been read.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// What a refused save says on screen: how many errors kept the last good
/// sheet, and the first of them, located — the one to fix first.
fn refusal(errors: &[crcbl::ui::style::Diagnostic]) -> String {
    let first = errors
        .first()
        .map_or_else(String::new, |error| format!("\n{error}"));
    format!(
        "{SHEET_KEY}: {} error(s); keeping the last good sheet{first}",
        errors.len()
    )
}
