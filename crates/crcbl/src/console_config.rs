//! `config`: running a file of console lines, and the one that runs itself at
//! start-up.
//!
//! `docs/plan/52-debug-console.md` slice 9's "a `config` command that runs a
//! file of commands" — Source's `exec`, under the name the plan gives it. A
//! line typed at the console is one line; a config file is the twenty a person
//! would otherwise retype every session, and running one is the same
//! [`Registry::execute`](crcbl_console::Registry::execute) over the same
//! [`Context`], so a command reachable from the prompt is reachable from a file
//! and no second execution path exists to disagree with the first.
//!
//! # Why it is here and not in `crcbl-console`
//!
//! `crcbl-console` depends on nothing and names no engine type — plan decision
//! 1 — and this command has to reach a **file**, which on `wasm32` is not a
//! filesystem at all. Both halves of that are already answered in this crate:
//! [`crate::settings`]'s `save` writes through
//! [`SettingsStack::with_platform_storage`], which is a config directory
//! natively and the page's Origin Private File System store in a browser. So
//! `config` reads through the same seam, out of the same directory the
//! player's `settings.toml` lives in, and a browser build runs a config file
//! for real rather than reporting that files do not exist here.
//!
//! # What an argument may name
//!
//! **A bare name, never a path.** ASCII letters, digits, `-` and `_`, with
//! [`CONFIG_SUFFIX`] optional, resolved against that one directory —
//! `file_named` is the rule and it is applied before any storage is asked for
//! anything. A separator, a `..`, a leading `/` and a drive letter are each
//! refused by name, so `config ../../.ssh/id_ed25519` is a printed refusal
//! rather than a read. `crcbl_store`'s own containment check is the second line
//! of defence and not the first: the argument is a line somebody typed.
//!
//! # What a file may do to itself
//!
//! A file that runs itself, or two that run each other, would recurse until the
//! stack ran out. Two bounds, both in `enter`: a file already running is
//! refused by name, which ends every cycle at its first repeat, and
//! [`CONFIG_NESTING_LIMIT`] files may be open at once, which ends a chain of
//! distinct files. Neither is a hang and neither is a panic — each is a fault
//! on the line that asked, and the file that asked carries on.
//!
//! # What a failed line does
//!
//! Source keeps going, and so does this: a line that faults prints
//! `name.cfg:3: <what went wrong>` and the file runs on to line 4. The half
//! that is *not* Source's is the last line every run prints — how many lines
//! ran and how many of them failed — because a file that silently half-applied
//! is the failure worth designing against, and a summary is the only thing that
//! is read when the twenty lines above it scrolled past.
//!
//! # The one file that runs without being asked
//!
//! [`AUTOEXEC`] is Source's `autoexec.cfg`, and `run_autoexec` below is the
//! engine running it: `Loop::new` calls it once, after the console is built and
//! before the first frame, so a variable can be set *before* anything reads it.
//! Without it there is no way to configure a run that is over before anyone could
//! type — a frame-budget measurement, a golden capture, a demo started from a
//! launcher — because every other route into the console needs a keyboard.
//!
//! It is the same file, read the same way, run through the same `run_text`.
//! What differs is what it does when there is nothing to run: **it is silent.**
//! A run that reads no settings file reads no autoexec either
//! ([`Autoexec::NoSettingsFile`], the gate that keeps a golden run out of
//! whichever home directory it executes in), and a machine that has no
//! `autoexec.cfg` — which is almost every machine — says nothing at all. What it
//! does *not* swallow is a file that is there and would not run: that is
//! printed, and the boot carries on.

use std::cell::RefCell;
use std::path::Path;

use crcbl_console::{Context, Fault};
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

use crate::settings::ConsoleHost;

/// The extension a config file carries: Source's own, and the one this command
/// appends to a name that does not already have it.
pub const CONFIG_SUFFIX: &str = ".cfg";

/// The stem of the file the engine runs at start-up without being asked:
/// Source's own name for it.
///
/// The file is this plus [`CONFIG_SUFFIX`], in the same settings directory
/// `config` reads from — so it is a config file like any other and
/// `config autoexec` runs the same lines again by hand. See `run_autoexec`.
pub const AUTOEXEC: &str = "autoexec";

/// How many config files may be running at once.
///
/// The bound on a chain of *distinct* files — a cycle is refused by name before
/// this is reached — and it is deliberately small: a config file that runs
/// another is already unusual, and eight is past anything a person nests on
/// purpose while being far short of a stack a recursive read could exhaust.
pub const CONFIG_NESTING_LIMIT: usize = 8;

/// What a whole-line comment starts with.
///
/// Only at the start of a line, after its indentation: the console's own parser
/// treats `//` as ordinary text — a URL, a Windows share — and a scanner that
/// stripped it anywhere would change what a line means depending on whether it
/// came from a file or a prompt.
const COMMENT: &str = "//";

