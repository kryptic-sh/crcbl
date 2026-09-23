//! `ISteamMatchmaking` lobbies: the group of friends about to play.
//!
//! A [`Lobby`] is the client's membership of one Steam lobby, and leaving is
//! its `Drop`. It is made only by a call's answer —
//! [`LobbyCreated`] from [`Matchmaking::create_lobby`], [`LobbyEntered`] from
//! [`Matchmaking::join_lobby`] — so every lobby this client is in has exactly
//! one owner value. It holds no borrow of [`Steam`]: a game stores it, and
//! passes `&Steam` to each call.
//!
//! While a `Lobby` lives the pump tracks its owner: after every
//! member change and data change it re-reads `GetLobbyOwner`, and queues
//! [`SteamEvent::LobbyOwnerChanged`](crate::SteamEvent::LobbyOwnerChanged)
//! when it moved — Steam passes ownership on by itself when an owner leaves,
//! and sends no callback of its own for it.

use std::{
    marker::PhantomData,
    rc::{Rc, Weak},
    sync::Arc,
};

use crate::{
    SteamId,
    call::{CallRow, SteamCall, private::Answer},
    callbacks::{Base, read},
    client::{Client, Steam},
    error::{EResult, SteamError, c_string},
    ffi::structs,
};

/// The longest lobby data key Steam accepts (`k_nMaxLobbyKeyLength`,
/// `isteammatchmaking.h`).
pub const MAX_LOBBY_KEY_LENGTH: usize = 255;

/// The longest lobby chat message Steam carries — "up to 4k", in
/// `isteammatchmaking.h`'s comment on `SendLobbyChatMsg`.
pub const MAX_LOBBY_CHAT_MESSAGE: usize = 4096;

/// A lobby's id — a `CSteamID` of lobby type, as its 64-bit value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LobbyId(pub u64);

/// Who can find and join a lobby (`ELobbyType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LobbyKind {
    /// Invite only (`k_ELobbyTypePrivate`).
    Private,
    /// Friends and invitees, who may invite their own friends
    /// (`k_ELobbyTypeFriendsOnly`) — what a co-op squad wants.
    FriendsOnly,
    /// Friends, and anyone searching (`k_ELobbyTypePublic`).
    Public,
    /// Found by search, but not shown to friends (`k_ELobbyTypeInvisible`).
    Invisible,
}

impl LobbyKind {
    /// The `ELobbyType` value.
    const fn raw(self) -> i32 {
        match self {
            Self::Private => 0,
            Self::FriendsOnly => 1,
            Self::Public => 2,
            Self::Invisible => 3,
        }
    }
}

/// How a lobby member's membership changed (`EChatMemberStateChange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberChange {
    /// Joined (`k_EChatMemberStateChangeEntered`).
    Entered,
    /// Left (`k_EChatMemberStateChangeLeft`).
    Left,
    /// Dropped without leaving (`k_EChatMemberStateChangeDisconnected`).
    Disconnected,
    /// Kicked (`k_EChatMemberStateChangeKicked`).
    Kicked,
    /// Kicked and banned (`k_EChatMemberStateChangeBanned`).
    Banned,
    /// A flag set this crate does not name, kept rather than guessed at.
    Other(u32),
}

impl MemberChange {
    /// `k_EChatMemberStateChangeEntered`.
    const ENTERED: u32 = 0x0001;
    /// `k_EChatMemberStateChangeLeft`.
    const LEFT: u32 = 0x0002;
    /// `k_EChatMemberStateChangeDisconnected`.
    const DISCONNECTED: u32 = 0x0004;
    /// `k_EChatMemberStateChangeKicked`.
    const KICKED: u32 = 0x0008;
    /// `k_EChatMemberStateChangeBanned`.
    const BANNED: u32 = 0x0010;

    /// Maps `m_rgfChatMemberStateChange`. A ban is also a kick, so the most
    /// severe named flag wins; a value with no named flag, or with one
    /// outside them, is [`Other`](Self::Other).
    #[must_use]
    pub const fn from_flags(flags: u32) -> Self {
        let named = Self::ENTERED | Self::LEFT | Self::DISCONNECTED | Self::KICKED | Self::BANNED;
        if flags & !named != 0 {
            Self::Other(flags)
        } else if flags & Self::BANNED != 0 {
            Self::Banned
        } else if flags & Self::KICKED != 0 {
            Self::Kicked
        } else if flags & Self::DISCONNECTED != 0 {
            Self::Disconnected
        } else if flags & Self::LEFT != 0 {
            Self::Left
        } else if flags & Self::ENTERED != 0 {
            Self::Entered
        } else {
            Self::Other(flags)
        }
    }

