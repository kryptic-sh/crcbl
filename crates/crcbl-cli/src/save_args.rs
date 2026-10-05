//! `crcbl save`'s command line: the three branches and their arguments.
//!
//! Its own module rather than more of `crate::args`, for `crate::scene_args`'
//! reason: `crate::args` names [`SaveArgs`] in its `Command` and hands the
//! arguments here.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::args::{Command, Invocation, check_app_name};

/// `crcbl save --help`.
///
/// The preview's byte count is written out because a `const &str` cannot
/// interpolate; `the_save_help_names_the_hex_cap` pins it to
/// `crate::save_cmd::HEX_PREVIEW_BYTES`.
pub const SAVE_USAGE: &str = "\
crcbl save — inspect a game's save files from outside the game

USAGE:
    crcbl save list [--app <NAME> | --dir <DIR>]
    crcbl save dump <FILE> [--hex]
    crcbl save diff <A> <B>

Every file is read the way the game reads it, through the save container's own
reader: an older format version is migrated in memory and the file on disk is
left as it is, and a file that is damaged, truncated or from a newer engine is
refused with the reason the game would give.

What is shown is the container: its header — format version, tick, playtime,
engine version, scene — and its sectors, each by its coordinates, its length
and a SHA-256 of its bytes. A sector's payload is the game's own encoding and
is never decoded here; `--hex` shows its first bytes.

list
    One line per file in the game's save directory: its name, size, tick,
    playtime, format version and engine version. A file that does not open is
    listed with the reason, not skipped. Subdirectories are not entered.
    The directory is the game's data directory — on Linux that is
    ~/.local/share/<APP> — or DIR. Without --app or --dir, APP is the package
    name of the nearest Cargo.toml at or above the current directory, as for
    `crcbl settings`. Nothing is created: a game that has never saved lists no
    files.

dump
    FILE's header and its sector table. With --hex, the first 64 bytes of
    each sector, as hex.

diff
    The header fields that differ, the sectors only one file has, and each
    sector whose bytes differ, with the first offset they differ at and both
    lengths. Sectors are matched by their coordinates, so the order a file
    lists them in is not compared.

restore is not built: no game keeps an autosave ring for it to restore from.

EXIT CODES:
    list, dump:  0 ok              1 the command failed   2 bad invocation
    diff:        0 the same        1 different            2 trouble: a file
                                                            did not open, or a
                                                            bad invocation
    diff takes `cmp`'s codes, so `--json` says \"ok\":true over two saves that
    differ.

OPTIONS:
        --app <NAME>  The game whose saves `list` reads.
        --dir <DIR>   The directory `list` reads, in place of a game's.
        --hex         Show each sector's first bytes in `dump`.
        --json        Emit one JSON object instead of human output.
    -h, --help        Print this text.
    A FILE starting with `-` follows `--`.";

/// Which directory `crcbl save list` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveDir {
    /// The named game's data directory.
    App(String),
    /// A directory given outright.
    Dir(PathBuf),
    /// The data directory of the game the project here builds — a filesystem
    /// question, so it is left to the command.
    Project,
}

/// Which branch of `crcbl save` was asked for, with its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveAction {
    /// One line per file in a save directory.
    List(SaveDir),
    /// One file's header and sector table.
    Dump {
        /// The save to read.
        file: PathBuf,
        /// Whether to preview each sector's bytes.
        hex: bool,
    },
    /// What differs between two files.
    Diff {
        /// The first save.
        a: PathBuf,
        /// The second save.
        b: PathBuf,
    },
}

impl SaveAction {
    /// The name this branch is reported under in `--json`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::List(_) => "list",
            Self::Dump { .. } => "dump",
            Self::Diff { .. } => "diff",
        }
    }
}

/// `crcbl save`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveArgs {
    /// What to do.
    pub action: SaveAction,
    /// Machine-readable output.
    pub json: bool,
}

