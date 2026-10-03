//! The recovery bar: a strip under the toolbar, there while the editor offers
//! back recovery copies an earlier run left, one row per copy with Open copy
//! and Delete, and Later to put the offer away.
//!
//! **Offered, never forced** (decided 2026-10-03): unlike the unsaved bar it
//! holds nothing — the panels and the viewport go on working under it — and
//! nothing happens to a copy until a button is clicked. What the rows say and
//! what a click does are [`crate::app`]'s (its `recovery` module); the bar
//! only draws the text it was given and hands back which button was clicked.
//!
//! **The keyboard answers it too.** O, Ctrl+Delete and L answer for the
//! first row, the newest copy ([`crate::keys::recovery`]), whose buttons say
//! so. Every button is an ordinary focusable button, so once a panel holds
//! the keyboard Tab walks onto the bar like anywhere else and Enter or Space
//! presses what it lands on.

#[cfg(test)]
use crcbl::ui::tree::NodeKey;
use crcbl::ui::tree::Ui;

/// A click on the recovery bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAnswer {
    /// Open the copy on this row, counted from the top.
    Open(usize),
    /// Delete the copy on this row.
    Delete(usize),
    /// Put the offer away for this run.
    Later,
}

/// What the bar says: a heading, and a line per copy.
#[derive(Debug)]
struct Offer {
    heading: String,
    rows: Vec<String>,
}

/// The recovery bar's state between frames.
#[derive(Debug, Default)]
pub(super) struct Bar {
    /// What the bar says, while it is up.
    offer: Option<Offer>,
    /// Every button as the last frame laid it out, in the order drawn: each
    /// row's Open copy and Delete, then Later. Kept for the tests alone,
    /// which click them; nothing else reads a button's rectangle.
    #[cfg(test)]
    buttons: Vec<NodeKey>,
}

impl Bar {
    /// Puts the bar up saying `heading` over `rows`, in place of anything it
    /// said before.
    pub(super) fn begin(&mut self, heading: String, rows: Vec<String>) {
        self.offer = Some(Offer { heading, rows });
    }

    /// Takes the bar down.
    pub(super) fn close(&mut self) {
        self.offer = None;
        #[cfg(test)]
        self.buttons.clear();
    }

    /// What the bar says, while it is up: its heading and its rows.
    pub(super) fn text(&self) -> Option<(&str, &[String])> {
        self.offer
            .as_ref()
            .map(|offer| (offer.heading.as_str(), offer.rows.as_slice()))
    }

    /// Each row's Open copy and Delete buttons, and Later, as the last frame
    /// laid them out — or [`None`] while the bar is down.
    #[cfg(test)]
    pub(super) fn buttons(&self) -> Option<(Vec<[NodeKey; 2]>, NodeKey)> {
        let (later, rows) = self.buttons.split_last()?;
        let (rows, _) = rows.as_chunks::<2>();
        Some((rows.to_vec(), *later))
    }

    /// Builds the bar, if it is up, and hands back the answer a click on it
    /// gave this frame.
    pub(super) fn build(&mut self, ui: &mut Ui) -> Option<RecoveryAnswer> {
        let Some(offer) = &self.offer else {
            return None;
        };
        let mut answer = None;
        #[cfg(test)]
        let mut keys = Vec::with_capacity(offer.rows.len() * 2 + 1);
        ui.block("#recovery", &[], |ui| {
            ui.span(".recovery-heading", offer.heading.as_str(), &[]);
            for (index, row) in offer.rows.iter().enumerate() {
                // The first row is the one the keys answer for, and says so.
                let (open, delete) = match index {
                    0 => ("Open copy (O)", "Delete (Ctrl+Del)"),
                    _ => ("Open copy", "Delete"),
                };
                ui.block_keyed(index, ".recovery-row", &[], |ui| {
                    ui.span(".recovery-text", row.as_str(), &[]);
                    let open = ui.button(".recovery-open", open);
                    let delete = ui.button(".recovery-delete", delete);
                    if open.clicked {
                        answer = Some(RecoveryAnswer::Open(index));
                    }
                    if delete.clicked {
                        answer = Some(RecoveryAnswer::Delete(index));
                    }
                    #[cfg(test)]
                    keys.extend([open.key, delete.key]);
                });
            }
            let button = ui.button("#recovery-later", "Later (L)");
            if button.clicked {
                answer = Some(RecoveryAnswer::Later);
            }
            #[cfg(test)]
            keys.push(button.key);
        });
        #[cfg(test)]
        {
            self.buttons = keys;
        }
        answer
    }
}
