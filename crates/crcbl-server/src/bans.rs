//! The server-local denylist: the players a [`Host`](crate::Host) refuses at
//! the handshake, each with the reason the refused client is shown.
//!
//! Keyed by [`PlayerId`], so a ban outlives the session it was made in and
//! the client's restarts — and **holds back only a player who presents the
//! same id**: on an open server the id is self-asserted, and a client that
//! draws a new one is a new player to this list. `PlayerId`'s docs say why,
//! and what an authenticated tier would change.
//!
//! The list is the host's; where it is kept between runs is the embedding's.
//! [`Denylist::to_text`] and [`Denylist::parse`] are the file's form — a line
//! a ban, an operator can read and edit by hand — and `crcbl::lan` reads and
//! writes it beside a dedicated server's other files.

use std::collections::BTreeMap;
use std::fmt;

use crcbl_core::PlayerId;

/// The longest reason a ban keeps, in characters. A reason travels in the
/// refusal's message, which the client shows on one line; past this it is cut.
pub const MAX_BAN_REASON_CHARS: usize = 200;

/// What [`Denylist::to_text`] writes first: what the file is, for whoever
/// opens it.
const HEADER: &str = "# crcbl denylist: one banned player a line, the id, then the reason\n";

/// The players a server refuses, each with why. See the [module docs](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Denylist {
    entries: BTreeMap<PlayerId, String>,
}

impl Denylist {
    /// An empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bans `player` for `reason`, replacing any earlier reason. The reason is
    /// kept on one line — every run of whitespace, line breaks included,
    /// becomes one space — and cut to [`MAX_BAN_REASON_CHARS`]. Returns
    /// whether the player was not banned before.
    pub fn ban(&mut self, player: PlayerId, reason: &str) -> bool {
        let one_line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
        let reason: String = one_line.chars().take(MAX_BAN_REASON_CHARS).collect();
        self.entries.insert(player, reason).is_none()
    }

    /// Lifts `player`'s ban. Returns whether there was one.
    pub fn unban(&mut self, player: PlayerId) -> bool {
        self.entries.remove(&player).is_some()
    }

    /// Why `player` is banned, or `None` when they are not. An empty reason
    /// is still a ban.
    #[must_use]
    pub fn reason(&self, player: PlayerId) -> Option<&str> {
        self.entries.get(&player).map(String::as_str)
    }

    /// Every ban, by id.
    pub fn iter(&self) -> impl Iterator<Item = (PlayerId, &str)> {
        self.entries
            .iter()
            .map(|(player, reason)| (*player, reason.as_str()))
    }

    /// How many players are banned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nobody is banned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The list as its file: a comment line, then a line a ban — the id in
    /// [`PlayerId`]'s printed form, a space, and the reason — by id.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = String::from(HEADER);
        for (player, reason) in self.iter() {
            text.push_str(&player.to_string());
            if !reason.is_empty() {
                text.push(' ');
                text.push_str(reason);
            }
            text.push('\n');
        }
        text
    }

    /// The list a file holds, as [`to_text`](Self::to_text) writes it or an
    /// operator edited it: blank lines and lines starting `#` are skipped,
    /// and a line's id ends at its first blank, the rest of the line being
    /// the reason ([`ban`](Self::ban)'s rules apply). An id on two lines
    /// keeps the later reason.
    ///
    /// # Errors
    ///
    /// [`DenylistError`] naming the first line whose id does not read, and
    /// why — a list the server cannot read whole is not one it guesses at.
    pub fn parse(text: &str) -> Result<Self, DenylistError> {
        let mut list = Self::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (id, reason) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let player = id.parse().map_err(|error| DenylistError {
                line: index + 1,
                error,
            })?;
            list.ban(player, reason);
        }
        Ok(list)
    }
}

/// Why [`Denylist::parse`] refused a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenylistError {
    /// The line, counting from 1.
    pub line: usize,
    /// What was wrong with its id.
    pub error: crcbl_core::player::ParsePlayerIdError,
}

impl fmt::Display for DenylistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.error)
    }
}

impl std::error::Error for DenylistError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The file reads back to the same list** — every id and every reason,
    /// one with none among them — and a list read from it writes the same
    /// file again.
    #[test]
    fn the_file_reads_back_to_the_same_list() {
        let mut list = Denylist::new();
        assert!(list.ban(PlayerId::from_seed(1), "griefing the base"));
        assert!(list.ban(PlayerId::from_seed(2), ""));
        assert!(list.ban(PlayerId::from_seed(3), "spam"));
        let text = list.to_text();
        let read = Denylist::parse(&text).expect("the list's own file reads");
        assert_eq!(read, list);
        assert_eq!(
            read.reason(PlayerId::from_seed(1)),
            Some("griefing the base")
        );
        assert_eq!(read.reason(PlayerId::from_seed(2)), Some(""));
        assert_eq!(read.to_text(), text);
    }

    /// **An operator's edits read**: comments and blank lines are skipped,
    /// the blanks around a line are not part of it, and an id given twice
    /// keeps the later reason.
    #[test]
    fn an_operators_edits_read() {
        let id = PlayerId::from_seed(9);
        let text = format!("# banned\n\n  {id}\tfirst  \n{id} second reason\n");
        let list = Denylist::parse(&text).expect("an edited list reads");
        assert_eq!(list.len(), 1);
        assert_eq!(list.reason(id), Some("second reason"));
    }

    /// **A line whose id does not read refuses the whole file**, naming the
    /// line, rather than loading the rest and dropping a ban.
    #[test]
    fn a_line_whose_id_does_not_read_refuses_the_file() {
        let text = format!("{}\nnot-an-id reason\n", PlayerId::from_seed(1));
        let error = Denylist::parse(&text).expect_err("a bad id is refused");
        assert_eq!(error.line, 2);
        assert!(error.to_string().starts_with("line 2: "), "{error}");
    }

    /// **A reason stays on one line and within its limit**, so it can neither
    /// break the file's line-a-ban form nor overflow the refusal it rides in.
    #[test]
    fn a_reason_stays_on_one_line_and_within_its_limit() {
        let mut list = Denylist::new();
        let id = PlayerId::from_seed(4);
        list.ban(id, "two\nlines\r\n  and   blanks");
        assert_eq!(list.reason(id), Some("two lines and blanks"));
        list.ban(id, &"x".repeat(MAX_BAN_REASON_CHARS + 50));
        assert_eq!(
            list.reason(id).map(|reason| reason.chars().count()),
            Some(MAX_BAN_REASON_CHARS)
        );
        assert_eq!(
            Denylist::parse(&list.to_text()).expect("reads").reason(id),
            list.reason(id)
        );
    }

    /// **An unban lifts exactly the one ban**, and says whether there was one.
    #[test]
    fn an_unban_lifts_exactly_the_one_ban() {
        let mut list = Denylist::new();
        list.ban(PlayerId::from_seed(1), "a");
        list.ban(PlayerId::from_seed(2), "b");
        assert!(list.unban(PlayerId::from_seed(1)));
        assert!(!list.unban(PlayerId::from_seed(1)), "already lifted");
        assert_eq!(list.reason(PlayerId::from_seed(1)), None);
        assert_eq!(list.reason(PlayerId::from_seed(2)), Some("b"));
    }
}