/// Parses `crcbl save`'s arguments, which follow the word `save`.
pub fn parse_save(mut args: impl Iterator<Item = OsString>) -> Invocation {
    let mut json = false;
    let mut hex = false;
    let mut app = None;
    let mut dir = None;
    // The branch and its files, in any position among the flags, as
    // `crcbl settings` takes them.
    let mut positional: Vec<OsString> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(SAVE_USAGE),
            Some("--json") => json = true,
            Some("--hex") => hex = true,
            // A file name may start with a `-`, and this is the way to type
            // one — `run`'s and `settings`' separator.
            Some("--") => positional.extend(args.by_ref()),
            Some("--app") => {
                let Some(value) = args.next() else {
                    return bad("--app needs a name");
                };
                let Some(name) = value.to_str() else {
                    return Invocation::BadUsage(format!(
                        "`save` needs a name for --app that is valid UTF-8; `{}` is not",
                        value.to_string_lossy()
                    ));
                };
                if let Err(why) = check_app_name(name) {
                    return Invocation::BadUsage(format!(
                        "`--app {name}` is not a usable directory name: {why}"
                    ));
                }
                app = Some(name.to_owned());
            }
            // A path, so it stays an `OsString` all the way to `PathBuf`.
            Some("--dir") => match args.next() {
                Some(value) if !value.is_empty() => dir = Some(PathBuf::from(value)),
                _ => return bad("--dir needs a directory"),
            },
            Some(other) if other.starts_with('-') => {
                return Invocation::BadUsage(format!("`save` has no option `{other}`"));
            }
            _ => positional.push(arg),
        }
    }

    let Some((first, files)) = positional.split_first() else {
        return bad("`save` needs a subcommand (list, dump, diff)");
    };
    let branch = match first.to_str() {
        Some(branch @ ("list" | "dump" | "diff")) => branch,
        Some("restore") => {
            return bad(
                "`save restore` is not built: no game keeps an autosave ring for it to \
                 restore from (`docs/backlog.md`, `crcbl save`)",
            );
        }
        _ => {
            return Invocation::BadUsage(format!(
                "`save` has no subcommand `{}` (known: list, dump, diff)",
                first.to_string_lossy()
            ));
        }
    };

    // Each flag belongs to one branch, and one given to another is refused
    // rather than ignored: a `diff --hex` that printed no hex would read as
    // two saves with nothing to show.
    if branch != "list" && (app.is_some() || dir.is_some()) {
        return Invocation::BadUsage(format!(
            "`save {branch}` reads the files it is given; --app and --dir are `list`'s"
        ));
    }
    if branch != "dump" && hex {
        return Invocation::BadUsage(format!("`save {branch}` has no --hex; it is `dump`'s"));
    }

    let wanted = match branch {
        "list" => 0,
        "dump" => 1,
        _ => 2,
    };
    if files.len() != wanted {
        return Invocation::BadUsage(format!(
            "`save {branch}` takes {wanted} file(s); {} given",
            files.len()
        ));
    }

    let action = match branch {
        "list" => SaveAction::List(match (app, dir) {
            (Some(_), Some(_)) => {
                return bad("`save list` reads one directory: --app or --dir, not both");
            }
            (Some(app), None) => SaveDir::App(app),
            (None, Some(dir)) => SaveDir::Dir(dir),
            (None, None) => SaveDir::Project,
        }),
        "dump" => SaveAction::Dump {
            file: PathBuf::from(&files[0]),
            hex,
        },
        _ => SaveAction::Diff {
            a: PathBuf::from(&files[0]),
            b: PathBuf::from(&files[1]),
        },
    };
    Invocation::Command(Command::Save(SaveArgs { action, json }))
}

/// A malformed invocation.
fn bad(message: &str) -> Invocation {
    Invocation::BadUsage(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(args: &[&str]) -> Invocation {
        parse_save(args.iter().map(OsString::from))
    }

    fn action(args: &[&str]) -> SaveAction {
        match save(args) {
            Invocation::Command(Command::Save(parsed)) => parsed.action,
            other => panic!("{args:?} did not parse: {other:?}"),
        }
    }

    /// Each branch takes its own files and flags, in any order among them.
    #[test]
    fn each_branch_takes_its_own_files_and_flags() {
        assert_eq!(action(&["list"]), SaveAction::List(SaveDir::Project));
        assert_eq!(
            action(&["--app", "towers", "list"]),
            SaveAction::List(SaveDir::App("towers".to_owned()))
        );
        assert_eq!(
            action(&["list", "--dir", "saves"]),
            SaveAction::List(SaveDir::Dir(PathBuf::from("saves")))
        );
        assert_eq!(
            action(&["dump", "--hex", "a.crb"]),
            SaveAction::Dump {
                file: PathBuf::from("a.crb"),
                hex: true
            }
        );
        assert_eq!(
            action(&["diff", "a.crb", "--", "-b.crb"]),
            SaveAction::Diff {
                a: PathBuf::from("a.crb"),
                b: PathBuf::from("-b.crb")
            }
        );
    }

    /// A flag on a branch it does not belong to, a wrong count of files, an
    /// unknown branch and `restore` are each a bad invocation.
    #[test]
    fn a_flag_or_a_file_a_branch_does_not_take_is_refused() {
        for args in [
            vec![],
            vec!["frobnicate"],
            vec!["restore", "a.crb"],
            vec!["list", "a.crb"],
            vec!["list", "--app", "g", "--dir", "d"],
            vec!["list", "--hex"],
            vec!["list", "--app", ".."],
            vec!["list", "--dir"],
            vec!["dump"],
            vec!["dump", "a.crb", "b.crb"],
            vec!["dump", "a.crb", "--app", "g"],
            vec!["diff", "a.crb"],
            vec!["diff", "a.crb", "b.crb", "--hex"],
            vec!["diff", "a.crb", "b.crb", "--dir", "d"],
            vec!["dump", "a.crb", "--frobnicate"],
        ] {
            assert!(
                matches!(save(&args), Invocation::BadUsage(_)),
                "{args:?} was not refused"
            );
        }
    }

    /// `--json` is read on every branch.
    #[test]
    fn json_is_read_on_every_branch() {
        for args in [
            vec!["list", "--json"],
            vec!["dump", "a.crb", "--json"],
            vec!["diff", "a.crb", "b.crb", "--json"],
        ] {
            let Invocation::Command(command) = save(&args) else {
                panic!("{args:?} did not parse");
            };
            assert!(command.json(), "{args:?}");
        }
    }

    /// The help names the preview's real cap, which it cannot interpolate.
    #[test]
    fn the_save_help_names_the_hex_cap() {
        let cap = crate::save_cmd::HEX_PREVIEW_BYTES;
        assert!(
            SAVE_USAGE.contains(&format!("first {cap} bytes")),
            "`save --help` does not name the {cap}-byte preview:\n{SAVE_USAGE}"
        );
    }
}
