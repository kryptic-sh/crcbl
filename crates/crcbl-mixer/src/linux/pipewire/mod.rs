//! The mixer over PipeWire's own protocol.
//!
//! One connection to the server's socket, shared by a reader thread and the
//! caller. The reader keeps a [`Graph`] of the nodes, cards, links and
//! defaults it hears about, answers the server's pings, and calls the waker
//! after each change; the caller reads snapshots of the graph and writes
//! actions to the socket. The sequence and message layouts are
//! `module-protocol-native`'s (`protocol-native.c`); what an action writes is
//! what `module-protocol-pulse` writes for the same `pactl` command, so the two
//! act alike.
//!
//! **Opening waits for the first full picture.** After the registry has listed
//! every global and the client has bound the ones it follows, a second core
//! `sync` comes back only once the server has sent each bound object's info and
//! params, so [`PipeWire::open`] returns a mixer whose first
//! [`state`](PipeWire::state) is already complete.

mod graph;
pub(crate) mod pod;
mod spa;
mod wire;

use std::collections::BTreeMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use graph::{CardInfo, Graph, Node};
use pod::{Builder, Object, Value};
use wire::{Message, Reader, Writer};

use crate::model::{CardId, Direction, MixerState};
use crate::{Action, Interest, MixerError, Target, level};

/// How long [`PipeWire::open`] waits for the server's first full picture.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// The first object id the client hands out: 0 is the core, 1 the client and
/// 2 the registry.
const FIRST_PROXY: u32 = 3;
/// The registry's id.
const REGISTRY_ID: u32 = 2;

/// What a proxy id the client bound stands for.
#[derive(Clone, Copy, Debug)]
enum Bound {
    Node(u32),
    Card(u32),
    Metadata,
}

/// Where the reader is in opening.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for the registry's globals; the value is the sync that ends
    /// them.
    Globals(i32),
    /// Waiting for the bound objects' info and params.
    Objects(i32),
    Ready,
}

#[derive(Debug)]
struct Inner {
    graph: Graph,
    proxies: BTreeMap<u32, Bound>,
    /// Each bound global's proxy id.
    by_global: BTreeMap<u32, u32>,
    next_proxy: u32,
    next_sync: i32,
    phase: Phase,
    interest: Interest,
}

struct Shared {
    writer: Mutex<Writer>,
    inner: Mutex<Inner>,
    waker: Box<dyn Fn() + Send + Sync>,
}

/// Locks `mutex`, carrying on past a poisoned one: the data is a cache of the
/// server's and the next event repairs it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A connection to PipeWire, and the thread reading it.
pub(crate) struct PipeWire {
    shared: Arc<Shared>,
    socket: UnixStream,
    reader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PipeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipeWire").finish_non_exhaustive()
    }
}

impl PipeWire {
    /// Whether a PipeWire socket exists to connect to.
    pub(crate) fn available() -> bool {
        wire::socket_path().is_some_and(|path| path.exists())
    }

