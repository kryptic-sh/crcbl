//! What a tool needs to take part in a game it plays: the actions a person may
//! take, how a chosen one becomes that game's own command bytes, and the run's
//! numbers — see [`PlayControls`].
//!
//! ```text
//!     a game ──▶ Registry::play_controls("bricks", CONTROLS)
//!                     │
//!                     ├── controls_for()  the actions, for a tool to list
//!                     ├── encode_play()   a chosen action and its arguments,
//!                     │                   checked against the action, then
//!                     │                   the game's command bytes
//!                     └── (status)(world), (refusals)(world)
//!                                         the run's numbers and the commands
//!                                         it turned down, read off the world
//!                                         the game's module plays in
//! ```
//!
//! # Commands, not a panel per game
//!
//! A tool hands a playing module **the bytes a networked client would**, in
//! the [`ClientInputs`](crcbl_ecs::ClientInputs) its next tick is given: the
//! engine's rule that every action is a command value, with nothing reachable
//! only through a widget. So a game describes its actions as data — a name
//! and the kind of each argument — and supplies the encoder its own client's
//! commands go through; a tool renders that description however it renders
//! things, and the game validates what arrives as it validates any client's
//! command, refusing by its own rules.
//!
//! # The status lives here rather than on `GameModule`
//!
//! The run's numbers could have been a provided method on
//! [`GameModule`](crcbl_ecs::GameModule) answering an empty list. They are a
//! function of the **world** instead, registered beside the encoder: the
//! module trait is the engine's seam with every game, its server and its wasm
//! binding, and a tool's readout is not something any of those need. A game
//! puts what a tool may read into the world it plays in — the same seam a
//! [runtime](super::Registry::register_runtime) component is — and the
//! description says how to read it.

use std::fmt;

use crcbl_ecs::World;

use super::Registry;

/// What a game offers a tool that plays one of its scenes: the actions a
/// person may take, the encoder that turns one into the game's command bytes,
/// and how to read the run off the world its module plays in.
///
/// Registered with [`Registry::play_controls`] under the system the game's
/// module is registered under ([`Registry::module`]), so a tool shows it while
/// that module runs and never on another game's scene.
#[derive(Clone, Copy)]
pub struct PlayControls {
    /// The actions, in the order a tool lists them. An action's index in this
    /// list is what [`encode`](Self::encode) is handed.
    pub actions: &'static [PlayAction],
    /// The game's encoder: an action's index and its arguments, already
    /// checked against that action's parameters by
    /// [`Registry::encode_play`], as the bytes the game's own client sends for
    /// the same command — or why this game cannot spell them.
    pub encode: PlayEncoder,
    /// The run's numbers, labelled, in the order a tool shows them — empty for
    /// a world the game's module is not playing in.
    pub status: PlayStatus,
    /// Takes the reasons the game turned down commands since the last call,
    /// oldest first: what a tool tells the person who sent them.
    pub refusals: PlayRefusals,
}

/// [`PlayControls::encode`]: an action's index and its checked arguments, as
/// the game's command bytes, or why the game cannot spell them.
pub type PlayEncoder = fn(usize, &[PlayArg]) -> Result<Vec<u8>, String>;

/// [`PlayControls::status`]: the run's numbers, read off the world a game's
/// module plays in, each with its label.
pub type PlayStatus = fn(&mut World) -> Vec<(&'static str, String)>;

/// [`PlayControls::refusals`]: the commands a game turned down since the last
/// call, each as the reason it gave, taken off the world its module plays in.
pub type PlayRefusals = fn(&mut World) -> Vec<String>;

impl fmt::Debug for PlayControls {
    /// The actions, which are the description; the three functions are
    /// addresses, which a log line does not want.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayControls")
            .field("actions", &self.actions)
            .finish_non_exhaustive()
    }
}

/// One thing a person may do to a game while it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayAction {
    /// What a tool labels it with.
    pub name: &'static str,
    /// What it takes, in the order [`PlayControls::encode`] is handed them.
    pub params: &'static [ParamKind],
}

/// What one argument of a [`PlayAction`] is, and where a tool gets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// An entity of the scene system it names, picked in the scene — handed
    /// to the encoder as [`PlayArg::Picked`].
    Picked(&'static str),
    /// One of a fixed list of choices, by label — handed to the encoder as
    /// [`PlayArg::Choice`].
    Choice(&'static [&'static str]),
}

/// One argument a tool hands a [`PlayAction`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayArg {
    /// For [`ParamKind::Picked`]: the picked entity's place among its
    /// system's entities **in the order the chunk file spells them** — its
    /// index in [`Registry::entities`], which is id order.
    ///
    /// A place rather than a [`SceneEntityId`](crcbl_scene::scn::SceneEntityId),
    /// because a game numbers its scene's rows the way its loader reads them,
    /// and an id is nothing to a game that has kept no id: towers' build
    /// plots are numbered in file order, which is what its commands carry.
    Picked(usize),
    /// For [`ParamKind::Choice`]: the index of the chosen label.
    Choice(usize),
}

impl ParamKind {
    /// Whether `arg` is an argument of this kind: a picked entity for
    /// [`Picked`](Self::Picked), and for [`Choice`](Self::Choice) an index
    /// one of its labels has.
    #[must_use]
    pub fn accepts(self, arg: PlayArg) -> bool {
        match (self, arg) {
            (Self::Picked(_), PlayArg::Picked(_)) => true,
            (Self::Choice(labels), PlayArg::Choice(index)) => index < labels.len(),
            _ => false,
        }
    }
}

