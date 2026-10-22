//! The mixer over PulseAudio's native protocol: a system without PipeWire.
//!
//! One connection to the server's socket. A packet is a 20-byte descriptor —
//! five big-endian words: the payload's length, its channel (all ones for a
//! command), two offset words and flags — then a tagstruct payload
//! ([`tagstruct`]). Every command carries a tag the server answers with, in a
//! `REPLY` or an `ERROR`. The reader thread keeps the five lists (sinks,
//! sources, sink inputs, source outputs, cards) and the defaults, re-asking for
//! a list whenever a subscription event names its facility, and wakes the app
//! once per burst, only when what its [`Interest`] shows changed — the
//! PipeWire backend's rule, with a `GET_SERVER_INFO` round trip as the settle.
//!
//! `pipewire-pulse` speaks this protocol too, which is how this backend is
//! tested on a PipeWire machine: `CRCBL_MIXER_BACKEND=pulse` picks it there.

mod proto;
mod tagstruct;

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use proto::{CardInfo, DeviceInfo, NO_INDEX, StreamInfo, VOLUME_NORM};
use tagstruct::{Reader, Writer};

use crate::model::{
    Card, CardId, Choice, Device, DeviceId, Direction, MixerState, Stream, StreamId,
};
use crate::{Action, Interest, MixerError, Target, level};

/// How long [`Pulse::open`] waits for the server's first full picture.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// The descriptor's channel for a command packet.
const COMMAND_CHANNEL: u32 = u32::MAX;

/// The largest payload this client accepts: PulseAudio's
/// `FRAME_SIZE_MAX_ALLOW`, 16 MiB.
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// What a sent command's reply is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Auth,
    ClientName,
    Subscribe,
    ServerInfo,
    Sinks,
    Sources,
    SinkInputs,
    SourceOutputs,
    Cards,
    /// A change the caller asked for; its reply changes nothing here.
    Action,
}

/// Everything the client knows.
#[derive(Debug, Default)]
struct Known {
    sinks: Vec<DeviceInfo>,
    sources: Vec<DeviceInfo>,
    sink_inputs: Vec<StreamInfo>,
    source_outputs: Vec<StreamInfo>,
    cards: Vec<CardInfo>,
    server: proto::ServerInfo,
}

#[derive(Debug)]
struct Inner {
    known: Known,
    version: u32,
    next_tag: u32,
    asked: BTreeMap<u32, Asked>,
    /// Lists still to arrive before the first full picture is in.
    opening: usize,
    interest: Interest,
    /// The server-info request that settles a burst of changes, while one is
    /// out.
    settle: Option<u32>,
    reported: Option<Visible>,
}

/// The part of the state an app with `interest` watches; see the PipeWire
/// backend's type of the same name.
#[derive(Debug, PartialEq)]
struct Visible {
    devices: Vec<Device>,
    cards: Vec<Card>,
    streams: Option<Vec<Stream>>,
}

impl Visible {
    fn of(state: MixerState, interest: Interest) -> Self {
        Self {
            devices: state.devices,
            cards: state.cards,
            streams: (interest == Interest::DevicesAndStreams).then_some(state.streams),
        }
    }
}

struct Shared {
    writer: Mutex<UnixStream>,
    inner: Mutex<Inner>,
    waker: Box<dyn Fn() + Send + Sync>,
    /// False once the server has gone: the reader saw its socket close.
    connected: AtomicBool,
    /// True while the mixer is being dropped, when the socket closing is
    /// ours and not news for the app.
    closing: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A socket failure before the first full picture: a server that resets,
/// refuses or closes the connection then is not there — its socket file can
/// outlive it, as it does while it restarts — so that is
/// [`MixerError::NoServer`], the answer an app retries on.
fn opening_error(error: io::Error) -> MixerError {
    match error.kind() {
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => MixerError::NoServer,
        _ => io_error(error),
    }
}

fn io_error(error: io::Error) -> MixerError {
    MixerError::Protocol(format!("PulseAudio's socket: {error}"))
}

/// Where PulseAudio's socket is: `$PULSE_SERVER` when it names a local one
/// (`unix:/path` or a bare path), else `native` under `$PULSE_RUNTIME_PATH`, or
/// under `pulse/` in `$XDG_RUNTIME_DIR`.
pub(crate) fn socket_path() -> Option<PathBuf> {
    if let Some(server) = std::env::var_os("PULSE_SERVER") {
        let server = server.to_string_lossy().into_owned();
        // A list of servers: the first local one.
        return server.split_whitespace().find_map(|entry| {
            let path = entry.strip_prefix("unix:").unwrap_or(entry);
            path.starts_with('/').then(|| PathBuf::from(path))
        });
    }
    if let Some(dir) = std::env::var_os("PULSE_RUNTIME_PATH") {
        return Some(PathBuf::from(dir).join("native"));
    }
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("pulse").join("native"))
}

