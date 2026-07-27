//! Session management — per-client lifecycle and reconnect logic.
//!
//! Each connected client has a [`SessionManager`] that tracks connection
//! state, maintains a ring of [`crate::delta::Baseline`] snapshots for
//! delta encoding, and handles the reconnect grace window.

use std::time::{Duration, Instant};

use crcbl_core::TickId;

use crate::delta::BaselineStore;
use crate::types::SessionId;

// ── SessionState ──────────────────────────────────────────────────────────────

/// Client session lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Not connected (or session terminated).
    Disconnected,
    /// Handshake in progress (Hello sent, awaiting response).
    Handshaking,
    /// Fully connected, receiving snapshots and sending acks.
    Connected,
    /// Transport dropped; waiting to reconnect within grace period.
    Reconnecting,
}

// ── SessionConfig ─────────────────────────────────────────────────────────────

/// Configuration knobs for the session manager.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// How long after a disconnect the client may reconnect and resume.
    pub reconnect_grace_period: Duration,
    /// Maximum number of baseline snapshots to retain for delta computation.
    pub baseline_ring_capacity: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            reconnect_grace_period: Duration::from_secs(30),
            baseline_ring_capacity: 64,
        }
    }
}

// ── SessionManager ────────────────────────────────────────────────────────────

/// Manages a single client's session lifecycle.
///
/// Owns the client's baseline store for delta encoding and tracks
/// connection state for reconnect logic.
#[derive(Debug)]
pub struct SessionManager {
    state: SessionState,
    session_id: SessionId,
    reconnect_deadline: Option<Instant>,
    last_acked_tick: Option<TickId>,
    baseline_store: BaselineStore,
    /// Set once on first successful handshake; validated on reconnect.
    engine_build_id: Option<u64>,
    schema_hash: Option<u64>,
}