    /// Whether the member is gone (Valve's `BChatMemberStateChangeRemoved`).
    #[must_use]
    pub const fn removed(self) -> bool {
        matches!(
            self,
            Self::Left | Self::Disconnected | Self::Kicked | Self::Banned
        )
    }
}

/// Why joining a lobby failed (`EChatRoomEnterResponse`, without its
/// `Success`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnterResponse {
    /// The lobby no longer exists (`k_EChatRoomEnterResponseDoesntExist`).
    DoesntExist,
    /// Not permitted (`k_EChatRoomEnterResponseNotAllowed`).
    NotAllowed,
    /// Full (`k_EChatRoomEnterResponseFull`).
    Full,
    /// An unexpected error (`k_EChatRoomEnterResponseError`).
    Error,
    /// Banned from it (`k_EChatRoomEnterResponseBanned`).
    Banned,
    /// A limited account (`k_EChatRoomEnterResponseLimited`).
    Limited,
    /// A locked clan chat (`k_EChatRoomEnterResponseClanDisabled`).
    ClanDisabled,
    /// A community lock on the account (`k_EChatRoomEnterResponseCommunityBan`).
    CommunityBan,
    /// A member has blocked this user (`k_EChatRoomEnterResponseMemberBlockedYou`).
    MemberBlockedYou,
    /// This user has blocked a member (`k_EChatRoomEnterResponseYouBlockedMember`).
    YouBlockedMember,
    /// Too many join attempts (`k_EChatRoomEnterResponseRatelimitExceeded`).
    RateLimitExceeded,
    /// A value this crate does not name.
    Other(u32),
}

impl EnterResponse {
    /// `k_EChatRoomEnterResponseSuccess`.
    const SUCCESS: u32 = 1;

    /// Maps a failed `EChatRoomEnterResponse`.
    const fn from_raw(raw: u32) -> Self {
        match raw {
            2 => Self::DoesntExist,
            3 => Self::NotAllowed,
            4 => Self::Full,
            5 => Self::Error,
            6 => Self::Banned,
            7 => Self::Limited,
            8 => Self::ClanDisabled,
            9 => Self::CommunityBan,
            10 => Self::MemberBlockedYou,
            11 => Self::YouBlockedMember,
            15 => Self::RateLimitExceeded,
            other => Self::Other(other),
        }
    }
}

/// A lobby the pump is tracking the owner of.
#[derive(Debug)]
pub(crate) struct Tracked {
    pub(crate) id: LobbyId,
    pub(crate) owner: SteamId,
    /// Alive while the [`Lobby`] is.
    pub(crate) alive: Weak<()>,
}

/// This client's membership of one Steam lobby. Dropping it leaves the lobby
/// (`LeaveLobby`), once.
pub struct Lobby {
    id: LobbyId,
    client: Arc<Client>,
    /// The pump's [`Tracked`] entry holds the `Weak`; it also keeps a
    /// `Lobby` `!Send`, so its `Drop` runs on the pump thread.
    _alive: Rc<()>,
    /// Belt and braces for the same: `Arc<Client>` alone would be `Send`.
    _not_send: PhantomData<*const ()>,
}

impl core::fmt::Debug for Lobby {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lobby")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for Lobby {
    fn drop(&mut self) {
        leave(&self.client, self.id);
    }
}

/// `LeaveLobby`.
fn leave(client: &Client, lobby: LobbyId) {
    // SAFETY: `client.matchmaking` is the non-null interface init resolved,
    // and Steam stays initialised while `client` lives.
    unsafe { (client.lib.fns.matchmaking.leave_lobby)(client.matchmaking, lobby.0) };
}

impl Lobby {
    /// Starts owning membership of `id`, and tracking its owner.
    fn joined(steam: &mut Steam, id: LobbyId) -> Self {
        let alive = Rc::new(());
        let owner = owner_of(&steam.client, id);
        steam.lobbies.push(Tracked {
            id,
            owner,
            alive: Rc::downgrade(&alive),
        });
        Self {
            id,
            client: Arc::clone(&steam.client),
            _alive: alive,
            _not_send: PhantomData,
        }
    }

    /// The lobby's id — what an invite, a rich-presence `connect` string or a
    /// `+connect_lobby` argument names.
    #[must_use]
    pub const fn id(&self) -> LobbyId {
        self.id
    }

    /// The lobby's owner now (`GetLobbyOwner`) — the host a joiner connects
    /// to.
    #[must_use]
    pub fn owner(&self, steam: &Steam) -> SteamId {
        owner_of(&steam.client, self.id)
    }