/// The authentication cookie: `$PULSE_COOKIE`, else `~/.config/pulse/cookie`,
/// else the older `~/.pulse-cookie`. A server that does not check one
/// (`pipewire-pulse`, or PulseAudio with `auth-anonymous`) takes zeros.
fn cookie() -> Vec<u8> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let candidates = std::env::var_os("PULSE_COOKIE")
        .map(PathBuf::from)
        .into_iter()
        .chain(config.map(|config| config.join("pulse").join("cookie")))
        .chain(home.map(|home| home.join(".pulse-cookie")));
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path)
            && bytes.len() == 256
        {
            return bytes;
        }
    }
    vec![0; 256]
}

/// A connection to PulseAudio, and the thread reading it.
pub(crate) struct Pulse {
    shared: Arc<Shared>,
    socket: UnixStream,
    reader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Pulse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pulse").finish_non_exhaustive()
    }
}

impl Pulse {
    /// Whether a PulseAudio socket exists to connect to.
    pub(crate) fn available() -> bool {
        socket_path().is_some_and(|path| path.exists())
    }

    pub(crate) fn open(waker: Box<dyn Fn() + Send + Sync>) -> Result<Self, MixerError> {
        let path = socket_path().ok_or(MixerError::NoServer)?;
        let socket = UnixStream::connect(&path).map_err(|_| MixerError::NoServer)?;
        let shared = Arc::new(Shared {
            writer: Mutex::new(socket.try_clone().map_err(io_error)?),
            inner: Mutex::new(Inner {
                known: Known::default(),
                version: proto::VERSION,
                next_tag: 0,
                asked: BTreeMap::new(),
                opening: 0,
                interest: Interest::DevicesAndStreams,
                settle: None,
                reported: None,
            }),
            waker,
            connected: AtomicBool::new(true),
            closing: AtomicBool::new(false),
        });
        {
            let mut writer = lock(&shared.writer);
            let mut inner = lock(&shared.inner);
            command(&mut writer, &mut inner, proto::AUTH, Asked::Auth, |w| {
                w.u32(proto::VERSION).bytes(&cookie());
            })?;
        }
        let (ready, opened) = mpsc::channel();
        let reader = {
            let shared = Arc::clone(&shared);
            let socket = socket.try_clone().map_err(io_error)?;
            std::thread::Builder::new()
                .name("crcbl-mixer pulse".into())
                .spawn(move || read_loop(&shared, socket, Some(ready)))
                .map_err(io_error)?
        };
        let pulse = Self {
            shared,
            socket,
            reader: Some(reader),
        };
        match opened.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(pulse),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(MixerError::Protocol(
                "PulseAudio did not finish describing itself in time".into(),
            )),
        }
    }

    pub(crate) fn state(&self) -> MixerState {
        state(&lock(&self.shared.inner).known)
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }

    pub(crate) fn set_interest(&self, interest: Interest) {
        let mut inner = lock(&self.shared.inner);
        inner.interest = interest;
        inner.reported = Some(Visible::of(state(&inner.known), interest));
    }

    pub(crate) fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        if !self.is_connected() {
            return Err(MixerError::Disconnected);
        }
        let mut writer = lock(&self.shared.writer);
        let mut inner = lock(&self.shared.inner);
        let state = state(&inner.known);
        apply(&mut writer, &mut inner, &state, target, action)
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Sends command `command` with a fresh tag, its arguments written by `args`,
/// and remembers what its reply is for.
fn command(
    writer: &mut UnixStream,
    inner: &mut Inner,
    command: u32,
    asked: Asked,
    args: impl FnOnce(&mut Writer),
) -> Result<u32, MixerError> {
    let tag = inner.next_tag;
    inner.next_tag = inner.next_tag.wrapping_add(1);
    let mut payload = Writer::default();
    payload.u32(command).u32(tag);
    args(&mut payload);
    let payload = payload.into_bytes();
    let mut packet = Vec::with_capacity(20 + payload.len());
    let length = u32::try_from(payload.len()).expect("a command under 4 GiB");
    for word in [length, COMMAND_CHANNEL, 0, 0, 0] {
        packet.extend_from_slice(&word.to_be_bytes());
    }
    packet.extend(payload);
    writer.write_all(&packet).map_err(io_error)?;
    inner.asked.insert(tag, asked);
    Ok(tag)
}

