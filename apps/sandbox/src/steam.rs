//! Steam in the sandbox, behind its `steam` feature.
//!
//! Steamworks slices 1b, 3a, 3b, 4, 7b and 8: on a windowed run,
//! initialise Steam under Valve's shared test app 480, log who is playing, and
//! lend the session to the loop, which pumps it once a frame, takes an opened
//! overlay as a focus loss — pausing and releasing held input exactly as
//! alt-tab does — and hands every event back through `HostedGame::steam_event`.
//! Steam Input is opened too, with the pad manifest written beside the
//! executable, and becomes the loop's pad source (XInput beside it skipping
//! Steam's virtual pads). Without Steam, the reason is logged once and the
//! sandbox runs on.
//!
//! The lobby half is driven from the keyboard and shown in the F3 debug
//! panel's "steam" section:
//!
//! - **F5** creates a friends-only lobby for four and sets the rich-presence
//!   `connect` key, so friends can "Join game" from their list;
//! - **F6** opens the overlay's invite dialog for it;
//! - **F7** leaves it;
//! - **F8** opens the overlay to the first friend's profile (or the player's
//!   own, with no friends);
//! - **F9** opens the overlay's browser at the Steamworks documentation.
//!
//! The panel also shows the friends-list size and whether the player's own
//! medium avatar has loaded — slice 3b's friends list and avatars.
//!
//! And slice 4's connection: the player who created the lobby listens on it,
//! and one who joined connects to its owner. Each side sends a greeting over
//! the new `SteamTransport` and logs what arrives, and a closed connection
//! is logged with its `EndReason` — `HostLeft` when the owner leaves.
//!
//! Every join path joins by itself: an accepted invite or "Join game" while
//! running (`LobbyJoinRequested`, `RichPresenceJoinRequested`), a launch
//! carrying `+connect_lobby <id>` (on the command line, or in
//! `launch_command_line`), and a relaunch while running
//! (`NewLaunchParameters`). Member, owner and chat changes are logged.
//!
//! Live only with the feature on and where `crcbl-steam` has items (64-bit
//! Linux, Windows and macOS). Everywhere else [`SteamLink`] is inert — it
//! never starts, pumps nothing and reports no overlay — so the rest of the
//! sandbox names it without asking. The workspace's `wasm32` sweep builds this
//! crate with every feature on, and there `crcbl-steam` is documentation alone,
//! which is why the target is asked here at all; the loop's two Steam hooks
//! exist only where the question answers yes, so the game's overrides of them
//! ask it too.

pub use imp::SteamLink;