    /// Every member, this client included (`GetNumLobbyMembers`,
    /// `GetLobbyMemberByIndex`).
    #[must_use]
    pub fn members(&self, steam: &Steam) -> Vec<SteamId> {
        members_of(&steam.client, self.id)
    }

    /// The most members the lobby admits (`GetLobbyMemberLimit`).
    #[must_use]
    pub fn member_limit(&self, steam: &Steam) -> i32 {
        let client = &steam.client;
        // SAFETY: see `leave`.
        unsafe {
            (client.lib.fns.matchmaking.get_lobby_member_limit)(client.matchmaking, self.id.0)
        }
    }

    /// Changes the member limit (`SetLobbyMemberLimit`; the owner only).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam says no.
    pub fn set_member_limit(&self, steam: &Steam, limit: i32) -> Result<(), SteamError> {
        let client = &steam.client;
        // SAFETY: see `leave`.
        let done = unsafe {
            (client.lib.fns.matchmaking.set_lobby_member_limit)(
                client.matchmaking,
                self.id.0,
                limit,
            )
        };
        refused_unless(done, "SetLobbyMemberLimit")
    }

    /// Changes who can find the lobby (`SetLobbyType`; the owner only).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam says no.
    pub fn set_kind(&self, steam: &Steam, kind: LobbyKind) -> Result<(), SteamError> {
        let client = &steam.client;
        // SAFETY: see `leave`; the value is a named `ELobbyType`.
        let done = unsafe {
            (client.lib.fns.matchmaking.set_lobby_type)(client.matchmaking, self.id.0, kind.raw())
        };
        refused_unless(done, "SetLobbyType")
    }

    /// Opens or closes the lobby to new members (`SetLobbyJoinable`; the
    /// owner only).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam says no.
    pub fn set_joinable(&self, steam: &Steam, joinable: bool) -> Result<(), SteamError> {
        let client = &steam.client;
        // SAFETY: see `leave`.
        let done = unsafe {
            (client.lib.fns.matchmaking.set_lobby_joinable)(client.matchmaking, self.id.0, joinable)
        };
        refused_unless(done, "SetLobbyJoinable")
    }

    /// Sends `friend` an invite to the lobby (`InviteUserToLobby`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam says no.
    pub fn invite(&self, steam: &Steam, friend: SteamId) -> Result<(), SteamError> {
        let client = &steam.client;
        // SAFETY: see `leave`.
        let done = unsafe {
            (client.lib.fns.matchmaking.invite_user_to_lobby)(
                client.matchmaking,
                self.id.0,
                friend.0,
            )
        };
        refused_unless(done, "InviteUserToLobby")
    }

