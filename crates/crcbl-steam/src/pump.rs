//! Manual dispatch onto the frame loop.
//!
//! Steam's callbacks are a pipe this crate polls: no C++ callback objects are
//! registered, and no function of ours is ever handed to Steam, so no foreign
//! code calls back into Rust. Each [`Steam::pump`] drains the pipe once,
//! decodes what it claims into a queue, and [`Steam::events`] hands the queue
//! to the game — the same pump-then-drain idiom as the shell's event loop.

use crate::{
    Steam, SteamEvent,
    callbacks::{self, Decoded},
    ffi::structs::CallbackMsg,
};

/// Counters over everything the pump has seen, for smoke tests and logs.
///
/// `decode_mismatches` and `null_payloads` must stay zero: either is SDK
/// drift or a broken library, never normal traffic. `unknown` grows in
/// ordinary play — the pipe carries callbacks nobody bound.
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
}

impl Steam {
    /// Drains Steam's callback pipe: `SteamAPI_ManualDispatch_RunFrame`, then
    /// each message between `GetNextCallback` and `FreeLastCallback`, then
    /// `SteamAPI_ReleaseCurrentThreadMemory` (which `SteamAPI_RunCallbacks`
    /// used to call, and manual dispatch does not).
    ///
    /// Call once per frame, then drain [`events`](Self::events).
    pub fn pump(&mut self) {
        let lib = self.client.lib;
        let pipe = self.client.pipe;
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

    /// Counters over everything [`pump`](Self::pump) has seen.
    #[must_use]
    pub fn diagnostics(&self) -> PumpDiagnostics {
        self.diagnostics
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
            Some(Decoded::Event(event)) => self.queue.push_back(event),
            Some(Decoded::CallCompleted(_)) => self.diagnostics.unclaimed_completions += 1,
            None => self.diagnostics.decode_mismatches += 1,
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
}
