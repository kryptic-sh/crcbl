//! Asynchronous calls: typed tokens, answered at the pump, redeemed by the
//! game.
//!
//! A Steam call that answers later (`CreateLobby`, `JoinLobby`, …) returns a
//! `SteamAPICall_t` handle, and the answer arrives on the pipe as
//! `SteamAPICallCompleted_t`. Registering the handle here is the only way to
//! make a [`SteamCall<T>`]; the registry records the callback id and size the
//! answer must have, and when the completion arrives the pump fetches it with
//! `SteamAPI_ManualDispatch_GetAPICallResult` — **exactly that id and size, or
//! not at all**: a completion that disagrees is [`CallError::Decode`], never a
//! reinterpretation. [`Steam::take`] then hands the game its typed answer, or
//! the token back if the answer has not come.
//!
//! **Dropping a token abandons the answer, never the effect.** A dropped
//! token's completion is counted in
//! [`PumpDiagnostics::unclaimed_completions`](crate::PumpDiagnostics), its
//! result fetched and discarded (as Valve's manual-dispatch template does for
//! every completion), and whatever the call acquired is released — a lobby
//! created or joined through a dropped token is left, so nothing is held that
//! no value owns. The same happens to an answer that arrived and was never
//! taken, once its token is dropped.

use std::{
    collections::HashMap,
    marker::PhantomData,
    rc::{Rc, Weak},
};

use crate::{
    Steam,
    callbacks::Base,
    client::Client,
    ffi::{HSteamPipe, SteamApiCall},
};

/// The largest answer fetched for a completion nobody registered. No call
/// result in the SDK comes near it; a larger claimed size is not trusted
/// enough to allocate for, and the answer is left with Steam.
const MAX_UNCLAIMED_RESULT: usize = 64 * 1024;

/// One call-result struct: its id, written as Valve writes it, and its size.
///
/// Nominally `pub` and never exported, for the reason `Client` is.
#[derive(Debug, Clone, Copy)]
pub struct CallRow {
    /// Valve's base for the id.
    pub(crate) base: Base,
    /// The offset from [`base`](Self::base).
    pub(crate) offset: i32,
    /// The C struct name, for the drift gate.
    #[cfg(test)]
    pub(crate) name: &'static str,
    /// `size_of` the declared struct.
    pub(crate) size: usize,
}

impl CallRow {
    /// The callback id: `base + offset`.
    pub(crate) const fn id(&self) -> i32 {
        self.base as i32 + self.offset
    }
}

/// Every call-result row, for the drift gate.
#[cfg(test)]
pub(crate) const CALL_ROWS: &[CallRow] = &[
    <crate::LobbyCreated as private::Answer>::ROW,
    <crate::LobbyEntered as private::Answer>::ROW,
    <crate::LeaderboardFound as private::Answer>::ROW,
    <crate::Entries as private::Answer>::ROW,
    <crate::ScoreUploaded as private::Answer>::ROW,
    <crate::EventRecording as private::Answer>::ROW,
    <crate::PhaseRecording as private::Answer>::ROW,
];

/// The crate-private half of [`CallResult`]: a sealed supertrait, so the
/// set of answers is this crate's and a game cannot declare one of its own.
pub(crate) mod private {
    use super::CallRow;
    use crate::{Steam, client::Client};

    /// What makes a type an answer.
    pub trait Answer: Sized {
        /// Its id and size.
        const ROW: CallRow;

        /// Builds the answer from exactly [`ROW`](Self::ROW)`.size` bytes —
        /// the registry checked the size — acquiring whatever it owns. `None`
        /// only for bytes of another length.
        fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self>;

        /// Releases what an answer nobody will take acquired: a created or
        /// joined lobby is left.
        fn abandon(bytes: &[u8], client: &Client);
    }
}

/// An answer to an asynchronous Steam call — [`LobbyCreated`](crate::LobbyCreated),
/// [`LobbyEntered`](crate::LobbyEntered), [`LeaderboardFound`](crate::LeaderboardFound),
/// [`ScoreUploaded`](crate::ScoreUploaded), [`Entries`](crate::Entries),
/// [`EventRecording`](crate::EventRecording),
/// [`PhaseRecording`](crate::PhaseRecording).
/// Sealed: the set is this crate's.
pub trait CallResult: private::Answer {}

impl<T: private::Answer> CallResult for T {}