    /// A lobby data value (`GetLobbyData`); empty when the key is unset.
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] for a key Steam
    /// could not hold.
    pub fn data(&self, steam: &Steam, key: &str) -> Result<String, SteamError> {
        let key = c_string(key, "lobby data key", MAX_LOBBY_KEY_LENGTH)?;
        let client = &steam.client;
        // SAFETY: see `leave`; `key` is NUL-terminated and outlives the call,
        // and the returned string is copied before any other Steam call.
        Ok(unsafe {
            let value = (client.lib.fns.matchmaking.get_lobby_data)(
                client.matchmaking,
                self.id.0,
                key.as_ptr(),
            );
            steam.copy_string(value)
        })
    }

    /// Sets a lobby data value (`SetLobbyData`; the owner only). Every member
    /// sees [`SteamEvent::LobbyDataChanged`](crate::SteamEvent::LobbyDataChanged).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] for a key Steam
    /// could not hold, a NUL in `value`, or [`SteamError::Refused`].
    pub fn set_data(&self, steam: &Steam, key: &str, value: &str) -> Result<(), SteamError> {
        let key = c_string(key, "lobby data key", MAX_LOBBY_KEY_LENGTH)?;
        let value = c_string(value, "lobby data value", usize::MAX)?;
        let client = &steam.client;
        // SAFETY: see `leave`; both strings are NUL-terminated and outlive the
        // call.
        let done = unsafe {
            (client.lib.fns.matchmaking.set_lobby_data)(
                client.matchmaking,
                self.id.0,
                key.as_ptr(),
                value.as_ptr(),
            )
        };
        refused_unless(done, "SetLobbyData")
    }

    /// A member's data value (`GetLobbyMemberData`); empty when unset.
    ///
    /// # Errors
    ///
    /// As [`data`](Self::data).
    pub fn member_data(
        &self,
        steam: &Steam,
        member: SteamId,
        key: &str,
    ) -> Result<String, SteamError> {
        let key = c_string(key, "lobby member data key", MAX_LOBBY_KEY_LENGTH)?;
        let client = &steam.client;
        // SAFETY: as in `data`.
        Ok(unsafe {
            let value = (client.lib.fns.matchmaking.get_lobby_member_data)(
                client.matchmaking,
                self.id.0,
                member.0,
                key.as_ptr(),
            );
            steam.copy_string(value)
        })
    }

    /// Sets this client's own member data (`SetLobbyMemberData`). Steam
    /// reports no failure for it.
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] for an argument
    /// Steam could not hold.
    pub fn set_member_data(&self, steam: &Steam, key: &str, value: &str) -> Result<(), SteamError> {
        let key = c_string(key, "lobby member data key", MAX_LOBBY_KEY_LENGTH)?;
        let value = c_string(value, "lobby member data value", usize::MAX)?;
        let client = &steam.client;
        // SAFETY: as in `set_data`.
        unsafe {
            (client.lib.fns.matchmaking.set_lobby_member_data)(
                client.matchmaking,
                self.id.0,
                key.as_ptr(),
                value.as_ptr(),
            );
        }
        Ok(())
    }

    /// Broadcasts a chat message to every member, this client included
    /// (`SendLobbyChatMsg`); each receives
    /// [`SteamEvent::LobbyChatMessage`](crate::SteamEvent::LobbyChatMessage).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooLong`] over [`MAX_LOBBY_CHAT_MESSAGE`], or
    /// [`SteamError::Refused`].
    pub fn send_chat(&self, steam: &Steam, body: &[u8]) -> Result<(), SteamError> {
        if body.len() > MAX_LOBBY_CHAT_MESSAGE {
            return Err(SteamError::TooLong {
                argument: "lobby chat message",
                len: body.len(),
                max: MAX_LOBBY_CHAT_MESSAGE,
            });
        }
        let len = i32::try_from(body.len()).map_err(|_| SteamError::Refused("SendLobbyChatMsg"))?;
        let client = &steam.client;
        // SAFETY: see `leave`; `body` is `len` readable bytes for the call.
        let done = unsafe {
            (client.lib.fns.matchmaking.send_lobby_chat_msg)(
                client.matchmaking,
                self.id.0,
                body.as_ptr().cast(),
                len,
            )
        };
        refused_unless(done, "SendLobbyChatMsg")
    }
}

/// `GetNumLobbyMembers` and `GetLobbyMemberByIndex`.
pub(crate) fn members_of(client: &Client, lobby: LobbyId) -> Vec<SteamId> {
    let fns = &client.lib.fns.matchmaking;
    // SAFETY: see `leave`; callers are on the pump thread; the index stays
    // below the count Steam gave.
    unsafe {
        let count = (fns.get_num_lobby_members)(client.matchmaking, lobby.0);
        (0..count.max(0))
            .map(|index| {
                SteamId((fns.get_lobby_member_by_index)(
                    client.matchmaking,
                    lobby.0,
                    index,
                ))
            })
            .collect()
    }
}

/// `GetLobbyOwner`.
fn owner_of(client: &Client, lobby: LobbyId) -> SteamId {
    // SAFETY: see `leave`.
    SteamId(unsafe { (client.lib.fns.matchmaking.get_lobby_owner)(client.matchmaking, lobby.0) })
}

/// `Ok` for Steam's `true`, [`SteamError::Refused`] naming `call` for `false`.
pub(crate) const fn refused_unless(done: bool, call: &'static str) -> Result<(), SteamError> {
    if done {
        Ok(())
    } else {
        Err(SteamError::Refused(call))
    }
}

impl Steam {
    /// Lobbies: create one, join one.
    pub fn matchmaking(&mut self) -> Matchmaking<'_> {
        Matchmaking { steam: self }
    }

    /// Re-reads the owner of a tracked lobby, queueing
    /// [`SteamEvent::LobbyOwnerChanged`](crate::SteamEvent::LobbyOwnerChanged)
    /// if it moved; forgets lobbies whose [`Lobby`] is gone.
    pub(crate) fn recheck_owner(&mut self, lobby: LobbyId) {
        self.lobbies
            .retain(|tracked| tracked.alive.strong_count() > 0);
        let client = Arc::clone(&self.client);
        let Some(tracked) = self.lobbies.iter_mut().find(|tracked| tracked.id == lobby) else {
            return;
        };
        let owner = owner_of(&client, lobby);
        if owner != tracked.owner {
            tracked.owner = owner;
            self.queue
                .push_back(crate::SteamEvent::LobbyOwnerChanged { lobby, owner });
        }
    }
}

/// `ISteamMatchmaking`, borrowed from a [`Steam`]; from [`Steam::matchmaking`].
#[derive(Debug)]
pub struct Matchmaking<'a> {
    steam: &'a mut Steam,
}

