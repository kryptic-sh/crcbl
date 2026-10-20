//! Whether a window may present again: [`Presenter::request_frame`]'s
//! request, the present it rides, and the
//! [`ShellEvent::FrameReady`](crcbl_shell::ShellEvent::FrameReady) that
//! answers it.
//!
//! The gate closes only on a present that carried a request, because only
//! then is an answer owed: a request rides the next commit, which is the one
//! the swapchain's present makes, and a request with no present after it may
//! never be answered at all. A present that reported the swapchain out of
//! date leaves a request where it was, for the next present to carry.
//!
//! Any answer opens it. Requests made before an answer share it, so an answer
//! that arrives before the present it was meant for — a commit of another
//! kind carried the request first — opens a gate that never closed, and the
//! next frame asks again. One frame unpaced, never a window left waiting.
//!
//! [`Presenter::request_frame`]: super::Presenter::request_frame

/// Where a window's frame request stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FrameGate {
    /// No request outstanding: present when ready, as a presenter whose
    /// caller never requests a frame always does.
    #[default]
    Open,
    /// A request was made and no present has carried it yet.
    Requested,
    /// A present carried the request: wait for the answer.
    Awaiting,
}

impl FrameGate {
    /// After [`Shell::request_frame`](crcbl_shell::Shell::request_frame)
    /// accepted a request. One made while awaiting joins the answer already
    /// owed.
    pub(super) const fn requested(self) -> Self {
        match self {
            Self::Awaiting => Self::Awaiting,
            Self::Open | Self::Requested => Self::Requested,
        }
    }

    /// After a present reached the swapchain.
    pub(super) const fn presented(self) -> Self {
        match self {
            Self::Requested => Self::Awaiting,
            other => other,
        }
    }

    /// Whether a frame may be acquired.
    pub(super) const fn is_due(self) -> bool {
        !matches!(self, Self::Awaiting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every state, for the sweeps below; written out so a new state is a
    /// decision made here.
    const STATES: [FrameGate; 3] = [FrameGate::Open, FrameGate::Requested, FrameGate::Awaiting];

    #[test]
    fn only_a_present_that_carried_a_request_closes_the_gate() {
        assert_eq!(FrameGate::Open.presented(), FrameGate::Open);
        assert_eq!(FrameGate::Requested.presented(), FrameGate::Awaiting);
        assert!(!FrameGate::Requested.presented().is_due());
        for state in STATES {
            assert_eq!(
                state.is_due(),
                state != FrameGate::Awaiting,
                "{state:?} answered the wrong way"
            );
        }
    }

    #[test]
    fn a_request_while_awaiting_waits_for_the_same_answer() {
        assert_eq!(FrameGate::Awaiting.requested(), FrameGate::Awaiting);
        assert_eq!(FrameGate::Open.requested(), FrameGate::Requested);
        assert_eq!(FrameGate::Requested.requested(), FrameGate::Requested);
    }
}
