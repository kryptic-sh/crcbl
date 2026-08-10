//! Protocol handshake gate — validates client [`Hello`] against server
//! expectations and produces an accept/reject decision.
//!
//! This module defines the wire-level handshake types. The codec module
//! will later use/re-export them for serialisation.

use crate::types::{ProtocolCompatibility, ResumeToken, SessionId};
use crcbl_core::TickId;

// ── Hello ─────────────────────────────────────────────────────────────────────

/// Sent by a client to initiate (or resume) a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// Protocol version — bump on breaking wire changes.
    pub protocol_version: u32,
    /// Combined hash of the engine + game build (modules — topic 16).
    pub engine_build_id: u64,
    /// Hash of every replicated component schema.
    /// A mismatch means the client was built against different definitions
    /// and must be cleanly rejected.
    pub schema_hash: u64,
    /// Monotonically increasing identifier binding a response to this hello.
    pub generation: u64,
    /// `None` for a fresh join; `Some` when attempting to resume a previous
    /// session (the server still decides whether the token is valid).
    pub session_token: Option<ResumeToken>,
}

// ── RejectReason ──────────────────────────────────────────────────────────────

/// Why the server rejected a handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectReason {
    /// Machine-readable rejection code; see the `RejectReason` constants.
    pub code: u8,
    /// Human-readable message for logs and diagnostics.
    pub msg: String,
}

impl RejectReason {
    /// The client's protocol version does not match the server's.
    pub const PROTOCOL_VERSION_MISMATCH: u8 = 0x01;
    /// The client's replicated schema hash does not match the server's.
    pub const SCHEMA_MISMATCH: u8 = 0x02;
    /// The server has no room for another session right now.
    pub const SERVER_FULL: u8 = 0x03;
    /// The presented resume token is wrong, absent, or no longer valid.
    pub const INVALID_SESSION_TOKEN: u8 = 0x04;
    /// The client's engine build id does not match the server's.
    pub const ENGINE_BUILD_ID_MISMATCH: u8 = 0x05;
    /// The server could not draw a resume credential from the OS CSPRNG.
    pub const ENTROPY_FAILURE: u8 = 0x06;

    /// Whether this code means "do not bother trying again with this build".
    ///
    /// A version, schema or build mismatch is a property of the two binaries
    /// and will not change while they are running. Everything else — a full
    /// server, an expired token, an entropy failure, or a code this build does
    /// not recognise — is transient, and a client that latched on it would
    /// wedge itself for a condition that resolves on its own. A forged reject
    /// is exactly the shape of a transient failure, which is the other reason
    /// the distinction matters: the handshake carries no MAC, so an attacker
    /// can always inject one.
    #[must_use]
    pub const fn is_permanent(&self) -> bool {
        matches!(
            self.code,
            Self::PROTOCOL_VERSION_MISMATCH
                | Self::SCHEMA_MISMATCH
                | Self::ENGINE_BUILD_ID_MISMATCH
        )
    }
}

// ── HandshakeResult ───────────────────────────────────────────────────────────

/// Outcome of validating a [`Hello`] against the server's expectations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeResult {
    Accept {
        generation: u64,
        session_id: SessionId,
        resume_token: ResumeToken,
        server_tick: TickId,
    },
    Reject {
        generation: u64,
        reason: RejectReason,
    },
}

// ── HandshakeGate ─────────────────────────────────────────────────────────────

/// Validates a client [`Hello`] against the server's expected parameters.
///
/// The gate is stateless — it checks only the three immutable identifiers
/// (protocol version, engine build id, schema hash). Session-token validation
/// is the caller's responsibility and is surfaced via the
/// [`HandshakeResult::Accept`] result.
#[derive(Debug)]
pub struct HandshakeGate {
    compatibility: ProtocolCompatibility,
}

impl HandshakeGate {
    /// Create a gate configured with the server's identifiers.
    pub fn new(compatibility: ProtocolCompatibility) -> Self {
        Self { compatibility }
    }