thread_local! {
    /// The config files running on this thread, outermost first.
    ///
    /// Thread-local rather than state on the host because it is *call-stack*
    /// state: a nested `config` runs inside the outer one's own call, on this
    /// thread, and unwinds with it. [`Nesting`] is what puts a name back.
    static RUNNING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

crcbl_console::concommand! {
    /// Run a file of console lines from this game's settings directory: `config video`.
    ///
    /// The name is a bare one — letters, digits, `-` and `_`, with `.cfg`
    /// optional — and never a path. Blank lines and lines starting `//` are
    /// skipped; a line that fails prints where it failed and the rest of the
    /// file still runs.
    pub fn config(cx, args) {
        let [arg] = args else {
            return Err(Fault::new(
                "config takes one name: the file in this game's settings directory to run",
            ));
        };
        let file = file_named(arg)?;
        let app_name = cx
            .host()
            .downcast_ref::<ConsoleHost>()
            .expect("the engine's console is only ever run over a `ConsoleHost`")
            .engine()
            .app_name()
            .map(str::to_owned);
        let Some(app_name) = app_name else {
            // The same refusal `save` makes, for the same reason: a golden run
            // or a headless harness reads no settings file, so it has no
            // directory of its own and must not read one out of whichever home
            // directory it happens to execute in.
            return Err(Fault::new(format!(
                "this run reads no settings file, so there is no directory to read `{file}` from"
            )));
        };
        // Every reason a read did not produce text is one fault to whoever
        // typed the name: they asked for this file, so "there is no such file"
        // is an answer to the line and not the state the boot's autoexec treats
        // it as.
        let text = read_file(&app_name, &file).map_err(NotRead::into_fault)?;
        run_text(cx, &file, &text)
    }
}

/// The file `arg` names, or a fault saying what a name may be.
///
/// **The rule:** a config name is ASCII letters, digits, `-` and `_`, with
/// [`CONFIG_SUFFIX`] optional, and it names one file in the settings directory.
/// Everything else — a `/` or a `\`, a `..`, a `.`, a leading `/`, a drive
/// letter, an empty name — is refused here, before the storage layer is asked
/// for anything.
fn file_named(arg: &str) -> Result<String, Fault> {
    let stem = arg.strip_suffix(CONFIG_SUFFIX).unwrap_or(arg);
    let bare = !stem.is_empty()
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !bare {
        return Err(Fault::new(format!(
            "`{arg}` is not a config name — a name is ASCII letters, digits, `-` and `_`, \
             with `{CONFIG_SUFFIX}` optional, and names one file in this game's settings \
             directory; it is not a path"
        )));
    }
    Ok(format!("{stem}{CONFIG_SUFFIX}"))
}

/// Why a config file's text is not in hand.
///
/// A shape of its own rather than one [`Fault`], because the boot's
/// [`run_autoexec`] has to tell some of these apart and a typed `config` must
/// not: a file nobody wrote is the ordinary state of a machine and not a
/// failure, and a platform with nowhere to read from is not a fault of the
/// file's. Whoever typed a name asked for that file and is told whichever it
/// was, so [`into_fault`](Self::into_fault) is the one place they collapse back
/// into the messages `config` has always printed.
#[derive(Debug)]
enum NotRead {
    /// The settings directory holds no file of that name.
    Missing(Fault),
    /// There is no settings directory to look in: this platform names none, or
    /// this page installed no store.
    Nowhere(Fault),
    /// The file is there and its text is not: an I/O error, a browser store
    /// that has not restored it yet, bytes that are not UTF-8.
    Unreadable(Fault),
}

impl NotRead {
    /// How a storage error reads for `file`.
    ///
    /// Separate from the read itself so the checks below can hold it to those
    /// answers over errors they build, rather than over whichever config
    /// directory the suite is running in — the classification is the half a
    /// silent boot depends on, and it is the half a read cannot demonstrate.
    fn from_storage(error: &StorageError, file: &str) -> Self {
        match error {
            // The one that must not become a line at start-up, and the one
            // `config` still reports word for word.
            StorageError::NotFound(_) => {
                Self::Missing(Fault::new(format!("`{file}` could not be read: {error}")))
            }
            // A browser read of something the shim has not restored yet is a
            // state and not a failure — `crcbl_store::web` polls it — so the
            // line says to ask again rather than that the file is missing.
            StorageError::Pending(_) => Self::Unreadable(Fault::new(format!(
                "`{file}` is not resident in this page's store yet — ask again in a moment"
            ))),
            error => Self::Unreadable(Fault::new(format!("`{file}` could not be read: {error}"))),
        }
    }

    /// The line a caller that asked for this file by name prints.
    fn into_fault(self) -> Fault {
        match self {
            Self::Missing(fault) | Self::Nowhere(fault) | Self::Unreadable(fault) => fault,
        }
    }
}

/// The text of `file`, out of the directory this run's settings live in.
///
/// [`SettingsStack::with_platform_storage`] is the same seam `save` writes
/// through, which is what makes "the settings directory" one answer rather than
/// two: natively the platform config directory, in a browser the store the page
/// installed.
///
/// **The one read**, for `config` and for the autoexec alike: a second copy of
/// it would be a second answer to where the settings directory is.
fn read_file(app_name: &str, file: &str) -> Result<String, NotRead> {
    let Some(read) =
        SettingsStack::with_platform_storage(app_name, |storage| storage.read(Path::new(file)))
    else {
        // The two platforms have different reasons for having nowhere to read
        // from, and telling a browser about a config directory would send
        // someone looking for one — the same split `SettingsStack::platform`
        // makes where it logs.
        #[cfg(not(target_arch = "wasm32"))]
        let nowhere = format!(
            "this platform names no config directory for `{app_name}`, so there is nowhere \
             to read `{file}` from"
        );
        #[cfg(target_arch = "wasm32")]
        let nowhere =
            format!("this page has installed no store, so there is nowhere to read `{file}` from");
        return Err(NotRead::Nowhere(Fault::new(nowhere)));
    };
    let bytes = read.map_err(|error| NotRead::from_storage(&error, file))?;
    String::from_utf8(bytes)
        .map_err(|_| NotRead::Unreadable(Fault::new(format!("`{file}` is not UTF-8 text"))))
}

/// Run every line of `text` as the config file `name`.
///
/// The whole of what `config` does once the bytes are in hand, the nesting
/// guard included — split from the command so the checks below can drive it
/// over text they hold, rather than over a file in whichever home directory the
/// suite is running in.
///
/// Lines are counted as an editor counts them: a blank line and a comment take
/// their number with them, so `name.cfg:7` is line 7 of the file.
///
/// # Errors
///
/// A [`Fault`] only where the file could not be *started*: it is already
/// running, or too many are. A line that fails is reported and stepped over —
/// see the module docs.
pub(crate) fn run_text(cx: &mut Context<'_>, name: &str, text: &str) -> Result<(), Fault> {
    let _nesting = enter(name)?;
    // Borrowed from the registry rather than from `cx`, so the context below
    // can hold `cx`'s host at the same time.
    let registry = cx.registry();
    let mut ran = 0_usize;
    let mut failed = 0_usize;

    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with(COMMENT) {
            continue;
        }
        ran += 1;

        // A context of its own per line, over the same registry and the same
        // host: what the line printed is collected here and forwarded, so a
        // fault's own line lands after the output of the line that faulted
        // rather than before it.
        let mut inner = Context::new(registry, cx.host_mut());
        let outcome = registry.execute(&mut inner, line);
        let clear = inner.clear_requested();
        for printed in inner.into_lines() {
            cx.print(printed);
        }
        if clear {
            cx.request_clear();
        }
        if let Err(fault) = outcome {
            failed += 1;
            cx.print(format!("{name}:{number}: {fault}"));
        }
    }

    let lines = if ran == 1 { "line" } else { "lines" };
    if ran == 0 {
        cx.print(format!("{name} holds nothing to run"));
    } else if failed == 0 {
        cx.print(format!("{name}: {ran} {lines}"));
    } else {
        cx.print(format!("{name}: {ran} {lines}, {failed} of them failed"));
    }
    Ok(())
}

