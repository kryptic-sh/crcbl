//! The opt-in log file: the same lines stderr gets, in a file that survives a
//! build with no console.
//!
//! A `windows_subsystem = "windows"` exe has no stderr at all, so without this
//! every line the engine writes is lost on exactly the machines a bug report
//! comes from. Nothing here runs unless a game asks — [`attach_file`](super::attach_file)
//! — or a player does, through [`FILE_ENV_VAR`].
//!
//! # Rotated at start-up, capped within a run
//!
//! Opening shifts the previous runs down one name — `app.log` becomes
//! `app.1.log`, `app.1.log` becomes `app.2.log` — and deletes whatever falls off
//! the end, so a directory holds the current run plus at most
//! [`LOG_FILES_KEPT`] earlier ones. The run that crashed is `app.1.log` by the
//! time the player relaunches, and one launch does not destroy it.
//!
//! Within a run the file stops at [`LOG_FILE_MAX_BYTES`], with one closing line
//! saying so. Stopping rather than rotating mid-run is the cheap choice, and it
//! keeps the start of the run — the banner, the adapter, the backend — which is
//! what a report is read for first. The cap is a guard against a warning in a
//! loop filling a disk, not a size a normal run approaches.
//!
//! # Each line is written through before `emit` returns
//!
//! The file is an unbuffered [`File`], and every line is one `write_all` of an
//! already-formatted string. Once that returns the bytes are the operating
//! system's, so they survive a panic, an abort and a segfault alike — which is
//! the case this file exists for, and a panic hook could only cover the first
//! of those. The cost is one system call per line that passes the filter, the
//! same as stderr already pays. There is deliberately no `fsync`: it would add
//! milliseconds per line to guard against the machine losing power, which is not
//! what takes a game down.

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

/// How many earlier runs' logs survive a start-up rotation, beside the current
/// run's.
pub const LOG_FILES_KEPT: usize = 5;

/// The most bytes one run writes to its log file before it stops, not counting
/// the closing line that says it stopped.
pub const LOG_FILE_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Environment variable a player sets to ask for the log file:
/// `CRCBL_LOG_FILE=1`.
///
/// Read by [`file_requested`](super::file_requested). The spellings are
/// `CRCBL_TRACE`'s — `1`, `true`, `on`, `yes` and their opposites — and anything
/// else is off.
pub const FILE_ENV_VAR: &str = "CRCBL_LOG_FILE";

/// The last line a capped file gets.
const CAPPED_NOTE: &str = "[log file size cap reached; the rest of this run went to stderr only]\n";

/// One run's log file, open for appending lines.
#[derive(Debug)]
pub(super) struct FileSink {
    file: File,
    path: PathBuf,
    written: u64,
    max_bytes: u64,
    capped: bool,
}

impl FileSink {
    /// Rotates `dir`'s `stem` logs, keeping `kept` earlier ones, and opens a
    /// fresh `<stem>.log` that stops after `max_bytes`.
    ///
    /// `dir` is created if it is missing.
    ///
    /// # Errors
    ///
    /// A `stem` that is not a plain file name — a separator, `..`, empty —
    /// is [`io::ErrorKind::InvalidInput`]; anything the filesystem refuses is
    /// its own error. A rotation that fails part-way leaves the earlier files
    /// where they got to and opens nothing.
    pub(super) fn open(dir: &Path, stem: &str, kept: usize, max_bytes: u64) -> io::Result<Self> {
        check_stem(stem)?;
        fs::create_dir_all(dir)?;
        rotate(dir, stem, kept)?;
        let path = dir.join(file_name(stem, 0));
        let file = File::create(&path)?;
        Ok(Self {
            file,
            path,
            written: 0,
            max_bytes,
            capped: false,
        })
    }

    /// Where this run's lines are going.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// A second handle on this run's file, for the panic hook to write through
    /// without taking the logger's lock — see `panic_hook`.
    ///
    /// Both handles share one open file, and with it one write position, so a
    /// line written through either lands after everything already written.
    pub(super) fn try_clone_file(&self) -> io::Result<File> {
        self.file.try_clone()
    }

    /// Writes one line, newline included, unless the cap has been reached.
    ///
    /// A write error is dropped for the reason stderr's is: a failed log write
    /// must never take the process down, and there is nowhere left to report it.
    pub(super) fn write_line(&mut self, line: &str) {
        if self.capped {
            return;
        }
        let len = line.len() as u64;
        if self.written + len > self.max_bytes {
            self.capped = true;
            let _ = self.file.write_all(CAPPED_NOTE.as_bytes());
            return;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.written += len;
        }
    }
}