impl Matchmaking<'_> {
    /// Creates a lobby and enters it (`CreateLobby`). The answer, a
    /// [`LobbyCreated`], arrives at a later [`pump`](Steam::pump).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn create_lobby(
        &mut self,
        kind: LobbyKind,
        max_members: i32,
    ) -> Result<SteamCall<LobbyCreated>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: see `leave`; the kind is a named `ELobbyType`.
        let handle = unsafe {
            (client.lib.fns.matchmaking.create_lobby)(client.matchmaking, kind.raw(), max_members)
        };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("CreateLobby"))
    }

    /// Joins a lobby (`JoinLobby`) — one named by
    /// [`SteamEvent::LobbyJoinRequested`](crate::SteamEvent::LobbyJoinRequested)
    /// or a `+connect_lobby` launch argument. The answer, a [`LobbyEntered`],
    /// arrives at a later [`pump`](Steam::pump).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn join_lobby(&mut self, lobby: LobbyId) -> Result<SteamCall<LobbyEntered>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: see `leave`.
        let handle =
            unsafe { (client.lib.fns.matchmaking.join_lobby)(client.matchmaking, lobby.0) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("JoinLobby"))
    }
}

/// The answer to [`Matchmaking::create_lobby`] (`LobbyCreated_t`).
#[derive(Debug)]
pub struct LobbyCreated {
    result: Result<Lobby, SteamError>,
}

impl LobbyCreated {
    /// The lobby, entered and owned by this client — or Steam's `EResult` for
    /// why not.
    ///
    /// # Errors
    ///
    /// [`SteamError::Result`] with Steam's reason: no connection, a timeout,
    /// access denied, too many lobbies.
    pub fn lobby(self) -> Result<Lobby, SteamError> {
        self.result
    }
}

impl Answer for LobbyCreated {
    const ROW: CallRow = CallRow {
        base: Base::Matchmaking,
        offset: 13,
        #[cfg(test)]
        name: "LobbyCreated_t",
        size: size_of::<structs::LobbyCreated>(),
    };

    fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self> {
        let raw = read::<structs::LobbyCreated>(bytes)?;
        let result = EResult(raw.result);
        Some(Self {
            result: if result == EResult::OK {
                Ok(Lobby::joined(steam, LobbyId(raw.lobby)))
            } else {
                Err(SteamError::Result(result))
            },
        })
    }

    fn abandon(bytes: &[u8], client: &Client) {
        if let Some(raw) = read::<structs::LobbyCreated>(bytes)
            && EResult(raw.result) == EResult::OK
        {
            leave(client, LobbyId(raw.lobby));
        }
    }
}

/// The answer to [`Matchmaking::join_lobby`] (`LobbyEnter_t`).
#[derive(Debug)]
pub struct LobbyEntered {
    result: Result<Lobby, SteamError>,
    locked: bool,
}

impl LobbyEntered {
    /// The lobby, entered — or why not.
    ///
    /// # Errors
    ///
    /// [`SteamError::LobbyEnter`] with Steam's `EChatRoomEnterResponse`;
    /// [`SteamError::AlreadyInLobby`] when a [`Lobby`] for it is already held.
    pub fn lobby(self) -> Result<Lobby, SteamError> {
        self.result
    }

    /// Whether only invitees may join (`m_bLocked`).
    #[must_use]
    pub const fn locked(&self) -> bool {
        self.locked
    }
}

impl Answer for LobbyEntered {
    const ROW: CallRow = CallRow {
        base: Base::Matchmaking,
        offset: 4,
        #[cfg(test)]
        name: "LobbyEnter_t",
        size: size_of::<structs::LobbyEnter>(),
    };

    fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self> {
        let raw = read::<structs::LobbyEnter>(bytes)?;
        let id = LobbyId(raw.lobby);
        let held = steam
            .lobbies
            .iter()
            .any(|tracked| tracked.id == id && tracked.alive.strong_count() > 0);
        Some(Self {
            result: if raw.response != EnterResponse::SUCCESS {
                Err(SteamError::LobbyEnter(EnterResponse::from_raw(
                    raw.response,
                )))
            } else if held {
                Err(SteamError::AlreadyInLobby(id))
            } else {
                Ok(Lobby::joined(steam, id))
            },
            locked: raw.locked != 0,
        })
    }

