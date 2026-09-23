//! Manual dispatch onto the frame loop.
//!
//! Steam's callbacks are a pipe this crate polls: no C++ callback objects are
//! registered, and no function of ours is ever handed to Steam, so no foreign
//! code calls back into Rust. Each [`Steam::pump`] drains the pipe once,
//! decodes what it claims into a queue, and [`Steam::events`] hands the queue
//! to the game — the same pump-then-drain idiom as the shell's event loop.

use std::sync::Arc;

use crate::{
    LobbyId, Steam, SteamEvent, SteamId, call,
    callbacks::{self, Decoded},
    ffi::structs::{CallbackMsg, SteamApiCallCompleted},
    matchmaking::MAX_LOBBY_CHAT_MESSAGE,
};

/// Counters over everything the pump has seen, and every string Steam handed
/// back, for smoke tests and logs.
///
/// `decode_mismatches`, `null_payloads` and `lossy_strings` must stay zero:
/// each is SDK drift or a broken library, never normal traffic. `unknown`
/// grows in ordinary play — the pipe carries callbacks nobody bound.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PumpDiagnostics {
    /// Every message drained.
    pub callbacks: u64,
    /// Messages with an id no row claims, skipped.
    pub unknown: u64,
    /// Claimed ids whose payload size disagreed with the declared struct,
    /// not decoded.
    pub decode_mismatches: u64,
    /// Claimed ids with a null payload pointer, refused.
    pub null_payloads: u64,
    /// `SteamAPICallCompleted_t`s for calls nothing is waiting on.
    pub unclaimed_completions: u64,
    /// Strings Steam returned that were not valid UTF-8 (read lossily) or
    /// were null (read as empty).
    pub lossy_strings: u64,
}

impl Steam {
    /// Drains Steam's callback pipe: `SteamAPI_ManualDispatch_RunFrame`, then
    /// each message between `GetNextCallback` and `FreeLastCallback`, then
    /// `SteamAPI_ReleaseCurrentThreadMemory` (which `SteamAPI_RunCallbacks`
    /// used to call, and manual dispatch does not). First, answers whose
    /// tokens were dropped untaken are released (see `SteamCall`).
    ///
    /// Call once per frame, then drain [`events`](Self::events).
    pub fn pump(&mut self) {
        let client = Arc::clone(&self.client);
        self.calls.prune(&client);
        let lib = client.lib;
        let pipe = client.pipe;
        // SAFETY: Steam is initialised with manual dispatch, and `pipe` is its
        // pipe.
        unsafe { (lib.fns.dispatch.run_frame)(pipe) };
        let mut msg = CallbackMsg::EMPTY;
        // SAFETY: as above; `msg` is a writable `CallbackMsg_t`.
        while unsafe { (lib.fns.dispatch.get_next_callback)(pipe, &raw mut msg) } {
            self.handle(msg);
            // SAFETY: exactly one `GetNextCallback` answered true since the
            // last free, and nothing from `msg`'s buffer is kept: `handle`
            // copied out whatever it decoded.
            unsafe { (lib.fns.dispatch.free_last_callback)(pipe) };
        }
        // SAFETY: takes no arguments; valid on any thread that used the API.
        unsafe { (lib.fns.lifecycle.release_thread_memory)() };
    }

