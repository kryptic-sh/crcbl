//! Windows: WASAPI, the system's audio API, through the `windows` crate.
//!
//! **One thread owns COM.** [`open`] starts a thread that joins COM's
//! multithreaded apartment ([`com::Apartment`]) and makes and holds every
//! audio object the mixer uses ([`audio::Audio`]): the device enumerator, each
//! active endpoint's volume control and session manager, and each session's
//! volume. The callbacks Windows calls on threads of its own ([`notify`]) only
//! send [`Message::Changed`] down the thread's channel. The thread lets a burst
//! of them settle, re-reads, publishes the new [`MixerState`] under a mutex,
//! and calls the waker only when what the app's [`Interest`] shows changed
//! ([`Visible`]): an endpoint's volume moving wakes every app, a session
//! starting or its volume moving only an app that asked for streams.
//!
//! **Actions go to that thread too.** [`Backend::apply`] sends the action down
//! the same channel and waits for its answer, so the audio objects are only
//! ever called in the apartment they were made in, and the caller's thread is
//! never joined to COM: it may be a UI thread in a single-threaded apartment,
//! where joining the multithreaded one fails and calling these objects
//! unmarshalled would be unsound.
//!
//! **Ids** are numbers the mixer hands out per endpoint id string and per
//! session instance identifier, stable while the object exists; see [`ids`].
//!
//! **Volume** is the endpoint's or session's scalar volume as a percent, the
//! number Windows' volume flyout and mixer show, held to
//! [`level::SCALAR_MAX_PERCENT`](crate::level::SCALAR_MAX_PERCENT): Windows
//! has no boost. Windows reports no cards and no ports, and has no public API
//! to move a stream, so those actions are [`MixerError::Unsupported`]. Making
//! a device the default uses an undocumented interface; see
//! [`policy_config`].
//!
//! **The audio service going away** — stopped, or restarting — is seen two
//! ways: a session reporting `DisconnectReasonServerShutdown`, and any call
//! answering `AUDCLNT_E_SERVICE_NOT_RUNNING`. Either ends the mixer: one last
//! wake, then [`Backend::is_connected`] is false and [`Backend::apply`]
//! answers [`MixerError::Disconnected`]. With no session open nothing reports
//! the service stopping, so the loss shows at the next change the mixer reads,
//! or the next action; Windows sends no notification for it otherwise.

mod audio;
mod com;
mod endpoint;
mod ids;
mod notify;
mod policy_config;
mod session;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use audio::Audio;

use crate::visible::Visible;
use crate::{Action, Interest, MixerError, MixerState, Target};

/// How long [`open`] waits for the thread's first full picture.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the thread waits after a change for the next before it re-reads:
/// a slider dragged, or a session starting, signals several times at once.
const SETTLE: Duration = Duration::from_millis(15);

/// The longest a steady stream of changes delays a re-read, so a slider held
/// moving still shows as it moves.
const SETTLE_LIMIT: Duration = Duration::from_millis(100);

/// What the mixer's thread is told.
pub(super) enum Message {
    /// Something changed: re-read once the burst has settled.
    Changed,
    /// A session said the audio service is shutting down.
    ServiceGone,
    /// An action from [`Backend::apply`], and where its answer goes.
    Apply {
        target: Target,
        action: Action,
        reply: Sender<Result<(), MixerError>>,
    },
    /// The mixer is being dropped.
    Stop,
}

#[derive(Debug)]
struct Inner {
    state: MixerState,
    interest: Interest,
    /// What the app last saw, as its interest filters it.
    reported: Option<Visible>,
}

struct Shared {
    inner: Mutex<Inner>,
    waker: Box<dyn Fn() + Send + Sync>,
    /// False once the audio service has gone.
    connected: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sets `state` as the state, and wakes the app if what its interest shows
/// differs from what it last saw.
fn publish(shared: &Shared, state: MixerState) {
    let mut inner = lock(&shared.inner);
    let now = Visible::of(state.clone(), inner.interest);
    let wake = inner.reported.as_ref() != Some(&now);
    inner.reported = Some(now);
    inner.state = state;
    drop(inner);
    if wake {
        (shared.waker)();
    }
}

/// The mixer on Windows: its thread, and the state it publishes.
pub(crate) struct Backend {
    shared: Arc<Shared>,
    messages: Sender<Message>,
    thread: Option<JoinHandle<()>>,
    thread_id: ThreadId,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").finish_non_exhaustive()
    }
}

