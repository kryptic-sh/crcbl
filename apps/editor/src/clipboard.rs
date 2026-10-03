//! The read a paste of entities or of a field waits for.
//!
//! The clipping's text — what a copy offers and what a paste of entities
//! reads — is `crcbl::scene_edit::clipboard`'s, re-exported here; this module
//! is the editor's half, which waits on the system clipboard.
//!
//! # Why a paste reads plain text
//!
//! [`Paste::ask`] requests [`MimeType::TextUtf8`], not the RON mime: the copy
//! offers the same bytes under both, and a clipping that went through a chat
//! comes back as text alone.
//!
//! # What a paste is for
//!
//! The same keys paste entities into the scene or a value into one inspector
//! field, by where the keyboard is ([`crate::panel::Panels::field_target`]),
//! and the read is answered frames after the key was pressed. So the paste
//! remembers its [`PasteTarget`] from the press: a pointer that moved off the
//! field while the clipboard answered does not turn a field paste into an
//! entity paste.

use crate::panel::FieldTarget;
use crcbl::shell::{
    ClipboardContent, ClipboardRequestId, MimeType, Shell, ShellError, ShellEvent, WindowId,
};

pub use crcbl::scene_edit::clipboard::{Clipped, ClippedRow, decode, encode};

/// What a paste was asked for: see the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteTarget {
    /// Spawn the entities a clipping names.
    Entities,
    /// Write a value into one leaf: of an entity's component, or of the
    /// scene's environment.
    Field(FieldTarget),
}

/// A paste waiting on the clipboard's answer.
///
/// The read is asynchronous on every backend that can do it
/// ([`Shell::clipboard_request`] says why), so a paste asks in one frame and
/// spawns in whichever frame the answer arrives.
#[derive(Debug, Default)]
pub struct Paste {
    /// The read this editor issued and no answer has arrived for, and what it
    /// is for. A newer paste replaces it: the newer press is the one the
    /// person is waiting on.
    awaiting: Option<(ClipboardRequestId, PasteTarget)>,
    /// The answer and what it is for, until [`take`](Self::take) collects it.
    arrived: Option<(PasteTarget, ClipboardContent)>,
}

impl Paste {
    /// Asks `shell` for the clipboard's text, to paste as `target` says.
    ///
    /// # Errors
    ///
    /// The shell's refusal: [`ShellError::Unsupported`] on a backend with no
    /// clipboard.
    pub fn ask<S: Shell + ?Sized>(
        &mut self,
        shell: &mut S,
        window: WindowId,
        target: PasteTarget,
    ) -> Result<(), ShellError> {
        let request = shell.clipboard_request(window, MimeType::TextUtf8)?;
        self.awaiting = Some((request, target));
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
        let Some((_, target)) = self.awaiting.take_if(|(awaited, _)| awaited == request) else {
            return false;
        };
        self.arrived = Some((target, content.clone()));
        true
    }

    /// The answer and what it is for, once: [`None`] until it arrives and
    /// after it was taken.
    pub fn take(&mut self) -> Option<(PasteTarget, ClipboardContent)> {
        self.arrived.take()
    }
}
