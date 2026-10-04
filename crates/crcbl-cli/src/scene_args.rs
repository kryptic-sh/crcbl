//! `crcbl scene` and `crcbl edit`'s command lines: the verbs, their
//! arguments, and the `-e` text an `edit` carries a verb in.
//!
//! Its own module rather than more of `crate::args`, which already holds every
//! other subcommand's parser; `crate::args` names [`SceneArgs`] and
//! [`EditArgs`] in its `Command` and hands the arguments here.
//!
//! **Flags only, no JSON on the way in** (decided 2026-10-04 by the owner): a
//! verb's arguments are words on the command line, so this CLI still parses no
//! JSON and needs no JSON dependency — `crate::json` stays a writer.
//!
//! **Only `--` starts an option.** A coordinate is often negative, and `move
//! 4 -3 0 2` should be the move it looks like rather than a refused `-3`; the
//! scene verbs take long options only, so a single dash is a value.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::args::{Command, Invocation};

/// `crcbl scene --help`.
pub const SCENE_USAGE: &str = "\
crcbl scene — read or edit a scene directory, with its undo history

USAGE:
    crcbl scene list   <DIR>
    crcbl scene query  <DIR> <ENTITY|SYSTEM>
    crcbl scene spawn  <DIR> <SYSTEM> [--set <PATH>=<VALUE>]...
    crcbl scene set    <DIR> <ENTITY> <PATH> <VALUE> [--system <SYSTEM>]
    crcbl scene delete <DIR> <ENTITY>
    crcbl scene move   <DIR> <ENTITY> <X> <Y> <Z>
    crcbl scene undo   <DIR>
    crcbl scene redo   <DIR>

DIR is a `.scn/` scene directory. Each edit is applied through the editor's own
document, with its validation and its undo, and the scene is saved before the
command returns. The vocabulary is the editor's: its greybox blocks, physics
bodies, meshes, and the components of breakout, puppet and towers.

ENTITY is an id, `4` or `#4`, or an entity's name.

PATH is a field's dotted path in its component, `position.1` for a y. VALUE is
read the way the scene's own files spell that field — `2.5`, `-3`, `true`,
`\"a label\"` with its quotes — so a number keeps every digit it was typed with.
`set` takes `--system` when the entity has components in more than one system.

`move` writes the `position` of the component that places the entity: the field
the editor's gizmo moves. `spawn` makes an entity at its component's default,
with each `--set` written in the same edit.

THE HISTORY:
    Every edit, undo and redo is one entry of a history kept beside the scene
    in `DIR/.crcbl-history`, so `undo` in one run walks back an edit another
    run made. The history is bound to the scene's bytes: if the scene has been
    changed since — by a person, the editor, a checkout — the history is
    refused (exit 3) rather than replayed, and removing the file starts a new
    one. It keeps the newest 64 entries.

THE LOCK:
    An edit locks the scene for the length of the run, through a file beside
    it, `DIR/.crcbl-lock`, as the editor does for as long as it has the scene
    open. A scene another program holds is refused (exit 4), naming the holder
    where the system lets its lock file be read, and nothing is changed: close
    the scene there and run again. A crashed holder's lock is released by the
    system, so the file left behind blocks nobody. `list` and `query` change
    nothing and take no lock, so they read a scene an editor has open.

EXIT CODES:
    0   done
    1   the scene would not open or save
    2   the invocation was malformed
    3   the history beside the scene is damaged or was written for other files
    4   another program, the editor or another run, holds the scene's lock
    13  the scene is not editable now
    14  no such entity              15  no such system
    16  no such field               17  a value its field refuses
    18  the edit contradicts the scene as it stands
    19  nothing to undo             20  nothing to redo
    21  the edit failed for another reason
    Each code from 13 is 10 plus the edit protocol's refusal code, so a script
    reads the same reasons from this CLI and from the edit server.