/// Asks for every list and the server's info: the whole picture.
fn ask_everything(writer: &mut UnixStream, inner: &mut Inner) -> Result<(), MixerError> {
    for (cmd, asked) in [
        (proto::GET_SERVER_INFO, Asked::ServerInfo),
        (proto::GET_SINK_INFO_LIST, Asked::Sinks),
        (proto::GET_SOURCE_INFO_LIST, Asked::Sources),
        (proto::GET_SINK_INPUT_INFO_LIST, Asked::SinkInputs),
        (proto::GET_SOURCE_OUTPUT_INFO_LIST, Asked::SourceOutputs),
        (proto::GET_CARD_INFO_LIST, Asked::Cards),
    ] {
        command(writer, inner, cmd, asked, |_| {})?;
    }
    Ok(())
}

/// Reads one packet's payload, blocking.
fn read_packet(socket: &mut UnixStream) -> io::Result<(u32, Vec<u8>)> {
    let mut descriptor = [0u8; 20];
    socket.read_exact(&mut descriptor)?;
    let word = |at: usize| u32::from_be_bytes(descriptor[at..at + 4].try_into().expect("4"));
    let length = word(0) as usize;
    if length > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a packet past the protocol's limit",
        ));
    }
    let mut payload = vec![0; length];
    socket.read_exact(&mut payload)?;
    Ok((word(4), payload))
}

/// `opening` is where the first full picture, or the failure to get one, is
/// reported; `None` for a reader that is already open.
fn read_loop(
    shared: &Shared,
    mut socket: UnixStream,
    mut opening: Option<mpsc::Sender<Result<(), MixerError>>>,
) {
    loop {
        let (channel, payload) = match read_packet(&mut socket) {
            Ok(packet) => packet,
            Err(error) => {
                shared.connected.store(false, Ordering::SeqCst);
                if let Some(ready) = opening.take() {
                    let _ = ready.send(Err(opening_error(error)));
                } else if !shared.closing.load(Ordering::SeqCst) {
                    // The server went: one last wake, for the app to see
                    // `is_connected` false and open a new mixer.
                    (shared.waker)();
                }
                return;
            }
        };
        if channel != COMMAND_CHANNEL {
            continue;
        }
        match handle(shared, &payload) {
            Ok(Handled::Opened) => {
                let mut inner = lock(&shared.inner);
                let interest = inner.interest;
                inner.reported = Some(Visible::of(state(&inner.known), interest));
                drop(inner);
                if let Some(ready) = opening.take() {
                    let _ = ready.send(Ok(()));
                }
            }
            Ok(Handled::Settled) => {
                let mut inner = lock(&shared.inner);
                let interest = inner.interest;
                let now = Visible::of(state(&inner.known), interest);
                let wake = inner.reported.as_ref() != Some(&now);
                inner.reported = Some(now);
                drop(inner);
                if wake {
                    (shared.waker)();
                }
            }
            Ok(Handled::Nothing) => {}
            Err(error) => {
                if let Some(ready) = opening.take() {
                    let _ = ready.send(Err(error));
                    return;
                }
            }
        }
    }
}

enum Handled {
    Nothing,
    Opened,
    /// The settling round trip is back: wake if the app's view changed.
    Settled,
}

fn decode(error: tagstruct::DecodeError) -> MixerError {
    MixerError::Protocol(format!("PulseAudio sent a malformed packet: {}", error.0))
}

