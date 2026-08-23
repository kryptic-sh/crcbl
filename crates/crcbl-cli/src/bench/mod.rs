//! `crcbl bench` — one fixed scenario, warmed up, timed, and reported as a
//! distribution.
//!
//! `docs/plan/40-profiling.md` schedules "`crcbl bench` with fixed scenarios,
//! warm-up, percentiles, JSON output" and notes against it that "the job system
//! is the first thing that needs proving". This is the subcommand and its two
//! scenarios: `jobs`, which times [`crcbl::jobs::Pool`] in isolation — see
//! [`jobs`] — and `phys`, which times `crcbl-phys`'s broadphase on one
//! thread — see [`phys`]. Both are headless and neither opens a device.
//!
//! # Human output by default, `--json` on request
//!
//! The plan says "output is JSON by default". This subcommand does the
//! opposite, and that is a decision rather than an oversight: [`crate`]'s own
//! rule table says "`--json` on every subcommand, human output otherwise", and
//! [`crate::report::emit`] is what implements it for every other subcommand.
//! The global contract is the older and the wider rule, and a subcommand that
//! inverted it would be the one surprise in the tool — a script that reads
//! `crcbl lod` and `crcbl bench` the same way should not have to know which of
//! the two flipped the default. Nothing is lost: `--json` is one flag away, and
//! what it prints is the shape a stored baseline will be.
//!
//! # No mean, and the percentiles are refused when they would be a lie
//!
//! The plan's decision, in its own words: "a benchmark reports p50, p95, p99 and
//! max. Frame time is a tail problem — a mean hides exactly the stutter a player
//! notices." So there is no mean here and none is computed. Below
//! [`MIN_PERCENTILE_SAMPLES`] iterations a nearest-rank p95 *is* the maximum, so
//! the run reports its maximum and says why the percentiles are missing rather
//! than printing one number three times under three names.
//!
//! # The environment block is mandatory
//!
//! Also the plan's: "a benchmark pins everything it can and records everything
//! it cannot … a number without those is not comparable to another number." What
//! a scenario can pin, it pins — a fixed seed, fixed sizes, a fixed round
//! count, integer arithmetic throughout. What none of them can, this module
//! reports for all of them: the machine's architecture and OS, and the build
//! profile. A scenario appends what only it must report — `jobs` adds the
//! parallelism the spawner offered and the worker count the pool actually got;
//! `phys` has neither, because it opens no pool.
//!
//! **There is no adapter, backend or driver version**, because nothing here
//! opens a device; inventing those fields so the block resembles the plan's
//! GPU-scenario list would be reporting something that was never measured.
//!
//! **And there is no target triple.** A binary cannot read one: Cargo hands
//! `TARGET` to build scripts and to nothing else, and `std` exposes only
//! [`ARCH`](std::env::consts::ARCH), [`OS`](std::env::consts::OS) and
//! [`FAMILY`](std::env::consts::FAMILY) — no vendor, no environment. Those three
//! are reported under their own names. A triple reassembled from a hand-written
//! `cfg!` chain would be right on the targets somebody remembered and quietly
//! wrong on every other one, which is worse than three honest fields.
//!
//! # What this slice deliberately does not have
//!
//! `--compare <baseline>` and `--trace <path>` are separate rows of the same
//! delivery table and are not started here, so no scenario reads or writes a
//! stored baseline: a run is compared against another run by a person holding
//! two `--json` outputs.
//!
//! The remaining scenarios the plan names live with the samples that own them,
//! and none of those samples has a fixed scenario yet.

use std::fmt::Write as _;
use std::time::Duration;

use crcbl::core::stats::{MIN_PERCENTILE_SAMPLES, percentile_of};

use crate::args::{BenchArgs, BenchScenario};
use crate::json::Json;
use crate::report::{Failure, Outcome};

mod jobs;
mod phys;

