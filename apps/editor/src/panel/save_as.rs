//! The save-as line: a strip under the toolbar, there only while a directory
//! is being asked for, holding a text input the directory is typed into.
//!
//! **Typed, not chosen from a dialog**, because the shell offers none: a
//! native file dialog is a seam no backend has, and the editor's own text
//! input is already the way a name is given (a rename). Accept commits what
//! was typed and back cancels it, as a rename does; the strip hands the
//! committed text to [`crate::app`], which checks it
//! ([`crate::document::save_target`]) and saves through
//! [`Document::save_as`](crate::document::Document::save_as). The input is
//! engaged once it is first laid out, so the next key typed is the path's.

use crcbl::ui::tree::{Engagement, NodeKey, TextInputOptions, Ui};

/// What the input shows while it is empty: what to type.
const PLACEHOLDER: &str = "a directory for the scene, such as levels/first.scn";

/// What the strip says beside the input: how to finish.
const HINT: &str = "Enter saves, Escape cancels";

/// A directory being asked for.
#[derive(Debug)]
struct Asking {
    /// What the input holds.
    text: String,
    /// Whether the input has been engaged — asked for once it was first laid
    /// out, because the tree engages only a node it has laid out.
    engaged: bool,
}

/// The save-as line's state between frames.
#[derive(Debug, Default)]
pub(super) struct Strip {
    asking: Option<Asking>,
}

impl Strip {
    /// Opens the line holding `text`, its input engaged on the next frame.
    pub(super) fn begin(&mut self, text: String) {
        self.asking = Some(Asking {
            text,
            engaged: false,
        });
    }

    /// What the input holds, while the line is open.
    pub(super) fn text(&self) -> Option<&str> {
        self.asking.as_ref().map(|asking| asking.text.as_str())
    }

    /// Builds the line, if it is open, and hands back its input and where
    /// that input's engagement stood this frame.
    pub(super) fn build(&mut self, ui: &mut Ui) -> Option<(NodeKey, Engagement)> {
        let asking = self.asking.as_mut()?;
        let mut built = None;
        ui.block("#save-as", &[], |ui| {
            ui.span(".save-as-label", "Save as", &[]);
            let options = TextInputOptions {
                placeholder: PLACEHOLDER,
                masked: false,
            };
            let input = ui.text_input_with(".save-as-path", &mut asking.text, options);
            built = Some((input.key, input.engagement));
            ui.span(".save-as-hint", HINT, &[]);
        });
        built
    }

    /// Carries the line forward a frame: engages its input the first frame
    /// it is laid out, and closes it when the input commits — handing back
    /// what was typed — cancels, or did not take the engagement.
    ///
    /// `input` is what [`build`](Self::build) handed back this frame.
    pub(super) fn follow(
        &mut self,
        ui: &mut Ui,
        input: Option<(NodeKey, Engagement)>,
    ) -> Option<String> {
        let asking = self.asking.as_mut()?;
        let (key, engagement) = input?;
        if !asking.engaged {
            ui.engage(key);
            asking.engaged = true;
            return None;
        }
        match engagement {
            Engagement::Began | Engagement::Engaged => None,
            Engagement::Committed => self.asking.take().map(|asking| asking.text),
            Engagement::Cancelled | Engagement::Idle => {
                self.asking = None;
                None
            }
        }
    }
}
