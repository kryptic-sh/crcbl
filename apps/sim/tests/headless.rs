//! `crcbl-sim` as CI runs it: the real binary, no display, deterministic — its
//! arguments, its exit codes and its one-line output contract.
//!
//! This file tests the *deliverable* rather than a function inside it, for the
//! reason `apps/breakout/tests/headless.rs` and `apps/sandbox/tests/headless.rs`
//! exist under the same name: the compiled binary is what
//! `.github/workflows/ci.yml` and a developer both actually invoke, and only a
//! separate target can spawn it.
//!
//! The determinism harness had no tests at all, which is how `--tick-rate 0`
//! survived: it parsed cleanly and then divided by zero computing the tick
//! period, aborting with exit 101 rather than the documented exit 2.

use std::process::{Command, Output};

fn sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crcbl-sim"))
        .args(args)
        .output()
        .expect("the sim binary runs")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("not killed by a signal")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `hash:<hex> ticks:<n> final_tick:<n>` — the whole output contract.
fn parts(line: &str) -> (String, u64, u64) {
    let mut hash = None;
    let mut ticks = None;
    let mut final_tick = None;
    for field in line.split_whitespace() {
        match field.split_once(':') {
            Some(("hash", v)) => hash = Some(v.to_owned()),
            Some(("ticks", v)) => ticks = v.parse().ok(),
            Some(("final_tick", v)) => final_tick = v.parse().ok(),
            _ => {}
        }
    }
    (
        hash.unwrap_or_else(|| panic!("no hash in {line:?}")),
        ticks.unwrap_or_else(|| panic!("no ticks in {line:?}")),
        final_tick.unwrap_or_else(|| panic!("no final_tick in {line:?}")),
    )
}

#[test]
fn a_default_run_exits_zero_and_prints_its_contract() {
    let output = sim(&[]);
    assert_eq!(
        code(&output),
        0,
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (hash, ticks, final_tick) = parts(&stdout(&output));
    assert_eq!(hash.len(), 16, "hash is 16 hex digits: {hash}");
    assert_eq!(ticks, 1_000);
    assert_eq!(final_tick, 1_000);
}

/// **The two numbers must agree.** The loop used to break out of the tick drain
/// the moment the budget was met, leaving whole ticks in the accumulator that
/// the clock had already counted — so a determinism harness could report a tick
/// count and a final tick that differed for a reason unrelated to determinism.
#[test]
fn the_tick_count_and_the_final_tick_always_agree() {
    for n in ["1", "7", "60", "997"] {
        let output = sim(&["--ticks", n]);
        assert_eq!(code(&output), 0);
        let (_, ticks, final_tick) = parts(&stdout(&output));
        assert_eq!(ticks, n.parse::<u64>().unwrap(), "--ticks {n}");
        assert_eq!(final_tick, ticks, "--ticks {n}");
    }
}

/// Same input, same hash — the property the harness exists to assert.
#[test]
fn the_same_input_produces_the_same_hash() {
    let first = sim(&["--ticks", "500", "--seed", "42"]);
    let second = sim(&["--ticks", "500", "--seed", "42"]);
    assert_eq!(stdout(&first), stdout(&second));

    let other_seed = sim(&["--ticks", "500", "--seed", "43"]);
    assert_ne!(
        parts(&stdout(&first)).0,
        parts(&stdout(&other_seed)).0,
        "a different seed must build a different world",
    );

    let other_length = sim(&["--ticks", "501", "--seed", "42"]);
    assert_ne!(
        parts(&stdout(&first)).0,
        parts(&stdout(&other_length)).0,
        "the hash must depend on how long the world ran",
    );
}

/// The tick rate changes the clock, not the number of ticks: the harness
/// advances by exactly one period per iteration at any rate.
#[test]
fn the_tick_rate_does_not_change_the_tick_count() {
    for rate in ["1", "30", "60", "240"] {
        let output = sim(&["--ticks", "100", "--tick-rate", rate]);
        assert_eq!(code(&output), 0, "--tick-rate {rate}");
        let (_, ticks, final_tick) = parts(&stdout(&output));
        assert_eq!((ticks, final_tick), (100, 100), "--tick-rate {rate}");
    }
}

/// Bad invocations exit 2, the way the CLI, the sandbox and breakout all do.
/// `--tick-rate 0` in particular used to abort with exit 101 on a division by
/// zero, and `1_000_000_001` truncates its period to zero nanoseconds.
#[test]
fn a_bad_invocation_exits_two() {
    assert_eq!(code(&sim(&["--tick-rate", "0"])), 2);
    assert_eq!(code(&sim(&["--tick-rate", "1000000001"])), 2);
    assert_eq!(code(&sim(&["--tick-rate", "-1"])), 2);
    assert_eq!(code(&sim(&["--tick-rate", "banana"])), 2);
    assert_eq!(code(&sim(&["--tick-rate"])), 2);
    assert_eq!(code(&sim(&["--ticks", "banana"])), 2);
    assert_eq!(code(&sim(&["--seed", "banana"])), 2);
    assert_eq!(code(&sim(&["--nonsense"])), 2);
}

#[test]
fn help_exits_zero_and_documents_the_contract() {
    let help = sim(&["--help"]);
    assert_eq!(code(&help), 0);
    let text = stdout(&help);
    assert!(text.contains("--tick-rate"), "{text}");
    assert!(text.contains("hash:<hex>"), "{text}");
}

/// A zero-tick run is a legal degenerate case, not a crash.
#[test]
fn zero_ticks_is_a_run_of_length_zero() {
    let output = sim(&["--ticks", "0"]);
    assert_eq!(code(&output), 0);
    let (_, ticks, final_tick) = parts(&stdout(&output));
    assert_eq!((ticks, final_tick), (0, 0));
}