#[cfg(all(
    feature = "steam",
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod imp {
    use crcbl::{
        core::input::KeyCode,
        engine::{PadSource, SteamSource},
        net::{Message, Transport, TransportError},
        steam::{
            AppId, AvatarSize, CallState, FriendFlags, Lobby, LobbyCreated, LobbyEntered, LobbyId,
            LobbyKind, PAD_MANIFEST, PAD_MANIFEST_FILE, Steam, SteamCall, SteamEvent,
            SteamListener, SteamPads, SteamTransport, UserDialog, VirtualPort, WebPageMode,
            connect_lobby,
        },
        ui::{DebugModule, DebugPanel, DebugSection},
    };

    /// Valve's shared SpaceWar test app, which every Steamworks developer may
    /// run under. Development launches need `steam_appid.txt` containing it in
    /// the working directory.
    const SPACEWAR: AppId = AppId(480);

    /// The squad size the F5 lobby admits: the host and three friends.
    const SQUAD: i32 = 4;

    /// What F9 opens in the overlay's browser.
    const STEAMWORKS_DOCS: &str = "https://partner.steamgames.com/doc/home";

    /// The sandbox's Steam session, or the lack of one.
    #[derive(Debug)]
    pub struct SteamLink {
        /// `None` on a headless run and whenever init failed.
        steam: Option<Steam>,
        /// The lobby this client is in.
        lobby: Option<Lobby>,
        /// A `CreateLobby` waiting on its answer.
        creating: Option<SteamCall<LobbyCreated>>,
        /// A `JoinLobby` waiting on its answer.
        joining: Option<SteamCall<LobbyEntered>>,
        /// The listen socket, while this player owns the lobby they created.
        listener: Option<SteamListener>,
        /// Every open connection — to the owner as a joiner, from each joiner
        /// as the owner — and whether this sandbox has greeted it yet.
        links: Vec<(SteamTransport, bool)>,
    }

    impl SteamLink {
        /// No session: what a headless run keeps.
        pub const fn off() -> Self {
            Self {
                steam: None,
                lobby: None,
                creating: None,
                joining: None,
                listener: None,
                links: Vec::new(),
            }
        }

        /// Initialises Steam, logging who is playing — or, if it cannot, why,
        /// and carrying on without it — and joins the lobby a
        /// `+connect_lobby` launch names.
        pub fn start() -> Self {
            let mut link = Self::off();
            let steam = match Steam::init(SPACEWAR) {
                Ok(steam) => steam,
                Err(error) => {
                    crcbl::log::warn!("steam: running without it: {error}");
                    return link;
                }
            };
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
            link.steam = Some(steam);
            let launched = connect_lobby(std::env::args()).or_else(|| link.url_lobby());
            if let Some(lobby) = launched {
                crcbl::log::info!("steam: launched to join {lobby:?}");
                link.join(lobby);
            }
            link
        }

        /// The lobby a `steam://run` launch's command line names, if any.
        fn url_lobby(&self) -> Option<LobbyId> {
            let line = self.steam.as_ref()?.apps().launch_command_line();
            match line {
                Ok(line) => connect_lobby(line.split_whitespace()),
                Err(error) => {
                    crcbl::log::warn!("steam: launch command line unreadable: {error}");
                    None
                }
            }
        }

        /// Leaves any lobby held, and its connections, and asks to join
        /// `lobby`.
        fn join(&mut self, lobby: LobbyId) {
            let Some(steam) = &mut self.steam else {
                return;
            };
            self.listener = None;
            self.links.clear();
            self.lobby = None;
            match steam.matchmaking().join_lobby(lobby) {
                Ok(call) => self.joining = Some(call),
                Err(error) => crcbl::log::warn!("steam: join {lobby:?}: {error}"),
            }
        }

        /// The keys: F5 create, F6 invite, F7 leave, F8 profile, F9 web page.
        pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
            let Some(steam) = &mut self.steam else {
                return;
            };
            if !pressed {
                return;
            }
            match key {
                KeyCode::F5 if self.lobby.is_none() && self.creating.is_none() => {
                    match steam
                        .matchmaking()
                        .create_lobby(LobbyKind::FriendsOnly, SQUAD)
                    {
                        Ok(call) => self.creating = Some(call),
                        Err(error) => crcbl::log::warn!("steam: create lobby: {error}"),
                    }
                }
                KeyCode::F6 => match &self.lobby {
                    Some(lobby) => steam.friends().open_invite_dialog(lobby.id()),
                    None => crcbl::log::info!("steam: F5 creates a lobby to invite to first"),
                },
                KeyCode::F8 => {
                    let friends = steam.friends();
                    let whom = friends
                        .list(FriendFlags::IMMEDIATE)
                        .first()
                        .copied()
                        .unwrap_or_else(|| steam.user().steam_id());
                    friends.open_overlay_to_user(UserDialog::Profile, whom);
                }
                KeyCode::F9 => {
                    if let Err(error) = steam
                        .friends()
                        .open_overlay_to_web_page(STEAMWORKS_DOCS, WebPageMode::Default)
                    {
                        crcbl::log::warn!("steam: overlay web page: {error}");
                    }
                }
                KeyCode::F7 => {
                    if let Some(lobby) = self.lobby.take() {
                        crcbl::log::info!("steam: left {:?}", lobby.id());
                        steam.friends().clear_rich_presence();
                        self.listener = None;
                        self.links.clear();
                    }
                }
                _ => {}
            }
        }

        /// The session, lent to the loop to pump.
        pub fn source(&mut self) -> Option<&mut dyn SteamSource> {
            self.steam
                .as_mut()
                .map(|steam| steam as &mut dyn SteamSource)
        }

        /// Opens Steam Input over a pad manifest written beside the executable,
        /// as a shipped build would carry it, and answers it as the loop's pad
        /// source — or `None`, with the reason logged, and the loop keeps its
        /// own.
        pub fn pad_source(&mut self) -> Option<Box<dyn PadSource>> {
            let steam = self.steam.as_mut()?;
            let manifest = match std::env::current_exe() {
                Ok(exe) => exe.with_file_name(PAD_MANIFEST_FILE),
                Err(error) => {
                    crcbl::log::warn!("steam: no pads: the executable's path: {error}");
                    return None;
                }
            };
            if let Err(error) = std::fs::write(&manifest, PAD_MANIFEST) {
                crcbl::log::warn!("steam: no pads: writing {}: {error}", manifest.display());
                return None;
            }
            match SteamPads::open(steam, &manifest) {
                Ok(pads) => {
                    crcbl::log::info!("steam: Steam Input open over {}", manifest.display());
                    Some(crcbl::engine::steam::steam_input(pads))
                }
                Err(error) => {
                    crcbl::log::warn!("steam: no pads: {error}");
                    None
                }
            }
        }

        /// Redeems pending calls and serves the connections, once a frame —
        /// the loop has pumped by then.
        pub fn frame(&mut self) {
            self.take_calls();
            self.serve_links();
        }

        /// Accepts joiners, and reads every connection, logging what arrives
        /// and why a connection ended.
        fn serve_links(&mut self) {
            let Some(steam) = &self.steam else {
                return;
            };
            if let Some(listener) = &mut self.listener {
                while let Some(peer) = listener.accept(steam) {
                    crcbl::log::info!("steam: {:?} connected", peer.remote());
                    self.links.push((peer, false));
                }
            }
            let me = steam.user().steam_id();
            self.links.retain_mut(|(link, greeted)| {
                // Once per connection; a send Steam will not take yet (the
                // connection still coming up) is tried again next frame.
                if !*greeted {
                    let greeting = format!("hello from {me:?}").into_bytes();
                    match link.send_reliable(Message::reliable(greeting)) {
                        Ok(()) => *greeted = true,
                        Err(TransportError::Backpressure) => {}
                        Err(error) => {
                            crcbl::log::warn!("steam: greeting {:?}: {error}", link.remote());
                            *greeted = true;
                        }
                    }
                }
                drain(link)
            });
        }

        /// Acts on one event the loop's pump handed over. An opened overlay
        /// the loop has already taken as a focus loss; it is only logged here.
        pub fn event(&mut self, event: &SteamEvent) {
            match event {
                SteamEvent::OverlayActivated { active } => {
                    crcbl::log::info!(
                        "steam: overlay {}",
                        if *active { "opened" } else { "closed" }
                    );
                }
                SteamEvent::LobbyJoinRequested { lobby, friend } => {
                    crcbl::log::info!("steam: asked to join {lobby:?} through {friend:?}");
                    self.join(*lobby);
                }
                SteamEvent::RichPresenceJoinRequested { connect, .. } => {
                    match connect_lobby(connect.split_whitespace()) {
                        Some(lobby) => self.join(lobby),
                        None => crcbl::log::warn!("steam: no lobby in connect {connect:?}"),
                    }
                }
                SteamEvent::NewLaunchParameters => {
                    if let Some(lobby) = self.url_lobby() {
                        self.join(lobby);
                    }
                }
                other => crcbl::log::info!("steam: {other:?}"),
            }
        }

        /// Takes the answers to pending create and join calls.
        fn take_calls(&mut self) {
            let Some(steam) = &mut self.steam else {
                return;
            };
            if let Some(call) = self.creating.take() {
                match steam.take(call) {
                    CallState::Pending(call) => self.creating = Some(call),
                    CallState::Ready(created) => match created.lobby() {
                        Ok(lobby) => {
                            let connect = format!("+connect_lobby {}", lobby.id().0);
                            if let Err(error) =
                                steam.friends().set_rich_presence("connect", &connect)
                            {
                                crcbl::log::warn!("steam: rich presence: {error}");
                            }
                            crcbl::log::info!("steam: created {:?}; F6 invites", lobby.id());
                            match SteamListener::open(steam, &lobby, VirtualPort(0)) {
                                Ok(listener) => self.listener = Some(listener),
                                Err(error) => crcbl::log::warn!("steam: listen: {error}"),
                            }
                            self.lobby = Some(lobby);
                        }
                        Err(error) => crcbl::log::warn!("steam: create lobby: {error}"),
                    },
                    CallState::Failed(error) => crcbl::log::warn!("steam: create lobby: {error}"),
                }
            }
            if let Some(call) = self.joining.take() {
                match steam.take(call) {
                    CallState::Pending(call) => self.joining = Some(call),
                    CallState::Ready(entered) => match entered.lobby() {
                        Ok(lobby) => {
                            crcbl::log::info!(
                                "steam: joined {:?}, owner {:?}, members {:?}",
                                lobby.id(),
                                lobby.owner(steam),
                                lobby.members(steam)
                            );
                            let owner = lobby.owner(steam);
                            if owner != steam.user().steam_id() {
                                match SteamTransport::connect(steam, owner, VirtualPort(0)) {
                                    Ok(link) => self.links.push((link, false)),
                                    Err(error) => crcbl::log::warn!("steam: connect: {error}"),
                                }
                            }
                            self.lobby = Some(lobby);
                        }
                        Err(error) => crcbl::log::warn!("steam: join: {error}"),
                    },
                    CallState::Failed(error) => crcbl::log::warn!("steam: join: {error}"),
                }
            }
        }

        /// Adds the "steam" section to the F3 panel — only with a session, so
        /// a headless run's panel is the one it would have without the
        /// feature.
        pub fn debug_sections(&self, panel: &mut DebugPanel) {
            if self.steam.is_some() {
                panel.add(self);
            }
        }
    }

    /// Logs every message waiting on `link`; `false` once it has ended.
    fn drain(link: &mut SteamTransport) -> bool {
        loop {
            match link.recv() {
                Ok(Some(message)) => crcbl::log::info!(
                    "steam: from {:?}: {:?}",
                    link.remote(),
                    String::from_utf8_lossy(&message.payload)
                ),
                Ok(None) => return true,
                Err(TransportError::Disconnected) => {
                    crcbl::log::info!(
                        "steam: {:?} disconnected: {:?}",
                        link.remote(),
                        link.end_reason()
                    );
                    return false;
                }
                Err(error) => {
                    crcbl::log::warn!("steam: {:?}: {error}", link.remote());
                    return false;
                }
            }
        }
    }

    impl DebugModule for SteamLink {
        fn debug_section(&self, out: &mut DebugSection) {
            out.set_title("steam");
            let Some(steam) = &self.steam else {
                out.row_str("session", "none");
                return;
            };
            let me = steam.user().steam_id();
            out.row("me", format_args!("{me:?}"));
            out.row(
                "friends",
                format_args!("{}", steam.friends().list(FriendFlags::IMMEDIATE).len()),
            );
            match steam.friends().avatar(me, AvatarSize::Medium) {
                Ok(Some(avatar)) => {
                    out.row("avatar", format_args!("{}x{}", avatar.width, avatar.height))
                }
                Ok(None) => out.row_str("avatar", "none yet"),
                Err(error) => out.row("avatar", format_args!("{error}")),
            }
            match &self.lobby {
                Some(lobby) => {
                    out.row("lobby", format_args!("{:?}", lobby.id()));
                    out.row("owner", format_args!("{:?}", lobby.owner(steam)));
                    out.row("members", format_args!("{}", lobby.members(steam).len()));
                    out.row("links", format_args!("{}", self.links.len()));
                    out.row_str("keys", "F6 invite, F7 leave");
                }
                None if self.creating.is_some() || self.joining.is_some() => {
                    out.row_str("lobby", "waiting on Steam");
                }
                None => out.row_str("lobby", "none (F5 creates one)"),
            }
            let diagnostics = steam.diagnostics();
            out.row(
                "pump",
                format_args!(
                    "{} callbacks, {} mismatched",
                    diagnostics.callbacks, diagnostics.decode_mismatches
                ),
            );
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
    use crcbl::{core::input::KeyCode, engine::PadSource, ui::DebugPanel};

    /// No Steam: the feature is off, or `crcbl-steam` has no items on this
    /// target. Never starts, opens no pads, adds no panel section.
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

        /// No lobby keys without Steam.
        pub fn key_event(&mut self, _key: KeyCode, _pressed: bool) {}

        /// No Steam Input without Steam.
        pub fn pad_source(&mut self) -> Option<Box<dyn PadSource>> {
            None
        }

        /// No calls to redeem or connections to serve.
        pub fn frame(&mut self) {}

        /// No section without Steam.
        pub fn debug_sections(&self, _panel: &mut DebugPanel) {}
    }
}
