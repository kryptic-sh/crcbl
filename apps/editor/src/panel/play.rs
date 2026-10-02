//! The play strip: the running games' play controls and the run's numbers,
//! under the toolbar while a scene plays.
//!
//! **Drawn from the description, the same for every game.** Each running
//! module whose game registered [`PlayControls`] gets a row: a button per
//! action, and beside it a button per [`ParamKind::Choice`] that shows the
//! current choice and steps to the next on a click. A
//! [`ParamKind::Picked`] argument is the selection — an action naming
//! towers' plots takes the plot selected in the viewport or the outliner —
//! so a game needs no widget of its own. The run's numbers follow, as
//! `Label value`.
//!
//! **A click is a command, not an edit.** The action's arguments are
//! gathered here and [`Document::send_play`] encodes and queues them; the
//! status line says it was sent, or why it was not, and a refusal from the
//! game arrives on it after the tick that read the command — `crate::app`
//! takes those off the document.
//!
//! **Drawn only while a game offers controls**, so a scene whose games offer
//! none — every scene being edited, and every game that registers none — shows no
//! strip and keeps the viewport its full height.

use std::collections::BTreeMap;

use crcbl::registry::{ParamKind, PlayArg, PlayControls};
use crcbl::ui::tree::{NodeKey, Ui};

use crate::document::{Document, EditError};

/// Where one choice of one action is kept: the controls' system, the
/// action's index and the parameter's index.
type ChoiceKey = (String, usize, usize);

/// The strip's state between frames: each choice's current pick, and what
/// the last frame built.
#[derive(Debug, Default)]
pub(super) struct Strip {
    /// Each choice parameter's current index; one never stepped is the first
    /// label.
    choices: BTreeMap<ChoiceKey, usize>,
    /// The strip, as the last frame laid it out — [`None`] on a frame that
    /// drew none.
    key: Option<NodeKey>,
    /// Each button the last frame built and the label it read, in the order
    /// drawn.
    buttons: Vec<(String, NodeKey)>,
    /// The run's numbers the last frame drew, in the order drawn.
    shown: Vec<(&'static str, String)>,
}

/// An action a click on the strip asked for: the controls' system and the
/// action's index.
pub(super) struct Asked {
    system: String,
    action: usize,
}

impl Strip {
    /// Builds the strip for `controls` with the run's numbers `status`, or
    /// nothing at all when `controls` is empty. Returns the action a click
    /// asked for this frame; a click on a choice steps it here.
    pub(super) fn build(
        &mut self,
        ui: &mut Ui,
        controls: &[(String, PlayControls)],
        status: Vec<(&'static str, String)>,
    ) -> Option<Asked> {
        self.buttons.clear();
        if controls.is_empty() {
            self.key = None;
            self.shown.clear();
            return None;
        }
        let mut asked = None;
        let strip = ui.block("#play-controls", &[], |ui| {
            for (system, controls) in controls {
                ui.block_keyed(system, ".play-game", &[], |ui| {
                    for (action, described) in controls.actions.iter().enumerate() {
                        let button = ui.button(".play-action", described.name);
                        self.buttons.push((described.name.to_owned(), button.key));
                        if button.clicked {
                            asked = Some(Asked {
                                system: system.clone(),
                                action,
                            });
                        }
                        for (param, kind) in described.params.iter().enumerate() {
                            let ParamKind::Choice(labels) = *kind else {
                                continue;
                            };
                            let key = (system.clone(), action, param);
                            let chosen = self.choices.get(&key).copied().unwrap_or(0);
                            let label = labels.get(chosen).copied().unwrap_or_default();
                            let button = ui.button(".play-choice", label);
                            self.buttons.push((label.to_owned(), button.key));
                            if button.clicked && !labels.is_empty() {
                                self.choices.insert(key, (chosen + 1) % labels.len());
                            }
                        }
                    }
                });
            }
            for (label, value) in &status {
                let text = format!("{label} {value}");
                ui.span(".play-status", text.as_str(), &[]);
            }
        });
        self.key = Some(strip.key);
        self.shown = status;
        asked
    }

    /// The arguments `asked`'s action takes, gathered — each choice's current
    /// pick, each picked entity off `document`'s selection — and sent through
    /// [`Document::send_play`]. Returns what the status line says: that it
    /// was sent, or why it was not, as a warning.
    pub(super) fn send(
        &self,
        document: &mut Document,
        controls: &[(String, PlayControls)],
        asked: &Asked,
    ) -> Result<String, String> {
        let Some(described) = controls
            .iter()
            .find(|(system, _)| *system == asked.system)
            .and_then(|(_, controls)| controls.actions.get(asked.action))
        else {
            return Err(EditError::PlayCommand(format!(
                "no action {} under `{}`",
                asked.action, asked.system
            ))
            .to_string());
        };
        let mut args = Vec::with_capacity(described.params.len());
        for (param, kind) in described.params.iter().enumerate() {
            args.push(match *kind {
                ParamKind::Picked(system) => {
                    PlayArg::Picked(document.picked(system).ok_or_else(|| {
                        format!(
                            "{}: select one of `{system}` in the scene first",
                            described.name
                        )
                    })?)
                }
                ParamKind::Choice(_) => PlayArg::Choice(
                    self.choices
                        .get(&(asked.system.clone(), asked.action, param))
                        .copied()
                        .unwrap_or(0),
                ),
            });
        }
        document
            .send_play(&asked.system, asked.action, &args)
            .map_err(|error| error.to_string())?;
        Ok(format!("Sent {}", described.name))
    }

    /// The strip, as the last frame laid it out.
    #[cfg(test)]
    pub(super) const fn key(&self) -> Option<NodeKey> {
        self.key
    }

    /// Each button the last frame built and the label it read.
    #[cfg(test)]
    pub(super) fn buttons(&self) -> &[(String, NodeKey)] {
        &self.buttons
    }

    /// The run's numbers the last frame drew.
    #[cfg(test)]
    pub(super) fn shown(&self) -> &[(&'static str, String)] {
        &self.shown
    }
}