JSON:
    One object, after \"ok\" and \"command\":\"scene\". A field's value is its
    text, as the scene's files spell it, so a number keeps every digit.

    list     \"verb\":\"list\", \"scene\": its name,
             \"systems\": [{\"name\", \"entities\": [id...]}...],
             \"entities\": [{\"id\", \"name\" or null, \"systems\": [...]}...]
    query    \"verb\":\"query\", and \"entity\": ENTITY, or \"system\" and
             \"entities\": [ENTITY...], where ENTITY is a list entity with
             \"components\": [{\"system\", \"fields\": [{\"path\", \"value\"}...]}...]
    an edit  \"verb\", \"entity\": its id (not for undo or redo), and
             \"history\": {\"position\", \"length\"}
    refused  \"ok\":false, \"verb\", \"error\": the sentence, \"refusal\": the
             protocol's code and \"reason\": its name
    locked   \"ok\":false, \"verb\", \"error\", \"dir\", and \"holder\": the
             lock file's line where it could be read

OPTIONS:
        --json    Emit one JSON object instead of human output.
    -h, --help    Print this text.";

/// `crcbl edit --help`.
pub const EDIT_USAGE: &str = "\
crcbl edit — apply several edits to a scene directory in one run

USAGE:
    crcbl edit <DIR> -e <COMMAND> [-e <COMMAND>]...
    crcbl edit <DIR> --serve [PORT] [--lan]

Each COMMAND is a `crcbl scene` edit without its directory, applied in order:

    crcbl edit field.scn -e 'move 4 1 0 2' -e 'set 4 label \"gate\"' -e undo

The verbs are spawn, set, delete, move, undo and redo. A COMMAND is split at
whitespace, except that `set`'s value is the rest of the command, quotes and
spaces included, so `set`'s `--system` goes before its entity; a `spawn`
`--set` value cannot hold a space, so use `crcbl scene spawn` for one that
does.

Every command is its own entry in the scene's history. The scene is saved once,
after the last; if one is refused nothing is saved, and the exit code is that
refusal's — `crcbl scene --help` lists them.

The scene is locked for the whole run, so a scene the editor or another run
holds is refused with exit 4 before anything is applied; `crcbl scene --help`
says how under THE LOCK.

JSON:
    \"applied\": how many edits, and \"history\": {\"position\", \"length\"}; refused,
    \"failed\": the index of the refused `-e` from 0, beside the refusal's fields
    that `crcbl scene --help` lists.

SERVING:
    `--serve` keeps the scene open and serves it over UDP to clients of the
    edit protocol, which fetch it, follow every edit to it, and send edits,
    undos and redos of their own. Each is applied through the same document
    and the same history as the edits above, and the scene and its history
    are saved after every update that applied one, so a crash loses nothing
    a client was told applied.

    The scene is locked from start to `quit`: a scene another program holds
    exits 4 before anything is served, and while it is served `crcbl scene`'s
    edits on it exit 4 — remote edits are the only way in. A history the
    scene refuses exits 3, as above.

    PORT is the UDP port, any free one when it is left out or 0; the line
    the server starts with names it. The server listens on 127.0.0.1 only,
    so only this machine reaches it; `--lan` listens on every interface
    instead, where anyone who reaches the port can edit the scene, and the
    system may ask whether to let the network in.

    It reads commands at its standard input:
        status   print the status line now
        save     save now, after a save that failed
        quit     save, end every client's session, release the lock, exit 0
    A `quit` whose save fails says why and serves on, so no edit is lost:
    put right what stopped the save, or end the process to drop the edits.
    Its standard input ending is not a quit.

    The status line names the revision, the history's position, the clients
    in, whether the scene is saved, and how many messages were refused as
    malformed; it prints when any but the last changes, and every 10 seconds.

    `--serve` takes no `-e` and no `--json`: its output is a running log,
    not one object.

OPTIONS:
    -e <COMMAND>    One edit. At least one, unless serving.
        --serve [PORT]
                    Serve the scene to edit clients, until `quit`.
        --lan       With `--serve`: listen on every interface.
        --json      Emit one JSON object instead of human output.
    -h, --help      Print this text.";

/// One verb of `crcbl scene`, with its arguments as they were typed.
///
/// Text rather than ids and values: which entity a name is, and what a value
/// is read as, depend on the scene, which `crate::scene_cmd` opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneVerb {
    /// Every system and entity.
    List,
    /// One entity's fields, or every entity's in one system.
    Query {
        /// An entity or a system.
        target: String,
    },
    /// A change to the scene, which is what `crcbl edit`'s `-e` takes too.
    Edit(SceneEdit),
}

