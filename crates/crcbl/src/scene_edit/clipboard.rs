//! Entities as clipboard text: what a copy offers and a paste reads.
//!
//! `docs/plan/08-editor.md`'s feature 8. A copy is every selected entity's
//! systems and rows — the same RON [`Document::duplicate`] spawns from —
//! offered by the editor as both the engine's RON mime and plain text, so it
//! pastes into a second editor, into a text editor or a chat, and back. A
//! paste spawns every entity the text names under ids the document hands out
//! fresh, so a clipping never collides with what is already in the scene.
//!
//! ```text
//! Entities(
//!     entities: [
//!         Entity(
//!             system: "blocks",
//!             row: "(position:(3.0,1.25,0.0),half_extents:(1.2,1.25,1.5))",
//!         ),
//!     ],
//! )
//! ```
//!
//! **The row stays a string** rather than nesting the component's own RON. A
//! row only reads back through the typed codec of the system it names, so the
//! clipping cannot parse it without knowing that system first — and re-encoding
//! it through an untyped value on the way would round floats the scene file
//! writes exactly.
//!
//! **A named entity carries its name**, as `name: Some("Gate")` after the row,
//! and an unnamed one writes no `name` at all — so an unnamed entity's clipping
//! is the text it was before names existed, and a clipping from then decodes
//! now as an unnamed entity. Where the name lands is
//! [`Document::paste`]'s rule.
//!
//! **An entity in several systems** writes its first system's row as
//! `system` and `row`, as before, and every other one in `others` after them;
//! an entity in one system writes no `others` at all. So a single-system
//! clipping is the text it always was, and one written before an entity could
//! span systems decodes as an entity in one.
//!
//! ```text
//! Entity(
//!     system: "blocks",
//!     row: "(position:(3.0,1.25,0.0),half_extents:(1.2,1.25,1.5))",
//!     others: [
//!         Row(system: "sun", row: "(elevation:0.78, …)"),
//!     ],
//! )
//! ```
//!
//! [`Document::duplicate`]: super::Document::duplicate
//! [`Document::paste`]: super::Document::paste

use crate::ron;
use crate::scene::edit::SystemRow;
use serde::{Deserialize, Serialize};

/// The clipboard text: every entity copied, in the order it was copied.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "Entities", deny_unknown_fields)]
struct Clipping {
    entities: Vec<Clipped>,
}

/// One copied entity: the systems it goes back into, their rows, and its
/// name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "Entity", deny_unknown_fields)]
pub struct Clipped {
    /// The scene system whose chunk the first row belongs to.
    pub system: String,
    /// That system's component, as one chunk row's RON text.
    pub row: String,
    /// Every other system's row, in the order they were copied — empty, and
    /// not written, for an entity in one system. See the module docs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<ClippedRow>,
    /// What the entity was called, if it was named — unchecked text, because a
    /// clipping is whatever the clipboard holds; a paste checks it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Clipped {
    /// The clipping of an entity whose components are `rows`, called `name` —
    /// or [`None`] for no rows, which is no entity a paste could spawn.
    #[must_use]
    pub fn of(rows: Vec<SystemRow>, name: Option<String>) -> Option<Self> {
        let mut rows = rows.into_iter();
        let first = rows.next()?;
        Some(Self {
            system: first.system,
            row: first.row,
            others: rows
                .map(|row| ClippedRow {
                    system: row.system,
                    row: row.row,
                })
                .collect(),
            name,
        })
    }

    /// Every system's row, the first one first: what a paste spawns.
    #[must_use]
    pub fn rows(&self) -> Vec<SystemRow> {
        let first = SystemRow {
            system: self.system.clone(),
            row: self.row.clone(),
        };
        std::iter::once(first)
            .chain(self.others.iter().map(|other| SystemRow {
                system: other.system.clone(),
                row: other.row.clone(),
            }))
            .collect()
    }
}

/// One more system's row of a copied entity: see [`Clipped::others`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "Row", deny_unknown_fields)]
pub struct ClippedRow {
    /// The scene system whose chunk the row belongs to.
    pub system: String,
    /// The component, as one chunk row's RON text.
    pub row: String,
}

