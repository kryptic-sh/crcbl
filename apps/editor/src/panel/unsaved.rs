//! The unsaved bar: a strip under the toolbar, there only while the editor is
//! asking what to do about unsaved edits, saying what will be lost and
//! offering Save, Discard and Cancel.
//!
//! **Asked inside the editor, not by the window system** (decided
//! 2026-10-03): the shell has no dialog, and a strip like the path line is
//! the same thing in every backend. What it guards — a new scene, an open,
//! the window closing — is [`crate::app`]'s; the bar only says what it was
//! told and hands back which button was clicked. Enter, D and Escape answer
//! it from the keyboard ([`crate::keys::unsaved`]).
//!
//! **Modal while it is up.** The panels take no keyboard navigation and no
//! typing, and a press anywhere but the bar does nothing: an edit made while
//! the question stands would change what the answer means. The pointer still
//! hovers, so the bar's own buttons light up.

use crcbl::ui::tree::{NodeKey, Ui};

use crate::keys::Unsaved;

/// The unsaved bar's state between frames.
#[derive(Debug, Default)]
pub(super) struct Bar {
    /// What the bar says, while it is up.
    asking: Option<String>,
    /// The bar as the last frame laid it out, and its three buttons.
    keys: Option<(NodeKey, [NodeKey; 3])>,
}

impl Bar {
    /// Puts the bar up saying `text`, in place of anything it said before.
    pub(super) fn begin(&mut self, text: String) {
        self.asking = Some(text);
    }

    /// Takes the bar down.
    pub(super) fn close(&mut self) {
        self.asking = None;
        self.keys = None;
    }

    /// What the bar says, while it is up.
    pub(super) fn text(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    /// The bar as the last frame laid it out, while it is up.
    pub(super) fn key(&self) -> Option<NodeKey> {
        self.keys.map(|(bar, _)| bar)
    }

    /// The bar's Save, Discard and Cancel buttons, as the last frame laid
    /// them out.
    #[cfg(test)]
    pub(super) fn buttons(&self) -> Option<[NodeKey; 3]> {
        self.keys.map(|(_, buttons)| buttons)
    }

    /// Builds the bar, if it is up, and hands back the answer a click on it
    /// gave this frame.
    pub(super) fn build(&mut self, ui: &mut Ui) -> Option<Unsaved> {
        let text = self.asking.as_deref()?;
        let mut answer = None;
        let mut buttons = Vec::with_capacity(3);
        let bar = ui.block("#unsaved", &[], |ui| {
            ui.span(".unsaved-text", text, &[]);
            for (selector, label, choice) in [
                ("#unsaved-save", "Save (Enter)", Unsaved::Save),
                ("#unsaved-discard", "Discard (D)", Unsaved::Discard),
                ("#unsaved-cancel", "Cancel (Esc)", Unsaved::Cancel),
            ] {
                let button = ui.button(selector, label);
                if button.clicked {
                    answer = Some(choice);
                }
                buttons.push(button.key);
            }
        });
        self.keys = Some((
            bar.key,
            buttons
                .try_into()
                .expect("the bar builds exactly its three buttons"),
        ));
        answer
    }
}