/// Starts the mixer's thread and waits for its first full picture.
pub(crate) fn open(waker: Box<dyn Fn() + Send + Sync>) -> Result<Backend, MixerError> {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            state: MixerState::default(),
            interest: Interest::DevicesAndStreams,
            reported: None,
        }),
        waker,
        connected: AtomicBool::new(true),
    });
    let (messages, inbox) = mpsc::channel();
    let (ready, opened) = mpsc::channel();
    let thread = {
        let shared = Arc::clone(&shared);
        let messages = messages.clone();
        std::thread::Builder::new()
            .name("crcbl-mixer wasapi".into())
            .spawn(move || run(&shared, messages, &inbox, &ready))
            .map_err(|error| MixerError::Protocol(format!("the mixer's thread: {error}")))?
    };
    let backend = Backend {
        shared,
        messages,
        thread_id: thread.thread().id(),
        thread: Some(thread),
    };
    match opened.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(())) => Ok(backend),
        Ok(Err(error)) => Err(error),
        Err(RecvTimeoutError::Timeout) => Err(MixerError::Protocol(
            "WASAPI did not finish describing the system in time".into(),
        )),
        Err(RecvTimeoutError::Disconnected) => Err(MixerError::Protocol(
            "the mixer's thread stopped while opening".into(),
        )),
    }
}

impl Backend {
    pub(crate) fn state(&self) -> MixerState {
        lock(&self.shared.inner).state.clone()
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }

    pub(crate) fn set_interest(&self, interest: Interest) {
        let mut inner = lock(&self.shared.inner);
        inner.interest = interest;
        // What the app has seen, re-taken in the new interest's shape, so the
        // next comparison does not wake for the change of shape alone.
        inner.reported = Some(Visible::of(inner.state.clone(), interest));
    }