    /// Connects, reads the server's whole picture, and starts following it.
    pub(crate) fn open(waker: Box<dyn Fn() + Send + Sync>) -> Result<Self, MixerError> {
        let path = wire::socket_path().ok_or(MixerError::NoServer)?;
        let socket = UnixStream::connect(&path).map_err(|_| MixerError::NoServer)?;
        let shared = Arc::new(Shared {
            writer: Mutex::new(Writer::new(socket.try_clone().map_err(io_error)?)),
            inner: Mutex::new(Inner {
                graph: Graph::default(),
                proxies: BTreeMap::new(),
                by_global: BTreeMap::new(),
                next_proxy: FIRST_PROXY,
                next_sync: 1,
                phase: Phase::Ready,
                interest: Interest::DevicesAndStreams,
            }),
            waker,
        });
        {
            let mut writer = lock(&shared.writer);
            let mut inner = lock(&shared.inner);
            send(&mut writer, spa::CORE_ID, spa::CORE_HELLO, |b| {
                b.int(spa::VERSION_CORE);
            })?;
            send(
                &mut writer,
                spa::CLIENT_ID,
                spa::CLIENT_UPDATE_PROPERTIES,
                |b| {
                    b.structure(|b| {
                        b.int(1)
                            .string(Some("application.name"))
                            .string(Some("crcbl-mixer"));
                    });
                },
            )?;
            send(&mut writer, spa::CORE_ID, spa::CORE_GET_REGISTRY, |b| {
                b.int(spa::VERSION_REGISTRY).int(REGISTRY_ID.cast_signed());
            })?;
            let sync = sync(&mut writer, &mut inner)?;
            inner.phase = Phase::Globals(sync);
        }
        let (ready, opened) = mpsc::channel();
        let reader = {
            let shared = Arc::clone(&shared);
            let socket = socket.try_clone().map_err(io_error)?;
            std::thread::Builder::new()
                .name("crcbl-mixer pipewire".into())
                .spawn(move || read_loop(&shared, Reader::new(socket), &ready))
                .map_err(io_error)?
        };
        let pipewire = Self {
            shared,
            socket,
            reader: Some(reader),
        };
        match opened.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(pipewire),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(MixerError::Protocol(
                "PipeWire did not finish describing itself in time".into(),
            )),
        }
    }

    pub(crate) fn state(&self) -> MixerState {
        lock(&self.shared.inner).graph.state()
    }

    pub(crate) fn set_interest(&self, interest: Interest) {
        lock(&self.shared.inner).interest = interest;
    }

    /// Writes `action` on `target` to the server. The change arrives back as
    /// an event, which is when [`state`](Self::state) shows it.
    pub(crate) fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        let mut writer = lock(&self.shared.writer);
        let inner = lock(&self.shared.inner);
        let state = inner.graph.state();
        match (target, action) {
            (Target::Card(card), Action::SetProfile(profile)) => {
                set_profile(&mut writer, &inner, card, profile)
            }
            (Target::Card(_), _) | (_, Action::SetProfile(_)) => Err(MixerError::Invalid(
                "a profile is set on a card, and a card takes only a profile",
            )),
            (target, action) => {
                let node = resolve(&inner.graph, &state, target).ok_or(MixerError::NoSuchTarget)?;
                act_on_node(&mut writer, &inner, &state, node, action)
            }
        }
    }
}

