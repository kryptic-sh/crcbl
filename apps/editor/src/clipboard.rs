//! Entities on the system clipboard: the text a copy offers, and the read a
//! paste waits for.
//!
//! `docs/plan/08-editor.md`'s feature 8. A copy is the selected entity's
//! system and row — the same RON [`crate::Document::duplicate`] spawns from —
//! offered as both [`MimeType::CrcblRon`] and plain text, so it pastes into a
//! second editor, into a text editor or a chat, and back. A paste spawns every
//! entity the text names under ids the document hands out fresh, so a clipping
//! never collides with what is already in the scene.
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
//! # Why a paste reads plain text
//!
//! [`Paste::ask`] requests [`MimeType::TextUtf8`], not the RON mime: the copy
//! offers the same bytes under both, and a clipping that went through a chat
//! comes back as text alone.

use crcbl::ron;
use crcbl::serde::{Deserialize, Serialize};
use crcbl::shell::{
    ClipboardContent, ClipboardRequestId, MimeType, Shell, ShellError, ShellEvent, WindowId,
};

/// The clipboard text: every entity copied, in the order it was copied.
#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "crcbl::serde", rename = "Entities", deny_unknown_fields)]
struct Clipping {
    entities: Vec<Clipped>,
}

/// One copied entity: the system it goes back into and its row.
#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "crcbl::serde", rename = "Entity", deny_unknown_fields)]
struct Clipped {
    system: String,
    row: String,
}

/// The clipboard text for `entities`, each a `(system, row)` pair.
#[must_use]
pub fn encode(entities: Vec<(String, String)>) -> String {
    let clipping = Clipping {
        entities: entities
            .into_iter()
            .map(|(system, row)| Clipped { system, row })
            .collect(),
    };
    ron::ser::to_string_pretty(
        &clipping,
        ron::ser::PrettyConfig::default().struct_names(true),
    )
    .expect("a list of string pairs always serializes")
}

/// The `(system, row)` pairs `text` names.
///
/// # Errors
///
/// ron's own error, with its position, if the text is not a clipping — which is
/// what a paste of anything else copied from anywhere else is.
pub fn decode(text: &str) -> Result<Vec<(String, String)>, ron::error::SpannedError> {
    let clipping: Clipping = ron::from_str(text)?;
    Ok(clipping
        .entities
        .into_iter()
        .map(|Clipped { system, row }| (system, row))
        .collect())
}

/// A paste waiting on the clipboard's answer.
///
/// The read is asynchronous on every backend that can do it
/// ([`Shell::clipboard_request`] says why), so a paste asks in one frame and
/// spawns in whichever frame the answer arrives.
#[derive(Debug, Default)]
pub struct Paste {
    /// The read this editor issued and no answer has arrived for. A newer
    /// paste replaces it: the newer press is the one the person is waiting on.
    awaiting: Option<ClipboardRequestId>,
    /// The answer, until [`take`](Self::take) collects it.
    arrived: Option<ClipboardContent>,
}

impl Paste {
    /// Asks `shell` for the clipboard's text.
    ///
    /// # Errors
    ///
    /// The shell's refusal: [`ShellError::Unsupported`] on a backend with no
    /// clipboard.
    pub fn ask<S: Shell + ?Sized>(
        &mut self,
        shell: &mut S,
        window: WindowId,
    ) -> Result<(), ShellError> {
        self.awaiting = Some(shell.clipboard_request(window, MimeType::TextUtf8)?);
        Ok(())
    }

    /// Keeps `event` if it is the answer to this paste's read. Returns whether
    /// it was — another reader's answer, a text field's, is left alone.
    pub fn observe(&mut self, event: &ShellEvent) -> bool {
        let ShellEvent::ClipboardData {
            request, content, ..
        } = event
        else {
            return false;
        };
        if self
            .awaiting
            .take_if(|awaited| awaited == request)
            .is_none()
        {
            return false;
        }
        self.arrived = Some(content.clone());
        true
    }

    /// The answer, once: [`None`] until it arrives and after it was taken.
    pub fn take(&mut self) -> Option<ClipboardContent> {
        self.arrived.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A clipping reads back as the pairs it was written from**, including a
    /// row holding a quoted string.
    #[test]
    fn a_clipping_decodes_to_the_entities_it_encodes() {
        let entities = vec![
            ("blocks".to_owned(), "(position:(1.0,2.0,3.0))".to_owned()),
            (
                "marks".to_owned(),
                r#"(label:"a \"quoted\" name")"#.to_owned(),
            ),
        ];
        let text = encode(entities.clone());
        assert!(text.starts_with("Entities("), "{text}");
        assert_eq!(decode(&text).expect("its own text"), entities);
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
