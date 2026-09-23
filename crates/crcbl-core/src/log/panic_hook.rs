//! The panic hook [`attach_file`](super::attach_file) installs, so a panic's
//! own message reaches the log file.
//!
//! The default hook prints the message, the location and the thread to stderr,
//! and a `windows_subsystem = "windows"` exe has none: the file kept every line
//! up to a panic and not the one saying what it was. This hook writes that line
//! to the file and then calls the hook it replaced, so stderr's report — and any
//! hook the game set before attaching — still runs.
//!
//! # It writes through its own handle, never the logger's lock
//!
//! The hook runs on the panicking thread before it unwinds. Had it taken the
//! logger's `Mutex` it could block on it: another thread mid-write only
//! briefly, but a panic raised on a thread that already holds that lock would
//! wait on itself for ever — `std`'s `Mutex` is not reentrant, and relocking it
//! deadlocks or panics, and a panic inside a panic hook aborts. `try_lock`
//! avoids both and buys a new failure: it drops the panic's line whenever any
//! other thread happens to be logging, which on a game's render or audio thread
//! is often.
//!
//! So the hook owns a second handle on the same file,
//! [`try_clone_file`](super::file::FileSink::try_clone_file), and writes through
//! that with no lock at all. It never blocks, never drops the line, and a
//! poisoned logger lock is not its concern; `emit` already steps over poison
//! with `PoisonError::into_inner`. The two handles share one open file and so
//! one write position, so the panic's line is appended after what is already
//! written rather than over it. Racing another thread's line, the two can
//! interleave only where a `write_all` needs more than one write, which for a
//! regular file is a full disk or an interrupted call. It is written even past
//! [`LOG_FILE_MAX_BYTES`](super::LOG_FILE_MAX_BYTES): it is one line, and the
//! one a report is read for.
//!
//! It is written straight to the file, not through `emit`: stderr already gets
//! the previous hook's report, and `emit` would print a second copy there.

use std::fs::File;
use std::io::{self, Write as _};
use std::panic::{self, PanicHookInfo};
use std::sync::PoisonError;
use std::thread;
use std::time::Instant;

use super::{Level, StderrLogger, line};

/// The target the panic's line carries, where a log line carries its module.
const PANIC_TARGET: &str = "panic";

/// What the line says when the payload is neither `&str` nor `String` — a
/// `panic_any` of some other type.
const NON_STRING_PAYLOAD: &str = "<non-string payload>";

/// Chains a hook that writes each panic to `logger`'s file in front of the
/// process's current hook.
///
/// # Errors
///
/// When no file is attached, when the file cannot be given a second handle, and
/// when this thread is panicking — [`panic::take_hook`] panics there, and a
/// panic in `Drop` during an unwind is a place `attach_file` can be reached
/// from. The process's hook is untouched in every case.
pub(super) fn install(logger: &StderrLogger) -> io::Result<()> {
    if thread::panicking() {
        return Err(io::Error::other(
            "attached while this thread was panicking, when the panic hook cannot be swapped",
        ));
    }
    let file = logger
        .file
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .ok_or_else(|| io::Error::other("no log file is attached"))?
        .try_clone_file()?;
    let start = logger.start;
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        write_panic(&file, start, info);
        previous(info);
    }));
    Ok(())
}

/// Writes `info` to `file` as one `ERROR panic` line.
///
/// Must not panic, block or take a lock the panicking thread may hold — see the
/// module docs. A write error is dropped for `FileSink::write_line`'s reason:
/// there is nowhere left to report it, and the previous hook runs regardless.
fn write_panic(mut file: &File, start: Instant, info: &PanicHookInfo<'_>) {
    let thread = thread::current();
    let name = thread
        .name()
        .map_or_else(|| format!("{:?}", thread.id()), str::to_owned);
    let message = info.payload_as_str().unwrap_or(NON_STRING_PAYLOAD);
    let text = match info.location() {
        Some(location) => format!("thread '{name}' panicked at {location}: {message}"),
        None => format!("thread '{name}' panicked at an unknown location: {message}"),
    };
    let line = line(
        start.elapsed(),
        Level::Error,
        PANIC_TARGET,
        format_args!("{text}"),
    );
    let _ = file.write_all(line.as_bytes());
}

#[cfg(test)]
mod tests {
    //! Every test here runs its body in a fresh copy of this test binary.
    //!
    //! The hook is process-global and set once, and the logger slot it needs is
    //! too — `installing_the_logger_is_idempotent` asserts that slot is empty,
    //! so under a plain `cargo test`, which runs a crate's unit tests as threads
    //! of one process, a test installing either would fail it or be failed by
    //! it. A child gets a process nobody has touched: this is
    //! `a_host_owning_the_slot_means_this_module_installed_nothing`'s pattern.
    //! The parent owns the temp directory, so the child's file handles are
    //! closed by the time it is removed, and it kills a child that has not
    //! finished by [`CHILD_DEADLINE`] — the failure a hook blocking on a lock
    //! looks like.