impl SessionManager {
    /// Create a fresh session manager in the [`SessionState::Disconnected`] state.
    pub fn new(session_id: SessionId, config: &SessionConfig) -> Self {
        Self {
            state: SessionState::Disconnected,
            session_id,
            reconnect_deadline: None,
            last_acked_tick: None,
            baseline_store: BaselineStore::new(config.baseline_ring_capacity),
            engine_build_id: None,
            schema_hash: None,
        }
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Current session state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// The assigned session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The last tick acknowledged by the client.
    pub fn last_acked_tick(&self) -> Option<TickId> {
        self.last_acked_tick
    }

    /// Immutable reference to the baseline store.
    pub fn baseline_store(&self) -> &BaselineStore {
        &self.baseline_store
    }

    /// Mutable reference to the baseline store.
    pub fn baseline_store_mut(&mut self) -> &mut BaselineStore {
        &mut self.baseline_store
    }

    // ── Lifecycle transitions ────────────────────────────────────────────

    /// Begin the handshake. Transitions [`SessionState::Disconnected`] →
    /// [`SessionState::Handshaking`].
    ///
    /// # Panics (debug only)
    ///
    /// Panics if the session is not [`SessionState::Disconnected`].
    pub fn begin_handshake(&mut self) {
        debug_assert_eq!(
            self.state,
            SessionState::Disconnected,
            "begin_handshake only valid in Disconnected state"
        );
        self.state = SessionState::Handshaking;
    }

    /// Handshake completed successfully.
    ///
    /// Transitions [`SessionState::Handshaking`] → [`SessionState::Connected`]
    /// and records the client's identifiers for future reconnect validation.
    ///
    /// # Panics (debug only)
    ///
    /// Panics if the session is not [`SessionState::Handshaking`].
    pub fn on_connected(&mut self, engine_build_id: u64, schema_hash: u64) {
        debug_assert_eq!(
            self.state,
            SessionState::Handshaking,
            "on_connected only valid in Handshaking state"
        );
        self.state = SessionState::Connected;
        self.engine_build_id = Some(engine_build_id);
        self.schema_hash = Some(schema_hash);
    }

    /// Record the client's ack of a server tick.
    pub fn handle_ack(&mut self, tick: TickId) {
        self.last_acked_tick = Some(tick);
    }

    /// Handle a transport disconnect.
    ///
    /// Transitions [`SessionState::Connected`] → [`SessionState::Reconnecting`]
    /// and sets the reconnect deadline based on `config.reconnect_grace_period`.
    ///
    /// # Panics (debug only)
    ///
    /// Panics if the session is not [`SessionState::Connected`].
    pub fn on_disconnect(&mut self, now: Instant, config: &SessionConfig) {
        debug_assert_eq!(
            self.state,
            SessionState::Connected,
            "on_disconnect only valid in Connected state"
        );
        self.state = SessionState::Reconnecting;
        self.reconnect_deadline = Some(now + config.reconnect_grace_period);
    }

    /// Attempt to reconnect.
    ///
    /// Returns `true` if all of the following hold:
    /// - The session is in [`SessionState::Reconnecting`].
    /// - The current time is within the reconnect grace period.
    /// - The client's `engine_build_id` and `schema_hash` match the original.
    ///
    /// On success, transitions back to [`SessionState::Connected`].
    pub fn try_reconnect(
        &mut self,
        now: Instant,
        engine_build_id: u64,
        schema_hash: u64,
        _config: &SessionConfig,
    ) -> bool {
        if self.state != SessionState::Reconnecting {
            return false;
        }

        // Check grace period.
        if let Some(deadline) = self.reconnect_deadline
            && now > deadline
        {
            self.state = SessionState::Disconnected;
            return false;
        }

        // Check build / schema match.
        if self.engine_build_id != Some(engine_build_id) || self.schema_hash != Some(schema_hash) {
            return false;
        }

        // Valid reconnect — go back to connected.
        self.state = SessionState::Connected;
        self.reconnect_deadline = None;
        // Keep the baseline store intact so delta-encoding can resume.
        true
    }

    /// Purge an expired reconnect attempt.
    ///
    /// Transitions [`SessionState::Reconnecting`] → [`SessionState::Disconnected`]
    /// if the grace period has elapsed. Safe to call at any time; a no-op for
    /// non-reconnecting sessions or those still within their window.
    pub fn expire_if_timed_out(&mut self, now: Instant) {
        if self.state != SessionState::Reconnecting {
            return;
        }
        if let Some(deadline) = self.reconnect_deadline
            && now > deadline
        {
            self.state = SessionState::Disconnected;
            self.reconnect_deadline = None;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SessionConfig {
        SessionConfig {
            reconnect_grace_period: Duration::from_secs(10),
            baseline_ring_capacity: 8,
        }
    }

    fn now() -> Instant {
        Instant::now()
    }

    // ── Fresh session ─────────────────────────────────────────────────────

    #[test]
    fn fresh_session_starts_disconnected() {
        let mgr = SessionManager::new(SessionId(1), &config());
        assert_eq!(mgr.state(), SessionState::Disconnected);
        assert_eq!(mgr.session_id(), SessionId(1));
        assert_eq!(mgr.last_acked_tick(), None);
    }

    // ── Connect flow ──────────────────────────────────────────────────────

    #[test]
    fn connect_flow() {
        let mut mgr = SessionManager::new(SessionId(2), &config());
        mgr.begin_handshake();
        assert_eq!(mgr.state(), SessionState::Handshaking);
        mgr.on_connected(0xF00, 0xBAA);
        assert_eq!(mgr.state(), SessionState::Connected);
    }

    // ── Disconnect and reconnect within grace ─────────────────────────────

    #[test]
    fn disconnect_and_reconnect_within_grace() {
        let mut mgr = SessionManager::new(SessionId(3), &config());
        mgr.begin_handshake();
        mgr.on_connected(0xABCD, 0x1234);

        let t = now();
        mgr.on_disconnect(t, &config());
        assert_eq!(mgr.state(), SessionState::Reconnecting);

        let ok = mgr.try_reconnect(t, 0xABCD, 0x1234, &config());
        assert!(ok);
        assert_eq!(mgr.state(), SessionState::Connected);
    }

    // ── Reconnect rejected with wrong build id ────────────────────────────

    #[test]
    fn reconnect_rejected_with_wrong_build_id() {
        let mut mgr = SessionManager::new(SessionId(4), &config());
        mgr.begin_handshake();
        mgr.on_connected(0xAAAA, 0xBBBB);

        mgr.on_disconnect(now(), &config());
        let ok = mgr.try_reconnect(now(), 0xCCCC, 0xBBBB, &config());
        assert!(!ok);
        // State should still be Reconnecting — schema check failed before
        // state transition.
        assert_eq!(mgr.state(), SessionState::Reconnecting);
    }

    #[test]
    fn reconnect_rejected_with_wrong_schema_hash() {
        let mut mgr = SessionManager::new(SessionId(5), &config());
        mgr.begin_handshake();
        mgr.on_connected(0xAAAA, 0xBBBB);

        mgr.on_disconnect(now(), &config());
        let ok = mgr.try_reconnect(now(), 0xAAAA, 0xCCCC, &config());
        assert!(!ok);
        assert_eq!(mgr.state(), SessionState::Reconnecting);
    }

    // ── Reconnect rejected after grace ────────────────────────────────────

    #[test]
    fn reconnect_rejected_after_grace() {
        let cfg = SessionConfig {
            reconnect_grace_period: Duration::from_millis(10),
            baseline_ring_capacity: 8,
        };
        let mut mgr = SessionManager::new(SessionId(6), &cfg);
        mgr.begin_handshake();
        mgr.on_connected(0xA, 0xB);

        let t = now();
        mgr.on_disconnect(t, &cfg);

        // Advance time past the grace period.
        let late = t + Duration::from_millis(20);
        let ok = mgr.try_reconnect(late, 0xA, 0xB, &cfg);
        assert!(!ok);
        assert_eq!(mgr.state(), SessionState::Disconnected);
    }

    // ── expire_if_timed_out ───────────────────────────────────────────────

    #[test]
    fn expire_if_timed_out_purges_expired() {
        let cfg = SessionConfig {
            reconnect_grace_period: Duration::from_millis(5),
            baseline_ring_capacity: 8,
        };
        let mut mgr = SessionManager::new(SessionId(7), &cfg);
        mgr.begin_handshake();
        mgr.on_connected(1, 2);

        let t = now();
        mgr.on_disconnect(t, &cfg);
        assert_eq!(mgr.state(), SessionState::Reconnecting);

        // Still within grace — no-op.
        mgr.expire_if_timed_out(t + Duration::from_millis(1));
        assert_eq!(mgr.state(), SessionState::Reconnecting);

        // Past grace — should transition to Disconnected.
        mgr.expire_if_timed_out(t + Duration::from_millis(10));
        assert_eq!(mgr.state(), SessionState::Disconnected);
    }

    #[test]
    fn expire_if_timed_out_noop_on_connected() {
        let mut mgr = SessionManager::new(SessionId(8), &config());
        mgr.begin_handshake();
        mgr.on_connected(1, 2);
        mgr.expire_if_timed_out(now() + Duration::from_secs(60));
        assert_eq!(mgr.state(), SessionState::Connected);
    }

    // ── Ack tracking ──────────────────────────────────────────────────────

    #[test]
    fn ack_updates_last_acked_tick() {
        let mut mgr = SessionManager::new(SessionId(9), &config());
        assert_eq!(mgr.last_acked_tick(), None);
        mgr.handle_ack(TickId::from_raw(42));
        assert_eq!(mgr.last_acked_tick(), Some(TickId::from_raw(42)));
    }

    #[test]
    fn ack_overwrites_previous() {
        let mut mgr = SessionManager::new(SessionId(10), &config());
        mgr.handle_ack(TickId::from_raw(10));
        mgr.handle_ack(TickId::from_raw(20));
        assert_eq!(mgr.last_acked_tick(), Some(TickId::from_raw(20)));
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    #[test]
    fn baseline_store_accessors() {
        let mut mgr = SessionManager::new(SessionId(11), &config());
        // BaselineStore starts empty.
        assert!(mgr.baseline_store().newest().is_none());
        // Mutable access.
        use crate::delta::Baseline;
        mgr.baseline_store_mut()
            .insert(Baseline::from_snapshot(TickId::from_raw(1), &[]));
        assert!(mgr.baseline_store().newest().is_some());
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_output() {
        let _ = format!("{:?}", SessionState::Disconnected);
        let _ = format!("{:?}", SessionState::Handshaking);
        let _ = format!("{:?}", SessionState::Connected);
        let _ = format!("{:?}", SessionState::Reconnecting);

        let cfg = config();
        let _ = format!("{cfg:?}");

        let mgr = SessionManager::new(SessionId(12), &config());
        let _ = format!("{mgr:?}");
    }
}
