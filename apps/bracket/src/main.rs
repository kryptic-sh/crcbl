//! Bracket's native front end: the synthetic population driver.
//!
//! ```text
//! bracket sim [--seed N] [--players N] [--ticks N]
//! ```
//!
//! Runs a population through the queue, the match stub and the rating update,
//! and reports what the matchmaker traded away. Deterministic from the seed, so
//! a run is reproducible and a CI soak is a fixed expectation rather than a
//! sample.

use crcbl_bracket::sim::Sim;

/// The usage text, printed for `--help` and for anything unrecognised.
const USAGE: &str = "\
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

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(report) => {
            print!("{report}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("bracket: {message}\n\n{USAGE}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Parse `args`, run the simulation and render its report.
///
/// Split out from `main` so the tests below drive the same path the binary
/// does, argument parsing included.
fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(USAGE.to_string());
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