    /// Validate a client hello.
    ///
    /// Returns `HandshakeResult::Accept` when all checks pass, or
    /// `HandshakeResult::Reject` with an appropriate reason code otherwise.
    ///
    /// The caller supplies the assigned session values and server tick for the accept
    /// path; the gate does not generate credentials itself.
    pub fn validate(
        &self,
        hello: &Hello,
        session_id: SessionId,
        resume_token: ResumeToken,
        server_tick: TickId,
    ) -> HandshakeResult {
        if hello.protocol_version != self.compatibility.protocol_version {
            return HandshakeResult::Reject {
                generation: hello.generation,
                reason: RejectReason {
                    code: RejectReason::PROTOCOL_VERSION_MISMATCH,
                    msg: format!(
                        "protocol version mismatch: client {}, server {}",
                        hello.protocol_version, self.compatibility.protocol_version,
                    ),
                },
            };
        }

        if hello.engine_build_id != self.compatibility.engine_build_id {
            return HandshakeResult::Reject {
                generation: hello.generation,
                reason: RejectReason {
                    code: RejectReason::ENGINE_BUILD_ID_MISMATCH,
                    msg: format!(
                        "engine build id mismatch: client 0x{:016x}, server 0x{:016x}",
                        hello.engine_build_id, self.compatibility.engine_build_id,
                    ),
                },
            };
        }

        if hello.schema_hash != self.compatibility.schema_hash {
            return HandshakeResult::Reject {
                generation: hello.generation,
                reason: RejectReason {
                    code: RejectReason::SCHEMA_MISMATCH,
                    msg: format!(
                        "schema hash mismatch: client 0x{:016x}, server 0x{:016x}",
                        hello.schema_hash, self.compatibility.schema_hash,
                    ),
                },
            };
        }

        HandshakeResult::Accept {
            generation: hello.generation,
            session_id,
            resume_token,
            server_tick,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> HandshakeGate {
        HandshakeGate::new(ProtocolCompatibility {
            protocol_version: 3,
            engine_build_id: 0xABCD,
            schema_hash: 0xDEAD_BEEF_CAFE,
        })
    }

    fn token() -> ResumeToken {
        ResumeToken::from_bytes([0xA5; 32])
    }

    fn matching_hello() -> Hello {
        Hello {
            protocol_version: 3,
            engine_build_id: 0xABCD,
            schema_hash: 0xDEAD_BEEF_CAFE,
            generation: 1,
            session_token: None,
        }
    }

    // ── Accept path ───────────────────────────────────────────────────────

    #[test]
    fn valid_hello_is_accepted() {
        let g = gate();
        let result = g.validate(
            &matching_hello(),
            SessionId(42),
            token(),
            TickId::from_raw(100),
        );
        match result {
            HandshakeResult::Accept {
                session_id,
                server_tick,
                ..
            } => {
                assert_eq!(session_id, SessionId(42));
                assert_eq!(server_tick, TickId::from_raw(100));
            }
            HandshakeResult::Reject { reason, .. } => {
                panic!("expected Accept, got Reject: {reason:?}");
            }
        }
    }

    // ── Version mismatch ──────────────────────────────────────────────────

    #[test]
    fn protocol_version_two_rejects_legacy_version_one() {
        let result = gate().validate(
            &Hello {
                protocol_version: 1,
                ..matching_hello()
            },
            SessionId(1),
            token(),
            TickId::ZERO,
        );
        assert!(matches!(
            result,
            HandshakeResult::Reject {
                generation: 1,
                reason: RejectReason { code: 0x01, .. }
            }
        ));
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let g = gate();
        let hello = Hello {
            protocol_version: 999,
            ..matching_hello()
        };
        let result = g.validate(&hello, SessionId(1), token(), TickId::ZERO);
        match result {
            HandshakeResult::Reject { reason, .. } => {
                assert_eq!(reason.code, 0x01);
                assert!(reason.msg.contains("version mismatch"));
                assert!(reason.msg.contains("999"));
                assert!(reason.msg.contains('3'));
            }
            HandshakeResult::Accept { .. } => {
                panic!("expected Reject for version mismatch");
            }
        }
    }

    // ── Schema mismatch ───────────────────────────────────────────────────

    #[test]
    fn schema_mismatch_is_rejected() {
        let g = gate();
        let hello = Hello {
            schema_hash: 0xBAD,
            ..matching_hello()
        };
        let result = g.validate(&hello, SessionId(1), token(), TickId::ZERO);
        match result {
            HandshakeResult::Reject { reason, .. } => {
                assert_eq!(reason.code, 0x02);
                assert!(reason.msg.contains("schema hash mismatch"));
            }
            HandshakeResult::Accept { .. } => {
                panic!("expected Reject for schema mismatch");
            }
        }
    }

    #[test]
    fn engine_build_id_mismatch_is_rejected() {
        let g = gate();
        let hello = Hello {
            engine_build_id: 0xBAD,
            ..matching_hello()
        };
        let result = g.validate(&hello, SessionId(1), token(), TickId::ZERO);
        match result {
            HandshakeResult::Reject { reason, .. } => {
                assert_eq!(reason.code, 0x05);
                assert!(reason.msg.contains("engine build id mismatch"));
            }
            HandshakeResult::Accept { .. } => {
                panic!("expected Reject for engine build id mismatch");
            }
        }
    }

    // ── Session token forwarding ──────────────────────────────────────────

    #[test]
    fn hello_with_session_token_forwards_to_session() {
        let g = gate();
        let hello = Hello {
            session_token: Some(token()),
            ..matching_hello()
        };
        let result = g.validate(&hello, SessionId(1), token(), TickId::from_raw(10));
        // The gate accepts; session-token validation is the caller's
        // responsibility. The test verifies the token is preserved in the
        // Hello and doesn't affect accept/reject by the gate.
        assert!(matches!(result, HandshakeResult::Accept { .. }));
    }

    // ── Permanence classification ─────────────────────────────────────────

    #[test]
    fn only_build_mismatches_are_permanent() {
        let reason = |code| RejectReason {
            code,
            msg: String::new(),
        };
        for code in [
            RejectReason::PROTOCOL_VERSION_MISMATCH,
            RejectReason::SCHEMA_MISMATCH,
            RejectReason::ENGINE_BUILD_ID_MISMATCH,
        ] {
            assert!(reason(code).is_permanent(), "code {code:#04x}");
        }
        for code in [
            RejectReason::SERVER_FULL,
            RejectReason::INVALID_SESSION_TOKEN,
            RejectReason::ENTROPY_FAILURE,
            // An unrecognised code is transient by default: a build that does
            // not know what it means must not give up over it.
            0xFE,
        ] {
            assert!(!reason(code).is_permanent(), "code {code:#04x}");
        }
    }

    #[test]
    fn gate_rejections_use_the_named_codes() {
        let g = gate();
        let for_hello = |hello: Hello| match g.validate(&hello, SessionId(1), token(), TickId::ZERO)
        {
            HandshakeResult::Reject { reason, .. } => reason.code,
            HandshakeResult::Accept { .. } => panic!("expected a rejection"),
        };
        assert_eq!(
            for_hello(Hello {
                protocol_version: 999,
                ..matching_hello()
            }),
            RejectReason::PROTOCOL_VERSION_MISMATCH
        );
        assert_eq!(
            for_hello(Hello {
                engine_build_id: 0xBAD,
                ..matching_hello()
            }),
            RejectReason::ENGINE_BUILD_ID_MISMATCH
        );
        assert_eq!(
            for_hello(Hello {
                schema_hash: 0xBAD,
                ..matching_hello()
            }),
            RejectReason::SCHEMA_MISMATCH
        );
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn every_handshake_type_debug_prints_without_panicking() {
        let g = gate();
        let _ = format!("{g:?}");

        let h = matching_hello();
        let _ = format!("{h:?}");

        let r = RejectReason {
            code: 0x01,
            msg: "test".into(),
        };
        let _ = format!("{r:?}");

        let hr = HandshakeResult::Reject {
            generation: 1,
            reason: r,
        };
        let _ = format!("{hr:?}");

        let ha = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::ZERO,
        };
        let _ = format!("{ha:?}");
    }

    #[test]
    fn gate_new_stores_expected_values() {
        let g = HandshakeGate::new(ProtocolCompatibility {
            protocol_version: 7,
            engine_build_id: 0xF00,
            schema_hash: 0xBA2,
        });
        // Indirectly verify by matching and mismatching.
        let ok = g.validate(
            &Hello {
                protocol_version: 7,
                engine_build_id: 0xF00,
                schema_hash: 0xBA2,
                generation: 1,
                session_token: None,
            },
            SessionId(1),
            token(),
            TickId::ZERO,
        );
        assert!(matches!(ok, HandshakeResult::Accept { .. }));

        let bad = g.validate(
            &Hello {
                protocol_version: 6,
                engine_build_id: 0xF00,
                schema_hash: 0xBA2,
                generation: 1,
                session_token: None,
            },
            SessionId(1),
            token(),
            TickId::ZERO,
        );
        assert!(matches!(bad, HandshakeResult::Reject { .. }));
    }
}
