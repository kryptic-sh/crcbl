//! Steam in the sandbox, behind its `steam` feature.
//!
//! `docs/plan/42-steam.md` slice 1b: on a windowed run, initialise Steam under
//! Valve's shared test app 480, log who is playing, pump once a frame, log the
//! overlay, and hand an opened overlay to the loop as a focus loss — which
//! pauses and releases held input exactly as alt-tab does. Without Steam, the
//! reason is logged once and the sandbox runs on.
//!
//! Live only with the feature on and where `crcbl-steam` has items (64-bit
//! Linux, Windows and macOS). Everywhere else [`SteamLink`] is inert — it
//! never starts, pumps nothing and reports no overlay — so the rest of the
//! sandbox names it without asking. The workspace's `wasm32` sweep builds this
//! crate with every feature on, and there `crcbl-steam` is documentation alone,
//! which is why the target is asked here at all; `docs/plan/42-steam.md` slice
//! 8's loop limb is what takes the question away from games.

pub use imp::SteamLink;

#[cfg(all(
    feature = "steam",
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod imp {
    use crcbl::steam::{AppId, Steam, SteamEvent};

    /// Valve's shared SpaceWar test app, which every Steamworks developer may
    /// run under. Development launches need `steam_appid.txt` containing it in
    /// the working directory.
    const SPACEWAR: AppId = AppId(480);

    /// The sandbox's Steam session, or the lack of one.
    #[derive(Debug)]
    pub struct SteamLink {
        /// `None` on a headless run and whenever init failed.
        steam: Option<Steam>,
        /// The overlay opened since the loop last asked.
        overlay_opened: bool,
    }

    impl SteamLink {
        /// No session: what a headless run keeps.
        pub const fn off() -> Self {
            Self {
                steam: None,
                overlay_opened: false,
            }
        }

        /// Initialises Steam, logging who is playing — or, if it cannot, why,
        /// and carrying on without it.
        pub fn start() -> Self {
            let steam = match Steam::init(SPACEWAR) {
                Ok(steam) => {
                    crcbl::log::info!(
                        "steam: signed in as {:?} ({:?}, level {}), app {:?}, hardware {:?}, \
                         language {:?}, overlay enabled {}",
                        steam.friends().persona_name(),
                        steam.user().steam_id(),
                        steam.user().steam_level(),
                        steam.utils().app_id(),
                        steam.utils().steam_hardware(),
                        steam.apps().game_language(),
                        steam.utils().overlay_enabled(),
                    );
                    Some(steam)
                }
                Err(error) => {
                    crcbl::log::warn!("steam: running without it: {error}");
                    None
                }
            };
            Self {
                steam,
                overlay_opened: false,
            }
        }

        /// Drains Steam's callbacks, once a frame.
        pub fn pump(&mut self) {
            let Some(steam) = &mut self.steam else {
                return;
            };
            steam.pump();
            for event in steam.events() {
                match event {
                    SteamEvent::OverlayActivated { active } => {
                        crcbl::log::info!(
                            "steam: overlay {}",
                            if active { "opened" } else { "closed" }
                        );
                        self.overlay_opened |= active;
                    }
                    other => crcbl::log::debug!("steam: {other:?}"),
                }
            }
        }

        /// Whether the overlay opened since the last call.
        pub fn take_overlay_opened(&mut self) -> bool {
            std::mem::take(&mut self.overlay_opened)
        }
    }

    impl Drop for SteamLink {
        /// Logs what the pump saw over the run, which is the first place a
        /// declaration that drifted from the SDK would show.
        fn drop(&mut self) {
            if let Some(steam) = &self.steam {
                crcbl::log::info!("steam: {:?}", steam.diagnostics());
            }
        }
    }
}

#[cfg(not(all(
    feature = "steam",
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
)))]
mod imp {
    /// No Steam: the feature is off, or `crcbl-steam` has no items on this
    /// target. Never starts, pumps nothing, reports no overlay.
    #[derive(Debug)]
    pub struct SteamLink;

    impl SteamLink {
        /// No session.
        pub const fn off() -> Self {
            Self
        }

        /// Nothing to start.
        pub fn start() -> Self {
            Self
        }

        /// Nothing to pump.
        pub fn pump(&mut self) {}

        /// No overlay without Steam.
        pub fn take_overlay_opened(&mut self) -> bool {
            false
        }
    }
}