impl SceneVerb {
    /// The name this verb is reported under in `--json`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Query { .. } => "query",
            Self::Edit(edit) => edit.name(),
        }
    }
}

/// A verb that changes the scene, each one entry of its history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneEdit {
    /// A new entity in `system`.
    Spawn {
        /// Where it goes.
        system: String,
        /// `(path, value)`, written in the same edit.
        sets: Vec<(String, String)>,
    },
    /// One field written.
    Set {
        /// Whose.
        entity: String,
        /// The field's dotted path.
        path: String,
        /// The value, as the scene's files spell it.
        value: String,
        /// Which of the entity's components, when it has several.
        system: Option<String>,
    },
    /// An entity removed.
    Delete {
        /// Whose.
        entity: String,
    },
    /// An entity's placing component's `position` written.
    Move {
        /// Whose.
        entity: String,
        /// `x`, `y` and `z`, as typed.
        to: [String; 3],
    },
    /// The newest applied entry walked back.
    Undo,
    /// The newest undone entry applied again.
    Redo,
}

impl SceneEdit {
    /// The name this edit is reported under in `--json`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "spawn",
            Self::Set { .. } => "set",
            Self::Delete { .. } => "delete",
            Self::Move { .. } => "move",
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }
}

/// `crcbl scene`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneArgs {
    /// The scene directory.
    pub dir: PathBuf,
    /// What to do to it.
    pub verb: SceneVerb,
    /// Machine-readable output.
    pub json: bool,
}

/// `crcbl edit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditArgs {
    /// The scene directory.
    pub dir: PathBuf,
    /// The edits, in order; never empty.
    pub edits: Vec<SceneEdit>,
    /// Machine-readable output.
    pub json: bool,
}

/// `crcbl edit --serve`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeArgs {
    /// The scene directory.
    pub dir: PathBuf,
    /// The UDP port, 0 for any free one.
    pub port: u16,
    /// Whether to listen on every interface rather than on loopback alone.
    pub lan: bool,
}

/// Parses `crcbl scene`'s arguments, which follow the word `scene`.
pub fn parse_scene(args: impl Iterator<Item = OsString>) -> Invocation {
    let mut json = false;
    let mut words = Vec::new();
    for arg in args {
        let Some(word) = arg.to_str() else {
            // The directory is the one argument that is a path; everything
            // else lands in a text file, so only it may be other bytes.
            if words.len() == 1 {
                words.push(Word::Path(arg));
                continue;
            }
            return bad_text("scene", &arg);
        };
        match word {
            "-h" | "--help" => return Invocation::Help(SCENE_USAGE),
            "--json" => json = true,
            _ => words.push(Word::Text(word.to_owned())),
        }
    }
    let mut words = words.into_iter();
    let Some(verb) = words.next() else {
        return bad("`scene` needs a verb (list, query, spawn, set, delete, move, undo, redo)");
    };
    let verb = verb.text();
    let Some(dir) = words.next() else {
        return bad(&format!("`scene {verb}` needs a scene directory"));
    };
    let rest: Vec<String> = words.map(Word::text).collect();
    match parse_verb(&verb, &rest) {
        Ok(verb) => Invocation::Command(Command::Scene(SceneArgs {
            dir: dir.path(),
            verb,
            json,
        })),
        Err(message) => bad(&message),
    }
}