/// What the boot's `autoexec.cfg` did.
///
/// Answered rather than only printed, because most of these are **silence**: a
/// run with no settings file, a platform with nowhere to read from and a machine
/// with no `autoexec.cfg` each print nothing by design, so a check that could see
/// only the printed lines could not tell them from one another — nor any of them
/// from a boot that never ran the autoexec at all.
/// [`Loop::new`](crate::engine::Loop::new) carries on whichever it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autoexec {
    /// This run reads no settings file, so no storage was asked for anything.
    ///
    /// A golden run or a headless harness — see
    /// [`SettingsSource::None`](crate::engine::SettingsSource) — and the gate is
    /// deliberately this and not "the platform has a config directory":
    /// natively it *has* one, and reading it is exactly what such a run must not
    /// do.
    NoSettingsFile,
    /// There is nowhere to read from: this platform names no config directory,
    /// or this page has installed no store.
    Nowhere,
    /// There is no `autoexec.cfg`, which is what almost every machine answers.
    Missing,
    /// It was read, and its lines were run — a line of it may still have
    /// failed, which `run_text` reports as it goes.
    Ran,
    /// It is there and did not run: unreadable, not text, or refused by the
    /// nesting guard. The reason was printed.
    Unreadable,
}

/// Run this game's `autoexec.cfg`, if this run has one and it is there.
///
/// [`Loop::new`](crate::engine::Loop::new) calls this once through
/// [`Console::run_autoexec`](crate::debug_console::Console::run_autoexec),
/// before the first frame — see the module docs for why the file exists at all.
///
/// **The `app_name` gate is the first thing it does, and it is the important
/// one.** A run whose [`ConsoleHost`] was never given a name reads nothing:
/// `SettingsStack::with_platform_storage` would answer `Some` on a native
/// headless run and hand back `~/.config/<game>/autoexec.cfg`, so "this platform
/// has a config directory" is not a gate at all. It is the same refusal `config`
/// and `save` make, worded as silence because nobody asked for this file.
pub(crate) fn run_autoexec(cx: &mut Context<'_>) -> Autoexec {
    let app_name = cx
        .host()
        .downcast_ref::<ConsoleHost>()
        .expect("the engine's console is only ever run over a `ConsoleHost`")
        .engine()
        .app_name()
        .map(str::to_owned);
    let Some(app_name) = app_name else {
        return Autoexec::NoSettingsFile;
    };
    let file = format!("{AUTOEXEC}{CONFIG_SUFFIX}");
    let read = read_file(&app_name, &file);
    run_read(cx, &file, read)
}