impl Registry {
    /// Registers `controls` for the game whose module is registered under
    /// `system` — see [`PlayControls`].
    ///
    /// # Panics
    ///
    /// If `system` already has controls, for [`register`](Self::register)'s
    /// reason: a tool finds them by name, and two under one name would leave
    /// one of them never shown.
    pub fn play_controls(&mut self, system: impl Into<String>, controls: PlayControls) {
        let system = system.into();
        assert!(
            !self.controls.contains_key(&system),
            "play controls are already registered under `{system}`",
        );
        self.controls.insert(system, controls);
    }

    /// The controls registered under `system`, or [`None`] for a game that
    /// registered none.
    #[must_use]
    pub fn controls_for(&self, system: &str) -> Option<&PlayControls> {
        self.controls.get(system)
    }

    /// The command bytes for the action at index `action` of the controls
    /// under `system`, taking `args`: the arguments checked against the
    /// action's parameters — as many, each of its kind, every choice one the
    /// list has — and then handed to the game's own encoder.
    ///
    /// The check is the description's, made once here for every game, so an
    /// encoder is never handed an argument its own action did not ask for.
    /// What the bytes then mean — whether that plot is free, whether there is
    /// gold for it — is the game's to decide when they arrive, as it decides
    /// for any client.
    ///
    /// # Errors
    ///
    /// No controls under `system`, no action at that index, arguments that
    /// do not fit its parameters, or the encoder's own refusal — each naming
    /// what was wrong.
    pub fn encode_play(
        &self,
        system: &str,
        action: usize,
        args: &[PlayArg],
    ) -> Result<Vec<u8>, String> {
        let controls = self
            .controls
            .get(system)
            .ok_or_else(|| format!("no game registers play controls under `{system}`"))?;
        let described = controls.actions.get(action).ok_or_else(|| {
            format!(
                "the controls under `{system}` have {} actions, and no action {action}",
                controls.actions.len()
            )
        })?;
        if args.len() != described.params.len() {
            return Err(format!(
                "`{}` takes {} arguments, not {}",
                described.name,
                described.params.len(),
                args.len()
            ));
        }
        if let Some((index, (param, arg))) = described
            .params
            .iter()
            .zip(args)
            .enumerate()
            .find(|(_, (param, arg))| !param.accepts(**arg))
        {
            return Err(format!(
                "`{}` takes {param:?} as argument {index}, not {arg:?}",
                described.name
            ));
        }
        (controls.encode)(action, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test game's kinds of thing to build.
    const KINDS: &[&str] = &["small", "large"];

    /// The test game's actions: one taking a picked block and a kind, one
    /// taking nothing.
    const ACTIONS: &[PlayAction] = &[
        PlayAction {
            name: "Build",
            params: &[ParamKind::Picked("blocks"), ParamKind::Choice(KINDS)],
        },
        PlayAction {
            name: "Go",
            params: &[],
        },
    ];

    /// Spells an action as its index followed by each argument's number, so
    /// a test reads back exactly what the encoder was handed.
    fn encode(action: usize, args: &[PlayArg]) -> Result<Vec<u8>, String> {
        let mut bytes = vec![u8::try_from(action).map_err(|error| error.to_string())?];
        for arg in args {
            let (PlayArg::Picked(value) | PlayArg::Choice(value)) = *arg;
            bytes.push(u8::try_from(value).map_err(|error| error.to_string())?);
        }
        Ok(bytes)
    }

    fn no_status(_: &mut World) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    fn no_refusals(_: &mut World) -> Vec<String> {
        Vec::new()
    }

    const CONTROLS: PlayControls = PlayControls {
        actions: ACTIONS,
        encode,
        status: no_status,
        refusals: no_refusals,
    };

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.play_controls("blocks", CONTROLS);
        registry
    }

    /// **Arguments that fit their action reach the game's encoder as they
    /// were handed**, and the controls are found under the system they were
    /// registered under and no other.
    #[test]
    fn arguments_that_fit_reach_the_encoder() {
        let registry = registry();
        assert_eq!(
            registry
                .controls_for("blocks")
                .map(|controls| controls.actions),
            Some(ACTIONS),
        );
        assert!(registry.controls_for("beacons").is_none());
        assert_eq!(
            registry.encode_play("blocks", 0, &[PlayArg::Picked(3), PlayArg::Choice(1)]),
            Ok(vec![0, 3, 1]),
        );
        assert_eq!(registry.encode_play("blocks", 1, &[]), Ok(vec![1]));
    }

    /// **Arguments that do not fit their action never reach the encoder**:
    /// too few, too many, the wrong kind, a choice past the list, an action
    /// past the list and a system with no controls are each refused by name.
    #[test]
    fn arguments_that_do_not_fit_are_refused_by_name() {
        let registry = registry();
        let refused = |system: &str, action: usize, args: &[PlayArg]| {
            registry
                .encode_play(system, action, args)
                .expect_err("refused")
        };
        assert!(refused("blocks", 0, &[PlayArg::Picked(3)]).contains("takes 2 arguments"));
        assert!(refused("blocks", 1, &[PlayArg::Choice(0)]).contains("takes 0 arguments"));
        assert!(
            refused("blocks", 0, &[PlayArg::Choice(0), PlayArg::Choice(0)])
                .contains("as argument 0")
        );
        assert!(
            refused(
                "blocks",
                0,
                &[PlayArg::Picked(0), PlayArg::Choice(KINDS.len())]
            )
            .contains("as argument 1")
        );
        assert!(refused("blocks", ACTIONS.len(), &[]).contains("no action"));
        assert!(refused("beacons", 0, &[]).contains("`beacons`"));
    }

    /// Two sets of controls under one system would hide one of them.
    #[test]
    #[should_panic(expected = "already registered under `blocks`")]
    fn two_sets_of_controls_under_one_system_are_refused() {
        let mut registry = registry();
        registry.play_controls("blocks", CONTROLS);
    }
}
