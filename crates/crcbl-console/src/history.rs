//! The console's line history.

use std::collections::VecDeque;

/// How many lines the console remembers.
///
/// Source's default is deeper; this is what a debug session actually walks back
/// through, and the whole thing is copied nowhere — the ring is the storage.
pub const HISTORY_LINES: usize = 64;

/// The lines that were submitted, and where the up/down keys are in them.
///
/// The classic behaviour, and the part that is easy to get wrong: the line being
/// typed when the first `up` happens is **kept**, and `down` past the newest
/// entry gives it back. Without that, reaching for history throws away whatever
/// was half-typed.
#[derive(Clone, Debug, Default)]
pub struct History {
    lines: VecDeque<String>,
    /// Where `up`/`down` are, as an index into `lines`; `None` when the caller
    /// is on the line being typed rather than in the history.
    at: Option<usize>,
    /// The line that was being typed when the walk started.
    partial: String,
}

impl History {
    /// An empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The remembered lines, oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    /// How many lines are remembered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing is remembered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Remember a submitted line, and leave the walk.
    ///
    /// A blank line is not remembered, and neither is a line identical to the
    /// newest one — walking back through five copies of the same command is
    /// what a shell spares you and so does this.
    pub fn push(&mut self, line: &str) {
        self.at = None;
        self.partial.clear();
        if line.trim().is_empty() {
            return;
        }
        if self.lines.back().is_some_and(|newest| newest == line) {
            return;
        }
        if self.lines.len() == HISTORY_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line.to_owned());
    }

    /// Walk one line older. `current` is what is being typed, kept for the walk
    /// back down.
    ///
    /// Returns `None` when there is no history at all; at the oldest line it
    /// returns that line again rather than falling off the end.
    pub fn up(&mut self, current: &str) -> Option<&str> {
        if self.lines.is_empty() {
            return None;
        }
        match self.at {
            None => {
                self.partial = current.to_owned();
                self.at = Some(self.lines.len() - 1);
            }
            Some(at) => self.at = Some(at.saturating_sub(1)),
        }
        self.at.map(|at| self.lines[at].as_str())
    }

    /// Walk one line newer.
    ///
    /// Past the newest line it gives back the partial line [`up`](Self::up)
    /// kept and leaves the walk. Returns `None` when no walk is under way.
    pub fn down(&mut self) -> Option<&str> {
        let at = self.at?;
        if at + 1 < self.lines.len() {
            self.at = Some(at + 1);
            return Some(self.lines[at + 1].as_str());
        }
        self.at = None;
        Some(self.partial.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(lines: &[&str]) -> History {
        let mut history = History::new();
        for line in lines {
            history.push(line);
        }
        history
    }

    #[test]
    fn an_empty_history_has_nowhere_to_walk() {
        let mut history = History::new();
        assert!(history.is_empty());
        assert_eq!(history.up("half typed"), None);
        assert_eq!(history.down(), None);
    }

    #[test]
    fn up_walks_from_newest_to_oldest_and_stops_there() {
        let mut history = filled(&["one", "two", "three"]);
        assert_eq!(history.len(), 3);
        assert_eq!(history.up(""), Some("three"));
        assert_eq!(history.up(""), Some("two"));
        assert_eq!(history.up(""), Some("one"));
        assert_eq!(history.up(""), Some("one"));
    }

    #[test]
    fn down_walks_back_and_returns_the_line_that_was_being_typed() {
        let mut history = filled(&["one", "two"]);
        assert_eq!(history.up("half typed"), Some("two"));
        assert_eq!(history.up("ignored on the second up"), Some("one"));
        assert_eq!(history.down(), Some("two"));
        assert_eq!(history.down(), Some("half typed"));
        // And the walk is over, so `down` has nothing more to give.
        assert_eq!(history.down(), None);
    }

    #[test]
    fn a_push_ends_the_walk_so_the_next_up_starts_at_the_newest_line() {
        let mut history = filled(&["one", "two"]);
        assert_eq!(history.up(""), Some("two"));
        assert_eq!(history.up(""), Some("one"));
        history.push("three");
        assert_eq!(history.up(""), Some("three"));
    }

    #[test]
    fn a_blank_line_and_a_repeat_of_the_newest_are_not_remembered() {
        let mut history = filled(&["one", "one", "", "   ", "one"]);
        assert_eq!(history.lines().collect::<Vec<_>>(), ["one"]);
        history.push("two");
        history.push("one");
        assert_eq!(history.lines().collect::<Vec<_>>(), ["one", "two", "one"]);
    }

    #[test]
    fn the_history_is_bounded_and_drops_the_oldest_line() {
        let mut history = History::new();
        for i in 0..HISTORY_LINES + 10 {
            history.push(&format!("line {i}"));
        }
        assert_eq!(history.len(), HISTORY_LINES);
        let oldest = format!("line {}", 10);
        let newest = format!("line {}", HISTORY_LINES + 9);
        assert_eq!(history.lines().next(), Some(oldest.as_str()));
        assert_eq!(history.lines().last(), Some(newest.as_str()));
    }
}