/// A pending asynchronous call whose answer is a `T`; redeem it with
/// [`Steam::take`].
///
/// Only the registry makes one. It is moved into `take` and comes back only
/// while the answer is pending, so it cannot be redeemed twice. Dropping it
/// abandons the answer (see the module docs).
#[must_use = "a dropped token abandons the call's answer"]
#[derive(Debug)]
pub struct SteamCall<T: CallResult> {
    handle: SteamApiCall,
    /// Alive while the token is; the registry holds the `Weak`, and sees a
    /// dropped token by it.
    _alive: Rc<()>,
    answer: PhantomData<fn() -> T>,
}

/// What [`Steam::take`] found.
#[derive(Debug)]
pub enum CallState<T: CallResult> {
    /// Not answered yet: the token comes back, to take again on a later frame.
    Pending(SteamCall<T>),
    /// The answer.
    Ready(T),
    /// The call failed before it produced an answer.
    Failed(CallError),
}

/// Why an asynchronous call produced no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CallError {
    /// `GetAPICallResult` reported an IO failure (`pbFailed`): Steam lost the
    /// call, typically to a dropped connection.
    #[error("the Steam call failed in transit")]
    IoFailure,
    /// `GetAPICallResult` had no answer for the call.
    #[error("Steam had no answer for the call")]
    NoResult,
    /// The completion named a different callback id or size than the call's
    /// answer has — SDK drift. Nothing was fetched.
    #[error(
        "the call's answer is callback {id} of {got} bytes, expected {expected_id} of {expected}"
    )]
    Decode {
        /// The id the answer should have.
        expected_id: i32,
        /// The id the completion named.
        id: i32,
        /// The size the answer should have.
        expected: usize,
        /// The size the completion named.
        got: usize,
    },
    /// The token was never registered with this `Steam` — it came from an
    /// earlier one.
    #[error("the call belongs to another Steam session")]
    NotRegistered,
}

/// One registered call.
#[derive(Debug)]
struct Entry {
    row: CallRow,
    alive: Weak<()>,
    abandon: fn(&[u8], &Client),
    /// `None` until the completion arrives.
    answer: Option<Result<Vec<u8>, CallError>>,
}

/// The calls a `Steam` is waiting on.
#[derive(Debug, Default)]
pub(crate) struct CallRegistry {
    entries: HashMap<SteamApiCall, Entry>,
}

impl CallRegistry {
    /// Registers `handle` as a call answered by a `T`.
    ///
    /// `None` for `k_uAPICallInvalid` (zero): Steam did not start the call.
    pub(crate) fn register<T: CallResult>(&mut self, handle: SteamApiCall) -> Option<SteamCall<T>> {
        if handle == 0 {
            return None;
        }
        let alive = Rc::new(());
        self.entries.insert(
            handle,
            Entry {
                row: T::ROW,
                alive: Rc::downgrade(&alive),
                abandon: T::abandon,
                answer: None,
            },
        );
        Some(SteamCall {
            handle,
            _alive: alive,
            answer: PhantomData,
        })
    }

    /// Takes a completed call's answer bytes out, or hands the token back.
    fn take<T: CallResult>(
        &mut self,
        call: SteamCall<T>,
    ) -> Result<Result<Vec<u8>, CallError>, SteamCall<T>> {
        let Some(entry) = self.entries.get(&call.handle) else {
            return Ok(Err(CallError::NotRegistered));
        };
        if entry.answer.is_none() {
            return Err(call);
        }
        let entry = self.entries.remove(&call.handle);
        Ok(entry
            .and_then(|entry| entry.answer)
            .unwrap_or(Err(CallError::NotRegistered)))
    }

    /// Answers the call a `SteamAPICallCompleted_t` names, fetching its
    /// result through `fetch`. Returns whether a live token was waiting on it.
    pub(crate) fn complete(
        &mut self,
        client: &Client,
        call: SteamApiCall,
        id: i32,
        size: u32,
        mut fetch: impl FnMut(SteamApiCall, usize, i32) -> Result<Vec<u8>, CallError>,
    ) -> bool {
        let got = usize::try_from(size).unwrap_or(usize::MAX);
        let Some(entry) = self.entries.get_mut(&call) else {
            if got <= MAX_UNCLAIMED_RESULT {
                // Discarded, as Valve's template discards what it has no
                // handler for; the outcome is of no one's concern.
                drop(fetch(call, got, id));
            }
            return false;
        };
        let matches = id == entry.row.id() && got == entry.row.size;
        if entry.alive.strong_count() == 0 {
            // The token was dropped while the call was in flight.
            let entry = self.entries.remove(&call);
            if let Some(entry) = entry {
                if matches {
                    if let Ok(bytes) = fetch(call, got, id) {
                        (entry.abandon)(&bytes, client);
                    }
                } else if got <= MAX_UNCLAIMED_RESULT {
                    drop(fetch(call, got, id));
                }
            }
            return false;
        }
        entry.answer = Some(if matches {
            fetch(call, got, id)
        } else {
            Err(CallError::Decode {
                expected_id: entry.row.id(),
                id,
                expected: entry.row.size,
                got,
            })
        });
        true
    }