/// What the boot does with whatever the read of `file` answered.
///
/// Split from [`run_autoexec`] for [`run_text`]'s own reason: the read reaches
/// the platform's settings directory, and the checks below drive this over an
/// answer they hold rather than over whichever home directory the suite is
/// running in.
fn run_read(cx: &mut Context<'_>, file: &str, read: Result<String, NotRead>) -> Autoexec {
    match read {
        Ok(text) => {
            // The only way this faults at boot is the nesting guard, which
            // cannot be entered yet — but a start-up that quietly ran nothing is
            // the failure this module is shaped against, so it is printed rather
            // than dropped.
            if let Err(fault) = run_text(cx, file, &text) {
                cx.print(fault.to_string());
                return Autoexec::Unreadable;
            }
            Autoexec::Ran
        }
        // **Absence is not an event.** Almost nobody writes an `autoexec.cfg`,
        // and a start-up line saying they still have not would be in every log
        // this engine ever produces.
        Err(NotRead::Missing(_)) => Autoexec::Missing,
        // Nothing is wrong with the run: a page with no store installed reads no
        // settings file either. `info` rather than the console log, which is
        // where a player reads the answer to what they typed.
        Err(NotRead::Nowhere(reason)) => {
            crcbl_core::log::info!("no {file} was run: {reason}");
            Autoexec::Nowhere
        }
        // The file is there and its text is not, which nothing else will say.
        // `StorageError::Pending` in particular means the page booted out of
        // order — the shim restores the store before `boot()` — and somebody has
        // to see that.
        Err(NotRead::Unreadable(fault)) => {
            cx.print(fault.to_string());
            Autoexec::Unreadable
        }
    }
}

/// Claim `name` as running on this thread, or refuse it.
///
/// The two bounds the module docs state, and the reason they are two: the first
/// ends a **cycle** — a file that runs itself, directly or through another —
/// where a depth limit alone would run it [`CONFIG_NESTING_LIMIT`] times over
/// first; the second ends a **chain** of distinct files, which no cycle check
/// can see.
fn enter(name: &str) -> Result<Nesting, Fault> {
    RUNNING.with_borrow_mut(|running| {
        if running.iter().any(|open| open == name) {
            return Err(Fault::new(format!(
                "`{name}` is running already — a config file cannot run itself, \
                 nor one that runs it back"
            )));
        }
        if running.len() >= CONFIG_NESTING_LIMIT {
            return Err(Fault::new(format!(
                "config files are already {} deep, which is as far as they nest — \
                 `{name}` was not run",
                running.len()
            )));
        }
        running.push(name.to_owned());
        Ok(Nesting)
    })
}

/// One name held in [`RUNNING`] for as long as its file is running.
///
/// A guard rather than a matching `pop` at the end of [`run_text`], because a
/// `?` in the middle of that function would otherwise leave the name behind and
/// every later run of that file would be refused as a cycle.
#[derive(Debug)]
struct Nesting;

impl Drop for Nesting {
    fn drop(&mut self) {
        RUNNING.with_borrow_mut(|running| {
            running.pop();
        });
    }
}

#[cfg(test)]
mod tests {
    use crcbl_console::{Binding, Flags, Kind, Registry, Table, Value};
    use crcbl_store::settings::SettingsStack;

    use super::*;

    thread_local! {
        /// The files the fixture `config` below reads, name and text.
        static FILES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    }