    /// The events decoded since the last drain, oldest first.
    pub fn events(&mut self) -> impl Iterator<Item = SteamEvent> + '_ {
        self.queue.drain(..)
    }

    /// Counters over everything [`pump`](Self::pump) has seen, and every
    /// string read since init.
    #[must_use]
    pub fn diagnostics(&self) -> PumpDiagnostics {
        PumpDiagnostics {
            lossy_strings: self.lossy_strings.get(),
            ..self.diagnostics
        }
    }

    /// Decodes one message. Runs strictly between `GetNextCallback` and
    /// `FreeLastCallback`, while `msg.param` is valid.
    fn handle(&mut self, msg: CallbackMsg) {
        self.diagnostics.callbacks += 1;
        let Some(row) = callbacks::find(msg.callback) else {
            self.diagnostics.unknown += 1;
            return;
        };
        let size = usize::try_from(msg.param_size).ok();
        if size != Some(row.size) {
            self.diagnostics.decode_mismatches += 1;
            return;
        }
        if msg.param.is_null() {
            self.diagnostics.null_payloads += 1;
            return;
        }
        // SAFETY: Steam's buffer is `param_size` readable bytes, valid until
        // `FreeLastCallback`, which has not run; the slice does not outlive
        // this function, and `decode` copies out of it.
        let bytes = unsafe { core::slice::from_raw_parts(msg.param, row.size) };
        match (row.decode)(bytes) {
            Some(Decoded::Event(event)) => self.push_event(event),
            Some(Decoded::LossyEvent(event)) => {
                self.lossy_strings.set(self.lossy_strings.get() + 1);
                self.push_event(event);
            }
            Some(Decoded::CallCompleted(done)) => self.complete(done),
            Some(Decoded::ChatMessage { lobby, chat_id }) => self.read_chat(lobby, chat_id),
            Some(Decoded::LocalFileChange) => self.read_file_changes(),
            Some(Decoded::ConnectionStatus {
                connection,
                listen_socket,
                state,
                remote,
            }) => {
                // Only a connection arriving on a listen socket needs the
                // pump: every `SteamTransport` reads its own state.
                if listen_socket != 0 && state == crate::net::state::CONNECTING {
                    self.route_incoming(listen_socket, crate::net::Incoming { connection, remote });
                }
            }
            None => self.diagnostics.decode_mismatches += 1,
        }
    }

    /// Queues an event, then re-reads the owner of the lobby a member or data
    /// change names — the change that moves ownership arrives as one of those.
    fn push_event(&mut self, event: SteamEvent) {
        let lobby = match &event {
            SteamEvent::LobbyMemberChanged { lobby, .. }
            | SteamEvent::LobbyDataChanged { lobby, .. } => Some(*lobby),
            _ => None,
        };
        self.queue.push_back(event);
        if let Some(lobby) = lobby {
            self.recheck_owner(lobby);
        }
    }

    /// Hands a `SteamAPICallCompleted_t` to the registry, which fetches the
    /// answer now — Valve's header says `GetAPICallResult` belongs in the
    /// completion's handler.
    fn complete(&mut self, done: SteamApiCallCompleted) {
        let client = Arc::clone(&self.client);
        let pipe = client.pipe;
        let claimed = self.calls.complete(
            &client,
            done.async_call,
            done.callback,
            done.param_size,
            |call, size, id| call::fetch(&client, pipe, call, size, id),
        );
        if !claimed {
            self.diagnostics.unclaimed_completions += 1;
        }
    }

    /// Reads every cloud file change Steam holds (`GetLocalFileChangeCount`,
    /// `GetLocalFileChange`) and queues one event for each.
    fn read_file_changes(&mut self) {
        let client = Arc::clone(&self.client);
        let storage = &client.lib.fns.remote_storage;
        // SAFETY: `client.remote_storage` is the non-null interface init
        // resolved, and this is the pump thread.
        let count = unsafe { (storage.get_local_file_change_count)(client.remote_storage) };
        for index in 0..count {
            let mut change = 0_i32;
            let mut path_type = 0_i32;
            // SAFETY: as above; `index` is below the count Steam just gave,
            // and both out-parameters are writable for the call.
            let path = unsafe {
                (storage.get_local_file_change)(
                    client.remote_storage,
                    index,
                    &raw mut change,
                    &raw mut path_type,
                )
            };
            // SAFETY: straight out of the call, before any other Steam call.
            let path = unsafe { self.copy_string(path) };
            self.queue.push_back(SteamEvent::CloudFileChanged { path });
        }
    }

    /// Reads a lobby chat entry (`GetLobbyChatEntry`) and queues it.
    fn read_chat(&mut self, lobby: LobbyId, chat_id: u32) {
        let mut body = vec![0_u8; MAX_LOBBY_CHAT_MESSAGE];
        let (Ok(chat_id), Ok(capacity)) = (i32::try_from(chat_id), i32::try_from(body.len()))
        else {
            self.diagnostics.decode_mismatches += 1;
            return;
        };
        let client = &self.client;
        let mut sender = 0_u64;
        let mut kind = 0_i32;
        // SAFETY: `client.matchmaking` is the non-null interface init resolved;
        // `sender`, `body` (`capacity` bytes) and `kind` are writable for the
        // call and outlive it.
        let written = unsafe {
            (client.lib.fns.matchmaking.get_lobby_chat_entry)(
                client.matchmaking,
                lobby.0,
                chat_id,
                &raw mut sender,
                body.as_mut_ptr().cast(),
                capacity,
                &raw mut kind,
            )
        };
        // A negative count, or more than the buffer, is a library that broke
        // its own contract: counted, not guessed at.
        match usize::try_from(written) {
            Ok(written) if written <= body.len() => {
                body.truncate(written);
                self.queue.push_back(SteamEvent::LobbyChatMessage {
                    lobby,
                    sender: SteamId(sender),
                    kind,
                    body,
                });
            }
            _ => self.diagnostics.decode_mismatches += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AppId, SteamEvent,
        client::init_on,
        testing::{self, FakeMsg, script},
    };

    fn overlay(active: u8) -> FakeMsg {
        let mut bytes = vec![active, 0, 0, 0];
        bytes.extend_from_slice(&480u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        FakeMsg::payload(331, bytes)
    }

    fn completion() -> FakeMsg {
        let mut bytes = 99u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&513i32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        FakeMsg::payload(703, bytes)
    }

    #[test]
    fn overlay_activated_becomes_an_event_in_order() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.queue.extend([overlay(1), overlay(0)]));
        steam.pump();
        assert_eq!(
            steam.events().collect::<Vec<_>>(),
            [
                SteamEvent::OverlayActivated { active: true },
                SteamEvent::OverlayActivated { active: false },
            ]
        );
        assert_eq!(steam.events().count(), 0, "drained");
    }

    #[test]
    fn an_unknown_id_is_skipped_and_counted() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.queue.push_back(FakeMsg::payload(9999, vec![1, 2, 3])));
        steam.pump();
        let d = steam.diagnostics();
        assert_eq!((d.callbacks, d.unknown, d.decode_mismatches), (1, 1, 0));
        assert_eq!(steam.events().count(), 0);
    }

    #[test]
    fn a_claimed_id_with_the_wrong_size_is_counted_not_decoded() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let mut long = overlay(1);
        long.push_byte(0);
        script(|s| s.queue.push_back(long));
        steam.pump();
        assert_eq!(steam.diagnostics().decode_mismatches, 1);
        assert_eq!(steam.events().count(), 0);
    }

    #[test]
    fn a_negative_size_is_a_mismatch() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.queue.push_back(FakeMsg::null(331, -12)));
        steam.pump();
        assert_eq!(steam.diagnostics().decode_mismatches, 1);
    }

    #[test]
    fn a_null_payload_with_a_size_is_refused_and_counted() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.queue.push_back(FakeMsg::null(331, 12)));
        steam.pump();
        let d = steam.diagnostics();
        assert_eq!((d.null_payloads, d.decode_mismatches), (1, 0));
        assert_eq!(steam.events().count(), 0);
    }

    #[test]
    fn a_completion_nobody_waits_on_is_counted() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| s.queue.push_back(completion()));
        steam.pump();
        assert_eq!(steam.diagnostics().unclaimed_completions, 1);
        assert_eq!(steam.events().count(), 0);
    }

    #[test]
    fn every_message_is_freed_exactly_once_whatever_it_was() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        script(|s| {
            s.queue.extend([
                overlay(1),
                FakeMsg::payload(9999, vec![0; 4]),
                FakeMsg::null(331, 12),
                completion(),
                FakeMsg::payload(331, vec![0; 3]),
            ]);
        });
        steam.pump();
        let calls = script(|s| s.calls);
        assert_eq!(calls.next_true, 5);
        assert_eq!(calls.free, 5);
        assert_eq!(calls.free_without_next, 0);
        assert_eq!(calls.next_while_unfreed, 0);
        assert_eq!(calls.wrong_pipe, 0);
    }

    #[test]
    fn each_pump_runs_one_frame_and_releases_thread_memory_once() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        steam.pump();
        script(|s| s.queue.push_back(overlay(1)));
        steam.pump();
        steam.pump();
        let calls = script(|s| s.calls);
        assert_eq!(calls.run_frame, 3);
        assert_eq!(calls.release_thread_memory, 3);
        assert_eq!(steam.diagnostics().callbacks, 1);
    }

    #[test]
    fn a_join_request_with_a_lossy_connect_string_is_queued_and_counted() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let mut bytes = vec![0; 264];
        bytes[8..13].copy_from_slice(b"caf\xE9\0");
        script(|s| s.queue.push_back(FakeMsg::payload(337, bytes)));
        steam.pump();
        assert_eq!(
            steam.events().collect::<Vec<_>>(),
            [SteamEvent::RichPresenceJoinRequested {
                friend: None,
                connect: "caf\u{FFFD}".into(),
            }]
        );
        assert_eq!(steam.diagnostics().lossy_strings, 1);
    }
}
