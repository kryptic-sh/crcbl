//! The loop's half of a confirm key: the prompt it draws over a pending
//! display change, and the window-and-swapchain [`Stage`] the change is
//! applied through.
//!
//! [`crate::settings::confirm`] is the flow — apply live, hold the previous
//! value, count down on frame time, keep or revert. What a settings module
//! cannot do is reach the window or the swapchain, or draw: the loop owns the
//! shell and the bundle, so it is the loop that applies a change, asks the
//! player and puts the answer into force. Here so every game gets the prompt
//! without drawing one — `apps/options`' rows, the pause menu's `FULLSCREEN`
//! and the fullscreen key all reach the same one.
//!
//! # The prompt is a menu over the menu
//!
//! A [`MenuSet`] of its own, drawn after the game's panel and in the same
//! skin, so it is the engine's UI kit rather than a widget of its own. While it
//! is up it takes the keyboard, the pointer and the pads' menu navigation: a
//! player answering it must not also be pressing the panel underneath.

use crcbl_shell::{DisplayMode, Shell, WindowId};
use crcbl_ui::WidgetId;
use crcbl_ui::menu::{Caption, Menu, MenuItem, MenuSet};

use super::{GameGpu, ModeRequest, Pacing};
use crate::settings::confirm::PendingChange;
use crate::settings::{Landed, Stage, Unsupported};

/// The id of the prompt's button that keeps the change.
///
/// In the range below [`FIRST_GAME_ID`](super::FIRST_GAME_ID) the engine
/// reserves, beside the pause menu's three — the prompt is its own set, so no
/// game panel shares a press with it, but a reserved id still cannot be
/// mistaken for a game's.
pub const KEEP_ID: WidgetId = 4;

/// The id of the prompt's button that reverts the change. See [`KEEP_ID`].
pub const REVERT_ID: WidgetId = 5;

/// What the prompt asks.
pub const PROMPT_TITLE: &str = "Keep these display settings?";

/// Which of the prompt's two states a frame is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// No change is waiting; the set draws nothing.
    Hidden,
    /// A change is waiting on the player.
    Asking,
}

/// The prompt, and the change it is asking about.
#[derive(Debug)]
pub(super) struct ConfirmPrompt {
    menus: MenuSet<Shown>,
    pending: Option<PendingChange>,
}

impl ConfirmPrompt {
    /// Nothing waiting.
    pub(super) fn new() -> Self {
        Self {
            menus: MenuSet::new(
                Shown::Hidden,
                vec![(
                    Shown::Asking,
                    Menu::new(
                        PROMPT_TITLE,
                        vec![
                            MenuItem::new(KEEP_ID, "KEEP", ""),
                            MenuItem::new(REVERT_ID, "REVERT", ""),
                        ],
                    ),
                )],
            ),
            pending: None,
        }
    }

    /// Whether a change is waiting, and so whether the prompt is on screen.
    pub(super) const fn is_showing(&self) -> bool {
        self.pending.is_some()
    }

    /// The change waiting on the player.
    pub(super) const fn pending(&self) -> Option<&PendingChange> {
        self.pending.as_ref()
    }

    /// The change waiting on the player, to observe and count down.
    pub(super) const fn pending_mut(&mut self) -> Option<&mut PendingChange> {
        self.pending.as_mut()
    }

    /// The prompt's panel, for the loop's input and its drawing.
    pub(super) const fn menus_mut(&mut self) -> &mut MenuSet<Shown> {
        &mut self.menus
    }

    /// The prompt's panel, to lay out and draw.
    pub(super) const fn menus(&self) -> &MenuSet<Shown> {
        &self.menus
    }

    /// Puts `change` on screen, KEEP highlighted — the button a player who
    /// can see the prompt is there to press.
    pub(super) fn hold(&mut self, change: PendingChange) {
        self.pending = Some(change);
        self.menus.show(Shown::Asking);
        if let Some(menu) = self.menus.get_mut(Shown::Asking) {
            menu.select_id(KEEP_ID);
        }
        self.sync();
    }

    /// Takes the waiting change off screen, to keep or revert.
    pub(super) fn take(&mut self) -> Option<PendingChange> {
        self.menus.show(Shown::Hidden);
        self.pending.take()
    }

    /// Writes the waiting change's lines into the panel: what landed, and the
    /// seconds left. Every frame, so the countdown moves and an answer the
    /// window system gave late replaces the value the request left.
    pub(super) fn sync(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let lines = prompt_lines(pending);
        if let Some(menu) = self.menus.get_mut(Shown::Asking) {
            menu.subtitle = lines;
        }
    }
}

/// The prompt's lines under its title: the key and the value it **landed**
/// on — not the one asked for, which a window system may have refused — and
/// the countdown.
///
/// The key is written as a settings screen spells a row, `display_mode` as
/// `display mode`.
#[must_use]
pub fn prompt_lines(pending: &PendingChange) -> Vec<Caption> {
    let name = pending.key().rsplit('.').next().unwrap_or(pending.key());
    vec![
        Caption::hint(format!("{}: {}", name.replace('_', " "), pending.landed())),
        Caption::warning(format!("Reverting in {} s", pending.seconds_left())),
    ]
}

