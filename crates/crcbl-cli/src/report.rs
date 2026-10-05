//! Two output shapes for one result, and the exit-code contract.
//!
//! Topic 11 (`docs/notes/tooling.md`) fixes both halves: "`--json` on every
//! subcommand; human tables otherwise" and "exit codes are meaningful (0 ok, 1
//! command failed, 2 bad invocation)". Neither is a per-command decision, so
//! neither is implemented per command: every subcommand returns an
//! [`Outcome`] or a [`Failure`] carrying *both* renderings, and [`emit`] picks.
//!
//! The consequence worth stating: a failure is still machine-readable. `--json`
//! on a command that failed prints an object with `"ok": false` on **stdout**
//! and exits non-zero, rather than printing prose to stderr and leaving a
//! script to parse it. An agent or a CI job reads one place for both answers.

use std::process::ExitCode;

use crate::json::Json;

/// Exit code for "the command worked".
pub const EXIT_OK: u8 = 0;

/// Exit code for "the command failed".
pub const EXIT_FAILED: u8 = 1;

/// Exit code for "the invocation was malformed".
pub const EXIT_USAGE: u8 = 2;

/// Exit code for "the edit history beside a scene was refused" — `crcbl
/// scene` and `crcbl edit` only; see `crate::scene_args::SCENE_USAGE`.
pub const EXIT_HISTORY: u8 = 3;

/// Exit code for "another program holds the scene's lock" — `crcbl scene`'s
/// edits and `crcbl edit` only; see `crate::scene_args::SCENE_USAGE`.
pub const EXIT_LOCKED: u8 = 4;

/// Exit code for "the two saves differ" — `crcbl save diff` only, which takes
/// `cmp`'s convention (0 the same, 1 different, 2 trouble); see
/// `crate::save_args::SAVE_USAGE`. The same number as [`EXIT_FAILED`], and a
/// different meaning: the comparison worked, and `--json` says `"ok":true`.
pub const EXIT_DIFFERENT: u8 = 1;

/// Exit code for "the saves could not be compared" — `crcbl save diff` only,
/// `cmp`'s "trouble", which lands on [`EXIT_USAGE`]'s number: under that
/// convention a malformed invocation is trouble too.
pub const EXIT_TROUBLE: u8 = 2;

/// What `crcbl scene` and `crcbl edit` add to the edit protocol's refusal
/// code (`crcbl::net::EditRefusal`) to exit with it, so a refused edit's
/// reason is its exit code: the codes start at 1, so the lowest a refusal
/// exits with sits clear of the four above.
pub const REFUSED_BASE: u8 = 10;

/// A command that worked.
///
/// `PartialEq` but not `Eq`: a [`Json`] field may hold a float.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// What a person reads.
    pub human: String,
    /// Command-specific fields, merged after `ok` and `command`.
    pub json: Vec<(&'static str, Json)>,
}

/// A command that did not work.
///
/// `PartialEq` but not `Eq`, for [`Outcome`]'s reason.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// What a person reads. Also the JSON `error` field.
    pub message: String,
    /// Command-specific fields, merged after `ok`, `command` and `error`.
    pub json: Vec<(&'static str, Json)>,
    /// The process exit code. [`EXIT_FAILED`] unless the command has a reason.
    pub code: u8,
}

impl Failure {
    /// A plain failure: exit 1, no extra fields.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            json: Vec::new(),
            code: EXIT_FAILED,
        }
    }

    /// Adds a machine-readable field.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: Json) -> Self {
        self.json.push((key, value));
        self
    }
}

/// Prints a result in whichever form was asked for and returns the exit code.
pub fn emit(command: &'static str, json: bool, result: Result<Outcome, Failure>) -> ExitCode {
    emit_as(command, json, result, EXIT_OK)
}

/// [`emit`], for a command whose answer is its exit code even when it worked:
/// `success` is the code an [`Outcome`] exits with. `crcbl save diff` is the
/// one caller, exiting [`EXIT_DIFFERENT`] over two saves it compared and
/// found different.
pub fn emit_as(
    command: &'static str,
    json: bool,
    result: Result<Outcome, Failure>,
    success: u8,
) -> ExitCode {
    match result {
        Ok(outcome) => {
            if json {
                let mut fields = vec![("ok", Json::Bool(true)), ("command", Json::string(command))];
                fields.extend(outcome.json);
                println!("{}", Json::Object(fields));
            } else {
                println!("{}", outcome.human);
            }
            ExitCode::from(success)
        }
        Err(failure) => {
            if json {
                let mut fields = vec![
                    ("ok", Json::Bool(false)),
                    ("command", Json::string(command)),
                    ("error", Json::string(&failure.message)),
                ];
                fields.extend(failure.json);
                println!("{}", Json::Object(fields));
            } else {
                eprintln!("crcbl: {}", failure.message);
            }
            ExitCode::from(failure.code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_defaults_to_exit_one_and_carries_its_fields() {
        let failure = Failure::new("nope").with("phase", Json::string("P5"));
        assert_eq!(failure.code, EXIT_FAILED);
        assert_eq!(failure.json, vec![("phase", Json::string("P5"))]);
    }

    /// The two codes are constants precisely so a subcommand cannot invent a
    /// third one by accident.
    #[test]
    fn the_exit_contract_is_two_numbers() {
        assert_eq!((EXIT_FAILED, EXIT_USAGE), (1, 2));
    }

    /// `save diff`'s codes are `cmp`'s: different at 1, trouble at 2.
    #[test]
    fn the_diff_codes_are_cmps() {
        assert_eq!((EXIT_DIFFERENT, EXIT_TROUBLE), (1, 2));
    }

    /// The scene verbs' codes are documented numbers, clear of the two
    /// above: a history refused at 3, a scene locked at 4, and refusals from
    /// 10 plus their code.
    #[test]
    fn the_scene_codes_sit_clear_of_the_contract() {
        assert_eq!((EXIT_HISTORY, EXIT_LOCKED, REFUSED_BASE), (3, 4, 10));
    }
}