    /// Hands `action` to the mixer's thread and waits for the call's answer.
    ///
    /// **Not from inside the waker**, which runs on that thread: the action
    /// would wait on the thread that is running it, so it answers
    /// [`MixerError::Unsupported`] there instead.
    pub(crate) fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        if !self.is_connected() {
            return Err(MixerError::Disconnected);
        }
        if std::thread::current().id() == self.thread_id {
            return Err(MixerError::Unsupported(
                "an action from inside the waker, on the mixer's own thread",
            ));
        }
        let (reply, answer) = mpsc::channel();
        self.messages
            .send(Message::Apply {
                target,
                action: action.clone(),
                reply,
            })
            .map_err(|_| MixerError::Disconnected)?;
        // The thread drops the reply unanswered only when it has stopped.
        answer.recv().unwrap_or(Err(MixerError::Disconnected))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // A thread that has stopped already has dropped its inbox; the send
        // failing then is that.
        let _stopped = self.messages.send(Message::Stop);
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

/// How the thread's loop goes on after a message.
enum Flow {
    Continue,
    Refresh,
    Stop,
    /// The audio service has gone.
    Lost,
}

fn handle(audio: &Audio, message: Message) -> Flow {
    match message {
        Message::Changed => Flow::Refresh,
        Message::ServiceGone => Flow::Lost,
        Message::Apply {
            target,
            action,
            reply,
        } => {
            let result = audio.apply(target, &action);
            let lost = result == Err(MixerError::Disconnected);
            // The caller waits on the answer; it has gone only if its thread
            // did.
            let _ = reply.send(result);
            if lost { Flow::Lost } else { Flow::Continue }
        }
        Message::Stop => Flow::Stop,
    }
}

/// The mixer's thread: joins COM, reads the first picture and reports it on
/// `ready`, then follows changes until stopped or the service goes.
fn run(
    shared: &Shared,
    messages: Sender<Message>,
    inbox: &Receiver<Message>,
    ready: &Sender<Result<(), MixerError>>,
) {
    let apartment = match com::Apartment::join() {
        Ok(apartment) => apartment,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let opened = Audio::open(messages).and_then(|mut audio| {
        let state = audio.refresh()?;
        Ok((audio, state))
    });
    let mut audio = match opened {
        Ok((audio, state)) => {
            let mut inner = lock(&shared.inner);
            inner.reported = Some(Visible::of(state.clone(), inner.interest));
            inner.state = state;
            drop(inner);
            audio
        }
        Err(error) => {
            let error = if error == MixerError::Disconnected {
                MixerError::NoServer
            } else {
                error
            };
            let _ = ready.send(Err(error));
            return;
        }
    };
    // An opener that gave up waiting has gone; nobody would read the state.
    if ready.send(Ok(())).is_ok() && follow(shared, &mut audio, inbox) {
        shared.connected.store(false, Ordering::SeqCst);
        // The service went: one last wake, for the app to see `is_connected`
        // false and open a new mixer.
        (shared.waker)();
    }
    // Every audio object goes before the apartment does.
    drop(audio);
    drop(apartment);
}

/// Follows changes until told to stop (`false`) or the service goes (`true`).
fn follow(shared: &Shared, audio: &mut Audio, inbox: &Receiver<Message>) -> bool {
    loop {
        let Ok(message) = inbox.recv() else {
            return false;
        };
        match handle(audio, message) {
            Flow::Continue => continue,
            Flow::Stop => return false,
            Flow::Lost => return true,
            Flow::Refresh => {}
        }
        let first = Instant::now();
        loop {
            let left = SETTLE.min(SETTLE_LIMIT.saturating_sub(first.elapsed()));
            if left.is_zero() {
                break;
            }
            match inbox.recv_timeout(left) {
                Ok(message) => match handle(audio, message) {
                    Flow::Continue | Flow::Refresh => {}
                    Flow::Stop => return false,
                    Flow::Lost => return true,
                },
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
        match audio.refresh() {
            Ok(state) => publish(shared, state),
            // A refresh fails only when the service or the enumerator has
            // gone; see `audio`.
            Err(_) => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Device, DeviceId, Direction, Stream, StreamId};
    use std::sync::atomic::AtomicUsize;

    fn state(device_volume: u32, stream_volume: Option<u32>) -> MixerState {
        MixerState {
            devices: vec![Device {
                id: DeviceId(0),
                direction: Direction::Output,
                name: "{0.0.0.00000000}.{speakers}".into(),
                description: "Speakers".into(),
                volume: device_volume,
                muted: false,
                default: true,
                active_port: None,
                ports: Vec::new(),
                form_factor: Some("speaker".into()),
                bluetooth: false,
                card: None,
            }],
            streams: stream_volume
                .map(|volume| Stream {
                    id: StreamId(0),
                    direction: Direction::Output,
                    application: "player".into(),
                    title: None,
                    volume,
                    muted: false,
                    device: Some(DeviceId(0)),
                })
                .into_iter()
                .collect(),
            cards: Vec::new(),
        }
    }

    /// **A published state wakes only when the app's view changed**: a
    /// session coming, moving and going wakes a streams app each time and a
    /// devices-only app never; an endpoint's volume wakes both; the same
    /// state again wakes neither.
    #[test]
    fn a_publish_wakes_only_when_the_app_s_view_changed() {
        for (interest, stream_wakes) in [(Interest::Devices, 0), (Interest::DevicesAndStreams, 3)] {
            let wakes = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&wakes);
            let shared = Shared {
                inner: Mutex::new(Inner {
                    state: state(50, None),
                    interest,
                    reported: Some(Visible::of(state(50, None), interest)),
                }),
                waker: Box::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
                connected: AtomicBool::new(true),
            };
            for next in [state(50, Some(100)), state(50, Some(80)), state(50, None)] {
                publish(&shared, next);
            }
            assert_eq!(wakes.load(Ordering::SeqCst), stream_wakes, "{interest:?}");
            publish(&shared, state(40, None));
            publish(&shared, state(40, None));
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                stream_wakes + 1,
                "{interest:?}: an endpoint's volume wakes once"
            );
            assert_eq!(lock(&shared.inner).state, state(40, None));
        }
    }
}
