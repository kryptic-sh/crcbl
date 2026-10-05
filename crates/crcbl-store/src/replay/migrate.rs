//! The migration chain: a replay's input section at an older format version,
//! rewritten one version at a time until it is at [`REPLAY_FORMAT_VERSION`],
//! as the save container's chain rewrites a save body.
//!
//! The header and the entries are laid out alike in every version, so the
//! input section — what each version after the first changed — is all a step
//! rewrites. A step is a pure function over the section's bytes from one
//! version to the next, and the bytes are the interface because the old layout
//! is exactly what a step must know and nothing else must: once a section has
//! been through the chain, the one reader for the current format reads it, and
//! no reader for an old layout is kept alive beside it. The file on disk keeps
//! its version; only [`FileTransport`](super::FileTransport) reads the
//! migrated bytes.
//!
//! **A step adds no trust.** The current reader holds a migrated section to
//! every rule it holds a freshly written one to, so a step has nothing to
//! refuse: each only adds what its version added, empty.
//!
//! **Adding a version** is adding a step to [`STEPS`] and bumping
//! [`REPLAY_FORMAT_VERSION`]; the assertion beside [`STEPS`] fails the build
//! until the two agree.

use std::borrow::Cow;

use super::REPLAY_FORMAT_VERSION;
use crate::StorageError;

/// The oldest format version this build reads. Version 1 is the first there
/// was.
const OLDEST_MIGRATABLE: u16 = 1;

/// One step of the chain: the input section of a replay at one format
/// version, rewritten as the section of the same replay at the next.
pub(super) type Step = for<'a> fn(Cow<'a, [u8]>) -> Cow<'a, [u8]>;

/// The chain, oldest first: the step at index `i` migrates version
/// [`OLDEST_MIGRATABLE`]` + i` to the version after it.
const STEPS: &[Step] = &[v1_to_v2, v2_to_v3, v3_to_v4];

// Every version from the oldest migratable to the current one has its step.
const _: () = assert!(OLDEST_MIGRATABLE as usize + STEPS.len() == REPLAY_FORMAT_VERSION as usize);

/// The steps that take a section at `version` to the current one — none for
/// the current version itself.
///
/// Asked for before anything after the header is read, so a file of a version
/// this build does not know is refused by its version rather than by whatever
/// its bytes happen to break.
///
/// # Errors
///
/// A version past [`REPLAY_FORMAT_VERSION`] or before the oldest this build
/// reads, by name.
pub(super) fn steps_from(version: u16) -> Result<&'static [Step], StorageError> {
    if !(OLDEST_MIGRATABLE..=REPLAY_FORMAT_VERSION).contains(&version) {
        return Err(StorageError::Other(format!(
            "unsupported replay format version: {version} (this build reads \
             {OLDEST_MIGRATABLE} to {REPLAY_FORMAT_VERSION})"
        )));
    }
    Ok(&STEPS[usize::from(version - OLDEST_MIGRATABLE)..])
}

/// `section` through each of `steps` in turn — borrowed unchanged when no step
/// rewrites it.
pub(super) fn run<'a>(steps: &[Step], section: &'a [u8]) -> Cow<'a, [u8]> {
    steps
        .iter()
        .fold(Cow::Borrowed(section), |section, step| step(section))
}

/// Version 2 added the input section. A version 1 file has none, so it gains
/// an empty one: no sets and no hashes. Its reader never looked past the
/// entries, so whatever follows them is dropped unread, as it always was.
fn v1_to_v2(_after_the_entries: Cow<'_, [u8]>) -> Cow<'_, [u8]> {
    /// A version 2 section with no sets and no hashes: their two counts.
    const EMPTY: &[u8] = &[0; 4 + 4];
    Cow::Borrowed(EMPTY)
}

/// Version 3 ended the section with the peer track. A version 2 section ends
/// after its hashes, so it gains an empty track: its count, zero.
fn v2_to_v3(section: Cow<'_, [u8]>) -> Cow<'_, [u8]> {
    let mut section = section.into_owned();
    section.extend_from_slice(&0u32.to_le_bytes());
    Cow::Owned(section)
}

/// Version 4 gave a join that names its player a roster kind of its own, and
/// left every other layout as it was. A version 3 join is the kind that names
/// nobody, which version 4 still reads as a join whose player was not
/// recorded, so a version 3 section is a version 4 section unchanged.
fn v3_to_v4(section: Cow<'_, [u8]>) -> Cow<'_, [u8]> {
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_readable_version_reaches_the_current_one_and_no_other_is_read() {
        for version in OLDEST_MIGRATABLE..=REPLAY_FORMAT_VERSION {
            let steps = steps_from(version).expect("a readable version");
            assert_eq!(
                usize::from(version) + steps.len(),
                usize::from(REPLAY_FORMAT_VERSION),
                "version {version}"
            );
        }
        for version in [0, REPLAY_FORMAT_VERSION + 1, u16::MAX] {
            let refused = steps_from(version).expect_err("not a readable version");
            assert!(
                refused
                    .to_string()
                    .contains("unsupported replay format version"),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_current_version_is_read_as_it_lies() {
        let section = [1, 2, 3];
        let read = run(steps_from(REPLAY_FORMAT_VERSION).unwrap(), &section);
        assert!(matches!(read, Cow::Borrowed(bytes) if bytes == section));
    }

    /// Each step adds what its version added, empty, and leaves what was
    /// there before it where it was.
    #[test]
    fn each_step_adds_its_versions_part_empty() {
        assert_eq!(*v1_to_v2(Cow::Borrowed(&[0xFF])), [0; 8]);
        assert_eq!(*v2_to_v3(Cow::Borrowed(&[7, 8])), [7, 8, 0, 0, 0, 0]);
        assert_eq!(*v3_to_v4(Cow::Borrowed(&[7, 8])), [7, 8]);
    }
}
