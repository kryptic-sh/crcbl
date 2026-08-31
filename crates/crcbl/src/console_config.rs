//! `config`: running a file of console lines.
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

use std::cell::RefCell;
use std::path::Path;

use crcbl_console::{Context, Fault};
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

use crate::settings::ConsoleHost;

/// The extension a config file carries: Source's own, and the one this command
/// appends to a name that does not already have it.
pub const CONFIG_SUFFIX: &str = ".cfg";

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
        let text = read_file(&app_name, &file)?;
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

/// The text of `file`, out of the directory this run's settings live in.
///
/// [`SettingsStack::with_platform_storage`] is the same seam `save` writes
/// through, which is what makes "the settings directory" one answer rather than
/// two: natively the platform config directory, in a browser the store the page
/// installed.
fn read_file(app_name: &str, file: &str) -> Result<String, Fault> {
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
        return Err(Fault::new(nowhere));
    };
    let bytes = read.map_err(|error| match error {
        // A browser read of something the shim has not restored yet is a state
        // and not a failure — `crcbl_store::web` polls it — so the line says to
        // ask again rather than that the file is missing.
        StorageError::Pending(_) => Fault::new(format!(
            "`{file}` is not resident in this page's store yet — ask again in a moment"
        )),
        error => Fault::new(format!("`{file}` could not be read: {error}")),
    })?;
    String::from_utf8(bytes).map_err(|_| Fault::new(format!("`{file}` is not UTF-8 text")))
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
}