    fn abandon(bytes: &[u8], client: &Client) {
        if let Some(raw) = read::<structs::LobbyEnter>(bytes)
            && raw.response == EnterResponse::SUCCESS
        {
            leave(client, LobbyId(raw.lobby));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppId, CallState, SteamEvent,
        client::init_on,
        testing::{self, FakeMsg, completion, lobby_enter, script},
    };

    /// The lobby every test joins.
    const LOBBY: u64 = 0x0186_0000_0000_0042;
    /// Another member.
    const FRIEND: u64 = 76_561_197_960_287_931;

    /// A `Steam` that has joined [`LOBBY`], owned by `owner`.
    fn joined(owner: u64) -> (Steam, Lobby) {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| {
            s.next_call = 88;
            s.lobby_owner = owner;
            s.results.push((88, lobby_enter(LOBBY, 1, false), false));
        });
        let call = steam.matchmaking().join_lobby(LobbyId(LOBBY)).unwrap();
        let row = LobbyEntered::ROW;
        script(|s| s.queue.push_back(completion(88, row.id(), row.size)));
        steam.pump();
        let CallState::Ready(entered) = steam.take(call) else {
            panic!("not answered");
        };
        assert!(!entered.locked());
        (steam, entered.lobby().unwrap())
    }

    /// A `LobbyChatUpdate_t` on the pipe.
    fn member_change(member: u64, by: u64, flags: u32) -> FakeMsg {
        use crate::ffi::structs::LobbyChatUpdate as Raw;
        FakeMsg::payload(
            506,
            testing::payload::<Raw>(&[
                (core::mem::offset_of!(Raw, lobby), &LOBBY.to_le_bytes()),
                (core::mem::offset_of!(Raw, changed), &member.to_le_bytes()),
                (core::mem::offset_of!(Raw, making_change), &by.to_le_bytes()),
                (
                    core::mem::offset_of!(Raw, state_change),
                    &flags.to_le_bytes(),
                ),
            ]),
        )
    }

    #[test]
    fn member_flags_map_to_the_most_severe_change() {
        assert_eq!(MemberChange::from_flags(0x1), MemberChange::Entered);
        assert_eq!(MemberChange::from_flags(0x2), MemberChange::Left);
        assert_eq!(MemberChange::from_flags(0x4), MemberChange::Disconnected);
        assert_eq!(MemberChange::from_flags(0x8), MemberChange::Kicked);
        assert_eq!(MemberChange::from_flags(0x8 | 0x10), MemberChange::Banned);
        assert_eq!(MemberChange::from_flags(0), MemberChange::Other(0));
        assert_eq!(
            MemberChange::from_flags(0x20 | 0x1),
            MemberChange::Other(0x21)
        );
        assert!(!MemberChange::Entered.removed());
        assert!(MemberChange::Disconnected.removed());
    }

    #[test]
    fn a_joined_lobby_is_left_exactly_once_when_dropped() {
        let (steam, lobby) = joined(testing::STEAM_ID);
        assert_eq!(script(|s| s.joined.clone()), [LOBBY]);
        assert!(script(|s| s.left.is_empty()));
        drop(lobby);
        assert_eq!(script(|s| s.left.clone()), [LOBBY]);
        drop(steam);
        assert_eq!(
            script(|s| s.left.clone()),
            [LOBBY],
            "and not again at shutdown"
        );
    }

    /// Steam answers a join of a lobby this client is already in with
    /// success; a second owner value would leave it when either dropped.
    #[test]
    fn joining_a_lobby_already_held_leaves_the_one_owner_in_charge() {
        let (mut steam, lobby) = joined(testing::STEAM_ID);
        let again = testing::joined_lobby_answer(&mut steam, LOBBY);
        assert_eq!(
            again.lobby().unwrap_err(),
            SteamError::AlreadyInLobby(LobbyId(LOBBY))
        );
        assert!(script(|s| s.left.is_empty()), "the lobby is still held");
        drop(lobby);
        assert_eq!(script(|s| s.left.clone()), [LOBBY]);
    }