/// Parses `crcbl edit`'s arguments, which follow the word `edit`.
pub fn parse_edit(args: impl Iterator<Item = OsString>) -> Invocation {
    let mut args = args.peekable();
    let mut json = false;
    let mut dir = None;
    let mut edits = Vec::new();
    let mut serve = None;
    let mut lan = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Invocation::Help(EDIT_USAGE),
            Some("--json") => json = true,
            Some("--lan") => lan = true,
            Some("--serve") => {
                // The port is optional, so the next argument is taken only
                // when it reads as one — `LanMode::consume`'s rule for
                // `--host [PORT]`.
                let port = args
                    .peek()
                    .and_then(|next| next.to_str())
                    .and_then(|next| next.parse::<u16>().ok());
                if port.is_some() {
                    args.next();
                }
                serve = Some(port.unwrap_or(0));
            }
            Some("-e") => {
                let Some(command) = args.next() else {
                    return bad("-e needs a command, such as `-e 'move 4 1 0 2'`");
                };
                let Some(command) = command.to_str() else {
                    return bad_text("edit", &command);
                };
                match parse_command(command) {
                    Ok(edit) => edits.push(edit),
                    Err(message) => return bad(&format!("-e `{command}`: {message}")),
                }
            }
            Some(other) if other.starts_with('-') => {
                return bad(&format!("`edit` has no option `{other}`"));
            }
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => {
                return bad(&format!(
                    "`edit` takes one scene directory; `{}` is one too many",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    let Some(dir) = dir else {
        return bad("`edit` needs a scene directory");
    };
    if let Some(port) = serve {
        if !edits.is_empty() {
            return bad("`--serve` takes no `-e`: a served scene is edited by its clients");
        }
        if json {
            return bad("`--serve` prints a running log, not one JSON object; drop `--json`");
        }
        return Invocation::Command(Command::Serve(ServeArgs { dir, port, lan }));
    }
    if lan {
        return bad("`--lan` says where `--serve` listens, and goes with it");
    }
    if edits.is_empty() {
        return bad("`edit` needs at least one `-e <COMMAND>` (or `--serve`)");
    }
    Invocation::Command(Command::Edit(EditArgs { dir, edits, json }))
}

/// An argument of `crcbl scene`, which is text but for the directory.
enum Word {
    Text(String),
    Path(OsString),
}

impl Word {
    /// The argument as text, for every place but the directory.
    fn text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Path(path) => path.to_string_lossy().into_owned(),
        }
    }

    /// The argument as a path, for the directory.
    fn path(self) -> PathBuf {
        match self {
            Self::Text(text) => PathBuf::from(text),
            Self::Path(path) => PathBuf::from(path),
        }
    }
}

/// One `-e` command: a verb and its arguments split at whitespace, but for
/// `set`, whose value is the rest of the command as written.
fn parse_command(command: &str) -> Result<SceneEdit, String> {
    let words = spans(command);
    let Some(&(start, end)) = words.first() else {
        return Err("it names no verb".to_owned());
    };
    let verb = &command[start..end];
    let mut rest: Vec<String> = words[1..]
        .iter()
        .map(|&(start, end)| command[start..end].to_owned())
        .collect();
    if verb == "set" {
        // `set [--system <S>] <ENTITY> <PATH> <VALUE...>`: the value starts
        // at the word after the path, and runs to the end of the command.
        let options = if rest.first().is_some_and(|word| word == "--system") {
            2
        } else {
            0
        };
        let value_at = 1 + options + 2;
        if let Some(&(start, _)) = words.get(value_at) {
            rest.truncate(value_at - 1);
            rest.push(command[start..].trim_end().to_owned());
        }
    }
    match parse_verb(verb, &rest)? {
        SceneVerb::Edit(edit) => Ok(edit),
        read => Err(format!("`{}` reads the scene; -e takes edits", read.name())),
    }
}

/// The byte ranges of `text`'s whitespace-separated words.
fn spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (at, character) in text.char_indices() {
        match (character.is_whitespace(), start) {
            (true, Some(from)) => {
                spans.push((from, at));
                start = None;
            }
            (false, None) => start = Some(at),
            _ => {}
        }
    }
    if let Some(from) = start {
        spans.push((from, text.len()));
    }
    spans
}

