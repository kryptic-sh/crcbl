//! A dedicated server's denylist file: the host's
//! [`Denylist`], kept between runs.
//!
//! The file is [`Denylist::to_text`]'s form — a line a ban, which an operator
//! can read and edit by hand while the server is stopped — written whole
//! through [`write_atomic`](crate::store::write_atomic) after every change, so
//! a server killed mid-write keeps the list from before it. A missing file is
//! an empty list: the first ban writes it. A file that does not read whole
//! refuses to start the list rather than serve with a ban dropped.
//!
//! [`LanHost::keep_bans`](super::LanHost::keep_bans) is what reads it, and
//! [`LanHost::ban`](super::LanHost::ban) and
//! [`LanHost::unban`](super::LanHost::unban) what write it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::server::{Denylist, DenylistError};
use crate::store::StorageError;

/// Why a denylist file was not read or not written.
#[derive(Debug)]
pub enum BanFileError {
    /// The file is there and would not be read.
    Read(PathBuf, io::Error),
    /// The file read, and a line of it is no ban.
    Parse(PathBuf, DenylistError),
    /// The list was not written. The change it would have kept holds in
    /// the running host until it stops.
    Write(PathBuf, StorageError),
}

impl fmt::Display for BanFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Self::Parse(path, error) => write!(f, "{}, {error}", path.display()),
            Self::Write(path, error) => write!(
                f,
                "cannot write {}: {error}; the change holds until the server stops",
                path.display()
            ),
        }
    }
}

impl std::error::Error for BanFileError {}

/// The list the file at `path` holds — empty when there is no file.
///
/// # Errors
///
/// [`BanFileError::Read`] for a file that is there and will not read, and
/// [`BanFileError::Parse`] for one with a line that is no ban.
pub fn read(path: &Path) -> Result<Denylist, BanFileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Denylist::new()),
        Err(error) => return Err(BanFileError::Read(path.to_path_buf(), error)),
    };
    Denylist::parse(&text).map_err(|error| BanFileError::Parse(path.to_path_buf(), error))
}

/// Writes `list` to the file at `path`, whole, replacing what was there.
///
/// # Errors
///
/// [`BanFileError::Write`] when the write failed.
pub fn write(path: &Path, list: &Denylist) -> Result<(), BanFileError> {
    crate::store::write_atomic(path, list.to_text().as_bytes())
        .map_err(|error| BanFileError::Write(path.to_path_buf(), error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::PlayerId;

    /// **The file round-trips**: a list written reads back the same, and a
    /// missing file reads as an empty list.
    #[test]
    fn the_file_round_trips_and_a_missing_one_is_empty() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("bans.txt");
        assert!(read(&path).expect("no file is no bans").is_empty());
        let mut list = Denylist::new();
        list.ban(PlayerId::from_seed(1), "griefing");
        list.ban(PlayerId::from_seed(2), "");
        write(&path, &list).expect("the temp dir is writable");
        assert_eq!(read(&path).expect("it reads back"), list);
    }

    /// **A file with a bad line refuses to load**, naming the file and the
    /// line, rather than loading the bans around it.
    #[test]
    fn a_file_with_a_bad_line_refuses_to_load() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("bans.txt");
        std::fs::write(&path, "not-an-id\n").expect("the temp dir is writable");
        let error = read(&path).expect_err("a bad line refuses the file");
        assert!(matches!(error, BanFileError::Parse(_, ref parse) if parse.line == 1));
        assert!(error.to_string().contains("bans.txt"), "{error}");
    }
}
