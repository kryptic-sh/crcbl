//! Stylesheets for [`crate::tree`]: UI section 3 in `docs/notes/tooling.md`.
//!
//! ```text
//! default.css (the engine's)  →  app sheets, in the order added  →  inline
//! ```
//!
//! A node's style is what its matching rules set, in cascade order, over the
//! initial values — with `color`, `font-size`, `font-family`, `line-height` and
//! `text-align` inherited from its parent — and then its builder's inline
//! [`Declaration`]s on top. Rules from the engine's [`DEFAULT_CSS`] sit below
//! every app sheet's, whatever their selectors; within one of those two origins
//! a rule of a higher [`Specificity`] tier beats a lower one, and within a tier
//! the later rule wins. Inline declarations beat every rule.
//!
//! * `selector` — the selector grammar, specificity and matching.
//! * `value` — [`Declaration`], the typed values.
//! * `property` — every property the subset has, and its value grammar.
//! * `var` — custom properties and `var()`.
//! * `sheet` — parsing with `cssparser`, and its [`Diagnostic`]s.
//! * `cascade` — the rule index, pseudo-class dependencies and the definition
//!   cache.
//!
//! # Reloading
//!
//! A sheet loaded from a path is read again when
//! [`Ui::poll_stylesheets`](crate::tree::Ui::poll_stylesheets) finds its
//! modification time or length changed, at most once per
//! [`STYLESHEET_POLL_INTERVAL`] of the caller's clock — the polled watch
//! `apps/viewer/src/watch.rs` uses, and no file-watcher dependency. A reload
//! whose parse has an error keeps the last good sheet. Every change to the set
//! of sheets takes effect at the next frame's start and bumps the stylesheet
//! generation, which every node's cached style is keyed by: one full
//! re-resolve, and nothing stale after it.
//!
//! **A write caught half-finished** parses as whatever it has so far, which is
//! usually an error — the last good sheet stays — and otherwise a shorter
//! sheet that the write's next stamp replaces a poll later.

mod cascade;
mod property;
mod selector;
mod sheet;
mod value;
mod var;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

pub(crate) use cascade::{Candidates, InheritedId, Origin, Resolver, RuleIndex};
pub(crate) use selector::{Element, NodeSelector};
pub use selector::{PseudoClasses, Selector, Specificity};
pub use sheet::{Diagnostic, Severity, Stylesheet};
pub use value::{Corners, Declaration, Sides};

/// The engine's own stylesheet: the look of the tree's widgets, below every
/// sheet an app adds.
pub const DEFAULT_CSS: &str = include_str!("default.css");

/// The shortest time between two looks at a loaded sheet's file, on the clock
/// [`crate::tree::Ui::poll_stylesheets`] is given.
///
/// A quarter of a second, as the viewer's document watch: under what an edit
/// feels instant at, and a `stat` a sheet four times a second is on no budget.
pub const STYLESHEET_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Names an app stylesheet a [`crate::tree::Ui`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SheetId(usize);

/// What style resolution did in one frame: what the UI inspector and the debug
/// panel read to see thrash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleStats {
    /// Nodes built.
    pub nodes: u32,
    /// Nodes whose candidate rules were matched, rather than their style
    /// reused from the frame before.
    pub resolves: u32,
    /// Definitions merged from matched rules, rather than found in the cache.
    pub definitions: u32,
}

/// The engine sheet, parsed once per process.
fn default_sheet() -> &'static Stylesheet {
    static SHEET: OnceLock<Stylesheet> = OnceLock::new();
    SHEET.get_or_init(|| {
        let (sheet, diagnostics) = Stylesheet::parse("crcbl-ui/default.css", DEFAULT_CSS);
        report(&diagnostics);
        sheet
    })
}

/// The index of the engine sheet alone, built once per process and shared by
/// every tree that adds nothing to it.
fn default_index() -> Arc<RuleIndex> {
    static INDEX: OnceLock<Arc<RuleIndex>> = OnceLock::new();
    INDEX
        .get_or_init(|| Arc::new(RuleIndex::build([(Origin::Engine, default_sheet())])))
        .clone()
}

fn report(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        crcbl_core::warn!("{diagnostic}");
    }
}

/// What a sheet on disk looked like when it was last read: the pair the
/// viewer's watch compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

#[derive(Debug)]
struct AppSheet {
    sheet: Arc<Stylesheet>,
    /// Where it was loaded from, for a sheet that was.
    path: Option<PathBuf>,
    /// The stamp last offered for reading; `None` also for a file that could
    /// not be stat'd.
    stamp: Option<Stamp>,
}

