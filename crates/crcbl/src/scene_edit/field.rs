//! One field on the clipboard: `docs/plan/08-editor.md` feature 8's field
//! half.
//!
//! A field is one **leaf** of an entity's component — the value one inspector
//! widget edits, named by the [`crate::reflect`] path an
//! [`EditCommand::SetProperty`] carries. Copying it gives its value as the
//! text the scene's chunk file spells it with, and pasting reads text back the
//! way the scene's loader reads that kind of value — so a number copied from a
//! row pastes into another row, into a text editor and back into a chunk file
//! unchanged.
//!
//! # The serde path
//!
//! A chunk row is the component's own `serde` derive over ron, and a leaf of
//! it is one of ron's primitives: a float is `f64`'s ron text, a string is a
//! quoted ron string, and so on. [`text_of`] writes a leaf through
//! [`crate::ron::to_string`] of the [`Value`]'s own Rust type, and
//! [`value_of`] reads one through [`crate::ron::from_str`] of the type the leaf
//! currently holds — the `Deserialize` the loader calls for that leaf. A float
//! prints through Rust's shortest round trip, so a copy pastes back to the same
//! bits.
//!
//! **A paste is validated twice and applied once.** The text must parse as the
//! leaf's kind, and the component's leaf must then accept the value —
//! [`crate::reflect::Reflect::set`] refuses a number that does not fit a
//! narrower leaf — and only then is it one [`EditCommand::SetProperty`]:
//! undoable, refused in play mode, and never a half-written field.
//!
//! # Only leaves
//!
//! A composite field — a whole `position` — has no text of its own here: its
//! RON shape is the component type's `serde` derive (a `[f64; 3]` is a tuple,
//! a `Vec` a list), which the reflected value does not carry, and a text built
//! from the reflected shape would be a second serializer that could disagree
//! with the first. Each axis copies and pastes on its own.

use crate::reflect::{Kind, Reflect, Value};
use crate::ron;
use crate::scene::scn::SceneEntityId;

use super::{Document, EditError};
use crate::scene::edit::EditCommand;

impl Document {
    /// The text of the leaf `path` names inside `id`'s component in `system`,
    /// as the scene's chunk file spells it — see the module docs.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read): [`EditError::NoEntity`] for an id this document
    /// does not hold, [`EditError::NotAttached`] for a system that does not
    /// hold it, [`EditError::Path`] for a path that names nothing or stops
    /// short of a leaf.
    pub fn copy_field(
        &mut self,
        id: SceneEntityId,
        system: &str,
        path: &str,
    ) -> Result<String, EditError> {
        Ok(text_of(&self.read(id, system, path)?))
    }

    /// Writes the value `text` spells into the leaf `path` names inside `id`'s
    /// component in `system`, as one [`EditCommand::SetProperty`].
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode; [`EditError::NoEntity`],
    /// [`EditError::NotAttached`] or [`EditError::Path`] as
    /// [`read`](Self::read);
    /// [`EditError::FieldPaste`] for text that is not a value of the leaf's
    /// kind; and [`EditError::Path`] again for a value the leaf refuses. The
    /// field is unchanged and nothing is recorded in every case.
    pub fn paste_field(
        &mut self,
        id: SceneEntityId,
        system: &str,
        path: &str,
        text: &str,
    ) -> Result<(), EditError> {
        self.paste_fields(id, system, &[(path, text)])
    }

    /// [`paste_field`](Self::paste_field) for each `(path, text)` of `fields`,
    /// as **one** entry: an [`EditCommand::Batch`] of the writes in order, or
    /// the one [`EditCommand::SetProperty`] for one field. Every text is read
    /// before anything is written, so one that is not a value of its field
    /// writes none of them. No fields, no entry.
    ///
    /// # Errors
    ///
    /// As [`paste_field`](Self::paste_field), for the first field refused.
    pub fn paste_fields(
        &mut self,
        id: SceneEntityId,
        system: &str,
        fields: &[(&str, &str)],
    ) -> Result<(), EditError> {
        self.refuse_in_play()?;
        if fields.is_empty() {
            return Ok(());
        }
        let sets = self.parsed_sets(id, system, fields)?;
        self.apply(EditCommand::one_or_batch(sets))
    }

