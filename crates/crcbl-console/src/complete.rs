//! Tab completion over the gathered table.

use crate::registry::{Entry, Registry, cmp_names};
use crate::value::Kind;

/// What `Tab` fills in, and what it could have filled in.
///
/// `common` is the **token** to put in place of the one being completed, not the
/// whole line: the caller knows where the token started, and returning the line
/// would make this crate guess at a caret it cannot see. It is the longest
/// prefix every candidate shares, so filling it in never chooses between them —
/// Source's behaviour, and readline's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    /// The longest prefix the candidates share, empty when there are none.
    pub common: String,
    /// Every candidate, sorted without regard to case.
    pub candidates: Vec<&'static str>,
}

impl Registry {
    /// Complete the token at the end of `prefix`.
    ///
    /// With no whitespace in it, that token is a name and the candidates are
    /// every variable and command starting with it. With whitespace, the first
    /// token names a variable and what is being completed is its **value**,
    /// which is completable when the variable is a [`Kind::Enum`] — a bool, an
    /// int or a float has no set to offer.
    ///
    /// The value is everything after the name, spaces included, because an enum
    /// value may hold one: `debug_view ambient occlusion` is one value and not
    /// two, and [`Registry::execute`] joins the arguments the same way.
    ///
    /// Matching ignores case, the way [`lookup`](Registry::lookup) does, and the
    /// filled-in text is the declared spelling rather than the typed one.
    #[must_use]
    pub fn complete(&self, prefix: &str) -> Completion {
        let trimmed = prefix.trim_start();
        let Some((name, rest)) = trimmed.split_once(char::is_whitespace) else {
            return complete_from(self.entries().iter().map(|entry| entry.name()), trimmed);
        };

        let partial = rest.trim_start();
        let Some(Entry::Var(var)) = self.lookup(name) else {
            return Completion::default();
        };
        let Kind::Enum(values) = var.kind() else {
            return Completion::default();
        };
        complete_from(values.iter().copied(), partial)
    }
}

/// The candidates of `names` starting with `partial`, and the prefix they share.
fn complete_from(names: impl Iterator<Item = &'static str>, partial: &str) -> Completion {
    let folded = partial.to_ascii_lowercase();
    let mut candidates: Vec<&'static str> = names
        .filter(|name| name.to_ascii_lowercase().starts_with(&folded))
        .collect();
    candidates.sort_by(|a, b| cmp_names(a, b));
    let common = common_prefix(&candidates);
    Completion { common, candidates }
}

/// The longest prefix every candidate shares, spelled as the first one spells
/// it.
fn common_prefix(candidates: &[&'static str]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut shared = first.chars().count();
    for candidate in &candidates[1..] {
        shared = shared.min(
            first
                .chars()
                .zip(candidate.chars())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count(),
        );
    }
    first.chars().take(shared).collect()
}
