//! The path line: a strip under the toolbar, there only while a directory is
//! being asked for — to save the scene into, or to open — holding a text
//! input the directory is typed into.
//!
//! **Typed, not chosen from a dialog**, because the shell offers none: a
//! native file dialog is a seam no backend has, and the editor's own text
//! input is already the way a name is given (a rename). Accept commits what
//! was typed and back cancels it, as a rename does; the strip hands the
//! committed text to [`crate::app`], which checks it
//! ([`crate::document::save_target`] or [`crate::document::open_target`]) and
//! saves through [`Document::save_as`](crate::document::Document::save_as) or
//! opens through [`Document::open_dir`](crate::document::Document::open_dir).
//! The input is engaged once it is first laid out, so the next key typed is
//! the path's. One line serves both, so opening one replaces the other.

use crcbl::ui::tree::{Engagement, NodeKey, TextInputOptions, Ui};

/// What a directory is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// To save the scene into and make its own: save-as.
    SaveAs,
    /// To open in place of the scene being edited.
    Open,
}

impl Purpose {
    /// What the strip is called, beside the input.
    const fn label(self) -> &'static str {
        match self {
            Self::SaveAs => "Save as",
            Self::Open => "Open",
        }
    }

    /// What the input shows while it is empty: what to type.
    const fn placeholder(self) -> &'static str {
        match self {
            Self::SaveAs => "a directory for the scene, such as levels/first.scn",
            Self::Open => "a scene directory, such as levels/first.scn",
        }
    }

    /// What the strip says after the input: how to finish.
    const fn hint(self) -> &'static str {
        match self {
            Self::SaveAs => "Enter saves, Escape cancels",
            Self::Open => "Enter opens, Escape cancels",
        }
    }
}

/// A directory being asked for.
#[derive(Debug)]
struct Asking {
    purpose: Purpose,
    /// What the input holds.
    text: String,
    /// Whether the input has been engaged — asked for once it was first laid
    /// out, because the tree engages only a node it has laid out.
    engaged: bool,
}

/// The path line's state between frames.
#[derive(Debug, Default)]
pub(super) struct Strip {
    asking: Option<Asking>,
}

impl Strip {
    /// Opens the line for `purpose` holding `text`, its input engaged on the
    /// next frame — in place of whatever it was asking for before.
    pub(super) fn begin(&mut self, purpose: Purpose, text: String) {
        self.asking = Some(Asking {
            purpose,
            text,
            engaged: false,
        });
    }

    /// Closes the line, dropping what it held.
    pub(super) fn close(&mut self) {
        self.asking = None;
    }

    /// What the input holds, while the line is open for `purpose`.
    pub(super) fn text(&self, purpose: Purpose) -> Option<&str> {
        self.asking
            .as_ref()
            .filter(|asking| asking.purpose == purpose)
            .map(|asking| asking.text.as_str())
    }

    /// Builds the line, if it is open, and hands back its input and where
    /// that input's engagement stood this frame.
    pub(super) fn build(&mut self, ui: &mut Ui) -> Option<(NodeKey, Engagement)> {
        let asking = self.asking.as_mut()?;
        let purpose = asking.purpose;
        let mut built = None;
        ui.block("#path-line", &[], |ui| {
            ui.span(".path-line-label", purpose.label(), &[]);
            let options = TextInputOptions {
                placeholder: purpose.placeholder(),
                masked: false,
            };
            let input = ui.text_input_with(".path-line-path", &mut asking.text, options);
            built = Some((input.key, input.engagement));
            ui.span(".path-line-hint", purpose.hint(), &[]);
        });
        built
    }

    /// Carries the line forward a frame: engages its input the first frame
    /// it is laid out, and closes it when the input commits — handing back
    /// what it was for and what was typed — cancels, or did not take the
    /// engagement.
    ///
    /// `input` is what [`build`](Self::build) handed back this frame.
    pub(super) fn follow(
        &mut self,
        ui: &mut Ui,
        input: Option<(NodeKey, Engagement)>,
    ) -> Option<(Purpose, String)> {
        let asking = self.asking.as_mut()?;
        let (key, engagement) = input?;
        if !asking.engaged {
            ui.engage(key);
            asking.engaged = true;
            return None;
        }
        match engagement {
            Engagement::Began | Engagement::Engaged => None,
            Engagement::Committed => self
                .asking
                .take()
                .map(|asking| (asking.purpose, asking.text)),
            Engagement::Cancelled | Engagement::Idle => {
                self.asking = None;
                None
            }
        }
    }
}
