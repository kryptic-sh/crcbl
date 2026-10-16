//! A target with no system fonts this crate can reach — `wasm32`, whose
//! browser shows a page none of its fonts' files: [`Source::None`], and every
//! lookup misses.
//!
//! The lookups still run, over a catalog of no directories and defaults of no
//! families, so that the code every platform shares is the code this one
//! runs too, rather than a stub only this target compiles.

use crate::scan::Catalog;
use crate::{Defaults, FontMatch, FontQuery, Source};

/// No families for anything.
const DEFAULTS: Defaults = Defaults {
    sans_serif: &[],
    serif: &[],
    monospace: &[],
    emoji: &[],
    cascade: &[],
};

#[derive(Debug)]
pub(crate) struct Platform {
    catalog: Catalog,
}

impl Platform {
    pub fn new() -> Self {
        Self {
            catalog: Catalog::new(Vec::new()),
        }
    }

    pub fn source(&self) -> Source {
        Source::None
    }

    pub fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        DEFAULTS.find(query, |family| self.catalog.faces_of(family))
    }

    pub fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        DEFAULTS.fallbacks(query, Vec::new(), |query| self.find(query))
    }
}