    #[test]
    fn a_refused_join_is_its_enter_response_and_holds_nothing() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| {
            s.next_call = 88;
            s.results.push((88, lobby_enter(LOBBY, 4, true), false));
        });
        let call = steam.matchmaking().join_lobby(LobbyId(LOBBY)).unwrap();
        let row = LobbyEntered::ROW;
        script(|s| s.queue.push_back(completion(88, row.id(), row.size)));
        steam.pump();
        let CallState::Ready(entered) = steam.take(call) else {
            panic!("not answered");
        };
        assert!(entered.locked());
        assert_eq!(
            entered.lobby().unwrap_err(),
            SteamError::LobbyEnter(EnterResponse::Full)
        );
        assert!(script(|s| s.left.is_empty()));
        assert!(steam.lobbies.is_empty(), "nothing is tracked");
    }

    #[test]
    fn enter_responses_map_and_keep_unknown_values() {
        assert_eq!(EnterResponse::from_raw(2), EnterResponse::DoesntExist);
        assert_eq!(EnterResponse::from_raw(11), EnterResponse::YouBlockedMember);
        assert_eq!(
            EnterResponse::from_raw(15),
            EnterResponse::RateLimitExceeded
        );
        assert_eq!(EnterResponse::from_raw(12), EnterResponse::Other(12));
    }

    #[test]
    fn the_owner_leaving_is_a_member_change_then_an_owner_change() {
        let (mut steam, lobby) = joined(FRIEND);
        assert_eq!(lobby.owner(&steam), SteamId(FRIEND));
        // Steam passes ownership on before it reports the old owner gone.
        script(|s| {
            s.lobby_owner = testing::STEAM_ID;
            s.queue.push_back(member_change(FRIEND, FRIEND, 0x2));
        });
        steam.pump();
        assert_eq!(
            steam.events().collect::<Vec<_>>(),
            [
                SteamEvent::LobbyMemberChanged {
                    lobby: LobbyId(LOBBY),
                    member: SteamId(FRIEND),
                    change: MemberChange::Left,
                    by: SteamId(FRIEND),
                },
                SteamEvent::LobbyOwnerChanged {
                    lobby: LobbyId(LOBBY),
                    owner: SteamId(testing::STEAM_ID),
                },
            ]
        );
        // Another change with the owner unmoved reports no owner change.
        script(|s| s.queue.push_back(member_change(FRIEND, FRIEND, 0x1)));
        steam.pump();
        assert_eq!(steam.events().count(), 1);
    }

    #[test]
    fn a_dropped_lobby_is_no_longer_tracked() {
        let (mut steam, lobby) = joined(FRIEND);
        drop(lobby);
        script(|s| {
            s.lobby_owner = testing::STEAM_ID;
            s.queue.push_back(member_change(FRIEND, FRIEND, 0x2));
        });
        steam.pump();
        assert!(
            steam
                .events()
                .all(|event| !matches!(event, SteamEvent::LobbyOwnerChanged { .. })),
            "an owner change for a lobby nobody holds"
        );
        assert!(steam.lobbies.is_empty());
    }

    #[test]
    fn a_data_change_also_rechecks_the_owner() {
        use crate::ffi::structs::LobbyDataUpdate as Raw;
        let (mut steam, _lobby) = joined(FRIEND);
        let update = testing::payload::<Raw>(&[
            (core::mem::offset_of!(Raw, lobby), &LOBBY.to_le_bytes()),
            (core::mem::offset_of!(Raw, member), &LOBBY.to_le_bytes()),
            (core::mem::offset_of!(Raw, success), &[1]),
        ]);
        script(|s| {
            s.lobby_owner = testing::STEAM_ID;
            s.queue.push_back(FakeMsg::payload(505, update));
        });
        steam.pump();
        let events: Vec<_> = steam.events().collect();
        assert!(
            events.contains(&SteamEvent::LobbyOwnerChanged {
                lobby: LobbyId(LOBBY),
                owner: SteamId(testing::STEAM_ID),
            }),
            "{events:?}"
        );
    }

    #[test]
    fn a_chat_message_is_read_out_of_steam_and_queued() {
        use crate::ffi::structs::LobbyChatMsg as Raw;
        let (mut steam, _lobby) = joined(FRIEND);
        let message = testing::payload::<Raw>(&[
            (core::mem::offset_of!(Raw, lobby), &LOBBY.to_le_bytes()),
            (core::mem::offset_of!(Raw, user), &FRIEND.to_le_bytes()),
            (core::mem::offset_of!(Raw, chat_id), &3u32.to_le_bytes()),
        ]);
        script(|s| {
            s.chat_entry = (FRIEND, 1, b"ready?".to_vec(), 6);
            s.queue.push_back(FakeMsg::payload(507, message.clone()));
        });
        steam.pump();
        assert_eq!(
            steam.events().collect::<Vec<_>>(),
            [SteamEvent::LobbyChatMessage {
                lobby: LobbyId(LOBBY),
                sender: SteamId(FRIEND),
                kind: 1,
                body: b"ready?".to_vec(),
            }]
        );
        // A count Steam could not have written is counted, not trusted.
        for count in [-1, i32::try_from(MAX_LOBBY_CHAT_MESSAGE).unwrap() + 1] {
            script(|s| {
                s.chat_entry.3 = count;
                s.queue.push_back(FakeMsg::payload(507, message.clone()));
            });
            steam.pump();
            assert_eq!(steam.events().count(), 0, "{count}");
        }
        assert_eq!(steam.diagnostics().decode_mismatches, 2);
    }

    #[test]
    fn lobby_calls_reach_steam_with_their_arguments() {
        let (steam, lobby) = joined(testing::STEAM_ID);
        script(|s| {
            s.members = vec![testing::STEAM_ID, FRIEND];
            s.member_limit = 4;
        });
        assert_eq!(
            lobby.members(&steam),
            [SteamId(testing::STEAM_ID), SteamId(FRIEND)]
        );
        assert_eq!(lobby.member_limit(&steam), 4);
        lobby.set_data(&steam, "mode", "coop").unwrap();
        lobby.set_member_data(&steam, "ready", "1").unwrap();
        lobby.invite(&steam, SteamId(FRIEND)).unwrap();
        lobby.set_joinable(&steam, false).unwrap();
        lobby.set_kind(&steam, LobbyKind::Private).unwrap();
        lobby.set_member_limit(&steam, 3).unwrap();
        lobby.send_chat(&steam, b"hi").unwrap();
        let expected = [
            ("SetLobbyData", "mode", "coop"),
            ("SetLobbyMemberData", "ready", "1"),
            ("InviteUserToLobby", "76561197960287931", ""),
            ("SetLobbyJoinable", "false", ""),
            ("SetLobbyType", "0", ""),
            ("SetLobbyMemberLimit", "3", ""),
        ];
        let writes = script(|s| s.lobby_writes.clone());
        assert_eq!(writes.len(), expected.len());
        for ((call, id, key, value), (want_call, want_key, want_value)) in
            writes.iter().zip(expected)
        {
            assert_eq!(
                (*call, *id, key.as_str(), value.as_str()),
                (want_call, LOBBY, want_key, want_value)
            );
        }
        assert_eq!(script(|s| s.chat_sent.clone()), [b"hi".to_vec()]);
        script(|s| s.set_string(b"coop"));
        assert_eq!(lobby.data(&steam, "mode").unwrap(), "coop");
        assert_eq!(
            lobby.member_data(&steam, SteamId(FRIEND), "ready").unwrap(),
            "coop"
        );
    }

    #[test]
    fn a_refusal_is_an_error_naming_the_call() {
        let (steam, lobby) = joined(testing::STEAM_ID);
        script(|s| s.refuse = true);
        assert_eq!(
            lobby.set_data(&steam, "k", "v"),
            Err(SteamError::Refused("SetLobbyData"))
        );
        assert_eq!(
            lobby.invite(&steam, SteamId(FRIEND)),
            Err(SteamError::Refused("InviteUserToLobby"))
        );
        assert_eq!(
            lobby.send_chat(&steam, b"x"),
            Err(SteamError::Refused("SendLobbyChatMsg"))
        );
    }

    #[test]
    fn oversized_keys_and_messages_are_refused_before_the_call() {
        let (steam, lobby) = joined(testing::STEAM_ID);
        // `k_nMaxLobbyKeyLength` and the header's "up to 4k", as numbers.
        let key = "k".repeat(256);
        assert_eq!(
            lobby.set_data(&steam, &key, "v"),
            Err(SteamError::TooLong {
                argument: "lobby data key",
                len: 256,
                max: 255,
            })
        );
        assert_eq!(
            lobby.send_chat(&steam, &[0; 4097]),
            Err(SteamError::TooLong {
                argument: "lobby chat message",
                len: 4097,
                max: 4096,
            })
        );
        assert_eq!(
            lobby.set_data(&steam, "k\0", "v"),
            Err(SteamError::InteriorNul("lobby data key"))
        );
        assert!(
            script(|s| s.lobby_writes.is_empty() && s.chat_sent.is_empty()),
            "nothing refused reached Steam"
        );
        lobby
            .send_chat(&steam, &[0; MAX_LOBBY_CHAT_MESSAGE])
            .unwrap();
    }

    #[test]
    fn the_invite_dialog_and_game_invites_reach_steam() {
        let (steam, lobby) = joined(testing::STEAM_ID);
        steam.friends().open_invite_dialog(lobby.id());
        steam
            .friends()
            .invite_to_game(SteamId(FRIEND), "+connect_lobby 42")
            .unwrap();
        assert_eq!(script(|s| s.invite_dialogs.clone()), [LOBBY]);
        assert_eq!(
            script(|s| s.game_invites.clone()),
            [(FRIEND, "+connect_lobby 42".to_owned())]
        );
        script(|s| s.refuse = true);
        assert_eq!(
            steam.friends().invite_to_game(SteamId(FRIEND), "x"),
            Err(SteamError::Refused("InviteUserToGame"))
        );
    }
}