/// Runs `crcbl bench`.
///
/// # Errors
///
/// [`Failure`] if the pool cannot be built, if a phase did not run the chunks or
/// the queries it was asked for, or if the workload did not compute what a
/// serial pass over the same seeds computes.
pub fn run(args: &BenchArgs) -> Result<Outcome, Failure> {
    match args.scenario {
        BenchScenario::Jobs => Ok(jobs::report(args, &jobs::measure(args)?)),
        BenchScenario::Phys => Ok(phys::report(args, &phys::measure(args)?)),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The percentiles this reports, in the order they are printed.
///
/// **No mean**, per the plan's decision — and the list is a constant so that a
/// mean cannot be added to one rendering and forgotten in the other.
const PERCENTILES: &[(usize, &str)] = &[(50, "p50"), (95, "p95"), (99, "p99")];

/// One timed phase, as a line a person reads and the fields a script reads.
///
/// `label` opens the human line and is the phase's name — `per call` for the
/// one distribution `jobs` has, and one of [`phys`]'s three otherwise. The
/// fields are the same shape either way, so a consumer reads three phases the
/// way it reads one.
///
/// Below [`MIN_PERCENTILE_SAMPLES`] samples the percentiles are omitted from
/// both renderings and the reason is stated, because a nearest-rank p95 over
/// fewer than that is the maximum wearing a percentile's name.
fn timing(label: &str, sorted: &[u64]) -> (String, Vec<(&'static str, Json)>) {
    let mut fields = vec![
        ("unit", Json::string("ns")),
        ("iterations", Json::Number(sorted.len() as i64)),
        (
            "min_percentile_samples",
            Json::Number(MIN_PERCENTILE_SAMPLES as i64),
        ),
    ];

    let Some(&max) = sorted.last() else {
        return (format!("{label}: nothing was timed"), fields);
    };
    if sorted.len() < MIN_PERCENTILE_SAMPLES {
        fields.push(("max", Json::Number(max as i64)));
        return (
            format!(
                "{label}: max {}, and no percentile — {} samples is below the \
                 {MIN_PERCENTILE_SAMPLES} a nearest-rank p95 needs to be one",
                micros(max),
                sorted.len()
            ),
            fields,
        );
    }

    let mut line = format!("{label}:");
    for &(percent, key) in PERCENTILES {
        // Infallible here: `last()` above answered, so the slice is not empty
        // and every rank in `1..=len` is a sample it holds.
        let value = percentile_of(sorted, percent).expect("a non-empty slice has every rank");
        let _ = write!(line, " {key} {},", micros(value));
        fields.push((key, Json::Number(value as i64)));
    }
    let _ = write!(line, " max {}", micros(max));
    fields.push(("max", Json::Number(max as i64)));
    (line, fields)
}

/// The environment fields every scenario reports, in the order they print.
///
/// Shared because the plan's "a number without those is not comparable to
/// another number" is a rule about every benchmark, not about one of them —
/// and because two copies of a four-field list is where the second scenario
/// quietly stops recording the profile. A scenario with more to pin appends to
/// this; see [`jobs::report`], which adds the pool's counts.
fn base_environment() -> Vec<(&'static str, Json)> {
    vec![
        ("arch", Json::string(std::env::consts::ARCH)),
        ("os", Json::string(std::env::consts::OS)),
        ("family", Json::string(std::env::consts::FAMILY)),
        ("profile", Json::string(profile())),
    ]
}

/// [`base_environment`] as the line a person reads, with no trailing newline.
fn environment_line() -> String {
    format!(
        "environment: {} {} ({}), {}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::env::consts::FAMILY,
        profile(),
    )
}

/// The build profile this binary was compiled at.
///
/// `debug_assertions` is what a binary can read — Cargo hands the profile name
/// to build scripts and to nothing else — and it is the distinction that
/// matters: a checked build and an optimised one differ by an order of
/// magnitude, and a timing that does not say which is not comparable to another.
fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Nanoseconds as the microseconds a scenario's timed sample lands at.
fn micros(nanos: u64) -> String {
    format!("{:.3} µs", nanos as f64 / 1.0e3)
}

/// A duration as whole nanoseconds, saturating rather than wrapping.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `"key":<integer>` out of a rendered field list, or `None` when the
    /// key is absent — which is how the run says it has no percentile.
    fn number(fields: &[(&'static str, Json)], key: &str) -> Option<i64> {
        fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| match value {
                Json::Number(number) => *number,
                other => panic!("`{key}` is not a number: {other}"),
            })
    }

    /// Ascending samples spaced a microsecond apart, so every rank is a
    /// different number and an off-by-one lands on a value this test names.
    fn spread(count: usize) -> Vec<u64> {
        (1..=count as u64).map(|value| value * 1_000).collect()
    }

    /// **The percentiles are the samples at their ranks, and they ascend.**
    ///
    /// Hand-computed rather than taken off a run: twenty samples of 1..=20 µs
    /// put p50 at the tenth, p95 at the nineteenth, and p99 and the max at the
    /// twentieth. The whole line is pinned, so a percentile printed under
    /// another's name — or a mean appearing beside them — fails here.
    #[test]
    fn the_reported_percentiles_are_the_samples_at_their_ranks_and_ascend() {
        let (line, fields) = timing("per call", &spread(MIN_PERCENTILE_SAMPLES));
        assert_eq!(number(&fields, "p50"), Some(10_000));
        assert_eq!(number(&fields, "p95"), Some(19_000));
        assert_eq!(number(&fields, "p99"), Some(20_000));
        assert_eq!(number(&fields, "max"), Some(20_000));
        assert_eq!(
            line,
            "per call: p50 10.000 µs, p95 19.000 µs, p99 20.000 µs, max 20.000 µs"
        );
        assert_eq!(
            number(&fields, "min_percentile_samples"),
            Some(MIN_PERCENTILE_SAMPLES as i64)
        );
    }

    /// **One sample short of the minimum there is no percentile**, in either
    /// rendering, and the line says which threshold it fell under rather than
    /// printing the maximum three times over.
    #[test]
    fn a_short_run_reports_its_maximum_and_says_why_it_has_no_percentile() {
        let sorted = spread(MIN_PERCENTILE_SAMPLES - 1);
        let (line, fields) = timing("per call", &sorted);
        assert_eq!(number(&fields, "max"), Some(19_000));
        for key in PERCENTILES {
            assert_eq!(number(&fields, key.1), None, "{} survived", key.1);
        }
        assert!(line.contains("max 19.000 µs"), "{line}");
        assert!(line.contains("no percentile"), "{line}");
        assert!(
            line.contains(&MIN_PERCENTILE_SAMPLES.to_string()),
            "the reason has to name the threshold: {line}"
        );
    }
}