/// The clipboard text for `entities`.
#[must_use]
pub fn encode(entities: Vec<Clipped>) -> String {
    ron::ser::to_string_pretty(
        &Clipping { entities },
        ron::ser::PrettyConfig::default().struct_names(true),
    )
    .expect("a list of strings always serializes")
}

/// The entities `text` names.
///
/// # Errors
///
/// ron's own error, with its position, if the text is not a clipping — which is
/// what a paste of anything else copied from anywhere else is.
pub fn decode(text: &str) -> Result<Vec<Clipped>, ron::error::SpannedError> {
    let clipping: Clipping = ron::from_str(text)?;
    Ok(clipping.entities)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clipped(system: &str, row: &str, name: Option<&str>) -> Clipped {
        Clipped {
            system: system.to_owned(),
            row: row.to_owned(),
            others: Vec::new(),
            name: name.map(str::to_owned),
        }
    }

    fn row(system: &str, row: &str) -> SystemRow {
        SystemRow {
            system: system.to_owned(),
            row: row.to_owned(),
        }
    }

    /// **An entity in several systems clips every row and reads back as all of
    /// them, the first first** — and one in a single system writes no `others`,
    /// so its text is what it was before entities could span systems.
    #[test]
    fn a_clipping_carries_every_systems_row() {
        let rows = vec![
            row("blocks", "(position:(1.0,2.0,3.0))"),
            row("sun", "(intensity:2.0)"),
            row("marks", "(label:\"m\")"),
        ];
        let several = Clipped::of(rows.clone(), Some("Gate".to_owned())).expect("three rows");
        let text = encode(vec![several]);
        assert!(text.contains("others:"), "{text}");
        let decoded = decode(&text).expect("its own text");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].rows(), rows);
        assert_eq!(decoded[0].name.as_deref(), Some("Gate"));

        let single = Clipped::of(rows[..1].to_vec(), None).expect("one row");
        assert_eq!(single, clipped("blocks", "(position:(1.0,2.0,3.0))", None));
        assert!(!encode(vec![single]).contains("others"));
        assert_eq!(Clipped::of(Vec::new(), None), None);
    }

    /// **A clipping reads back as the entities it was written from**, including
    /// a row holding a quoted string and a name.
    #[test]
    fn a_clipping_decodes_to_the_entities_it_encodes() {
        let entities = vec![
            clipped("blocks", "(position:(1.0,2.0,3.0))", Some("Gate")),
            clipped("marks", r#"(label:"a \"quoted\" name")"#, None),
        ];
        let text = encode(entities.clone());
        assert!(text.starts_with("Entities("), "{text}");
        assert_eq!(decode(&text).expect("its own text"), entities);
    }

    /// **An unnamed entity's clipping is the text it was before names
    /// existed**, and such a clipping decodes as an unnamed entity — so a
    /// clipping from an older editor still pastes.
    #[test]
    fn an_unnamed_clipping_is_the_text_from_before_names() {
        let before = "Entities(\n    entities: [\n        Entity(\n            system: \
                      \"blocks\",\n            row: \"(position:(1.0,2.0,3.0))\",\n        ),\n    \
                      ],\n)";
        let entities = vec![clipped("blocks", "(position:(1.0,2.0,3.0))", None)];
        // ron's default newline is the host's, so the clipping is compared
        // with Windows' line ends taken out.
        assert_eq!(encode(entities.clone()).replace("\r\n", "\n"), before);
        assert_eq!(decode(before).expect("an older clipping"), entities);
    }

    /// Text that is not a clipping — the ordinary thing to find on a clipboard
    /// — is refused rather than read as nothing.
    #[test]
    fn text_that_is_not_a_clipping_is_refused() {
        assert!(decode("hello").is_err());
        assert!(decode("Entities(entities: [], extra: 1)").is_err());
        assert_eq!(decode("Entities(entities: [])").expect("empty"), []);
    }
}