    use std::any::Any;
    use std::env;
    use std::fs;
    use std::io::Read as _;
    use std::panic::Location;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread::ThreadId;
    use std::time::Duration;

    use super::super::file::tests::TempDir;
    use super::super::{
        Filter, LOGGER, attach_file, attach_file_without_panic_hook, try_init_logging,
    };
    use super::*;

    /// Set in a child to the directory it attaches its file in.
    const CHILD_DIR: &str = "CRCBL_LOG_PANIC_HOOK_CHILD_DIR";

    /// How long a child gets before it is taken to have hung. Far past the
    /// fraction of a second a child takes, so only a hang reaches it.
    const CHILD_DEADLINE: Duration = Duration::from_secs(30);

    /// The stem every child attaches its file under.
    const STEM: &str = "game";

    /// In a child: installs the logger and runs `body` with the directory to
    /// attach in. In the parent: re-runs test `name` alone in a child, and fails
    /// if the child failed, ran no test, or did not finish.
    fn in_child(name: &str, body: impl FnOnce(&Path)) {
        if let Some(dir) = env::var_os(CHILD_DIR) {
            assert!(
                try_init_logging(Filter::parse("error")).is_ok(),
                "a fresh process has a free logger slot"
            );
            body(Path::new(&dir));
            return;
        }

        let temp = TempDir::new(&format!("panic-hook-{name}"));
        let mut child = Command::new(env::current_exe().expect("a test binary knows its own path"))
            .args(["--exact", &format!("log::panic_hook::tests::{name}")])
            .env(CHILD_DIR, &temp.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("re-running this test binary");
        // Drained on threads of their own, so a child writing more than a pipe
        // holds cannot stall on a parent that is only polling it.
        let drain = |pipe: Option<Box<dyn io::Read + Send>>| {
            thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_string(&mut text);
                }
                text
            })
        };
        let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));

        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("polling the child") {
                break Some(status);
            }
            if started.elapsed() > CHILD_DEADLINE {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let report = format!(
            "{}{}",
            stdout.join().expect("the stdout reader"),
            stderr.join().expect("the stderr reader")
        );
        let status =
            status.unwrap_or_else(|| panic!("the child hung past {CHILD_DEADLINE:?}:\n{report}"));
        assert!(status.success(), "the child failed:\n{report}");
        // `--exact` with a name that matches nothing exits zero, so without this
        // every test here passes vacuously the moment one is renamed.
        assert!(
            report.contains("1 passed"),
            "the child ran no test — has this one been renamed?\n{report}"
        );
    }

    /// Panics on a thread named `name`, or an unnamed one for `None`, with
    /// `payload`, and returns the thread's id and the `file:line:col` the panic
    /// is raised at.
    fn panic_on_thread<M: Any + Send>(name: Option<&str>, payload: M) -> (ThreadId, String) {
        /// Sends its caller's location, then panics there: `panic_any` is
        /// `#[track_caller]` too, so both name the same line and column.
        #[track_caller]
        fn raise<M: Any + Send>(sender: &mpsc::Sender<String>, payload: M) {
            sender
                .send(Location::caller().to_string())
                .expect("the test is listening");
            panic::panic_any(payload);
        }

        let (sender, location) = mpsc::channel();
        let builder = match name {
            Some(name) => thread::Builder::new().name(name.to_owned()),
            None => thread::Builder::new(),
        };
        let worker = builder
            .spawn(move || raise(&sender, payload))
            .expect("spawning the panicking thread");
        let id = worker.thread().id();
        worker.join().expect_err("the thread panicked");
        (
            id,
            location.recv().expect("the thread said where it panics"),
        )
    }

    fn read_log(dir: &Path) -> String {
        fs::read_to_string(dir.join(format!("{STEM}.log"))).expect("reading the log")
    }

    /// The lines in `text` the hook wrote.
    fn panic_lines(text: &str) -> Vec<&str> {
        let marker = format!(" ERROR {PANIC_TARGET}] ");
        text.lines().filter(|line| line.contains(&marker)).collect()
    }

    /// **A panic on another thread is in the file by the time `join` returns**:
    /// its message, where it was raised and the thread's name, in one line.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn a_panic_writes_its_message_location_and_thread() {
        in_child("a_panic_writes_its_message_location_and_thread", |dir| {
            attach_file(dir, STEM).expect("attaching the file");
            let (_, location) =
                panic_on_thread(Some("reactor"), format!("the core is {}", "on fire"));

            let text = read_log(dir);
            let lines = panic_lines(&text);
            assert_eq!(lines.len(), 1, "{text}");
            let written = lines[0]
                .split_once(&format!("{PANIC_TARGET}] "))
                .expect("the line's target")
                .1;
            let (at, message) = written
                .strip_prefix("thread 'reactor' panicked at ")
                .and_then(|rest| rest.split_once(": "))
                .unwrap_or_else(|| panic!("not the hook's shape: {written}"));
            assert_eq!(at, location);
            assert_eq!(message, "the core is on fire");
        });
    }

    /// **An unnamed thread is named by its id, and a payload that is not a
    /// string still gets a line.**
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn an_unnamed_thread_and_a_non_string_payload_still_get_a_line() {
        in_child(
            "an_unnamed_thread_and_a_non_string_payload_still_get_a_line",
            |dir| {
                attach_file(dir, STEM).expect("attaching the file");
                let (id, _) = panic_on_thread(None, 7_u8);
                let (_, _) = panic_on_thread(None, "a static str");

                let text = read_log(dir);
                let lines = panic_lines(&text);
                assert_eq!(lines.len(), 2, "{text}");
                assert!(
                    lines[0].contains(&format!("thread '{id:?}' panicked at "))
                        && lines[0].ends_with(&format!(": {NON_STRING_PAYLOAD}")),
                    "{text}"
                );
                assert!(lines[1].ends_with(": a static str"), "{text}");
            },
        );
    }

    /// **The hook it replaced still runs**, so a game's own hook — and the
    /// default one's stderr report — are not lost to the file.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn the_previous_hook_still_runs() {
        in_child("the_previous_hook_still_runs", |dir| {
            static CALLS: AtomicUsize = AtomicUsize::new(0);
            let default = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                CALLS.fetch_add(1, Ordering::Relaxed);
                default(info);
            }));
            attach_file(dir, STEM).expect("attaching the file");

            panic_on_thread(Some("worker"), "once");
            assert_eq!(CALLS.load(Ordering::Relaxed), 1, "the game's hook ran");
            assert_eq!(panic_lines(&read_log(dir)).len(), 1, "and so did ours");
        });
    }

    /// **Attaching twice installs one hook**, so one panic is one line.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn a_second_attach_installs_no_second_hook() {
        in_child("a_second_attach_installs_no_second_hook", |dir| {
            attach_file(dir, STEM).expect("attaching the file");
            for again in [attach_file, attach_file_without_panic_hook] {
                let error = again(dir, STEM).expect_err("a second file for the run");
                assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            }

            panic_on_thread(Some("worker"), "once");
            let text = read_log(dir);
            assert_eq!(panic_lines(&text).len(), 1, "{text}");
        });
    }

    /// **The opt-out installs nothing**: the file gets no line, and the hook in
    /// place — here the default — is the one that runs.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn attaching_without_the_hook_installs_none() {
        in_child("attaching_without_the_hook_installs_none", |dir| {
            attach_file_without_panic_hook(dir, STEM).expect("attaching the file");
            panic_on_thread(Some("worker"), "unrecorded");
            crate::error!("after the panic");

            let text = read_log(dir);
            assert!(panic_lines(&text).is_empty(), "{text}");
            assert!(
                text.contains("after the panic"),
                "the file is attached: {text}"
            );
        });
    }

    /// **A panic on a thread holding the logger's file lock still gets its
    /// line, and logging carries on through the poisoned lock.**
    ///
    /// Relocking a `std` `Mutex` on the thread that holds it deadlocks or
    /// panics, so a hook that took the lock would hang here — which
    /// [`in_child`]'s deadline turns into a failure — or abort the child.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn a_panic_holding_the_file_lock_still_writes_its_line() {
        in_child(
            "a_panic_holding_the_file_lock_still_writes_its_line",
            |dir| {
                attach_file(dir, STEM).expect("attaching the file");
                let logger = LOGGER.get().expect("the logger is installed");
                thread::Builder::new()
                    .name("holder".to_owned())
                    .spawn(move || {
                        let _held = logger.file.lock().unwrap_or_else(PoisonError::into_inner);
                        panic!("panicked holding the file lock");
                    })
                    .expect("spawning the panicking thread")
                    .join()
                    .expect_err("the thread panicked");
                assert!(logger.file.is_poisoned(), "the unwind poisoned the lock");
                crate::error!("after the poisoned lock");

                let text = read_log(dir);
                let lines = panic_lines(&text);
                assert_eq!(lines.len(), 1, "{text}");
                assert!(
                    lines[0].contains("thread 'holder' panicked at ")
                        && lines[0].ends_with(": panicked holding the file lock"),
                    "{text}"
                );
                assert!(text.contains("after the poisoned lock"), "{text}");
            },
        );
    }
}