fn handle(shared: &Shared, payload: &[u8]) -> Result<Handled, MixerError> {
    let mut reader = Reader::new(payload);
    let kind = reader.u32_tag().map_err(decode)?;
    let tag = reader.u32_tag().map_err(decode)?;
    let mut writer = lock(&shared.writer);
    let mut inner = lock(&shared.inner);
    match kind {
        proto::SUBSCRIBE_EVENT => {
            let event = reader.u32_tag().map_err(decode)?;
            let ask = match event & proto::facility::MASK {
                proto::facility::SINK => Some((proto::GET_SINK_INFO_LIST, Asked::Sinks)),
                proto::facility::SOURCE => Some((proto::GET_SOURCE_INFO_LIST, Asked::Sources)),
                proto::facility::SINK_INPUT => {
                    Some((proto::GET_SINK_INPUT_INFO_LIST, Asked::SinkInputs))
                }
                proto::facility::SOURCE_OUTPUT => {
                    Some((proto::GET_SOURCE_OUTPUT_INFO_LIST, Asked::SourceOutputs))
                }
                proto::facility::CARD => Some((proto::GET_CARD_INFO_LIST, Asked::Cards)),
                proto::facility::SERVER => Some((proto::GET_SERVER_INFO, Asked::ServerInfo)),
                _ => None,
            };
            if let Some((cmd, asked)) = ask {
                command(&mut writer, &mut inner, cmd, asked, |_| {})?;
                // The settle goes after the list it waits for, so its reply
                // comes after that list's.
                if inner.settle.is_none() {
                    let tag = command(
                        &mut writer,
                        &mut inner,
                        proto::GET_SERVER_INFO,
                        Asked::ServerInfo,
                        |_| {},
                    )?;
                    inner.settle = Some(tag);
                }
            }
            Ok(Handled::Nothing)
        }
        proto::REPLY => {
            let Some(asked) = inner.asked.remove(&tag) else {
                return Ok(Handled::Nothing);
            };
            let version = inner.version;
            match asked {
                Asked::Auth => {
                    let server = reader.u32_tag().map_err(decode)? & 0xffff;
                    inner.version = server.min(proto::VERSION);
                    command(
                        &mut writer,
                        &mut inner,
                        proto::SET_CLIENT_NAME,
                        Asked::ClientName,
                        |w| {
                            w.props(&[("application.name", "crcbl-mixer")]);
                        },
                    )?;
                    Ok(Handled::Nothing)
                }
                Asked::ClientName => {
                    // Everything the mask names: sinks, sources, sink inputs,
                    // source outputs, the server and cards.
                    let mask = [
                        proto::facility::SINK,
                        proto::facility::SOURCE,
                        proto::facility::SINK_INPUT,
                        proto::facility::SOURCE_OUTPUT,
                        proto::facility::SERVER,
                        proto::facility::CARD,
                    ]
                    .iter()
                    .fold(0, |mask, facility| mask | (1 << facility));
                    command(
                        &mut writer,
                        &mut inner,
                        proto::SUBSCRIBE,
                        Asked::Subscribe,
                        |w| {
                            w.u32(mask);
                        },
                    )?;
                    ask_everything(&mut writer, &mut inner)?;
                    inner.opening = 6;
                    Ok(Handled::Nothing)
                }
                Asked::Subscribe | Asked::Action => Ok(Handled::Nothing),
                Asked::ServerInfo => {
                    inner.known.server = proto::server_info(&mut reader).map_err(decode)?;
                    Ok(listed(&mut inner, tag))
                }
                Asked::Sinks | Asked::Sources => {
                    let mut list = Vec::new();
                    while !reader.is_empty() {
                        list.push(proto::device_info(&mut reader, version).map_err(decode)?);
                    }
                    if asked == Asked::Sinks {
                        inner.known.sinks = list;
                    } else {
                        inner.known.sources = list;
                    }
                    Ok(listed(&mut inner, tag))
                }
                Asked::SinkInputs | Asked::SourceOutputs => {
                    let mut list = Vec::new();
                    while !reader.is_empty() {
                        list.push(
                            if asked == Asked::SinkInputs {
                                proto::sink_input_info(&mut reader, version)
                            } else {
                                proto::source_output_info(&mut reader, version)
                            }
                            .map_err(decode)?,
                        );
                    }
                    if asked == Asked::SinkInputs {
                        inner.known.sink_inputs = list;
                    } else {
                        inner.known.source_outputs = list;
                    }
                    Ok(listed(&mut inner, tag))
                }
                Asked::Cards => {
                    let mut list = Vec::new();
                    while !reader.is_empty() {
                        list.push(proto::card_info(&mut reader, version).map_err(decode)?);
                    }
                    inner.known.cards = list;
                    Ok(listed(&mut inner, tag))
                }
            }
        }
        proto::ERROR => {
            let asked = inner.asked.remove(&tag);
            match asked {
                // A refused change changes nothing, and the caller has its
                // answer from the state; anything else is a broken connection.
                Some(Asked::Action) | None => Ok(Handled::Nothing),
                Some(_) => {
                    let code = reader.u32_tag().unwrap_or_default();
                    Err(MixerError::Protocol(format!(
                        "PulseAudio refused the client: error {code}"
                    )))
                }
            }
        }
        _ => Ok(Handled::Nothing),
    }
}

