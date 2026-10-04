//! The unsaved bar: a strip under the toolbar, there only while the editor is
//! asking what to do about unsaved edits, saying what will be lost and
//! offering Save, Discard and Cancel.
//!
//! **The same bar asks about a scene changed on disk** (decided 2026-10-04):
//! a Save that finds the scene's files changed since the editor read or
//! wrote them puts it up with Overwrite, Reload and Cancel — the same three
//! answers by the same keys, since each is the same choice made about the
//! other side's work: keep this side's ([`Asking::ChangedOnDisk`]).
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

/// What the bar is asking about, which names its buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asking {
    /// Unsaved edits something would lose: Save, Discard, Cancel.
    Unsaved,
    /// A scene changed on disk that a save would overwrite: Overwrite,
    /// Reload, Cancel.
    ChangedOnDisk,
}

impl Asking {
    /// The labels of the bar's buttons, in the order of [`Unsaved::Save`],
    /// [`Unsaved::Discard`] and [`Unsaved::Cancel`].
    const fn labels(self) -> [&'static str; 3] {
        match self {
            Self::Unsaved => ["Save (Enter)", "Discard (D)", "Cancel (Esc)"],
            Self::ChangedOnDisk => ["Overwrite (Enter)", "Reload (D)", "Cancel (Esc)"],
        }
    }
}

/// The unsaved bar's state between frames.
#[derive(Debug, Default)]
pub(super) struct Bar {
    /// What the bar says and what it asks about, while it is up.
    asking: Option<(String, Asking)>,
    /// The bar as the last frame laid it out, and its three buttons.
    keys: Option<(NodeKey, [NodeKey; 3])>,
}

impl Bar {
    /// Puts the bar up saying `text` about `asking`, in place of anything it
    /// said before.
    pub(super) fn begin(&mut self, text: String, asking: Asking) {
        self.asking = Some((text, asking));
    }

    /// Takes the bar down.
    pub(super) fn close(&mut self) {
        self.asking = None;
        self.keys = None;
    }

    /// What the bar says, while it is up.
    pub(super) fn text(&self) -> Option<&str> {
        self.asking.as_ref().map(|(text, _)| text.as_str())
    }

    /// What the bar asks about, while it is up.
    pub(super) fn asking(&self) -> Option<Asking> {
        self.asking.as_ref().map(|&(_, asking)| asking)
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
        let (text, asking) = self.asking.as_ref()?;
        let [save, discard, cancel] = asking.labels();
        let mut answer = None;
        let mut buttons = Vec::with_capacity(3);
        let bar = ui.block("#unsaved", &[], |ui| {
            ui.span(".unsaved-text", text.as_str(), &[]);
            for (selector, label, choice) in [
                ("#unsaved-save", save, Unsaved::Save),
                ("#unsaved-discard", discard, Unsaved::Discard),
                ("#unsaved-cancel", cancel, Unsaved::Cancel),
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