    crcbl_console::concommand! {
        /// Run a fixture file — the real `config` with the storage read swapped out.
        ///
        /// Every line of it but the read is the real one's: the same
        /// [`file_named`] rule and the same [`run_text`], which is what makes
        /// a nested `config` inside a fixture reach the same nesting guard the
        /// engine's does. Only the bytes come from [`FILES`] instead of from
        /// the platform's settings directory, so no check here writes into
        /// whichever home directory the suite is running in.
        ///
        /// It answers to `config`, deliberately: a nested line inside a fixture
        /// is spelled `config other`, and a stand-in under another name would
        /// exercise a recursion the shipped command does not have.
        pub fn config(cx, args) {
            let [arg] = args else {
                return Err(Fault::new("the fixture config takes one name"));
            };
            let file = file_named(arg)?;
            let text = FILES.with_borrow(|files| {
                files
                    .iter()
                    .find(|(name, _)| *name == file)
                    .map(|(_, text)| text.clone())
            });
            let Some(text) = text else {
                return Err(Fault::new(format!("no fixture called `{file}`")));
            };
            run_text(cx, &file, &text)
        }
    }

    /// The fixture registry: `config` above, plus the built-ins `Registry`
    /// always carries — `echo` is what a fixture line is written with.
    /// A host with state, and a [`Binding`] that writes into it.
    ///
    /// **Why a binding and not a `ConVar`:** a `ConVar` is a global atomic and
    /// would land whatever context wrote it, so it could not tell the caller's
    /// host from a scratch one. A binding reaches its host through
    /// `Context::host_mut`, which is the thing `run_text` has to pass along —
    /// and settings keys, the reason a config file exists at all, are all
    /// bindings.
    #[derive(Debug, Default, PartialEq)]
    struct Host {
        gain: i64,
    }

    static GAIN: Binding = Binding::new(
        "gain",
        "a number the fixture host remembers",
        Kind::Int { min: 0, max: 99 },
        Flags::NONE,
        |host| Value::Int(host.downcast_ref::<Host>().expect("the fixture host").gain),
        |host, value| {
            let Value::Int(gain) = value else {
                return Err(Fault::new("gain takes a number"));
            };
            host.downcast_mut::<Host>().expect("the fixture host").gain = *gain;
            Ok(())
        },
    );

    /// The fixture registry: `config` above, the `gain` binding, plus the
    /// built-ins `Registry` always carries — `echo` is what a fixture line is
    /// written with.
    fn registry() -> Registry {
        static COMMANDS: &[&crcbl_console::ConCommand] = &[&config];
        static BINDINGS: &[&Binding] = &[&GAIN];
        Registry::gather(&[Table::new(&[], BINDINGS, COMMANDS)])
            .expect("no two entries claim one name")
    }

    /// Load `files` as the fixture set and run `first`, answering what the
    /// console printed.
    fn run(files: &[(&str, &str)], first: &str) -> Vec<String> {
        FILES.with_borrow_mut(|held| {
            *held = files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect();
        });
        let registry = registry();
        let mut host = ();
        let mut cx = Context::new(&registry, &mut host);
        let outcome = registry.execute(&mut cx, &format!("config {first}"));
        let mut lines = cx.into_lines();
        if let Err(fault) = outcome {
            lines.push(fault.message().to_owned());
        }
        lines
    }

    /// **What a config file does is land on the caller's host.**
    ///
    /// The point of the whole command, and the one contract none of the checks
    /// below reach: they all run over a `()` host and assert on printed lines,
    /// so `run_text` could execute every line against a scratch context and
    /// each of them would still pass. Settings keys are `Binding`s, a `Binding`
    /// writes through `Context::host_mut`, and a config file that printed the
    /// right lines while applying nothing is exactly the half-applied failure
    /// this module is shaped against.
    ///
    /// Verified 2026-08-31 by giving each line its own scratch `()` host:
    /// every other check here stayed green.
    #[test]
    fn what_a_file_sets_lands_on_the_caller_s_host() {
        let files = [("both.cfg", "gain 7\necho done")];
        FILES.with_borrow_mut(|held| {
            *held = files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect();
        });
        let registry = registry();
        let mut host = Host::default();
        let mut cx = Context::new(&registry, &mut host);
        registry
            .execute(&mut cx, "config both")
            .expect("the file runs");
        drop(cx);
        assert_eq!(
            host,
            Host { gain: 7 },
            "the line the file ran wrote through to the host the caller handed in, not to one \
             the run made for itself"
        );
    }

    /// **A name is a bare name, and everything a path could be is refused** —
    /// the traversal guard, which is the whole reason the argument is not
    /// handed to the storage layer as typed.
    #[test]
    fn an_argument_that_is_not_a_bare_name_is_refused() {
        for argument in [
            "../settings.toml",
            "..",
            ".",
            "../../.ssh/id_ed25519",
            "/etc/passwd",
            "a/b",
            "a\\b",
            "C:\\games\\autoexec.cfg",
            "video.cfg.cfg",
            "video conf",
            "",
            ".cfg",
        ] {
            let fault =
                file_named(argument).expect_err(&format!("`{argument}` is not a bare config name"));
            assert!(
                fault.message().contains("is not a config name"),
                "`{argument}`: {}",
                fault.message()
            );
        }
    }