/// A list or the server info arrived, under `tag`: the first picture, the
/// settle, or neither.
fn listed(inner: &mut Inner, tag: u32) -> Handled {
    if inner.opening > 0 {
        inner.opening -= 1;
        if inner.opening == 0 {
            return Handled::Opened;
        }
        return Handled::Nothing;
    }
    if inner.settle == Some(tag) {
        inner.settle = None;
        return Handled::Settled;
    }
    Handled::Nothing
}

/// A list of channel volumes as one percent.
fn percent(volumes: &[u32]) -> u32 {
    if volumes.is_empty() {
        return 0;
    }
    let count = u32::try_from(volumes.len()).unwrap_or(u32::MAX);
    let sum: u64 = volumes
        .iter()
        // Rounded to the nearest percent per channel, as `pa_cvolume_snprint`
        // shows it.
        .map(|volume| {
            (u64::from(*volume) * 100 + u64::from(VOLUME_NORM / 2)) / u64::from(VOLUME_NORM)
        })
        .sum();
    u32::try_from((sum + u64::from(count / 2)) / u64::from(count)).unwrap_or(u32::MAX)
}

fn choice(port: &proto::Port) -> Choice {
    Choice {
        name: port.name.clone(),
        description: port.description.clone(),
        available: port.available != 1,
    }
}

fn device(info: &DeviceInfo, direction: Direction, default: Option<&str>) -> Device {
    let prop = |key: &str| info.props.get(key).cloned();
    Device {
        id: DeviceId(info.index),
        direction,
        name: info.name.clone(),
        description: info.description.clone(),
        volume: percent(&info.volumes),
        muted: info.muted,
        default: default == Some(info.name.as_str()),
        active_port: info.active_port.clone(),
        ports: info.ports.iter().map(choice).collect(),
        form_factor: prop("device.form_factor"),
        bluetooth: prop("device.bus").is_some_and(|bus| bus == "bluetooth")
            || prop("device.api").is_some_and(|api| api == "bluez5"),
        card: (info.card != NO_INDEX).then_some(CardId(info.card)),
    }
}

fn stream(info: &StreamInfo, direction: Direction) -> Stream {
    Stream {
        id: StreamId(info.index),
        direction,
        application: info
            .props
            .get("application.name")
            .cloned()
            .or_else(|| info.name.clone())
            .unwrap_or_default(),
        title: info
            .props
            .get("media.name")
            .cloned()
            .or_else(|| info.name.clone()),
        volume: percent(&info.volumes),
        muted: info.muted,
        device: (info.device != NO_INDEX).then_some(DeviceId(info.device)),
    }
}

/// The snapshot the client's knowledge amounts to. A monitor source is the
/// sink's echo, not a device a person picks, so it is left out, as are the
/// streams recording from one.
fn state(known: &Known) -> MixerState {
    let monitors: Vec<u32> = known
        .sources
        .iter()
        .filter(|source| source.monitor_of != NO_INDEX)
        .map(|source| source.index)
        .collect();
    let devices = known
        .sinks
        .iter()
        .map(|sink| {
            device(
                sink,
                Direction::Output,
                known.server.default_sink.as_deref(),
            )
        })
        .chain(
            known
                .sources
                .iter()
                .filter(|source| source.monitor_of == NO_INDEX)
                .map(|source| {
                    device(
                        source,
                        Direction::Input,
                        known.server.default_source.as_deref(),
                    )
                }),
        )
        .collect();
    let streams = known
        .sink_inputs
        .iter()
        .map(|input| stream(input, Direction::Output))
        .chain(
            known
                .source_outputs
                .iter()
                .filter(|output| !monitors.contains(&output.device))
                .map(|output| stream(output, Direction::Input)),
        )
        .collect();
    let cards = known
        .cards
        .iter()
        .map(|card| Card {
            id: CardId(card.index),
            name: card.name.clone(),
            description: card
                .props
                .get("device.description")
                .cloned()
                .unwrap_or_else(|| card.name.clone()),
            active_profile: card.active_profile.clone(),
            profiles: card.profiles.iter().map(choice).collect(),
        })
        .collect();
    MixerState {
        devices,
        streams,
        cards,
    }
}

