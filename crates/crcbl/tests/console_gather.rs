//! The engine's gather, held to the workspace's manifests.
//!
//! Plan decision 2's *other* guard, and the one
//! `crates/crcbl-core/tests/console_table.rs` cannot make: that test keeps a
//! crate's table in step with its own source, and says nothing about whether the
//! engine gathers that table at all. A crate could declare a command, list it
//! correctly, pass its own guard, and be reachable from no console in the
//! workspace.
//!
//! So this reads the manifests. Every crate that depends on `crcbl-console`
//! either **is** that crate or is named in
//! [`crcbl::debug_console::engine_tables`], and a new one that is forgotten is a
//! red test here.
//!
//! **It matches text, not TOML**, for the reason
//! `crcbl_console::guard::names_in` does: a dependency line is
//! `crcbl-console = …` at the start of a line in every manifest in this
//! workspace, and a parser would be a dev-dependency bought for one scan. A
//! manifest that spelled the dependency some other way — renamed, or under a
//! target table on one line — would go unseen, which is stated here rather than
//! guarded against.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The crate this whole mechanism belongs to, which gathers its own built-ins
/// through `Registry::gather` and so is never named in the engine's list.
const REGISTRY_CRATE: &str = "crcbl-console";

/// The workspace root, from this test's own working directory.
///
/// An integration test runs at its crate's root, which is `crates/crcbl`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/crcbl sits two levels below the workspace root")
        .to_path_buf()
}

/// Every crate under `dir` whose manifest names `crcbl-console` as a
/// dependency.
fn dependants(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let entries = fs::read_dir(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    for entry in entries {
        let manifest = entry
            .expect("a readable directory entry")
            .path()
            .join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        if !text.lines().any(|line| {
            line.trim_start()
                .starts_with(&format!("{REGISTRY_CRATE} ="))
        }) {
            continue;
        }
        let name = text
            .lines()
            .find_map(|line| line.strip_prefix("name = "))
            .map(|name| name.trim().trim_matches('"').to_owned())
            .unwrap_or_else(|| panic!("{} names no package", manifest.display()));
        found.push(name);
    }
    found
}

/// **Every crate that depends on `crcbl-console` reaches the engine's gather.**
#[test]
fn every_crate_that_owns_console_entries_is_gathered_by_the_engine() {
    let root = workspace_root();
    let mut owners: Vec<String> = dependants(&root.join("crates"));
    owners.extend(dependants(&root.join("apps")));
    owners.retain(|name| name != REGISTRY_CRATE);
    owners.sort();
    owners.dedup();
    // A scan that matched no manifest would pass over an empty gather forever.
    assert!(
        owners.len() >= 2,
        "the manifest scan found {owners:?}, which is too few to be the workspace's answer",
    );

    let gathered: BTreeSet<&str> = crcbl::debug_console::engine_tables()
        .into_iter()
        .map(|(crate_name, _)| crate_name)
        .collect();
    for owner in &owners {
        assert!(
            gathered.contains(owner.as_str()),
            "`{owner}` depends on {REGISTRY_CRATE} and is not in the engine's gather: {gathered:?}",
        );
    }
}

/// **The gather itself resolves**, which is the failure a duplicate name would
/// produce at `Loop::new` and nowhere else.
///
/// A run's own registry carries the game's table as well; this is the engine's
/// half, which is the half every host shares.
#[test]
fn the_engines_own_tables_gather_without_a_collision() {
    let tables: Vec<crcbl_console::Table> = crcbl::debug_console::engine_tables()
        .into_iter()
        .map(|(_, table)| table)
        .collect();
    let registry = crcbl_console::Registry::gather(&tables)
        .expect("no two of the engine's own entries claim one name");

    // Named rather than counted: one from each table, so a gather that dropped
    // a table would fail here rather than pass on a number that moved with it.
    for name in ["help", "log", "quit", "antialiasing"] {
        assert!(
            registry.lookup(name).is_some(),
            "`{name}` is not in the engine's registry",
        );
    }
}