    /// The write of each `(path, text)` of `fields` into `id`'s component in
    /// `system`, each text read as the kind of value its leaf holds now —
    /// what a paste applies.
    pub(super) fn parsed_sets(
        &mut self,
        id: SceneEntityId,
        system: &str,
        fields: &[(&str, &str)],
    ) -> Result<Vec<EditCommand>, EditError> {
        let mut sets = Vec::with_capacity(fields.len());
        for &(path, text) in fields {
            let current = self.read(id, system, path)?;
            let value = value_of(&current, text).map_err(|error| EditError::FieldPaste {
                path: path.to_owned(),
                message: error.code.to_string(),
            })?;
            sets.push(EditCommand::SetProperty {
                entity: id,
                system: system.to_owned(),
                path: path.to_owned(),
                value,
            });
        }
        Ok(sets)
    }

    /// Every field of `id`'s component in `system`, in the order the
    /// component declares them, as `(path, text)`: each leaf's text as
    /// [`copy_field`](Self::copy_field) gives it, and each enum by the name
    /// of the variant it holds, before that variant's own fields. What a
    /// reader of the whole component — `crcbl scene query` — prints.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, and
    /// [`EditError::NotAttached`] for a system that does not hold it.
    pub fn field_texts(
        &mut self,
        id: SceneEntityId,
        system: &str,
    ) -> Result<Vec<(String, String)>, EditError> {
        let component: &dyn Reflect = self.component_of(id, system)?;
        let mut texts = Vec::new();
        texts_of(component, "", &mut texts);
        Ok(texts)
    }
}

/// Pushes `value`'s fields, the value itself at `path` included, onto `texts`
/// — see [`Document::field_texts`].
fn texts_of(value: &dyn Reflect, path: &str, texts: &mut Vec<(String, String)>) {
    let child = |name: &str| {
        if path.is_empty() {
            name.to_owned()
        } else {
            format!("{path}.{name}")
        }
    };
    match value.kind() {
        Kind::Leaf(_) => {
            if let Some(leaf) = value.get() {
                texts.push((path.to_owned(), text_of(&leaf)));
            }
        }
        Kind::List { len } => {
            for index in 0..len {
                if let Some(element) = value.field(index) {
                    texts_of(element, &child(&index.to_string()), texts);
                }
            }
        }
        kind @ (Kind::Struct | Kind::Enum) => {
            if kind == Kind::Enum {
                texts.push((
                    path.to_owned(),
                    value.variant().unwrap_or_default().to_owned(),
                ));
            }
            for (index, field) in value.fields().iter().enumerate() {
                if let Some(inner) = value.field(index) {
                    texts_of(inner, &child(field.name), texts);
                }
            }
        }
    }
}

/// `value` as ron text — the text of a leaf in a chunk row, and what
/// [`Document::copy_field`] answers.
#[must_use]
pub fn text_of(value: &Value) -> String {
    let written = match value {
        Value::Bool(value) => ron::to_string(value),
        Value::Int(value) => ron::to_string(value),
        Value::UInt(value) => ron::to_string(value),
        Value::Float(value) => ron::to_string(value),
        Value::Text(value) => ron::to_string(value),
    };
    written.expect("ron writes every primitive")
}

/// The [`Value`] of `like`'s kind that `text` spells, read as ron.
pub(super) fn value_of(like: &Value, text: &str) -> Result<Value, ron::error::SpannedError> {
    Ok(match like {
        Value::Bool(_) => Value::Bool(ron::from_str(text)?),
        Value::Int(_) => Value::Int(ron::from_str(text)?),
        Value::UInt(_) => Value::UInt(ron::from_str(text)?),
        Value::Float(_) => Value::Float(ron::from_str(text)?),
        Value::Text(_) => Value::Text(ron::from_str(text)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every kind of leaf writes as ron and reads back as itself**, and text
    /// of another kind is refused rather than read as something.
    #[test]
    fn every_kind_of_leaf_round_trips_through_its_ron_text() {
        for (value, text) in [
            (Value::Bool(true), "true"),
            (Value::Int(-3), "-3"),
            (Value::UInt(7), "7"),
            (Value::Float(0.1), "0.1"),
            // Every digit an `f64` needs, and none an `f32` would keep.
            (Value::Float(0.1 + 0.2), "0.30000000000000004"),
            (Value::Float(3.0), "3.0"),
            (Value::Text("a \"b\"".to_owned()), r#""a \"b\"""#),
        ] {
            assert_eq!(text_of(&value), text);
            assert_eq!(value_of(&value, text).expect("its own text"), value);
        }
        assert!(value_of(&Value::Bool(false), "1").is_err());
        assert!(value_of(&Value::UInt(0), "-1").is_err());
        assert!(value_of(&Value::Int(0), "1.5").is_err());
        assert!(value_of(&Value::Text(String::new()), "bare").is_err());
    }
}