/// The verb `verb` with arguments `rest`, the directory already taken.
fn parse_verb(verb: &str, rest: &[String]) -> Result<SceneVerb, String> {
    let mut system = None;
    let mut sets = Vec::new();
    let mut positional = Vec::new();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--system" if verb == "set" => {
                let name = words.next().ok_or("--system needs a system")?;
                system = Some(name.clone());
            }
            "--set" if verb == "spawn" => {
                let pair = words.next().ok_or("--set needs <PATH>=<VALUE>")?;
                let (path, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--set `{pair}` is not <PATH>=<VALUE>"))?;
                if path.is_empty() {
                    return Err(format!("--set `{pair}` names no field"));
                }
                sets.push((path.to_owned(), value.to_owned()));
            }
            option if option.starts_with("--") => {
                return Err(format!("`scene {verb}` has no option `{option}`"));
            }
            _ => positional.push(word.clone()),
        }
    }
    let wanted = match verb {
        "list" | "undo" | "redo" => 0,
        "query" | "spawn" | "delete" => 1,
        "set" => 3,
        "move" => 4,
        other => {
            return Err(format!(
                "`scene` has no verb `{other}` (known: list, query, spawn, set, delete, move, \
                 undo, redo)"
            ));
        }
    };
    if positional.len() != wanted {
        return Err(format!(
            "`scene {verb}` takes {wanted} argument(s) after the directory, not {}",
            positional.len()
        ));
    }
    let mut positional = positional.into_iter();
    let mut next = || positional.next().expect("the count was checked");
    Ok(match verb {
        "list" => SceneVerb::List,
        "query" => SceneVerb::Query { target: next() },
        "undo" => SceneVerb::Edit(SceneEdit::Undo),
        "redo" => SceneVerb::Edit(SceneEdit::Redo),
        "spawn" => SceneVerb::Edit(SceneEdit::Spawn {
            system: next(),
            sets,
        }),
        "delete" => SceneVerb::Edit(SceneEdit::Delete { entity: next() }),
        "set" => SceneVerb::Edit(SceneEdit::Set {
            entity: next(),
            path: next(),
            value: next(),
            system,
        }),
        _ => SceneVerb::Edit(SceneEdit::Move {
            entity: next(),
            to: [next(), next(), next()],
        }),
    })
}

/// A malformed invocation.
fn bad(message: &str) -> Invocation {
    Invocation::BadUsage(message.to_owned())
}