    /// **And a bare name resolves to one file**, with the suffix appended once
    /// however it was typed.
    #[test]
    fn a_bare_name_resolves_to_one_file_in_the_settings_directory() {
        assert_eq!(file_named("video").expect("a bare name"), "video.cfg");
        assert_eq!(file_named("video.cfg").expect("a bare name"), "video.cfg");
        assert_eq!(
            file_named("my_setup-2").expect("a bare name"),
            "my_setup-2.cfg"
        );
    }

    /// **A traversal typed into a nested line is refused there too**, which is
    /// the case a guard in the command alone would miss: the fixture and the
    /// engine's `config` both go through [`file_named`], so a file cannot reach
    /// what a prompt cannot.
    #[test]
    fn a_traversal_inside_a_config_file_is_refused_where_it_is_typed() {
        let printed = run(&[("outer.cfg", "config ../../.ssh/id_ed25519")], "outer");
        assert!(
            printed
                .iter()
                .any(|line| line.starts_with("outer.cfg:1:")
                    && line.contains("is not a config name")),
            "{printed:?}"
        );
    }

    /// **A file that runs itself is refused at the repeat**, rather than
    /// recursing — and the file it interrupted carries on.
    #[test]
    fn a_file_that_runs_itself_is_refused_rather_than_run_again() {
        let printed = run(
            &[("loop.cfg", "echo before\nconfig loop\necho after")],
            "loop",
        );
        assert!(
            printed
                .iter()
                .any(|line| line.starts_with("loop.cfg:2:") && line.contains("is running already")),
            "{printed:?}"
        );
        // The line after the refused one still ran, and the file was counted
        // once rather than twice — a recursion that had happened would print
        // `before` twice.
        assert_eq!(
            printed.iter().filter(|line| *line == "before").count(),
            1,
            "{printed:?}"
        );
        assert!(printed.contains(&"after".to_owned()), "{printed:?}");
    }

    /// **Two files that run each other stop at the first repeat**, which is the
    /// cycle a self-reference check would miss.
    #[test]
    fn two_files_that_run_each_other_stop_at_the_first_repeat() {
        let printed = run(
            &[
                ("a.cfg", "echo in-a\nconfig b"),
                ("b.cfg", "echo in-b\nconfig a"),
            ],
            "a",
        );
        assert!(
            printed
                .iter()
                .any(|line| line.starts_with("b.cfg:2:") && line.contains("is running already")),
            "{printed:?}"
        );
        assert_eq!(
            printed.iter().filter(|line| *line == "in-a").count(),
            1,
            "{printed:?}"
        );
        assert_eq!(
            printed.iter().filter(|line| *line == "in-b").count(),
            1,
            "{printed:?}"
        );
    }

    /// **A chain of distinct files stops at [`CONFIG_NESTING_LIMIT`]**, which
    /// no cycle check can see: every file in it is different from every other.
    ///
    /// The chain is longer than the limit on purpose, and that is asserted
    /// first — a chain no longer than the bound would run to its end and prove
    /// nothing about the bound.
    #[test]
    fn a_chain_of_distinct_files_stops_at_the_nesting_limit() {
        // A literal, and longer than the bound: a chain written as
        // `CONFIG_NESTING_LIMIT + n` would grow with the bound and could never
        // catch one that was raised.
        const CHAIN: usize = 12;

        let files: Vec<(String, String)> = (1..=CHAIN)
            .map(|step| {
                (
                    format!("deep{step}.cfg"),
                    format!("echo ran{step}\nconfig deep{}", step + 1),
                )
            })
            .collect();
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let printed = run(&borrowed, "deep1");

        let entered = printed
            .iter()
            .filter(|line| line.starts_with("ran"))
            .count();
        assert!(
            entered < CHAIN,
            "the chain ran to its end, so no bound was reached: {printed:?}"
        );
        assert_eq!(entered, CONFIG_NESTING_LIMIT, "{printed:?}");
        assert!(
            printed.iter().any(|line| {
                line.starts_with(&format!("deep{CONFIG_NESTING_LIMIT}.cfg:2:"))
                    && line.contains("as far as they nest")
            }),
            "{printed:?}"
        );
    }

    /// **The bound is released when a file finishes**, so two files run one
    /// after another are not a chain — the guard that is never released reads
    /// as a passing test right up until the second `config` of a session.
    #[test]
    fn a_file_that_finished_does_not_count_against_the_next_one() {
        let files = [("once.cfg", "echo ran"), ("twice.cfg", "config once")];
        for _ in 0..CONFIG_NESTING_LIMIT + 2 {
            let printed = run(&files, "twice");
            assert!(printed.contains(&"ran".to_owned()), "{printed:?}");
        }
    }

