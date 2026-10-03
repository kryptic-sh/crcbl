//! The play strip: the running games' play controls and the run's numbers,
//! under the toolbar while a scene plays.
//!
//! **Drawn from the description, the same for every game.** Each running
//! module whose game registered [`PlayControls`] gets a row, in the order the
//! modules tick: a button per action, and beside it a button per
//! [`ParamKind::Choice`] that shows the current choice and steps to the next
//! on a click. A [`ParamKind::Picked`] argument is the selection — an action
//! naming towers' plots takes the plot selected in the viewport or the
//! outliner — and a [`ParamKind::PickedRuntime`] one is the runtime pick, the
//! spawned entity last clicked in the viewport (towers' _Upgrade_ takes the
//! tower clicked), so a game needs no widget of its own. The run's numbers
//! follow, as `Label value`. Only the scene's own games have rows: a game
//! whose module the scene's systems do not start is not running.
//!
//! **The number keys are the actions, in the order drawn**: the first nine
//! actions across every row are labelled with their key, as the toolbar's
//! buttons are, and `1` to `9` send them ([`numbered`](Strip::numbered)) —
//! through [`crate::keys`], so a text field being typed into keeps its
//! digits.
//!
//! **Each button has a tooltip** saying what it sends and what the action
//! takes — read off its [`ParamKind`]s, since a [`PlayAction`] carries no
//! description of its own — with the number key again; a choice's says what
//! it steps through.
//!
//! **A choice lasts one play.** Stop takes the strip away and every choice's
//! pick with it, so each play starts on each choice's first label, as the run
//! it starts is a fresh one.
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

use crcbl::registry::{ParamKind, PlayAction, PlayArg, PlayControls};
use crcbl::ui::tree::{NodeKey, Ui};

use crate::document::{Document, EditError};

/// Where one choice of one action is kept: the controls' system, the
/// action's index and the parameter's index.
type ChoiceKey = (String, usize, usize);

/// The strip's state between frames: each choice's current pick, and what
/// the last frame built.
#[derive(Debug, Default)]
pub(super) struct Strip {
    /// Each choice parameter's current index this play; one never stepped is
    /// the first label.
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
    /// nothing at all when `controls` is empty — which is also where a stop
    /// throws every choice's pick away. Returns the action a click asked for
    /// this frame; a click on a choice steps it here.
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
            self.choices.clear();
            return None;
        }
        let mut asked = None;
        let mut number = 0;
        let strip = ui.block("#play-controls", &[], |ui| {
            for (system, controls) in controls {
                ui.block_keyed(system, ".play-game", &[], |ui| {
                    for (action, described) in controls.actions.iter().enumerate() {
                        number += 1;
                        let label = keyed(described.name, number);
                        let button = ui.button(".play-action", label.as_str());
                        ui.tooltip(&button, action_tip(described, number).as_str());
                        self.buttons.push((label, button.key));
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
                            let tip = format!(
                                "{}'s choice: a click steps it on through {}",
                                described.name,
                                labels.join(", ")
                            );
                            ui.tooltip(&button, tip.as_str());
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

    /// The action the number key for `index` asks for: the action at
    /// `index` counting every row's actions in the order drawn, or [`None`]
    /// past the last — and for every index while no game offers controls.
    pub(super) fn numbered(controls: &[(String, PlayControls)], index: usize) -> Option<Asked> {
        controls
            .iter()
            .flat_map(|(system, controls)| {
                (0..controls.actions.len()).map(move |action| Asked {
                    system: system.clone(),
                    action,
                })
            })
            .nth(index)
    }

    /// The arguments `asked`'s action takes, gathered — each choice's current
    /// pick, each picked entity off `document`'s selection, each runtime one
    /// off its runtime pick — and sent through [`Document::send_play`].
    /// Returns what the status line says: that it was sent, or why it was
    /// not, as a warning.
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
                ParamKind::PickedRuntime(system) => {
                    PlayArg::PickedRuntime(document.picked_runtime(system).ok_or_else(|| {
                        format!(
                            "{}: click one of `{system}` in the viewport first",
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

/// `text` — a button's name, or its tooltip — with the number key that sends
/// the `number`th action drawn, for the actions there are keys for: as the
/// toolbar labels each button with its key.
fn keyed(text: &str, number: usize) -> String {
    if number <= crate::keys::PLAY_ACTIONS.len() {
        format!("{text} ({number})")
    } else {
        text.to_owned()
    }
}

/// What the tooltip of the `number`th action drawn says: that it sends it,
/// each argument it takes by where it comes from, and its number key.
fn action_tip(described: &PlayAction, number: usize) -> String {
    let takes: Vec<String> = described
        .params
        .iter()
        .map(|kind| match *kind {
            ParamKind::Picked(system) => format!("the `{system}` selected in the scene"),
            ParamKind::PickedRuntime(system) => {
                format!("the `{system}` last clicked in the viewport")
            }
            ParamKind::Choice(_) => "the choice beside it".to_owned(),
        })
        .collect();
    let sends = if takes.is_empty() {
        format!("Send {} to the game", described.name)
    } else {
        format!(
            "Send {} to the game, for {}",
            described.name,
            takes.join(" and ")
        )
    };
    keyed(&sends, number)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A play-strip tooltip says what the action takes and names its number
    /// key**, and an action past the keys has no key to name.
    #[test]
    fn an_actions_tooltip_says_what_it_takes_and_names_its_key() {
        let place = PlayAction {
            name: "Place tower",
            params: &[ParamKind::Picked("plots"), ParamKind::Choice(&["Bolt"])],
        };
        assert_eq!(
            action_tip(&place, 1),
            "Send Place tower to the game, for the `plots` selected in the scene and the \
             choice beside it (1)"
        );
        let upgrade = PlayAction {
            name: "Upgrade",
            params: &[ParamKind::PickedRuntime("towers")],
        };
        let past = crate::keys::PLAY_ACTIONS.len() + 1;
        assert_eq!(
            action_tip(&upgrade, past),
            "Send Upgrade to the game, for the `towers` last clicked in the viewport"
        );
        let start = PlayAction {
            name: "Start wave",
            params: &[],
        };
        assert_eq!(action_tip(&start, 3), "Send Start wave to the game (3)");
    }
}