/// Refuses a stem that would put the file anywhere but directly in its
/// directory.
fn check_stem(stem: &str) -> io::Result<()> {
    let mut components = Path::new(stem).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) if name == stem => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{stem}` is not a plain file name for a log"),
        )),
    }
}

/// `<stem>.log` for the current run, `<stem>.<n>.log` for the one `n` runs ago.
fn file_name(stem: &str, generation: usize) -> String {
    if generation == 0 {
        format!("{stem}.log")
    } else {
        format!("{stem}.{generation}.log")
    }
}

/// Shifts every earlier log down one generation and drops the one that falls
/// past `kept`.
///
/// Oldest first, so each rename lands on a name the step before freed — except
/// the first, which lands on the generation being dropped and replaces it:
/// [`fs::rename`] replaces an existing destination on every platform std
/// supports, so the drop needs no delete of its own. A missing generation is a
/// gap to step over, not an error: the first run has none of them.
fn rotate(dir: &Path, stem: &str, kept: usize) -> io::Result<()> {
    for generation in (0..kept).rev() {
        match fs::rename(
            dir.join(file_name(stem, generation)),
            dir.join(file_name(stem, generation + 1)),
        ) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A fresh directory of this test's own under the system temp directory,
    /// removed when dropped. `crcbl-core` has no `tempfile`, and a new
    /// dependency edge for the log's test modules is not worth it.
    pub(in crate::log) struct TempDir(pub(in crate::log) PathBuf);

    impl TempDir {
        /// # Panics
        ///
        /// If the directory is already there — a leftover would decide the
        /// assertions, so it is refused rather than cleared.
        pub(in crate::log) fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("crcbl-core-log-file-{name}-{}", std::process::id()));
            fs::create_dir(&path).expect("a fresh temp directory for this test");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("the log directory exists")
            .map(|entry| {
                entry
                    .expect("a readable entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// **Rotation keeps the current run and exactly [`LOG_FILES_KEPT`] earlier
    /// ones, newest nearest.**
    ///
    /// Two more runs than it keeps, so the oldest two have to have been
    /// dropped, and each file's content names the run that wrote it, so a
    /// rotation that shuffled the order would be caught as well as one that
    /// kept too many.
    #[test]
    fn rotation_keeps_exactly_the_bounded_count() {
        let temp = TempDir::new("rotation");
        let runs = LOG_FILES_KEPT + 2;
        for run in 0..runs {
            let mut sink = FileSink::open(&temp.0, "game", LOG_FILES_KEPT, LOG_FILE_MAX_BYTES)
                .expect("opening the log");
            sink.write_line(&format!("run {run}\n"));
        }

        let mut want: Vec<String> = (1..=LOG_FILES_KEPT)
            .map(|generation| file_name("game", generation))
            .collect();
        want.push("game.log".to_owned());
        want.sort();
        assert_eq!(names(&temp.0), want);

        for generation in 0..=LOG_FILES_KEPT {
            let text = fs::read_to_string(temp.0.join(file_name("game", generation)))
                .expect("reading a kept log");
            assert_eq!(
                text,
                format!("run {}\n", runs - 1 - generation),
                "generation {generation}"
            );
        }
    }

    /// **A line the logger emits is in the file when the call returns**, whole
    /// and exactly as stderr printed it, after the run's banner.
    ///
    /// Read back without dropping or flushing anything, which is the claim the
    /// module makes about a crash: nothing is sitting in a buffer. Logged at
    /// `Error` so the check does not depend on which filter a sibling test put
    /// in force under a threaded `cargo test`.
    #[test]
    fn an_emitted_line_reaches_the_file() {
        let temp = TempDir::new("emit");
        let logger = super::super::StderrLogger::new();
        let path = logger
            .attach(&temp.0, "game", LOG_FILES_KEPT, LOG_FILE_MAX_BYTES)
            .expect("attaching the file");
        assert_eq!(path, temp.0.join("game.log"));

        let target = "crcbl_core::log::file::tests";
        logger.emit(
            super::super::Level::Error,
            target,
            format_args!("the reactor is {}", "on fire"),
        );

        let text = fs::read_to_string(&path).expect("reading the log");
        let mut lines = text.lines();
        let banner = lines.next().expect("a banner line");
        assert!(banner.contains("run started "), "{text}");
        let line = lines.next().expect("the emitted line");
        assert!(
            line.ends_with(&format!("ERROR {target}] the reactor is on fire")),
            "{text}"
        );
        assert_eq!(lines.next(), None, "{text}");

        let again = logger
            .attach(&temp.0, "game", LOG_FILES_KEPT, LOG_FILE_MAX_BYTES)
            .expect_err("a second file for the same run");
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
    }

    /// **The file stops at the cap, says so once, and takes nothing after.**
    #[test]
    fn a_run_stops_writing_at_the_size_cap() {
        let temp = TempDir::new("cap");
        let line = "0123456789\n";
        let max = 5 * line.len() as u64 + 3;
        let mut sink = FileSink::open(&temp.0, "game", 0, max).expect("opening the log");
        for _ in 0..20 {
            sink.write_line(line);
        }
        let text = fs::read_to_string(sink.path()).expect("reading the log");
        assert_eq!(text, format!("{}{CAPPED_NOTE}", line.repeat(5)));
        assert!(text.len() as u64 <= max + CAPPED_NOTE.len() as u64);
    }

    #[test]
    fn a_stem_that_names_a_path_is_refused() {
        let temp = TempDir::new("stem");
        for stem in ["", "..", "a/b", "../escape", "."] {
            let error = FileSink::open(&temp.0, stem, 1, 1024).expect_err(stem);
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{stem:?}");
        }
        assert!(!temp.0.join("..").join("escape.log").exists());
    }
}
