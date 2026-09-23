//! Admission by ticket, for a listen host: provisional until Steam answers.
//!
//! Steam's verdict on a ticket arrives a round trip after
//! [`Auth::begin_session`](crate::Auth::begin_session), and can change later —
//! a ticket cancelled by its issuer, a ban landing mid-session. An
//! [`AuthGate`] keeps the host's view of each user whose session began:
//! **provisional** from the begin, **admitted** on an `OK` verdict, and gone
//! with a [`Verdict`] the host acts on when Steam says no — then or later —
//! or when no answer came within the gate's timeout. It is state only: the
//! host begins and ends the sessions ([`AuthSession`](crate::AuthSession)),
//! feeds the gate the events, and asks it for expiries on its own clock.

use std::time::{Duration, Instant};

use crate::{AuthResponse, SteamEvent, SteamId};

/// What happened to a user the gate was tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Steam validated the ticket: admit the user fully.
    Admitted(SteamId),
    /// Steam refused the ticket, on the first answer or a later one: drop
    /// the user.
    Rejected(SteamId, AuthResponse),
    /// Steam never answered within the timeout: drop the user.
    TimedOut(SteamId),
}

/// Where one tracked user stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// Waiting for Steam until the deadline.
    Provisional { deadline: Instant },
    /// Validated; tracked still, since Steam can change its mind.
    Admitted,
}

/// The host's view of every ticket being validated — see the module docs.
#[derive(Debug, Clone)]
pub struct AuthGate {
    timeout: Duration,
    users: Vec<(SteamId, Standing)>,
}

impl AuthGate {
    /// A gate that times a provisional user out after `timeout` without an
    /// answer.
    #[must_use]
    pub const fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            users: Vec::new(),
        }
    }

    /// Starts tracking `user`, provisionally, from `now` — call it when
    /// `Auth::begin_session` accepted their ticket. A user already tracked
    /// starts over.
    pub fn begin(&mut self, user: SteamId, now: Instant) {
        self.forget(user);
        self.users.push((
            user,
            Standing::Provisional {
                deadline: now + self.timeout,
            },
        ));
    }

    /// Stops tracking `user` — they left — without a verdict.
    pub fn forget(&mut self, user: SteamId) {
        self.users.retain(|(tracked, _)| *tracked != user);
    }

    /// Whether `user` is tracked and not yet validated.
    #[must_use]
    pub fn is_provisional(&self, user: SteamId) -> bool {
        self.standing(user)
            .is_some_and(|standing| matches!(standing, Standing::Provisional { .. }))
    }

    /// Whether `user` is tracked and validated.
    #[must_use]
    pub fn is_admitted(&self, user: SteamId) -> bool {
        self.standing(user) == Some(Standing::Admitted)
    }

    /// Takes Steam's verdict from an event: an `OK` for a provisional user
    /// admits them, and anything else for a tracked user rejects them, now or
    /// long after admission. Events for users the gate does not track, and
    /// events of other kinds, change nothing.
    pub fn observe(&mut self, event: &SteamEvent) -> Option<Verdict> {
        let SteamEvent::AuthSessionVerdict { user, response, .. } = *event else {
            return None;
        };
        let index = self
            .users
            .iter()
            .position(|(tracked, _)| *tracked == user)?;
        if response == AuthResponse::Ok {
            let standing = &mut self.users[index].1;
            let was_provisional = matches!(standing, Standing::Provisional { .. });
            *standing = Standing::Admitted;
            was_provisional.then_some(Verdict::Admitted(user))
        } else {
            self.users.remove(index);
            Some(Verdict::Rejected(user, response))
        }
    }

    /// Every provisional user whose deadline has come by `now`, no longer
    /// tracked.
    pub fn expire(&mut self, now: Instant) -> Vec<Verdict> {
        let mut expired = Vec::new();
        self.users.retain(|&(user, standing)| match standing {
            Standing::Provisional { deadline } if now >= deadline => {
                expired.push(Verdict::TimedOut(user));
                false
            }
            _ => true,
        });
        expired
    }

    fn standing(&self, user: SteamId) -> Option<Standing> {
        self.users
            .iter()
            .find(|(tracked, _)| *tracked == user)
            .map(|&(_, standing)| standing)
    }
}