impl Drop for PipeWire {
    fn drop(&mut self) {
        // The reader is blocked in a read; shutting the socket down ends it
        // with an end of file.
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn io_error(error: io::Error) -> MixerError {
    MixerError::Protocol(format!("PipeWire's socket: {error}"))
}

/// Sends a message whose body is the struct `fields` writes.
fn send(
    writer: &mut Writer,
    id: u32,
    opcode: u8,
    fields: impl FnOnce(&mut Builder),
) -> Result<(), MixerError> {
    let mut builder = Builder::default();
    builder.structure(fields);
    writer
        .send(id, opcode, &builder.into_bytes())
        .map(drop)
        .map_err(io_error)
}

/// Asks the core for a `done` once it has handled everything sent so far, and
/// returns the sequence number the `done` will carry.
fn sync(writer: &mut Writer, inner: &mut Inner) -> Result<i32, MixerError> {
    let seq = inner.next_sync;
    inner.next_sync += 1;
    send(writer, spa::CORE_ID, spa::CORE_SYNC, |b| {
        b.int(spa::CORE_ID.cast_signed()).int(seq);
    })?;
    Ok(seq)
}

/// The global id of the node `target` names in `state`. The state reports
/// serials ([`graph::Node::serial`]); the protocol addresses globals.
fn resolve(graph: &Graph, state: &MixerState, target: Target) -> Option<u32> {
    let serial = match target {
        Target::Default(direction) => state.default_device(direction).map(|device| device.id.0),
        Target::Device(device) => state.device(device).map(|device| device.id.0),
        Target::Stream(stream) => state.stream(stream).map(|stream| stream.id.0),
        Target::Card(_) => None,
    }?;
    global_of(&graph.nodes, serial, graph::Node::serial)
}

/// The global id whose object has `serial`.
fn global_of<T>(
    objects: &BTreeMap<u32, T>,
    serial: u32,
    of: impl Fn(&T, u32) -> u32,
) -> Option<u32> {
    objects
        .iter()
        .find_map(|(&global, object)| (of(object, global) == serial).then_some(global))
}

/// The reader thread: every message until the socket closes.
fn read_loop(shared: &Shared, mut reader: Reader, ready: &mpsc::Sender<Result<(), MixerError>>) {
    let mut opening = Some(ready.clone());
    loop {
        let message = match reader.next() {
            Ok(message) => message,
            Err(error) => {
                if let Some(ready) = opening.take() {
                    let _ = ready.send(Err(io_error(error)));
                }
                return;
            }
        };
        let outcome = handle(shared, &message);
        match outcome {
            Ok(Handled::Opened) => {
                if let Some(ready) = opening.take() {
                    let _ = ready.send(Ok(()));
                }
            }
            Ok(Handled::Changed { devices }) => {
                let inner = lock(&shared.inner);
                let wake = inner.phase == Phase::Ready
                    && (devices || inner.interest == Interest::DevicesAndStreams);
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

/// What one message did.
enum Handled {
    Nothing,
    /// The first full picture is in.
    Opened,
    /// The picture changed; `devices` when a device or card was part of it,
    /// rather than only streams.
    Changed {
        devices: bool,
    },
}

fn handle(shared: &Shared, message: &Message) -> Result<Handled, MixerError> {
    let (body, _) =
        pod::decode(&message.body).map_err(|error| MixerError::Protocol(error.to_string()))?;
    let fields = body.as_struct().unwrap_or_default();
    let int = |at: usize| fields.get(at).and_then(Value::as_int);
    match (message.id, message.opcode) {
        (spa::CORE_ID, spa::CORE_EVENT_DONE) => {
            let seq = int(1).unwrap_or_default();
            let mut writer = lock(&shared.writer);
            let mut inner = lock(&shared.inner);
            match inner.phase {
                Phase::Globals(want) if want == seq => {
                    let next = sync(&mut writer, &mut inner)?;
                    inner.phase = Phase::Objects(next);
                    Ok(Handled::Nothing)
                }
                Phase::Objects(want) if want == seq => {
                    inner.phase = Phase::Ready;
                    Ok(Handled::Opened)
                }
                _ => Ok(Handled::Nothing),
            }
        }
        (spa::CORE_ID, spa::CORE_EVENT_PING) => {
            let (id, seq) = (int(0).unwrap_or_default(), int(1).unwrap_or_default());
            send(
                &mut lock(&shared.writer),
                spa::CORE_ID,
                spa::CORE_PONG,
                |b| {
                    b.int(id).int(seq);
                },
            )?;
            Ok(Handled::Nothing)
        }
        (spa::CORE_ID, spa::CORE_EVENT_ERROR) => {
            let phase = lock(&shared.inner).phase;
            if phase == Phase::Ready {
                // An action the server refused. It changes nothing, and the
                // caller already has its answer from the state.
                Ok(Handled::Nothing)
            } else {
                let text = fields.get(3).and_then(Value::as_str).unwrap_or("an error");
                Err(MixerError::Protocol(format!(
                    "PipeWire refused the client: {text}"
                )))
            }
        }
        (REGISTRY_ID, spa::REGISTRY_EVENT_GLOBAL) => global(shared, fields),
        (REGISTRY_ID, spa::REGISTRY_EVENT_GLOBAL_REMOVE) => {
            let id = int(0).unwrap_or_default().cast_unsigned();
            let mut inner = lock(&shared.inner);
            let stream = inner.graph.is_stream(id) || inner.graph.links.contains_key(&id);
            let known = inner.graph.nodes.remove(&id).is_some()
                | inner.graph.cards.remove(&id).is_some()
                | inner.graph.links.remove(&id).is_some();
            if let Some(proxy) = inner.by_global.remove(&id) {
                inner.proxies.remove(&proxy);
            }
            Ok(if known {
                Handled::Changed { devices: !stream }
            } else {
                Handled::Nothing
            })
        }
        (id, opcode) => {
            let bound = lock(&shared.inner).proxies.get(&id).copied();
            match (bound, opcode) {
                (Some(Bound::Node(global)), spa::EVENT_INFO) => {
                    // Node info: id, ports, change mask, port counts, state,
                    // error, then the props dict.
                    let props = fields.get(8).map(dict).unwrap_or_default();
                    let mut inner = lock(&shared.inner);
                    let devices = !inner.graph.is_stream(global);
                    inner
                        .graph
                        .nodes
                        .entry(global)
                        .or_default()
                        .props
                        .extend(props);
                    Ok(Handled::Changed { devices })
                }
                (Some(Bound::Card(global)), spa::EVENT_INFO) => {
                    card_info(shared, id, global, fields)
                }
                (Some(bound @ (Bound::Node(_) | Bound::Card(_))), spa::EVENT_PARAM) => {
                    param(shared, bound, fields)
                }
                (Some(Bound::Metadata), spa::METADATA_EVENT_PROPERTY) => metadata(shared, fields),
                _ => Ok(Handled::Nothing),
            }
        }
    }
}

/// A dict struct's pairs: a count, then that many key and value strings.
fn dict(value: &Value) -> BTreeMap<String, String> {
    let Some(fields) = value.as_struct() else {
        return BTreeMap::new();
    };
    fields[1..]
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].as_str()?.to_owned())))
        .collect()
}

/// A new global: bound and subscribed to when the mixer follows it.
fn global(shared: &Shared, fields: &[Value]) -> Result<Handled, MixerError> {
    let id = fields
        .first()
        .and_then(Value::as_int)
        .unwrap_or_default()
        .cast_unsigned();
    let kind = fields.get(2).and_then(Value::as_str).unwrap_or_default();
    let props = fields.get(4).map(dict).unwrap_or_default();
    let mut writer = lock(&shared.writer);
    let mut inner = lock(&shared.inner);
    let (bind, subscribe): (Option<Bound>, &[u32]) = match kind {
        spa::INTERFACE_NODE => {
            let class = props
                .get("media.class")
                .map(String::as_str)
                .unwrap_or_default();
            if graph::role(class, &props).is_none() {
                return Ok(Handled::Nothing);
            }
            inner.graph.nodes.insert(
                id,
                Node {
                    props: props.clone(),
                    params: Vec::new(),
                    batch: None,
                },
            );
            (Some(Bound::Node(id)), &[spa::PARAM_PROPS])
        }
        spa::INTERFACE_DEVICE => {
            if props.get("media.class").map(String::as_str) != Some("Audio/Device") {
                return Ok(Handled::Nothing);
            }
            inner.graph.cards.insert(
                id,
                CardInfo {
                    props: props.clone(),
                    ..CardInfo::default()
                },
            );
            (Some(Bound::Card(id)), &CARD_PARAMS)
        }
        spa::INTERFACE_METADATA => {
            if props.get("metadata.name").map(String::as_str) != Some("default") {
                return Ok(Handled::Nothing);
            }
            (Some(Bound::Metadata), &[])
        }
        spa::INTERFACE_LINK => {
            let end = |key: &str| props.get(key).and_then(|node| node.parse::<u32>().ok());
            if let (Some(output), Some(input)) = (end("link.output.node"), end("link.input.node")) {
                inner.graph.links.insert(id, (output, input));
                return Ok(Handled::Changed { devices: false });
            }
            return Ok(Handled::Nothing);
        }
        _ => return Ok(Handled::Nothing),
    };
    let Some(bound) = bind else {
        return Ok(Handled::Nothing);
    };
    let proxy = inner.next_proxy;
    inner.next_proxy += 1;
    inner.proxies.insert(proxy, bound);
    inner.by_global.insert(id, proxy);
    send(&mut writer, REGISTRY_ID, spa::REGISTRY_BIND, |b| {
        b.int(id.cast_signed())
            .string(Some(kind))
            .int(spa::VERSION_OBJECT)
            .int(proxy.cast_signed());
    })?;
    if !subscribe.is_empty() {
        send(&mut writer, proxy, spa::SUBSCRIBE_PARAMS, |b| {
            b.id_array(subscribe);
        })?;
    }
    let devices = !matches!(bound, Bound::Node(node) if inner.graph.is_stream(node));
    Ok(Handled::Changed { devices })
}

/// The params a card's subscription follows.
const CARD_PARAMS: [u32; 4] = [
    spa::PARAM_ENUM_PROFILE,
    spa::PARAM_PROFILE,
    spa::PARAM_ENUM_ROUTE,
    spa::PARAM_ROUTE,
];

/// A card's info: id, change mask, its props dict, then its params as a
/// count and `(id, flags)` pairs.
///
/// **A card's subscription does not resend a changed param**; its info says a
/// param changed by flipping that param's [`spa::PARAM_INFO_SERIAL`] bit, and a
/// client asks for it again — what `module-protocol-pulse`'s manager does on
/// every device info. Without it, a volume set through a route stays at its
/// old value here while every other client sees the new one.
fn card_info(
    shared: &Shared,
    proxy: u32,
    global: u32,
    fields: &[Value],
) -> Result<Handled, MixerError> {
    let props = fields.get(2).map(dict).unwrap_or_default();
    let params: Vec<(u32, i32)> = fields
        .get(3)
        .and_then(Value::as_struct)
        .map(|params| {
            params[1..]
                .as_chunks::<2>()
                .0
                .iter()
                .filter_map(|pair| Some((pair[0].as_id()?, pair[1].as_int()?)))
                .collect()
        })
        .unwrap_or_default();
    let mut writer = lock(&shared.writer);
    let mut inner = lock(&shared.inner);
    let card = inner.graph.cards.entry(global).or_default();
    card.props.extend(props);
    let mut stale = Vec::new();
    for (id, flags) in params {
        if !CARD_PARAMS.contains(&id) {
            continue;
        }
        let serial = flags & spa::PARAM_INFO_SERIAL;
        if card
            .serials
            .insert(id, serial)
            .is_some_and(|before| before != serial)
        {
            stale.push(id);
        }
    }
    for id in stale {
        let seq = inner.next_sync;
        inner.next_sync += 1;
        send(&mut writer, proxy, spa::ENUM_PARAMS, |b| {
            b.int(seq).id(id).int(0).int(0).none();
        })?;
    }
    Ok(Handled::Changed { devices: true })
}

/// A param event: seq, param id, index, next, and the param.
fn param(shared: &Shared, bound: Bound, fields: &[Value]) -> Result<Handled, MixerError> {
    let seq = fields.first().and_then(Value::as_int).unwrap_or_default();
    let id = fields.get(1).and_then(Value::as_id).unwrap_or_default();
    let index = fields.get(2).and_then(Value::as_int).unwrap_or_default();
    let Some(object) = fields.get(4).and_then(Value::as_object).cloned() else {
        return Ok(Handled::Nothing);
    };
    let mut inner = lock(&shared.inner);
    match bound {
        Bound::Node(global) => {
            let devices = !inner.graph.is_stream(global);
            if id == spa::PARAM_PROPS {
                let node = inner.graph.nodes.entry(global).or_default();
                if graph::Batch::starts_afresh(&mut node.batch, seq, index) {
                    node.params.clear();
                }
                node.params.push(object);
            }
            Ok(Handled::Changed { devices })
        }
        Bound::Card(global) => {
            let card = inner.graph.cards.entry(global).or_default();
            let mut batch = card.batches.get(&id).copied();
            let afresh = graph::Batch::starts_afresh(&mut batch, seq, index);
            card.batches.extend(batch.map(|batch| (id, batch)));
            let list = card.params.entry(id).or_default();
            if afresh {
                list.clear();
            }
            list.push(object);
            Ok(Handled::Changed { devices: true })
        }
        Bound::Metadata => Ok(Handled::Nothing),
    }
}

/// A metadata property: subject, key, type, value. Only the core's defaults
/// matter here.
fn metadata(shared: &Shared, fields: &[Value]) -> Result<Handled, MixerError> {
    let subject = fields.first().and_then(Value::as_int).unwrap_or(-1);
    let key = fields.get(1).and_then(Value::as_str).unwrap_or_default();
    let value = fields.get(3).and_then(Value::as_str);
    if subject != spa::CORE_ID.cast_signed() {
        return Ok(Handled::Nothing);
    }
    let name = value.and_then(graph::json_name);
    let mut inner = lock(&shared.inner);
    match key {
        "default.audio.sink" => inner.graph.default_sink = name,
        "default.audio.source" => inner.graph.default_source = name,
        _ => return Ok(Handled::Nothing),
    }
    Ok(Handled::Changed { devices: true })
}

/// A volume or mute change on node `node`, or a default or a move: written
/// where `module-protocol-pulse` writes the same command.
fn act_on_node(
    writer: &mut Writer,
    inner: &Inner,
    state: &MixerState,
    node: u32,
    action: &Action,
) -> Result<(), MixerError> {
    let serial = inner
        .graph
        .nodes
        .get(&node)
        .map_or(node, |info| info.serial(node));
    let device = state.devices.iter().find(|device| device.id.0 == serial);
    let stream = state.streams.iter().find(|stream| stream.id.0 == serial);
    let (volume, muted) = device
        .map(|device| (device.volume, device.muted))
        .or_else(|| stream.map(|stream| (stream.volume, stream.muted)))
        .ok_or(MixerError::NoSuchTarget)?;
    match action {
        Action::SetMute(mute) => set_volume(writer, inner, node, None, Some(*mute)),
        Action::ToggleMute => set_volume(writer, inner, node, None, Some(!muted)),
        Action::SetVolume(percent) => set_volume(writer, inner, node, Some(*percent), None),
        Action::Step(delta) => set_volume(
            writer,
            inner,
            node,
            Some(level::stepped(volume, *delta)),
            None,
        ),
        Action::MakeDefault => {
            let device = device.ok_or(MixerError::Invalid("only a device can be the default"))?;
            let key = match device.direction {
                Direction::Output => "default.configured.audio.sink",
                Direction::Input => "default.configured.audio.source",
            };
            let value = format!("{{ \"name\": {} }}", json_string(&device.name));
            set_metadata(writer, inner, 0, key, "Spa:String:JSON", &value)
        }
        Action::MoveTo(target) => {
            stream.ok_or(MixerError::Invalid("only a stream can be moved"))?;
            // `target.node` takes the device's global id and `target.object`
            // its serial, as `do_move_stream` writes them.
            let to = global_of(&inner.graph.nodes, target.0, graph::Node::serial)
                .ok_or(MixerError::NoSuchTarget)?;
            set_metadata(
                writer,
                inner,
                node,
                "target.node",
                "Spa:Id",
                &to.to_string(),
            )?;
            set_metadata(
                writer,
                inner,
                node,
                "target.object",
                "Spa:Id",
                &target.0.to_string(),
            )
        }
        Action::SetPort(port) => {
            device.ok_or(MixerError::Invalid("only a device has ports"))?;
            set_port(writer, inner, node, port)
        }
        Action::SetProfile(_) => Err(MixerError::Invalid("a profile is set on a card")),
    }
}

/// The card global and route device a device node draws its route from.
fn route_of(inner: &Inner, node: u32) -> Option<(u32, i32)> {
    let props = &inner.graph.nodes.get(&node)?.props;
    let card = props.get("device.id")?.parse::<u32>().ok()?;
    let device = props.get("card.profile.device")?.parse::<i32>().ok()?;
    inner
        .graph
        .cards
        .contains_key(&card)
        .then_some((card, device))
}

/// The active route object for route device `device` on card `card`.
fn active_route(inner: &Inner, card: u32, device: i32) -> Option<&Object> {
    inner
        .graph
        .cards
        .get(&card)?
        .params
        .get(&spa::PARAM_ROUTE)?
        .iter()
        .find(|route| route.get(spa::ROUTE_DEVICE).and_then(Value::as_int) == Some(device))
}

/// How many channels node `node` has, from the volumes it last reported: the
/// route's for a card's device, its own otherwise. Two when it has said none.
fn channels(inner: &Inner, node: u32) -> usize {
    let route_props = route_of(inner, node)
        .and_then(|(card, device)| active_route(inner, card, device))
        .and_then(|route| route.get(spa::ROUTE_PROPS))
        .and_then(Value::as_object);
    route_props
        .or(inner
            .graph
            .nodes
            .get(&node)
            .and_then(graph::Node::volume_props))
        .and_then(|props| props.get(spa::PROP_CHANNEL_VOLUMES))
        .and_then(|volumes| volumes.chosen().as_floats())
        .map_or(2, |volumes| volumes.len().max(1))
}

/// Sets node `node`'s volume, mute or both: through its card's route when it
/// has one, as `set_card_volume_mute_delay` does, else on its own `Props`, as
/// `set_node_volume_mute` does.
fn set_volume(
    writer: &mut Writer,
    inner: &Inner,
    node: u32,
    percent: Option<u32>,
    mute: Option<bool>,
) -> Result<(), MixerError> {
    let linear = percent.map(level::linear_from_percent);
    let channels = channels(inner, node);
    let props = |b: &mut Builder| {
        b.object(spa::OBJECT_PROPS, spa::PARAM_PROPS, |b| {
            if let Some(linear) = linear {
                b.property(spa::PROP_CHANNEL_VOLUMES)
                    .float_array(&vec![linear; channels]);
            }
            if let Some(mute) = mute {
                b.property(spa::PROP_MUTE).bool(mute);
            }
        });
    };
    let route = route_of(inner, node).and_then(|(card, device)| {
        let index = active_route(inner, card, device)?
            .get(spa::ROUTE_INDEX)
            .and_then(Value::as_int)?;
        Some((card, device, index))
    });
    if let Some((card, device, index)) = route {
        let proxy = *inner.by_global.get(&card).ok_or(MixerError::NoSuchTarget)?;
        send(writer, proxy, spa::SET_PARAM, |b| {
            b.id(spa::PARAM_ROUTE)
                .int(0)
                .object(spa::OBJECT_PARAM_ROUTE, spa::PARAM_ROUTE, |b| {
                    b.property(spa::ROUTE_INDEX).int(index);
                    b.property(spa::ROUTE_DEVICE).int(device);
                    b.property(spa::ROUTE_PROPS);
                    props(b);
                    b.property(spa::ROUTE_SAVE).bool(true);
                });
        })
    } else {
        let proxy = *inner.by_global.get(&node).ok_or(MixerError::NoSuchTarget)?;
        send(writer, proxy, spa::SET_PARAM, |b| {
            b.id(spa::PARAM_PROPS).int(0);
            props(b);
        })
    }
}

/// Selects port `name` on device `node`, as `set_card_port` does.
fn set_port(writer: &mut Writer, inner: &Inner, node: u32, name: &str) -> Result<(), MixerError> {
    let (card, device) = route_of(inner, node).ok_or(MixerError::Unsupported(
        "this device has no card, so no ports to choose between",
    ))?;
    let index = inner
        .graph
        .cards
        .get(&card)
        .and_then(|card| card.params.get(&spa::PARAM_ENUM_ROUTE))
        .into_iter()
        .flatten()
        .find(|route| route.get(spa::ROUTE_NAME).and_then(Value::as_str) == Some(name))
        .and_then(|route| route.get(spa::ROUTE_INDEX).and_then(Value::as_int))
        .ok_or(MixerError::NoSuchTarget)?;
    let proxy = *inner.by_global.get(&card).ok_or(MixerError::NoSuchTarget)?;
    send(writer, proxy, spa::SET_PARAM, |b| {
        b.id(spa::PARAM_ROUTE)
            .int(0)
            .object(spa::OBJECT_PARAM_ROUTE, spa::PARAM_ROUTE, |b| {
                b.property(spa::ROUTE_INDEX).int(index);
                b.property(spa::ROUTE_DEVICE).int(device);
                b.property(spa::ROUTE_SAVE).bool(true);
            });
    })
}

/// Selects profile `name` on `card`, as `set_card_profile` does.
fn set_profile(
    writer: &mut Writer,
    inner: &Inner,
    card: CardId,
    name: &str,
) -> Result<(), MixerError> {
    let global = global_of(&inner.graph.cards, card.0, |card, global| {
        graph::serial(&card.props, global)
    })
    .ok_or(MixerError::NoSuchTarget)?;
    let info = &inner.graph.cards[&global];
    let index = info
        .params
        .get(&spa::PARAM_ENUM_PROFILE)
        .into_iter()
        .flatten()
        .find(|profile| profile.get(spa::PROFILE_NAME).and_then(Value::as_str) == Some(name))
        .and_then(|profile| profile.get(spa::PROFILE_INDEX).and_then(Value::as_int))
        .ok_or(MixerError::NoSuchTarget)?;
    let proxy = *inner
        .by_global
        .get(&global)
        .ok_or(MixerError::NoSuchTarget)?;
    send(writer, proxy, spa::SET_PARAM, |b| {
        b.id(spa::PARAM_PROFILE).int(0).object(
            spa::OBJECT_PARAM_PROFILE,
            spa::PARAM_PROFILE,
            |b| {
                b.property(spa::PROFILE_INDEX).int(index);
                b.property(spa::PROFILE_SAVE).bool(true);
            },
        );
    })
}

/// Sets a property on the `default` metadata.
fn set_metadata(
    writer: &mut Writer,
    inner: &Inner,
    subject: u32,
    key: &str,
    kind: &str,
    value: &str,
) -> Result<(), MixerError> {
    let proxy = inner
        .proxies
        .iter()
        .find_map(|(proxy, bound)| matches!(bound, Bound::Metadata).then_some(*proxy))
        .ok_or(MixerError::Unsupported(
            "this PipeWire has no session manager's default metadata",
        ))?;
    send(writer, proxy, spa::METADATA_SET_PROPERTY, |b| {
        b.int(subject.cast_signed())
            .string(Some(key))
            .string(Some(kind))
            .string(Some(value));
    })
}

/// `value` as a JSON string literal, quoted and escaped.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_quoted_as_json_and_read_back() {
        let quoted = json_string("odd \"name\"\\\n");
        assert_eq!(quoted, r#""odd \"name\"\\\u000a""#);
        assert_eq!(
            graph::json_name(&format!("{{ \"name\": {quoted} }}")).as_deref(),
            Some("odd \"name\"\\\n")
        );
    }
}
