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
    /// `None` for a fresh join; `Some` when attempting to resume a previous
    /// session (the server still decides whether the token is valid).
    pub session_token: Option<ResumeToken>,
}

// ── RejectReason ──────────────────────────────────────────────────────────────

/// Why the server rejected a handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectReason {
    /// Machine-readable rejection code:
    ///
    /// | Code | Meaning                |
    /// |------|------------------------|
    /// | 0x01 | protocol_version_mismatch |
    /// | 0x02 | schema_mismatch           |
    /// | 0x03 | server_full               |
    /// | 0x04 | invalid_session_token     |
    /// | 0x05 | engine_build_id_mismatch  |
    pub code: u8,
    /// Human-readable message for logs and diagnostics.
    pub msg: String,
}

// ── HandshakeResult ───────────────────────────────────────────────────────────

/// Outcome of validating a [`Hello`] against the server's expectations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeResult {
    Accept {
        session_id: SessionId,
        resume_token: ResumeToken,
        server_tick: TickId,
    },
    Reject {
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
                reason: RejectReason {
                    code: 0x01,
                    msg: format!(
                        "protocol version mismatch: client {}, server {}",
                        hello.protocol_version, self.compatibility.protocol_version,
                    ),
                },
            };
        }

        if hello.engine_build_id != self.compatibility.engine_build_id {
            return HandshakeResult::Reject {
                reason: RejectReason {
                    code: 0x05,
                    msg: format!(
                        "engine build id mismatch: client 0x{:016x}, server 0x{:016x}",
                        hello.engine_build_id, self.compatibility.engine_build_id,
                    ),
                },
            };
        }

        if hello.schema_hash != self.compatibility.schema_hash {
            return HandshakeResult::Reject {
                reason: RejectReason {
                    code: 0x02,
                    msg: format!(
                        "schema hash mismatch: client 0x{:016x}, server 0x{:016x}",
                        hello.schema_hash, self.compatibility.schema_hash,
                    ),
                },
            };
        }

        HandshakeResult::Accept {
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
            protocol_version: 2,
            engine_build_id: 0xABCD,
            schema_hash: 0xDEAD_BEEF_CAFE,
        })
    }

    fn token() -> ResumeToken {
        ResumeToken::from_bytes([0xA5; 32])
    }

    fn matching_hello() -> Hello {
        Hello {
            protocol_version: 2,
            engine_build_id: 0xABCD,
            schema_hash: 0xDEAD_BEEF_CAFE,
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
            HandshakeResult::Reject { reason } => {
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
            HandshakeResult::Reject { reason } => {
                assert_eq!(reason.code, 0x01);
                assert!(reason.msg.contains("version mismatch"));
                assert!(reason.msg.contains("999"));
                assert!(reason.msg.contains('2'));
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
            HandshakeResult::Reject { reason } => {
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
            HandshakeResult::Reject { reason } => {
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

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_output() {
        let g = gate();
        let _ = format!("{g:?}");

        let h = matching_hello();
        let _ = format!("{h:?}");

        let r = RejectReason {
            code: 0x01,
            msg: "test".into(),
        };
        let _ = format!("{r:?}");

        let hr = HandshakeResult::Reject { reason: r };
        let _ = format!("{hr:?}");

        let ha = HandshakeResult::Accept {
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
                session_token: None,
            },
            SessionId(1),
            token(),
            TickId::ZERO,
        );
        assert!(matches!(bad, HandshakeResult::Reject { .. }));
    }
}
