//! `names.ron`: what a person calls an entity, beside the id the file calls it.
//!
//! **A chunk of its own rather than a field on every row.** A row is a system's
//! component and nothing else — [`super::SystemChunk::row`] hands it out and
//! `attach_row` takes it back — so a name stored in it would be a field every
//! registered component had to carry, and a component type cannot know which
//! entity it is attached to. Keyed by [`SceneEntityId`], the names are about
//! the entity rather than about any one system's data, which is what they will
//! still be when one entity spans several systems.
//!
//! **Optional, and absent when empty.** A scene with no named entity writes no
//! `names.ron` and no `names` entry in its header, so every scene written
//! before names existed is byte-identical when it is saved again. The header
//! declares the file rather than the loader looking for it under its fixed
//! name: a browser build reads a [`crcbl_assets::MemorySource`] seeded file by
//! file, and a names file left out of the seeding would otherwise load as a
//! scene whose names had silently vanished — declared, it is a missing key
//! that names itself. It also spares every load of an unnamed scene a read
//! that fails, which over a network source is a round trip.
//!
//! ```text
//! Names(
//!     names: [
//!         (0, "Gate"),
//!         (3, "Spawner"),
//!     ],
//! )
//! ```
//!
//! A list of pairs in id order rather than a RON map, so that a file naming
//! one id twice is a refusal rather than the later entry silently winning.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{IdMap, SceneEntityId, ScnError};

/// The longest name an entity may have, in characters.
///
/// A name is a label in an outliner row and a word in a status line; past this
/// it is a paragraph, and the row cuts it off anyway.
pub const MAX_NAME_CHARS: usize = 64;

/// What a person calls an entity: non-empty, at most [`MAX_NAME_CHARS`]
/// characters, no control characters, and no surrounding whitespace.
///
/// The only way to build one is [`EntityName::new`], so a scene holding one
/// holds a name its own file can read back.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityName(String);

impl EntityName {
    /// `text` with its surrounding whitespace trimmed, if what is left is a
    /// name.
    ///
    /// Trimmed rather than refused, because a name typed into a field picks up
    /// a stray space at either end and nobody means one there.
    ///
    /// # Errors
    ///
    /// [`NameError`] saying which rule `text` breaks.
    pub fn new(text: &str) -> Result<Self, NameError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(NameError::Empty);
        }
        let chars = text.chars().count();
        if chars > MAX_NAME_CHARS {
            return Err(NameError::TooLong { chars });
        }
        if let Some(found) = text.chars().find(|c| c.is_control()) {
            return Err(NameError::Control(found));
        }
        Ok(Self(text.to_owned()))
    }

    /// The name's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a piece of text is not an [`EntityName`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// Nothing is left once the surrounding whitespace is trimmed.
    #[error("a name cannot be empty")]
    Empty,
    /// Longer than [`MAX_NAME_CHARS`].
    #[error("a name is at most {MAX_NAME_CHARS} characters, and this one is {chars}")]
    TooLong {
        /// How many characters it has, trimmed.
        chars: usize,
    },
    /// It holds a control character — a line break, a tab, an escape.
    #[error("a name cannot hold the control character {0:?}")]
    Control(char),
}

/// The file's shape. Private, as [`super::SceneFile`] is: the runtime form is a
/// map of [`EntityName`]s, and this is what it is written as.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Names", deny_unknown_fields)]
struct NamesFile {
    names: Vec<(SceneEntityId, String)>,
}

/// The names `text` holds, each checked against `ids` — the scene's entities,
/// already loaded — and against the rules [`EntityName::new`] holds.
///
/// # Errors
///
/// [`ScnError::Parse`] if the text is not a names file,
/// [`ScnError::DuplicateId`] for an id named twice, [`ScnError::NameOfNoEntity`]
/// for an id the scene does not hold, [`ScnError::Name`] for text that is not a
/// name, and [`ScnError::NoNames`] for a file that names nothing — the header
/// declared it, and the writer never writes an empty one.
pub(super) fn read(
    key: &str,
    text: &str,
    ids: &IdMap,
) -> Result<BTreeMap<SceneEntityId, EntityName>, ScnError> {
    let file: NamesFile = ron::from_str(text).map_err(|error| ScnError::parse(key, &error))?;
    if file.names.is_empty() {
        return Err(ScnError::NoNames {
            key: key.to_owned(),
        });
    }
    let mut names = BTreeMap::new();
    for (id, text) in file.names {
        if ids.entity(id).is_none() {
            return Err(ScnError::NameOfNoEntity {
                key: key.to_owned(),
                id,
            });
        }
        let name = EntityName::new(&text).map_err(|error| ScnError::Name {
            key: key.to_owned(),
            id,
            error,
        })?;
        if names.insert(id, name).is_some() {
            return Err(ScnError::DuplicateId {
                key: key.to_owned(),
                id,
            });
        }
    }
    Ok(names)
}

/// The text of `names.ron` for `names`, or [`None`] when there are none to
/// write — see the module docs for why the file is then left out.
///
/// # Errors
///
/// [`ScnError::NameOfNoEntity`] for a name whose entity `ids` no longer holds:
/// written, it would be a file the loader refuses.
pub(super) fn write(
    key: &str,
    names: &BTreeMap<SceneEntityId, EntityName>,
    ids: &IdMap,
) -> Result<Option<String>, ScnError> {
    if names.is_empty() {
        return Ok(None);
    }
    if let Some(&id) = names.keys().find(|&&id| ids.entity(id).is_none()) {
        return Err(ScnError::NameOfNoEntity {
            key: key.to_owned(),
            id,
        });
    }
    let file = NamesFile {
        names: names
            .iter()
            .map(|(id, name)| (*id, name.as_str().to_owned()))
            .collect(),
    };
    Ok(Some(super::to_ron(&file)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A name is trimmed, and each rule is refused by name**: empty, too long,
    /// and a control character.
    #[test]
    fn a_name_is_trimmed_and_each_rule_refuses_by_name() {
        assert_eq!(EntityName::new("  Gate ").expect("a name").as_str(), "Gate");
        assert_eq!(EntityName::new(" \t "), Err(NameError::Empty));
        let longest = "n".repeat(MAX_NAME_CHARS);
        assert!(EntityName::new(&longest).is_ok(), "the limit is inclusive");
        assert_eq!(
            EntityName::new(&format!("{longest}n")),
            Err(NameError::TooLong {
                chars: MAX_NAME_CHARS + 1
            }),
        );
        // Characters, not bytes: the limit is what a row shows.
        assert!(EntityName::new(&"é".repeat(MAX_NAME_CHARS)).is_ok());
        assert_eq!(EntityName::new("Gate\nTwo"), Err(NameError::Control('\n')));
        assert_eq!(
            EntityName::new("Gate\u{1b}"),
            Err(NameError::Control('\u{1b}'))
        );
    }
}