/// A tree's stylesheets, its cascade state and its counters.
#[derive(Debug)]
pub(crate) struct Styles {
    pub resolver: Resolver,
    app: Vec<AppSheet>,
    /// Changes to `app` not yet indexed; applied at the next frame's start.
    pending: bool,
    /// Bumped each time a change to the sheets is applied.
    pub generation: u64,
    last_poll: Option<Duration>,
    pub stats: StyleStats,
}

impl Default for Styles {
    fn default() -> Self {
        Self {
            resolver: Resolver::new(default_index()),
            app: Vec::new(),
            pending: false,
            generation: 0,
            last_poll: None,
            stats: StyleStats::default(),
        }
    }
}

impl Styles {
    /// Starts a frame: applies any change to the sheets, and trims the caches.
    pub fn begin_frame(&mut self) {
        self.stats = StyleStats::default();
        if self.pending {
            self.pending = false;
            let sheets = core::iter::once((Origin::Engine, default_sheet()))
                .chain(self.app.iter().map(|app| (Origin::App, &*app.sheet)));
            self.resolver.set_index(Arc::new(RuleIndex::build(sheets)));
            self.generation += 1;
        } else {
            self.resolver.trim();
        }
    }

    /// Adds a sheet parsed from `css`, reporting its diagnostics. With no
    /// earlier sheet to keep, whatever parsed is used, errors or not.
    pub fn add(&mut self, name: &str, css: &str) -> SheetId {
        let (sheet, diagnostics) = Stylesheet::parse(name, css);
        report(&diagnostics);
        self.push(AppSheet {
            sheet: Arc::new(sheet),
            path: None,
            stamp: None,
        })
    }

    fn push(&mut self, sheet: AppSheet) -> SheetId {
        self.app.push(sheet);
        self.pending = true;
        SheetId(self.app.len() - 1)
    }

    /// Adds the sheet at `path`, named by it, and watches it for
    /// [`Styles::poll`].
    pub fn load(&mut self, path: &Path) -> std::io::Result<SheetId> {
        let seen = stamp(path);
        let css = std::fs::read_to_string(path)?;
        let (sheet, diagnostics) = Stylesheet::parse(&path.display().to_string(), &css);
        report(&diagnostics);
        Ok(self.push(AppSheet {
            sheet: Arc::new(sheet),
            path: Some(path.to_path_buf()),
            stamp: seen,
        }))
    }

    /// Replaces a sheet with one parsed from `css` — unless that parse has an
    /// error, which keeps the sheet there is.
    pub fn replace(&mut self, id: SheetId, css: &str) -> Result<(), Vec<Diagnostic>> {
        let app = &mut self.app[id.0];
        let (sheet, diagnostics) = Stylesheet::parse(app.sheet.name(), css);
        report(&diagnostics);
        let errors: Vec<Diagnostic> = diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .collect();
        if !errors.is_empty() {
            crcbl_core::warn!(
                "{}: {} error(s); keeping the last good sheet",
                app.sheet.name(),
                errors.len()
            );
            return Err(errors);
        }
        app.sheet = Arc::new(sheet);
        self.pending = true;
        Ok(())
    }

    /// Reads again every loaded sheet whose file changed, at most once per
    /// [`STYLESHEET_POLL_INTERVAL`] of `now`. Returns how many were replaced.
    pub fn poll(&mut self, now: Duration) -> usize {
        if self
            .last_poll
            .is_some_and(|last| now.saturating_sub(last) < STYLESHEET_POLL_INTERVAL)
        {
            return 0;
        }
        self.last_poll = Some(now);

        let mut replaced = 0;
        for at in 0..self.app.len() {
            let Some(path) = self.app[at].path.clone() else {
                continue;
            };
            let seen = stamp(&path);
            if seen == self.app[at].stamp {
                continue;
            }
            // Offered once per change, read or not: bytes that failed to parse
            // would fail again, and the next real change is what tries again.
            self.app[at].stamp = seen;
            let read = if seen.is_some() {
                std::fs::read_to_string(&path)
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            };
            match read {
                Ok(css) => {
                    if self.replace(SheetId(at), &css).is_ok() {
                        replaced += 1;
                    }
                }
                Err(error) => crcbl_core::warn!(
                    "{}: cannot be read ({error}); keeping the last good sheet",
                    path.display()
                ),
            }
        }
        replaced
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The engine's own sheet parses without a single diagnostic** — a
    /// warning in it would be printed by every process that draws a panel.
    #[test]
    fn the_default_sheet_parses_cleanly() {
        let (sheet, diagnostics) = Stylesheet::parse("default.css", DEFAULT_CSS);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert!(sheet.rule_count() > 0, "the default sheet holds no rules");
    }
}
