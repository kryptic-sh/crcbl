//! The migration chain: a save body at an older format version, rewritten one
//! version at a time until it is at [`SAVE_FORMAT_VERSION`].
//!
//! Each step is a pure function over the body — everything before the checksum
//! — from one version to the next, and it writes the version it migrates to.
//! The bytes are the interface because the old layout is exactly what a step
//! must know and nothing else must: once a body has been through the chain, the
//! one reader for the current format reads it, and no reader for an old layout
//! is kept alive beside it.
//!
//! **A step adds no trust.** The checksum is verified against the bytes on disk
//! before the chain runs, and the current reader holds the migrated body to
//! every bound it holds a freshly written one to. A step only has to refuse
//! what it cannot rewrite — a header too short for the fields it moves — and
//! name why.
//!
//! **Adding a version** is adding a step to [`STEPS`] and bumping
//! [`SAVE_FORMAT_VERSION`]; the assertion beside [`STEPS`] fails the build
//! until the two agree.

use std::borrow::Cow;

use super::{
    FIXED_HEADER_SIZE, FormatError, NO_ENGINE_VERSION, NO_SCENE, SAVE_FORMAT_VERSION, VERSION_AT,
    version_of,
};

/// The oldest format version this build reads.
///
/// Version 1's checksum was a `DefaultHasher` digest, which a later Rust
/// release does not reproduce, so a version-1 file cannot be told from a
/// corrupt one — there is nothing a step could verify, and it is refused.
pub(super) const OLDEST_MIGRATABLE: u16 = 2;

/// One step of the chain: the body of a save at one format version, rewritten
/// as the body of the same save at the next, or why it cannot be.
type Step = fn(&[u8]) -> Result<Vec<u8>, &'static str>;

/// The chain, oldest first: the step at index `i` migrates version
/// [`OLDEST_MIGRATABLE`]` + i` to the version after it.
const STEPS: &[Step] = &[v2_to_v3];

// Every version from the oldest migratable to the current one has its step.
const _: () = assert!(OLDEST_MIGRATABLE as usize + STEPS.len() == SAVE_FORMAT_VERSION as usize);

/// `body`, written at `version`, as a body at [`SAVE_FORMAT_VERSION`] —
/// borrowed unchanged when it already is one.
///
/// # Errors
///
/// [`FormatError::Newer`] for a version past the current one,
/// [`FormatError::Unmigratable`] for one before [`OLDEST_MIGRATABLE`], and
/// [`FormatError::Migration`] for a step that refused its input or did not
/// write the version it migrates to.
pub(super) fn to_current(version: u16, body: &[u8]) -> Result<Cow<'_, [u8]>, FormatError> {
    if version > SAVE_FORMAT_VERSION {
        return Err(FormatError::Newer {
            found: version,
            current: SAVE_FORMAT_VERSION,
        });
    }
    if version < OLDEST_MIGRATABLE {
        return Err(FormatError::Unmigratable {
            found: version,
            oldest: OLDEST_MIGRATABLE,
        });
    }

    run(
        &STEPS[usize::from(version - OLDEST_MIGRATABLE)..],
        version,
        body,
    )
}

/// `body`, at `version`, through each of `steps` in turn — the chain from
/// `version` on, which the tests can hand a step of their own.
fn run<'a>(steps: &[Step], version: u16, body: &'a [u8]) -> Result<Cow<'a, [u8]>, FormatError> {
    let mut body = Cow::Borrowed(body);
    for (from, step) in (version..).zip(steps) {
        let to = from + 1;
        let next = step(&body).map_err(|reason| FormatError::Migration { from, to, reason })?;
        if version_of(&next) != Some(to) {
            return Err(FormatError::Migration {
                from,
                to,
                reason: "the step did not write the version it migrates to",
            });
        }
        body = Cow::Owned(next);
    }
    Ok(body)
}

/// Version 3 put the engine version and the optional scene reference between
/// the playtime and the sector count. A version-2 save recorded neither, so it
/// gains an empty engine version — "not recorded" — and the no-scene marker,
/// and everything after the playtime moves along behind them unchanged.
fn v2_to_v3(body: &[u8]) -> Result<Vec<u8>, &'static str> {
    /// Version 2's header: the fixed fields, then the `u32` sector count.
    const V2_HEADER_SIZE: usize = FIXED_HEADER_SIZE + 4;

    if body.len() < V2_HEADER_SIZE {
        return Err("the body is shorter than a version-2 header");
    }
    let (fixed, rest) = body.split_at(FIXED_HEADER_SIZE);
    let mut out = Vec::with_capacity(body.len() + 2);
    out.extend_from_slice(fixed);
    out[VERSION_AT..VERSION_AT + 2].copy_from_slice(&3u16.to_le_bytes());
    out.push(NO_ENGINE_VERSION);
    out.push(NO_SCENE);
    out.extend_from_slice(rest);
    Ok(out)
}

#[cfg(test)]
mod tests;