/// Sends `action` on `target`, as `pactl`'s command of the same name would.
fn apply(
    writer: &mut UnixStream,
    inner: &mut Inner,
    state: &MixerState,
    target: Target,
    action: &Action,
) -> Result<(), MixerError> {
    if let Target::Card(card) = target {
        let Action::SetProfile(profile) = action else {
            return Err(MixerError::Invalid("a card takes only a profile"));
        };
        let card = state
            .cards
            .iter()
            .find(|candidate| candidate.id == card)
            .ok_or(MixerError::NoSuchTarget)?;
        if !card.profiles.iter().any(|choice| &choice.name == profile) {
            return Err(MixerError::Invalid("the card has no profile of that name"));
        }
        command(writer, inner, proto::SET_CARD_PROFILE, Asked::Action, |w| {
            w.u32(card.id.0).str(None).str(Some(profile));
        })?;
        return Ok(());
    }
    if matches!(action, Action::SetProfile(_)) {
        return Err(MixerError::Invalid("a profile is set on a card"));
    }
    let device = match target {
        Target::Default(direction) => state.default_device(direction),
        Target::Device(id) => state.device(id),
        _ => None,
    };
    let stream = match target {
        Target::Stream(id) => state.stream(id),
        _ => None,
    };
    let (volume, muted, direction) = device
        .map(|device| (device.volume, device.muted, device.direction))
        .or_else(|| stream.map(|stream| (stream.volume, stream.muted, stream.direction)))
        .ok_or(MixerError::NoSuchTarget)?;
    let index = device.map_or_else(
        || stream.map(|stream| stream.id.0),
        |device| Some(device.id.0),
    );
    let index = index.ok_or(MixerError::NoSuchTarget)?;
    let channels = channels(inner, device.is_some(), direction, index);
    let set_volume = |writer: &mut UnixStream, inner: &mut Inner, percent: u32| {
        let raw = u32::try_from(
            u64::from(percent.min(level::MAX_PERCENT)) * u64::from(VOLUME_NORM) / 100,
        )
        .expect("held to 150 %");
        let cmd = match (device.is_some(), direction) {
            (true, Direction::Output) => proto::SET_SINK_VOLUME,
            (true, Direction::Input) => proto::SET_SOURCE_VOLUME,
            (false, Direction::Output) => proto::SET_SINK_INPUT_VOLUME,
            (false, Direction::Input) => proto::SET_SOURCE_OUTPUT_VOLUME,
        };
        command(writer, inner, cmd, Asked::Action, |w| {
            w.u32(index);
            if device.is_some() {
                w.str(None);
            }
            w.volumes(raw, channels);
        })
        .map(drop)
    };
    let set_mute = |writer: &mut UnixStream, inner: &mut Inner, mute: bool| {
        let cmd = match (device.is_some(), direction) {
            (true, Direction::Output) => proto::SET_SINK_MUTE,
            (true, Direction::Input) => proto::SET_SOURCE_MUTE,
            (false, Direction::Output) => proto::SET_SINK_INPUT_MUTE,
            (false, Direction::Input) => proto::SET_SOURCE_OUTPUT_MUTE,
        };
        command(writer, inner, cmd, Asked::Action, |w| {
            w.u32(index);
            if device.is_some() {
                w.str(None);
            }
            w.bool(mute);
        })
        .map(drop)
    };
    match action {
        Action::SetMute(mute) => set_mute(writer, inner, *mute),
        Action::ToggleMute => set_mute(writer, inner, !muted),
        Action::SetVolume(percent) => set_volume(writer, inner, *percent),
        Action::Step(delta) => set_volume(writer, inner, level::stepped(volume, *delta)),
        Action::MakeDefault => {
            let device = device.ok_or(MixerError::Invalid("only a device can be the default"))?;
            let cmd = match direction {
                Direction::Output => proto::SET_DEFAULT_SINK,
                Direction::Input => proto::SET_DEFAULT_SOURCE,
            };
            command(writer, inner, cmd, Asked::Action, |w| {
                w.str(Some(&device.name));
            })
            .map(drop)
        }
        Action::MoveTo(to) => {
            stream.ok_or(MixerError::Invalid("only a stream can be moved"))?;
            state.device(*to).ok_or(MixerError::NoSuchTarget)?;
            let cmd = match direction {
                Direction::Output => proto::MOVE_SINK_INPUT,
                Direction::Input => proto::MOVE_SOURCE_OUTPUT,
            };
            command(writer, inner, cmd, Asked::Action, |w| {
                w.u32(index).u32(to.0).str(None);
            })
            .map(drop)
        }
        Action::SetPort(port) => {
            let device = device.ok_or(MixerError::Invalid("only a device has ports"))?;
            if !device.ports.iter().any(|choice| &choice.name == port) {
                return Err(MixerError::Invalid("the device has no port of that name"));
            }
            let cmd = match direction {
                Direction::Output => proto::SET_SINK_PORT,
                Direction::Input => proto::SET_SOURCE_PORT,
            };
            command(writer, inner, cmd, Asked::Action, |w| {
                w.u32(index).str(None).str(Some(port));
            })
            .map(drop)
        }
        Action::SetProfile(_) => Err(MixerError::Invalid("a profile is set on a card")),
    }
}