    /// **Line 3 of 5 failing leaves the other four run**, and the failure names
    /// the line it was on.
    #[test]
    fn a_line_that_fails_names_itself_and_the_file_runs_on() {
        let printed = run(
            &[(
                "half.cfg",
                "echo one\necho two\nno_such_thing_here\necho four\necho five",
            )],
            "half",
        );
        for expected in ["one", "two", "four", "five"] {
            assert!(printed.contains(&expected.to_owned()), "{printed:?}");
        }
        assert!(
            printed
                .iter()
                .any(|line| line.starts_with("half.cfg:3:") && line.contains("unknown command")),
            "{printed:?}"
        );
        assert!(
            printed.contains(&"half.cfg: 5 lines, 1 of them failed".to_owned()),
            "{printed:?}"
        );
    }

    /// **The number a failure carries is the file's own line number**, blank
    /// lines and comments included — which is what makes it something to open
    /// an editor at.
    #[test]
    fn a_blank_line_and_a_comment_keep_their_line_numbers() {
        let printed = run(
            &[(
                "spaced.cfg",
                "// what this file is for\n\n   // indented, and still a comment\nno_such_thing_here\n",
            )],
            "spaced",
        );
        assert!(
            printed.iter().any(|line| line.starts_with("spaced.cfg:4:")),
            "{printed:?}"
        );
        // One line ran: the three above it are a comment, a blank and a
        // comment, and none of them reached the registry.
        assert!(
            printed.contains(&"spaced.cfg: 1 line, 1 of them failed".to_owned()),
            "{printed:?}"
        );
    }

    /// **A file with nothing in it says so** rather than reporting a silent
    /// success over a file somebody mistyped the contents of.
    #[test]
    fn a_file_holding_only_comments_says_it_ran_nothing() {
        let printed = run(&[("empty.cfg", "// nothing here\n\n")], "empty");
        assert!(
            printed.contains(&"empty.cfg holds nothing to run".to_owned()),
            "{printed:?}"
        );
    }

    /// **What a line printed reaches the caller in order**, so a file of
    /// `echo`s reads like the same lines typed at the prompt.
    #[test]
    fn the_output_of_every_line_reaches_the_caller_in_order() {
        let printed = run(&[("say.cfg", "echo first\necho second\necho third")], "say");
        assert_eq!(printed, ["first", "second", "third", "say.cfg: 3 lines",]);
    }

    /// **A `clear` inside a file still clears the panel**, which is the one
    /// thing a line asks of its context rather than printing.
    #[test]
    fn a_clear_inside_a_file_reaches_the_panel() {
        FILES.with_borrow_mut(|held| {
            *held = vec![("wipe.cfg".to_owned(), "clear".to_owned())];
        });
        let registry = registry();
        let mut host = ();
        let mut cx = Context::new(&registry, &mut host);
        registry
            .execute(&mut cx, "config wipe")
            .expect("the file runs");
        assert!(cx.clear_requested());
    }

    /// **A run with no settings file refuses to read one**, the way `save`
    /// refuses to write one: a golden run or a headless harness has no
    /// directory of its own, and reading out of whoever's home directory it
    /// executes in is exactly what it must not do.
    #[test]
    fn a_run_with_no_settings_file_has_nowhere_to_read_from() {
        static COMMANDS: &[&crcbl_console::ConCommand] = &[&super::config];
        let registry = Registry::gather(&[Table::new(&[], &[], COMMANDS)])
            .expect("no two entries claim one name");
        let mut host = ConsoleHost::new(SettingsStack::new());
        let mut cx = Context::new(&registry, &mut host);
        let fault = registry
            .execute(&mut cx, "config video")
            .expect_err("this host was never given a name to save under");
        assert_eq!(
            fault.message(),
            "this run reads no settings file, so there is no directory to read `video.cfg` from"
        );
    }

    /// Run [`run_read`] over `read`, answering what it did, what the host it
    /// ran against ended up holding, and what it printed.
    ///
    /// **One harness for the silent case and the loud one, deliberately.** "A
    /// missing autoexec prints nothing" asserted on its own would pass just as
    /// well over a `run_read` that did nothing whatever, so the check that a
    /// file which *is* there runs goes through this same harness: between the
    /// two of them the silence is a decision rather than an absence of code.
    fn autoexec(read: Result<String, NotRead>) -> (Autoexec, Host, Vec<String>) {
        let registry = registry();
        let mut host = Host::default();
        let mut cx = Context::new(&registry, &mut host);
        let did = run_read(&mut cx, &format!("{AUTOEXEC}{CONFIG_SUFFIX}"), read);
        let printed = cx.into_lines();
        (did, host, printed)
    }

