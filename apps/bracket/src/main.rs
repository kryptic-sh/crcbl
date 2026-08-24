//! bracket — the native front end, and the headless population driver.
//!
//! ```text
//! bracket     [--headless] [--frames N] [--size WxH] [--seed N] [--players N]
//! bracket sim [--seed N] [--players N] [--ticks N]
//! ```
//!
//! **The first word of argv chooses between two front ends.** With no
//! subcommand this file is every other sample's `main`: argv in, exit code out,
//! and the demo itself is the `crcbl_bracket` library this binary links — which
//! is also what the browser's wasm entry point drives.
//!
//! `sim` is the other, and it is why this file has more in it than any other
//! sample's. It runs a population through the queue, the match stub and the
//! rating update with no window and no GPU at all, and prints what the
//! matchmaker traded away: the same measurement the page draws, in a form a CI
//! soak can hold to a fixed expectation. Deterministic from the seed, so a run
//! is reproducible rather than a sample of one.
//!
//! Exit codes for the demo: 0 ran, 1 it failed, 2 bad arguments. `sim` reports a
//! bad argument as 1, which is the code it has always used.

use std::process::ExitCode;

use crcbl_bracket::sim::Sim;

/// The usage text for `sim`, printed for `bracket sim --help` and for anything
/// unrecognised after the subcommand.
///
/// The demo's own is [`crcbl_bracket::USAGE`], which names this subcommand so a
/// reader who typed `bracket --help` can find it.
const SIM_USAGE: &str = "\
bracket — matchmaking, rating and ranked session flow

USAGE:
    bracket sim [OPTIONS]

OPTIONS:
    --seed <N>       Seed the population and every match outcome (default 1)
    --players <N>    How many synthetic players (default 64, minimum 2)
    --ticks <N>      How many matchmaking ticks to run (default 2000)
    -h, --help       Print this
";

/// How many ladder places to print at each end.
const SHOWN: usize = 5;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The subcommand is claimed here, before the demo's parser is ever built,
    // so a word that parser does not recognise is genuinely unknown rather than
    // a command it forgot about.
    if args.first().is_some_and(|arg| arg == "sim") {
        return match run(&args) {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("bracket: {message}\n\n{SIM_USAGE}");
                ExitCode::FAILURE
            }
        };
    }

    crcbl::args::run_front_end(
        "bracket",
        crcbl_bracket::USAGE,
        crcbl_bracket::parse(args.into_iter()),
        crcbl_bracket::run,
        |summary| {
            format!(
                "bracket: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 ({} matches, {:.1} rating error, {} page commands, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.matches,
                summary.error,
                summary.commands,
                summary.exit,
            )
        },
    )
}

/// Parse a `sim` argv, run the population and render its report.
///
/// Split out from `main` so the tests below drive the same path the binary
/// does, argument parsing included — `args` is the whole argv, subcommand and
/// all, and this is where the subcommand is checked rather than assumed.
fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(SIM_USAGE.to_string());
    }
    let Some((command, rest)) = args.split_first() else {
        return Err("no command given".to_string());
    };
    if command != "sim" {
        return Err(format!("unknown command {command:?}"));
    }

    let mut seed = 1u64;
    let mut players = 64usize;
    let mut ticks = 2_000u64;

    let mut index = 0;
    while index < rest.len() {
        let flag = rest[index].as_str();
        let value = rest
            .get(index + 1)
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag {
            "--seed" => {
                seed = value
                    .parse()
                    .map_err(|_| format!("--seed wants a whole number, not {value:?}"))?;
            }
            "--players" => {
                players = value
                    .parse()
                    .map_err(|_| format!("--players wants a whole number, not {value:?}"))?;
                if players < 2 {
                    return Err("--players needs at least two of them".to_string());
                }
            }
            "--ticks" => {
                ticks = value
                    .parse()
                    .map_err(|_| format!("--ticks wants a whole number, not {value:?}"))?;
            }
            other => return Err(format!("unknown option {other:?}")),
        }
        index += 2;
    }

    let mut sim = Sim::new(seed, players);
    let started = sim.mean_rating_error();
    for _ in 0..ticks {
        sim.step();
    }
    Ok(report(&sim, started, seed, ticks))
}