    /// Drops every answered call whose token is gone, releasing what its
    /// answer acquired. Run at the start of every pump.
    pub(crate) fn prune(&mut self, client: &Client) {
        self.entries.retain(|_, entry| {
            let keep = entry.alive.strong_count() > 0 || entry.answer.is_none();
            if !keep && let Some(Ok(bytes)) = &entry.answer {
                (entry.abandon)(bytes, client);
            }
            keep
        });
    }

    /// How many calls are registered, answered or not.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `SteamAPI_ManualDispatch_GetAPICallResult` for exactly `size` bytes of
/// callback `id`.
pub(crate) fn fetch(
    client: &Client,
    pipe: HSteamPipe,
    call: SteamApiCall,
    size: usize,
    id: i32,
) -> Result<Vec<u8>, CallError> {
    let Ok(len) = i32::try_from(size) else {
        return Err(CallError::NoResult);
    };
    let mut bytes = vec![0_u8; size];
    let mut failed = false;
    // SAFETY: `bytes` is `size` writable bytes, `failed` a writable bool, and
    // this runs on the pump thread while handling the completion — where
    // Valve's header says the call belongs.
    let answered = unsafe {
        (client.lib.fns.dispatch.get_api_call_result)(
            pipe,
            call,
            bytes.as_mut_ptr().cast(),
            len,
            id,
            &raw mut failed,
        )
    };
    if failed {
        Err(CallError::IoFailure)
    } else if !answered {
        Err(CallError::NoResult)
    } else {
        Ok(bytes)
    }
}

impl Steam {
    /// Redeems an asynchronous call: its answer, the token back if Steam has
    /// not answered yet, or why it failed.
    ///
    /// The answer arrives at [`pump`](Self::pump); a game takes each pending
    /// call once a frame after pumping.
    pub fn take<T: CallResult>(&mut self, call: SteamCall<T>) -> CallState<T> {
        match self.calls.take(call) {
            Err(call) => CallState::Pending(call),
            Ok(Err(error)) => CallState::Failed(error),
            Ok(Ok(bytes)) => match T::build(&bytes, self) {
                Some(answer) => CallState::Ready(answer),
                None => CallState::Failed(CallError::Decode {
                    expected_id: T::ROW.id(),
                    id: T::ROW.id(),
                    expected: T::ROW.size,
                    got: bytes.len(),
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppId, LobbyCreated, LobbyId, LobbyKind, SteamError,
        client::init_on,
        testing::{self, completion, lobby_created, script},
    };

    /// `LobbyCreated_t`'s id and this OS's size.
    fn created_row() -> (i32, usize) {
        let row = <LobbyCreated as private::Answer>::ROW;
        (row.id(), row.size)
    }

    /// A `Steam` whose next async call is handle 77.
    fn steam() -> Steam {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.next_call = 77);
        steam
    }

    #[test]
    fn a_registered_call_is_pending_until_its_answer_arrives_then_decodes() {
        let mut steam = steam();
        let (id, size) = created_row();
        script(|s| s.results.push((77, lobby_created(1, 5), false)));
        let call = steam
            .matchmaking()
            .create_lobby(LobbyKind::FriendsOnly, 4)
            .unwrap();
        assert_eq!(script(|s| s.created.clone()), [(1, 4)]);

        let CallState::Pending(call) = steam.take(call) else {
            panic!("answered before any completion");
        };
        script(|s| s.queue.push_back(completion(77, id, size)));
        steam.pump();
        let CallState::Ready(created) = steam.take(call) else {
            panic!("not answered");
        };
        assert_eq!(created.lobby().unwrap().id(), LobbyId(5));
        // Fetched once, with exactly the registered id and size.
        let size = i32::try_from(size).unwrap();
        assert_eq!(script(|s| s.results_asked.clone()), [(77, size, id)]);
        assert_eq!(steam.diagnostics().unclaimed_completions, 0);
        assert_eq!(steam.calls.len(), 0, "a taken call is forgotten");
    }

    #[test]
    fn a_completion_that_disagrees_is_decode_and_nothing_is_fetched() {
        let (id, size) = created_row();
        for (named_id, named_size) in [(id + 1, size), (id, size + 4)] {
            let mut steam = steam();
            let call = steam
                .matchmaking()
                .create_lobby(LobbyKind::Private, 2)
                .unwrap();
            script(|s| s.queue.push_back(completion(77, named_id, named_size)));
            steam.pump();
            match steam.take(call) {
                CallState::Failed(CallError::Decode {
                    expected_id,
                    id: got_id,
                    expected,
                    got,
                }) => {
                    assert_eq!((expected_id, expected), (id, size));
                    assert_eq!((got_id, got), (named_id, named_size));
                }
                other => panic!("expected a decode failure, got {other:?}"),
            }
            assert!(
                script(|s| s.results_asked.is_empty()),
                "fetched with the wrong shape"
            );
        }
    }

    #[test]
    fn an_io_failure_and_a_missing_result_are_their_own_errors() {
        let (id, size) = created_row();
        for (results, expected) in [
            (vec![(77, lobby_created(1, 5), true)], CallError::IoFailure),
            (vec![], CallError::NoResult),
        ] {
            let mut steam = steam();
            script(|s| s.results = results);
            let call = steam
                .matchmaking()
                .create_lobby(LobbyKind::Private, 2)
                .unwrap();
            script(|s| s.queue.push_back(completion(77, id, size)));
            steam.pump();
            match steam.take(call) {
                CallState::Failed(error) => assert_eq!(error, expected),
                other => panic!("expected {expected:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn steams_failure_code_reaches_the_game() {
        let (id, size) = created_row();
        let mut steam = steam();
        script(|s| s.results.push((77, lobby_created(25, 0), false)));
        let call = steam
            .matchmaking()
            .create_lobby(LobbyKind::Private, 2)
            .unwrap();
        script(|s| s.queue.push_back(completion(77, id, size)));
        steam.pump();
        let CallState::Ready(created) = steam.take(call) else {
            panic!("not answered");
        };
        assert_eq!(
            created.lobby().unwrap_err(),
            SteamError::Result(crate::EResult::LIMIT_EXCEEDED)
        );
        assert!(
            script(|s| s.left.is_empty()),
            "no lobby was joined, so none is left"
        );
    }

    #[test]
    fn an_invalid_handle_is_refused_and_registers_nothing() {
        let mut steam = steam();
        script(|s| s.next_call = 0);
        assert_eq!(
            steam
                .matchmaking()
                .create_lobby(LobbyKind::Private, 2)
                .unwrap_err(),
            SteamError::Refused("CreateLobby")
        );
        assert_eq!(steam.calls.len(), 0);
    }

    #[test]
    fn a_dropped_tokens_completion_is_counted_and_its_lobby_left() {
        let (id, size) = created_row();
        let mut steam = steam();
        script(|s| s.results.push((77, lobby_created(1, 5), false)));
        drop(
            steam
                .matchmaking()
                .create_lobby(LobbyKind::Private, 2)
                .unwrap(),
        );
        script(|s| s.queue.push_back(completion(77, id, size)));
        steam.pump();
        assert_eq!(steam.diagnostics().unclaimed_completions, 1);
        assert_eq!(
            script(|s| s.left.clone()),
            [5],
            "the abandoned lobby is left"
        );
        assert_eq!(steam.calls.len(), 0);
    }

    #[test]
    fn an_answer_never_taken_is_released_when_its_token_drops() {
        let (id, size) = created_row();
        let mut steam = steam();
        script(|s| s.results.push((77, lobby_created(1, 5), false)));
        let call = steam
            .matchmaking()
            .create_lobby(LobbyKind::Private, 2)
            .unwrap();
        script(|s| s.queue.push_back(completion(77, id, size)));
        steam.pump();
        assert!(
            script(|s| s.left.is_empty()),
            "answered, still owned by its token"
        );
        drop(call);
        steam.pump();
        assert_eq!(script(|s| s.left.clone()), [5]);
        assert_eq!(steam.calls.len(), 0);
    }

    #[test]
    fn a_completion_nobody_registered_is_fetched_and_discarded() {
        let mut steam = steam();
        script(|s| s.queue.push_back(completion(99, 1234, 8)));
        steam.pump();
        assert_eq!(steam.diagnostics().unclaimed_completions, 1);
        assert_eq!(script(|s| s.results_asked.clone()), [(99, 8, 1234)]);
    }

    #[test]
    fn a_token_from_another_steam_is_not_registered_here() {
        let mut first = steam();
        let call = first
            .matchmaking()
            .create_lobby(LobbyKind::Private, 2)
            .unwrap();
        let mut second = init_on(testing::fake_lib(), AppId(480)).unwrap();
        match second.take(call) {
            CallState::Failed(CallError::NotRegistered) => {}
            other => panic!("expected NotRegistered, got {other:?}"),
        }
    }
}