    /// **A machine with no `autoexec.cfg` boots in silence**, which is almost
    /// every machine: a file nobody wrote is the ordinary state and not an
    /// event, and a start-up line saying so would be in every log this engine
    /// ever produced.
    #[test]
    fn a_missing_autoexec_prints_nothing_and_sets_nothing() {
        let (did, host, printed) = autoexec(Err(NotRead::from_storage(
            &StorageError::NotFound(Path::new("autoexec.cfg").to_path_buf()),
            "autoexec.cfg",
        )));
        assert_eq!(did, Autoexec::Missing);
        assert!(printed.is_empty(), "{printed:?}");
        assert_eq!(host, Host::default(), "nothing ran, so nothing was set");
    }

    /// **An `autoexec.cfg` that is there runs, and what it sets lands on the
    /// host** — the whole point of the file, and the half of the harness that
    /// says the check above is asserting on a decision.
    #[test]
    fn an_autoexec_that_is_there_runs_its_lines_and_its_effects_land() {
        let (did, host, printed) = autoexec(Ok("gain 7\necho ready".to_owned()));
        assert_eq!(did, Autoexec::Ran);
        assert_eq!(
            host,
            Host { gain: 7 },
            "the line the file ran wrote through to the host the boot handed in",
        );
        assert_eq!(printed, ["gain = 7", "ready", "autoexec.cfg: 2 lines"]);
    }

    /// **A file that is there and cannot be read is reported**, because nothing
    /// else will say so: a start-up that was configured and silently was not is
    /// the failure worth printing for, and `StorageError::Pending` in a browser
    /// means the page booted out of order.
    #[test]
    fn an_autoexec_that_could_not_be_read_is_reported() {
        let (did, _host, printed) = autoexec(Err(NotRead::from_storage(
            &StorageError::Pending(Path::new("autoexec.cfg").to_path_buf()),
            "autoexec.cfg",
        )));
        assert_eq!(did, Autoexec::Unreadable);
        assert!(
            printed
                .iter()
                .any(|line| line.contains("not resident in this page's store yet")),
            "{printed:?}"
        );
    }

    /// **A run that reads no settings file runs no autoexec**, and asks no
    /// storage for anything on the way to deciding that.
    ///
    /// The line that keeps a golden run or a headless harness out of whichever
    /// home directory it executes in. Natively
    /// `SettingsStack::with_platform_storage("quarry", ..)` answers `Some` and
    /// would hand back `~/.config/quarry/autoexec.cfg` on a run that was never
    /// meant to read a file at all, so "this platform has a config directory" is
    /// not the gate — an unset `EngineLink::app_name` is.
    ///
    /// [`Autoexec`] is answered rather than only printed for this check: every
    /// other silent outcome is silent too, so the empty output alone would not
    /// say which of them held, and a gate replaced by a default name would still
    /// print nothing on a machine with no `autoexec.cfg` to find.
    #[test]
    fn a_run_with_no_settings_file_runs_no_autoexec() {
        static COMMANDS: &[&crcbl_console::ConCommand] = &[&super::config];
        let registry = Registry::gather(&[Table::new(&[], &[], COMMANDS)])
            .expect("no two entries claim one name");
        let mut host = ConsoleHost::new(SettingsStack::new());
        let mut cx = Context::new(&registry, &mut host);
        let did = run_autoexec(&mut cx);
        assert_eq!(
            did,
            Autoexec::NoSettingsFile,
            "a host with no name to save under must not reach the platform's config directory",
        );
        let printed = cx.into_lines();
        assert!(printed.is_empty(), "{printed:?}");
    }

    /// **Only "there is no such file" is the silent one.** The answers
    /// [`NotRead`] splits a read into are what a silent boot rests on, and this
    /// is the half a check driving text cannot demonstrate.
    #[test]
    fn a_read_that_found_nothing_is_the_only_one_the_boot_may_swallow() {
        let path = Path::new("autoexec.cfg").to_path_buf();
        assert!(matches!(
            NotRead::from_storage(&StorageError::NotFound(path.clone()), "autoexec.cfg"),
            NotRead::Missing(_)
        ));
        for error in [
            StorageError::Pending(path.clone()),
            StorageError::PermissionDenied(path.clone()),
            StorageError::WrongType(path),
        ] {
            let read = NotRead::from_storage(&error, "autoexec.cfg");
            assert!(
                matches!(read, NotRead::Unreadable(_)),
                "{error} is not a file that is simply not there: {read:?}"
            );
        }
    }

    /// **And what `config` prints for a file that is not there is unchanged**,
    /// which is the thing splitting the read up could quietly have altered:
    /// `config nosuchfile` answers the person who typed it.
    #[test]
    fn a_typed_config_still_names_the_file_it_could_not_read() {
        let fault = NotRead::from_storage(
            &StorageError::NotFound(Path::new("video.cfg").to_path_buf()),
            "video.cfg",
        )
        .into_fault();
        assert_eq!(
            fault.message(),
            "`video.cfg` could not be read: path not found: video.cfg"
        );
    }
}