/// Render a finished run.
fn report(sim: &Sim, started: f64, seed: u64, ticks: u64) -> String {
    let mut out = String::new();
    let ladder = sim.ladder();
    out.push_str(&format!(
        "seed {seed}, {} players, {ticks} ticks, {} matches\n\n",
        sim.players().len(),
        sim.matches_played()
    ));
    out.push_str(&format!(
        "rating error   {started:.1} -> {:.1} points from true skill\n",
        sim.mean_rating_error()
    ));
    out.push_str(&format!(
        "match quality  {:.1} points apart on average\n",
        sim.mean_gap()
    ));
    out.push_str(&format!(
        "wait           {:.2} ticks on average\n\n",
        sim.mean_wait()
    ));

    out.push_str("  #  rating   true   error\n");
    for (place, id) in ladder.iter().enumerate() {
        // The head and the tail: the two ends are where a matchmaker runs out
        // of opponents, so they are where a ladder goes wrong first.
        if place >= SHOWN && place + SHOWN < ladder.len() {
            if place == SHOWN {
                out.push_str("  ..\n");
            }
            continue;
        }
        let player = sim.player(*id);
        out.push_str(&format!(
            "{:>3}  {:>6.0}  {:>5.0}  {:>+6.0}\n",
            place + 1,
            player.rating.points(),
            player.skill,
            player.rating.points() - player.skill
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn help_is_printed_rather_than_treated_as_a_command() {
        let report = run(&args(&["--help"])).expect("--help should not be an error");
        assert!(report.contains("USAGE"), "{report}");
    }

    #[test]
    fn a_run_reports_the_population_it_was_asked_for() {
        let report = run(&args(&["sim", "--players", "8", "--ticks", "50"])).expect("should run");
        assert!(report.contains("8 players"), "{report}");
        assert!(report.contains("50 ticks"), "{report}");
        assert!(report.contains("rating error"), "{report}");
    }

    #[test]
    fn the_same_arguments_produce_the_same_report() {
        let once = run(&args(&[
            "sim",
            "--seed",
            "9",
            "--players",
            "16",
            "--ticks",
            "200",
        ]));
        let twice = run(&args(&[
            "sim",
            "--seed",
            "9",
            "--players",
            "16",
            "--ticks",
            "200",
        ]));
        assert_eq!(once, twice);
    }

    #[test]
    fn a_different_seed_produces_a_different_report() {
        let once = run(&args(&[
            "sim",
            "--seed",
            "1",
            "--players",
            "16",
            "--ticks",
            "200",
        ]));
        let twice = run(&args(&[
            "sim",
            "--seed",
            "2",
            "--players",
            "16",
            "--ticks",
            "200",
        ]));
        assert_ne!(once, twice);
    }

    #[test]
    fn bad_arguments_are_refused_rather_than_guessed_at() {
        for bad in [
            vec![],
            args(&["play"]),
            args(&["sim", "--players", "1"]),
            args(&["sim", "--players", "many"]),
            args(&["sim", "--ticks"]),
            args(&["sim", "--nonsense"]),
        ] {
            assert!(run(&bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_long_run_closes_on_the_true_skills() {
        let report = run(&args(&[
            "sim",
            "--seed",
            "3",
            "--players",
            "32",
            "--ticks",
            "2000",
        ]))
        .expect("should run");
        // The report is what a reader judges the run by, so the claim is
        // checked through it rather than around it.
        let line = report
            .lines()
            .find(|line| line.starts_with("rating error"))
            .unwrap_or_else(|| panic!("no rating error line in:\n{report}"));
        let ended: f64 = line
            .rsplit("-> ")
            .next()
            .and_then(|rest| rest.split(' ').next())
            .and_then(|number| number.parse().ok())
            .unwrap_or_else(|| panic!("could not read the error out of {line:?}"));
        assert!(ended < 90.0, "{line}");
    }
}