/// An argument that had to be text and is not.
fn bad_text(command: &str, arg: &OsString) -> Invocation {
    Invocation::BadUsage(format!(
        "`{command}` needs its arguments as valid UTF-8; `{}` is not",
        arg.to_string_lossy()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(args: &[&str]) -> Invocation {
        parse_scene(args.iter().map(OsString::from))
    }

    fn edit(args: &[&str]) -> Invocation {
        parse_edit(args.iter().map(OsString::from))
    }

    fn scene_verb(args: &[&str]) -> SceneVerb {
        match scene(args) {
            Invocation::Command(Command::Scene(parsed)) => parsed.verb,
            other => panic!("{args:?} did not parse: {other:?}"),
        }
    }

    fn refused(invocation: Invocation) -> String {
        match invocation {
            Invocation::BadUsage(message) => message,
            other => panic!("not refused: {other:?}"),
        }
    }

    /// **A negative coordinate is a value**, not an option: only `--` starts
    /// one.
    #[test]
    fn a_negative_coordinate_is_a_value() {
        assert_eq!(
            scene_verb(&["move", "field.scn", "4", "-3", "0", "-2.5", "--json"]),
            SceneVerb::Edit(SceneEdit::Move {
                entity: "4".to_owned(),
                to: ["-3".to_owned(), "0".to_owned(), "-2.5".to_owned()],
            })
        );
    }

    /// Each verb takes its own count of arguments, and its own options.
    #[test]
    fn each_verb_takes_its_own_arguments() {
        assert_eq!(
            scene_verb(&[
                "spawn",
                "d",
                "plots",
                "--set",
                "position.0=1.5",
                "--set",
                "label=\"a\""
            ]),
            SceneVerb::Edit(SceneEdit::Spawn {
                system: "plots".to_owned(),
                sets: vec![
                    ("position.0".to_owned(), "1.5".to_owned()),
                    ("label".to_owned(), "\"a\"".to_owned()),
                ],
            })
        );
        assert_eq!(
            scene_verb(&["set", "d", "--system", "plots", "#4", "label", "\"a b\""]),
            SceneVerb::Edit(SceneEdit::Set {
                entity: "#4".to_owned(),
                path: "label".to_owned(),
                value: "\"a b\"".to_owned(),
                system: Some("plots".to_owned()),
            })
        );
        assert!(refused(scene(&["move", "d", "4", "1", "2"])).contains("takes 4"));
        assert!(refused(scene(&["list", "d", "extra"])).contains("takes 0"));
        assert!(
            refused(scene(&["spawn", "d", "plots", "--set", "position.0"]))
                .contains("<PATH>=<VALUE>")
        );
        assert!(refused(scene(&["delete", "d", "4", "--system", "plots"])).contains("no option"));
        assert!(refused(scene(&["paste", "d"])).contains("no verb `paste`"));
        assert!(refused(scene(&["list"])).contains("scene directory"));
    }

    /// **An `-e` command's `set` value is the rest of it**, spaces and quotes
    /// as written; the other verbs split at whitespace, and a read is refused.
    #[test]
    fn an_e_commands_set_value_is_the_rest_of_it() {
        let Invocation::Command(Command::Edit(parsed)) = edit(&[
            "d",
            "-e",
            "set --system plots 4 label \"far  gate\"  ",
            "-e",
            "move entry -1 0 2",
            "-e",
            "undo",
        ]) else {
            panic!("did not parse");
        };
        assert_eq!(
            parsed.edits,
            [
                SceneEdit::Set {
                    entity: "4".to_owned(),
                    path: "label".to_owned(),
                    value: "\"far  gate\"".to_owned(),
                    system: Some("plots".to_owned()),
                },
                SceneEdit::Move {
                    entity: "entry".to_owned(),
                    to: ["-1".to_owned(), "0".to_owned(), "2".to_owned()],
                },
                SceneEdit::Undo,
            ]
        );
        assert!(refused(edit(&["d", "-e", "list"])).contains("-e takes edits"));
        assert!(refused(edit(&["d"])).contains("at least one"));
        assert!(refused(edit(&["-e", "undo"])).contains("scene directory"));
    }

    /// **`--serve` takes an optional port and `--lan`**, and refuses `-e`,
    /// `--json`, and `--lan` without it.
    #[test]
    fn serve_takes_an_optional_port_and_lan() {
        let serve = |args: &[&str]| match edit(args) {
            Invocation::Command(Command::Serve(parsed)) => parsed,
            other => panic!("{args:?} did not parse: {other:?}"),
        };
        assert_eq!(
            serve(&["d", "--serve"]),
            ServeArgs {
                dir: PathBuf::from("d"),
                port: 0,
                lan: false,
            }
        );
        assert_eq!(
            serve(&["--serve", "7777", "d", "--lan"]),
            ServeArgs {
                dir: PathBuf::from("d"),
                port: 7777,
                lan: true,
            }
        );
        // A word that is not a port is the directory, not a value.
        assert_eq!(
            serve(&["--serve", "field.scn"]).dir,
            PathBuf::from("field.scn")
        );
        assert!(refused(edit(&["d", "--serve", "-e", "undo"])).contains("no `-e`"));
        assert!(refused(edit(&["d", "--serve", "--json"])).contains("drop `--json`"));
        assert!(refused(edit(&["d", "--lan", "-e", "undo"])).contains("goes with it"));
        assert!(refused(edit(&["--serve", "7777"])).contains("scene directory"));
    }

    /// **The help's numbers are the code's**: the history's bound, the lock
    /// file's name, and the exit code of every refusal the edit protocol
    /// names and of a history refused and a scene locked.
    #[test]
    fn the_help_states_the_codes_and_the_bound_the_code_uses() {
        let bound = crcbl::scene_edit::MAX_HISTORY_ENTRIES;
        assert!(
            SCENE_USAGE.contains(&format!("keeps the newest {bound} entries")),
            "the help names another bound than {bound}"
        );
        for code in 0x03..=0x0B_u8 {
            let reason = crcbl::net::EditRefusal(code);
            assert!(
                reason.name().is_some(),
                "code {code} is not a named refusal"
            );
            let exit = crate::report::REFUSED_BASE + code;
            assert!(
                SCENE_USAGE.contains(&format!("    {exit}  ")),
                "the help does not list exit {exit} for {reason}"
            );
        }
        for exit in [crate::report::EXIT_HISTORY, crate::report::EXIT_LOCKED] {
            assert!(
                SCENE_USAGE.contains(&format!("    {exit}   ")),
                "the help does not list exit {exit}"
            );
        }
        assert!(
            SCENE_USAGE.contains(&format!("`DIR/{}`", crcbl::scene_edit::SCENE_LOCK)),
            "the help names another lock file"
        );
    }
}
