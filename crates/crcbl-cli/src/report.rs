//! Two output shapes for one result, and the exit-code contract.
//!
//! `docs/plan/11-cli-headless.md` fixes both halves: "`--json` on every
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

/// Exit code for "the command failed".
pub const EXIT_FAILED: u8 = 1;

/// Exit code for "the invocation was malformed".
pub const EXIT_USAGE: u8 = 2;

/// A command that worked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// What a person reads.
    pub human: String,
    /// Command-specific fields, merged after `ok` and `command`.
    pub json: Vec<(&'static str, Json)>,
}

/// A command that did not work.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    match result {
        Ok(outcome) => {
            if json {
                let mut fields = vec![("ok", Json::Bool(true)), ("command", Json::string(command))];
                fields.extend(outcome.json);
                println!("{}", Json::Object(fields));
            } else {
                println!("{}", outcome.human);
            }
            ExitCode::SUCCESS
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
}