/// The window and the swapchain, as the [`Stage`] a confirm key is applied
/// through.
///
/// Built for one call and dropped, because it borrows the loop's shell and
/// bundle — the two things a settings module cannot hold.
pub(super) struct WindowStage<'a, S: Shell + ?Sized, G: GameGpu> {
    pub(super) shell: &'a mut S,
    pub(super) window: WindowId,
    pub(super) gpu: &'a mut G,
}

impl<S: Shell + ?Sized, G: GameGpu> Stage for WindowStage<'_, S, G> {
    /// [`ModeRequest::mode`]: what the window system has the window in.
    fn display_mode(&self) -> Result<DisplayMode, Unsupported> {
        ModeRequest::mode(&*self.shell, self.window).ok_or(Unsupported)
    }

    /// The request, and the mode the window is in straight after it — which
    /// on every backend that answers through a configure is still the old
    /// one; the pending change reads the answer when it arrives.
    ///
    /// A shell that refuses the request outright is logged and answered as
    /// what it is: the window stayed where it was, which is the landed value
    /// the prompt then shows.
    fn set_display_mode(&mut self, mode: DisplayMode) -> Result<Landed<DisplayMode>, Unsupported> {
        let previous = self.display_mode()?;
        match self.shell.set_mode(self.window, mode) {
            Ok(()) => log::info!("shell: asked for {mode}"),
            Err(error) => log::warn!("shell: refused {mode}: {error}"),
        }
        Ok(Landed {
            previous,
            landed: self.display_mode()?,
        })
    }

    /// [`GameGpu::set_pacing`].
    fn set_present_mode(&mut self, pacing: Pacing) -> Result<Landed<Pacing>, Unsupported> {
        self.gpu.set_pacing(pacing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_console::Value;
    use crcbl_ui::menu::CaptionTone;

    use crate::settings::confirm::{Change, change};
    use crate::settings::{DISPLAY_MODE_KEY, VIDEO_NAMESPACE};

    /// A held display change, from a stage that took it.
    fn held_borderless() -> PendingChange {
        struct Window(DisplayMode);
        impl Stage for Window {
            fn display_mode(&self) -> Result<DisplayMode, Unsupported> {
                Ok(self.0)
            }
            fn set_display_mode(
                &mut self,
                mode: DisplayMode,
            ) -> Result<Landed<DisplayMode>, Unsupported> {
                let previous = self.0;
                self.0 = mode;
                Ok(Landed {
                    previous,
                    landed: mode,
                })
            }
        }
        let mut stack =
            crcbl_store::settings::SettingsStack::from_storage(&crcbl_store::MemoryStorage::new());
        match change(
            &mut stack,
            &format!("{VIDEO_NAMESPACE}.{DISPLAY_MODE_KEY}"),
            &Value::Enum("borderless"),
            &mut Window(DisplayMode::Windowed),
        ) {
            Ok(Change::Pending(pending)) => pending,
            other => panic!("not held: {other:?}"),
        }
    }

    /// **The prompt names what landed and counts down in whole seconds**, the
    /// countdown in the warning tone.
    #[test]
    fn the_prompt_shows_what_landed_and_the_seconds_left() {
        let mut pending = held_borderless();
        let lines = prompt_lines(&pending);
        assert_eq!(lines[0].text, "display mode: borderless");
        assert_eq!(lines[1].text, "Reverting in 15 s");
        assert_eq!(lines[1].tone, CaptionTone::Warning);

        assert!(!pending.tick(std::time::Duration::from_millis(14_500)));
        assert_eq!(prompt_lines(&pending)[1].text, "Reverting in 1 s");
    }

    /// **Holding a change shows the panel with KEEP highlighted — every time,
    /// not only the first — and taking it hides the panel.**
    #[test]
    fn holding_shows_the_panel_and_taking_hides_it() {
        let mut prompt = ConfirmPrompt::new();
        assert!(!prompt.is_showing());
        assert!(prompt.menus().current().is_none(), "an idle prompt drew");

        prompt.hold(held_borderless());
        assert!(prompt.is_showing());
        let menu = prompt.menus().current().expect("the prompt is up");
        assert_eq!(menu.title, PROMPT_TITLE);
        assert_eq!(
            menu.selected_item().map(|item| item.id),
            Some(KEEP_ID),
            "the highlight is not on KEEP",
        );
        assert_eq!(menu.subtitle, prompt_lines(prompt.pending().expect("held")));

        assert!(prompt.take().is_some());
        assert!(prompt.menus().current().is_none(), "a taken prompt drew");

        // A panel keeps its selection between showings, so a prompt left on
        // REVERT would otherwise open the next one there.
        prompt.hold(held_borderless());
        prompt.menus_mut().select_next();
        assert!(prompt.take().is_some());
        prompt.hold(held_borderless());
        assert_eq!(
            prompt
                .menus()
                .current()
                .and_then(Menu::selected_item)
                .map(|item| item.id),
            Some(KEEP_ID),
            "the next prompt opened where the last one was left",
        );
    }

    /// The prompt's ids are the engine's, so no game's button can share one.
    #[test]
    fn the_prompt_ids_are_reserved() {
        for id in [KEEP_ID, REVERT_ID] {
            assert!(id < super::super::FIRST_GAME_ID);
            assert!(
                ![
                    super::super::RESUME_ID,
                    super::super::FULLSCREEN_ID,
                    super::super::DEBUG_OVERLAY_ID
                ]
                .contains(&id),
            );
        }
        assert_ne!(KEEP_ID, REVERT_ID);
    }
}