/// How many channels the target has, from the volume it last reported. Two
/// when it reported none.
fn channels(inner: &Inner, is_device: bool, direction: Direction, index: u32) -> u8 {
    let known = &inner.known;
    let volumes = match (is_device, direction) {
        (true, Direction::Output) => known
            .sinks
            .iter()
            .find(|i| i.index == index)
            .map(|i| &i.volumes),
        (true, Direction::Input) => known
            .sources
            .iter()
            .find(|i| i.index == index)
            .map(|i| &i.volumes),
        (false, Direction::Output) => known
            .sink_inputs
            .iter()
            .find(|i| i.index == index)
            .map(|i| &i.volumes),
        (false, Direction::Input) => known
            .source_outputs
            .iter()
            .find(|i| i.index == index)
            .map(|i| &i.volumes),
    };
    volumes
        .and_then(|volumes| u8::try_from(volumes.len()).ok())
        .filter(|channels| *channels > 0)
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The PipeWire backend's test of the same name, for this backend.
    #[test]
    fn a_server_that_goes_wakes_the_app_once_and_disconnects() {
        for closing in [false, true] {
            let (server, client) = UnixStream::pair().unwrap();
            let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = Arc::clone(&wakes);
            let shared = Arc::new(Shared {
                writer: Mutex::new(client.try_clone().unwrap()),
                inner: Mutex::new(Inner {
                    known: Known::default(),
                    version: proto::VERSION,
                    next_tag: 0,
                    asked: BTreeMap::new(),
                    opening: 0,
                    interest: Interest::DevicesAndStreams,
                    settle: None,
                    reported: None,
                }),
                waker: Box::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
                connected: AtomicBool::new(true),
                closing: AtomicBool::new(closing),
            });
            let reader = Arc::clone(&shared);
            let thread = std::thread::spawn(move || read_loop(&reader, client, None));
            drop(server);
            thread.join().unwrap();
            assert!(!shared.connected.load(Ordering::SeqCst));
            assert_eq!(wakes.load(Ordering::SeqCst), usize::from(!closing));
        }
    }

    #[test]
    fn a_channel_volume_is_the_percent_pactl_shows() {
        assert_eq!(percent(&[VOLUME_NORM / 2, VOLUME_NORM / 2]), 50);
        assert_eq!(percent(&[VOLUME_NORM * 3 / 2]), 150);
        assert_eq!(percent(&[24_248]), 37, "pactl's 37 % reading of HDMI1");
        assert_eq!(percent(&[]), 0);
    }
}
