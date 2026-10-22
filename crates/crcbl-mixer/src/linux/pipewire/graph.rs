//! What the PipeWire client has learned from the server, and the
//! [`MixerState`] it amounts to.
//!
//! The rules for reading a device are `module-protocol-pulse`'s
//! (`collect.c`'s `collect_device_info`), which is how `pactl` on a PipeWire
//! desktop answers, so the two agree: a device that belongs to a card reads its
//! volume and mute off the card's active route for that device (the route's
//! `device` field equal to the node's `card.profile.device`), and its port is
//! that route; a device without one reads its own `Props` param.

use std::collections::BTreeMap;

use super::pod::Object;
use super::spa;
use crate::level;
use crate::model::{
    Card, CardId, Choice, Device, DeviceId, Direction, MixerState, Stream, StreamId,
};

/// A node the mixer follows: a device or a stream.
#[derive(Debug, Default)]
pub(crate) struct Node {
    pub(crate) props: BTreeMap<String, String>,
    /// The node's `Props` param, in the order the node listed it: a node lists
    /// more than one object, the first carrying its volume and mute.
    pub(crate) params: Vec<Object>,
    /// Where `params` is in arriving; see [`Batch`].
    pub(crate) batch: Option<Batch>,
}

impl Node {
    /// The `Props` object that carries the node's channel volumes, if any
    /// does.
    pub(crate) fn volume_props(&self) -> Option<&Object> {
        self.params
            .iter()
            .find(|props| props.get(spa::PROP_CHANNEL_VOLUMES).is_some())
    }

    /// The id the mixer reports this node by: its `object.serial`, which the
    /// server never hands out twice, where a global id is reused once its
    /// object goes. `pactl` reports the same number as the index.
    pub(crate) fn serial(&self, global: u32) -> u32 {
        serial(&self.props, global)
    }
}

/// `object.serial` from `props`, or the global id when it has none.
pub(crate) fn serial(props: &BTreeMap<String, String>, global: u32) -> u32 {
    props
        .get("object.serial")
        .and_then(|serial| serial.parse().ok())
        .unwrap_or(global)
}

/// A card: a PipeWire device object of class `Audio/Device`.
#[derive(Debug, Default)]
pub(crate) struct CardInfo {
    pub(crate) props: BTreeMap<String, String>,
    /// Every param the card reported, by param id, in the order it listed
    /// them.
    pub(crate) params: BTreeMap<u32, Vec<Object>>,
    /// Each param's serial bit as the card's last info gave it; see
    /// `card_info` in the client.
    pub(crate) serials: BTreeMap<u32, i32>,
    /// Where each param's list is in arriving; see [`Batch`].
    pub(crate) batches: BTreeMap<u32, Batch>,
}

/// The sequence number and index of the last param event a list took.
///
/// A param's values arrive one event each, and a fresh set replaces the old
/// one. Which event starts a fresh set: one with a different sequence number
/// (a subscription's resend and each `enum_params` reply carry their own), or
/// one whose index does not follow the last. The index alone does not say —
/// an `enum_params` reply numbers its values by their place among all the
/// param's values, so a card's active routes arrive as 2, 3 and 5, not from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Batch {
    pub(crate) seq: i32,
    pub(crate) index: i32,
}

impl Batch {
    /// Whether an event of `seq` and `index` starts a fresh set after this
    /// one, and records it either way.
    pub(crate) fn starts_afresh(last: &mut Option<Self>, seq: i32, index: i32) -> bool {
        let afresh = last.is_none_or(|last| last.seq != seq || index <= last.index);
        *last = Some(Self { seq, index });
        afresh
    }
}

/// Everything the client knows, keyed by global id.
#[derive(Debug, Default)]
pub(crate) struct Graph {
    pub(crate) nodes: BTreeMap<u32, Node>,
    pub(crate) cards: BTreeMap<u32, CardInfo>,
    /// Each link's output node and input node.
    pub(crate) links: BTreeMap<u32, (u32, u32)>,
    /// The `node.name` of the default sink and source, from the `default`
    /// metadata.
    pub(crate) default_sink: Option<String>,
    pub(crate) default_source: Option<String>,
}

/// What a node is to the mixer, from its `media.class`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Device(Direction),
    Stream(Direction),
}

/// The role a node of `media.class` `class` plays, or `None` for a node the
/// mixer does not show (video, MIDI, a monitor stream).
pub(crate) fn role(class: &str, props: &BTreeMap<String, String>) -> Option<Role> {
    // A peak meter such as `pavucontrol`'s records a sink's monitor; it is not
    // a stream a person started.
    if props
        .get("stream.monitor")
        .is_some_and(|value| value == "true")
    {
        return None;
    }
    match class {
        "Audio/Sink" => Some(Role::Device(Direction::Output)),
        "Audio/Source" | "Audio/Source/Virtual" => Some(Role::Device(Direction::Input)),
        "Stream/Output/Audio" => Some(Role::Stream(Direction::Output)),
        "Stream/Input/Audio" => Some(Role::Stream(Direction::Input)),
        _ => None,
    }
}

impl Graph {
    fn node_role(&self, id: u32) -> Option<Role> {
        let node = self.nodes.get(&id)?;
        role(node.props.get("media.class")?, &node.props)
    }

    /// The snapshot the client's knowledge amounts to.
    pub(crate) fn state(&self) -> MixerState {
        let mut state = MixerState::default();
        for (&id, node) in &self.nodes {
            match self.node_role(id) {
                Some(Role::Device(direction)) => {
                    state.devices.push(self.device(id, node, direction));
                }
                Some(Role::Stream(direction)) => {
                    state.streams.push(self.stream(id, node, direction));
                }
                None => {}
            }
        }
        for (&id, card) in &self.cards {
            state.cards.push(card_of(id, card));
        }
        state
    }

    fn device(&self, id: u32, node: &Node, direction: Direction) -> Device {
        let name = node.props.get("node.name").cloned().unwrap_or_default();
        let card_id = node
            .props
            .get("device.id")
            .and_then(|id| id.parse::<u32>().ok());
        let card = card_id.and_then(|id| self.cards.get(&id));
        let route_device = node
            .props
            .get("card.profile.device")
            .and_then(|device| device.parse::<i32>().ok());
        let route = card.zip(route_device).and_then(|(card, device)| {
            card.params
                .get(&spa::PARAM_ROUTE)?
                .iter()
                .find(|route| route.get(spa::ROUTE_DEVICE).and_then(|v| v.as_int()) == Some(device))
        });
        // The route's props when the device has one, else the node's own.
        let volume_props = route
            .and_then(|route| route.get(spa::ROUTE_PROPS))
            .and_then(|props| props.as_object())
            .or(node.volume_props());
        let (volume, muted) = volume_props.map_or((0, false), volume_and_mute);
        let ports = card
            .zip(route_device)
            .map(|(card, device)| ports_of(card, device, direction))
            .unwrap_or_default();
        let lookup = |key: &str| {
            node.props
                .get(key)
                .or_else(|| card.and_then(|card| card.props.get(key)))
                .cloned()
        };
        let default = match direction {
            Direction::Output => self.default_sink.as_deref() == Some(name.as_str()),
            Direction::Input => self.default_source.as_deref() == Some(name.as_str()),
        };
        Device {
            id: DeviceId(node.serial(id)),
            direction,
            description: lookup("node.description")
                .or_else(|| lookup("node.nick"))
                .unwrap_or_else(|| name.clone()),
            name,
            volume,
            muted,
            default,
            // The active route's port, else the first port the device has, as
            // `collect_port_info` answers for a device with no route yet.
            active_port: route
                .and_then(|route| route.get(spa::ROUTE_NAME))
                .and_then(|name| name.as_str())
                .map(str::to_owned)
                .or_else(|| ports.first().map(|port| port.name.clone())),
            ports,
            form_factor: lookup("device.form-factor"),
            bluetooth: lookup("device.api").is_some_and(|api| api == "bluez5"),
            card: card_id
                .zip(card)
                .map(|(card_id, card)| CardId(serial(&card.props, card_id))),
        }
    }

    fn stream(&self, id: u32, node: &Node, direction: Direction) -> Stream {
        let (volume, muted) = node.volume_props().map_or((0, false), volume_and_mute);
        // A playing stream is the output end of its link to a sink; a
        // recording stream is the input end of its link from a source.
        let device = self
            .links
            .values()
            .find_map(|&(output, input)| match direction {
                Direction::Output if output == id => Some(input),
                Direction::Input if input == id => Some(output),
                _ => None,
            });
        Stream {
            id: StreamId(node.serial(id)),
            direction,
            application: node
                .props
                .get("application.name")
                .or_else(|| node.props.get("node.name"))
                .cloned()
                .unwrap_or_default(),
            title: node.props.get("media.name").cloned(),
            volume,
            muted,
            device: device
                .filter(|device| {
                    self.node_role(*device)
                        .is_some_and(|role| matches!(role, Role::Device(_)))
                })
                .map(|device| DeviceId(self.nodes[&device].serial(device))),
        }
    }
}

/// The percent and mute a `Props` object carries.
fn volume_and_mute(props: &Object) -> (u32, bool) {
    let volume = props
        .get(spa::PROP_CHANNEL_VOLUMES)
        .and_then(|value| value.chosen().as_floats())
        .map_or(0, |channels| level::percent_from_channels(&channels));
    let muted = props
        .get(spa::PROP_MUTE)
        .and_then(|value| value.chosen().as_bool())
        .unwrap_or(false);
    (volume, muted)
}

/// Whether a param's `available` field allows using it: `no` does not,
/// `yes` and `unknown` do, as `pavucontrol` greys out only `no`.
fn available(object: &Object, key: u32) -> bool {
    object
        .get(key)
        .and_then(|value| value.chosen().as_id())
        .is_none_or(|available| available != spa::AVAILABILITY_NO)
}

/// Whether `route`'s array field `key` holds `value`.
fn route_lists(route: &Object, key: u32, value: i32) -> bool {
    matches!(route.get(key), Some(super::pod::Value::Array(values))
        if values.iter().any(|item| item.as_int() == Some(value)))
}

/// The ports of route-device `device` in the `direction` given, from the
/// card's `EnumRoute`: the routes of that direction that the card's active
/// profile offers for that device, as `collect_port_info` filters them.
fn ports_of(card: &CardInfo, device: i32, direction: Direction) -> Vec<Choice> {
    let want = match direction {
        Direction::Output => spa::DIRECTION_OUTPUT,
        Direction::Input => spa::DIRECTION_INPUT,
    };
    let profile = card
        .params
        .get(&spa::PARAM_PROFILE)
        .and_then(|active| active.first())
        .and_then(|active| active.get(spa::PROFILE_INDEX))
        .and_then(|index| index.as_int());
    card.params
        .get(&spa::PARAM_ENUM_ROUTE)
        .into_iter()
        .flatten()
        .filter(|route| route.get(spa::ROUTE_DIRECTION).and_then(|v| v.as_id()) == Some(want))
        .filter(|route| route_lists(route, spa::ROUTE_DEVICES, device))
        .filter(|route| {
            profile.is_some_and(|profile| route_lists(route, spa::ROUTE_PROFILES, profile))
        })
        .filter_map(|route| {
            choice(
                route,
                spa::ROUTE_NAME,
                spa::ROUTE_DESCRIPTION,
                spa::ROUTE_AVAILABLE,
            )
        })
        .collect()
}

fn choice(object: &Object, name: u32, description: u32, availability: u32) -> Option<Choice> {
    let name = object.get(name)?.as_str()?.to_owned();
    Some(Choice {
        description: object
            .get(description)
            .and_then(|value| value.as_str())
            .map_or_else(|| name.clone(), str::to_owned),
        available: available(object, availability),
        name,
    })
}

fn card_of(id: u32, card: &CardInfo) -> Card {
    let profiles: Vec<Choice> = card
        .params
        .get(&spa::PARAM_ENUM_PROFILE)
        .into_iter()
        .flatten()
        .filter_map(|profile| {
            choice(
                profile,
                spa::PROFILE_NAME,
                spa::PROFILE_DESCRIPTION,
                spa::PROFILE_AVAILABLE,
            )
        })
        .collect();
    let active_profile = card
        .params
        .get(&spa::PARAM_PROFILE)
        .and_then(|active| active.first())
        .and_then(|active| active.get(spa::PROFILE_NAME))
        .and_then(|name| name.as_str())
        .map(str::to_owned);
    Card {
        id: CardId(serial(&card.props, id)),
        name: card.props.get("device.name").cloned().unwrap_or_default(),
        description: card
            .props
            .get("device.description")
            .or_else(|| card.props.get("device.nick"))
            .cloned()
            .unwrap_or_default(),
        active_profile,
        profiles,
    }
}

/// The `name` field of a metadata value such as `{ "name": "alsa_output.x" }`,
/// which is how `default.audio.sink` is written: a JSON object with one string
/// member. Escapes are decoded; anything that is not that shape is `None`.
pub(crate) fn json_name(value: &str) -> Option<String> {
    let rest = &value[value.find("\"name\"")? + "\"name\"".len()..];
    let rest = rest
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let mut name = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(name),
            '\\' => match chars.next()? {
                'n' => name.push('\n'),
                't' => name.push('\t'),
                'r' => name.push('\r'),
                'b' => name.push('\u{8}'),
                'f' => name.push('\u{c}'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    name.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => name.push(other),
            },
            c => name.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::pipewire::pod::{Property, Value};

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn object(kind: u32, id: u32, props: Vec<(u32, Value)>) -> Object {
        Object {
            kind,
            id,
            props: props
                .into_iter()
                .map(|(key, value)| Property {
                    key,
                    flags: 0,
                    value,
                })
                .collect(),
        }
    }

    fn enum_route(name: &str, description: &str, device: i32, profile: i32) -> Object {
        object(
            spa::OBJECT_PARAM_ROUTE,
            spa::PARAM_ENUM_ROUTE,
            vec![
                (spa::ROUTE_DIRECTION, Value::Id(spa::DIRECTION_OUTPUT)),
                (spa::ROUTE_NAME, Value::String(name.into())),
                (spa::ROUTE_DESCRIPTION, Value::String(description.into())),
                (
                    spa::ROUTE_AVAILABLE,
                    Value::Id(if device == 1 { spa::AVAILABILITY_NO } else { 2 }),
                ),
                (spa::ROUTE_DEVICES, Value::Array(vec![Value::Int(device)])),
                (spa::ROUTE_PROFILES, Value::Array(vec![Value::Int(profile)])),
            ],
        )
    }

    fn volume_props(linear: f32, mute: bool) -> Object {
        object(
            spa::OBJECT_PROPS,
            spa::PARAM_PROPS,
            vec![
                (
                    spa::PROP_CHANNEL_VOLUMES,
                    Value::Array(vec![Value::Float(linear), Value::Float(linear)]),
                ),
                (spa::PROP_MUTE, Value::Bool(mute)),
            ],
        )
    }

    #[test]
    fn the_default_sinks_name_is_read_out_of_its_json() {
        assert_eq!(
            json_name(r#"{ "name": "alsa_output.pci-0000_00_1f.3.analog-stereo" }"#).as_deref(),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo")
        );
        assert_eq!(
            json_name(r#"{"name":"a\"b\\cA"}"#).as_deref(),
            Some("a\"b\\cA")
        );
        assert_eq!(json_name(r#"{"other":"x"}"#), None);
        assert_eq!(json_name(r#"{"name":"unterminated"#), None);
    }

    /// A card's device reads its volume, mute and port off the card's active
    /// route for it, not off its own `Props`; a device without a card reads its
    /// own; a stream is on the device its link goes to.
    #[test]
    fn a_card_device_reads_its_route_and_a_stream_its_link() {
        let mut graph = Graph {
            default_sink: Some("alsa_output.usb".into()),
            ..Graph::default()
        };
        graph.cards.insert(
            40,
            CardInfo {
                props: props(&[
                    ("device.name", "alsa_card.usb"),
                    ("device.description", "USB Headset"),
                ]),
                serials: BTreeMap::new(),
                batches: BTreeMap::new(),
                params: BTreeMap::from([
                    (
                        spa::PARAM_ROUTE,
                        vec![object(
                            spa::OBJECT_PARAM_ROUTE,
                            spa::PARAM_ROUTE,
                            vec![
                                (spa::ROUTE_INDEX, Value::Int(3)),
                                (spa::ROUTE_DEVICE, Value::Int(1)),
                                (
                                    spa::ROUTE_NAME,
                                    Value::String("analog-output-headphones".into()),
                                ),
                                (spa::ROUTE_PROPS, Value::Object(volume_props(0.125, true))),
                            ],
                        )],
                    ),
                    (
                        spa::PARAM_PROFILE,
                        vec![object(
                            spa::OBJECT_PARAM_PROFILE,
                            spa::PARAM_PROFILE,
                            vec![(spa::PROFILE_INDEX, Value::Int(1))],
                        )],
                    ),
                    (
                        spa::PARAM_ENUM_ROUTE,
                        vec![
                            enum_route("analog-output-headphones", "Headphones", 1, 1),
                            // Offered by another profile only: not a port now.
                            enum_route("analog-output-speaker", "Speaker", 1, 2),
                            // Device 2's, for the node with no active route.
                            enum_route("hdmi-output-0", "HDMI", 2, 1),
                        ],
                    ),
                ]),
            },
        );
        graph.nodes.insert(
            50,
            Node {
                batch: None,
                props: props(&[
                    ("media.class", "Audio/Sink"),
                    ("node.name", "alsa_output.usb"),
                    ("node.description", "USB Headset Analog Stereo"),
                    ("device.id", "40"),
                    ("card.profile.device", "1"),
                ]),
                // The node's own Props differ from the route's, and must lose.
                params: vec![volume_props(1.0, false)],
            },
        );
        graph.nodes.insert(
            60,
            Node {
                batch: None,
                props: props(&[
                    ("media.class", "Audio/Sink"),
                    ("node.name", "bluez_output.aa"),
                    ("device.api", "bluez5"),
                ]),
                params: vec![volume_props(1.0, false)],
            },
        );
        graph.nodes.insert(
            70,
            Node {
                batch: None,
                props: props(&[
                    ("media.class", "Stream/Output/Audio"),
                    ("application.name", "Firefox"),
                    ("media.name", "A video"),
                ]),
                params: vec![volume_props(0.125, false)],
            },
        );
        graph.nodes.insert(
            55,
            Node {
                batch: None,
                props: props(&[
                    ("media.class", "Audio/Sink"),
                    ("node.name", "alsa_output.usb.hdmi"),
                    ("device.id", "40"),
                    ("card.profile.device", "2"),
                ]),
                params: vec![volume_props(1.0, false)],
            },
        );
        graph.links.insert(80, (70, 50));

        let state = graph.state();
        let usb = state.device(DeviceId(50)).unwrap();
        assert_eq!((usb.volume, usb.muted), (50, true), "the route's props");
        assert!(usb.default);
        assert_eq!(usb.active_port.as_deref(), Some("analog-output-headphones"));
        assert_eq!(usb.ports.len(), 1, "the speaker route is another profile's");
        assert!(!usb.ports[0].available);
        let hdmi = state.device(DeviceId(55)).unwrap();
        assert_eq!(
            hdmi.active_port.as_deref(),
            Some("hdmi-output-0"),
            "no active route: the first port, as pactl reports"
        );
        assert_eq!(
            (hdmi.volume, hdmi.muted),
            (100, false),
            "no route: its own props"
        );
        assert_eq!(usb.card, Some(CardId(40)));
        let bluetooth = state.device(DeviceId(60)).unwrap();
        assert_eq!(
            (bluetooth.volume, bluetooth.muted),
            (100, false),
            "its own props"
        );
        assert!(bluetooth.bluetooth && !bluetooth.default);
        let firefox = state.stream(StreamId(70)).unwrap();
        assert_eq!(firefox.application, "Firefox");
        assert_eq!(firefox.device, Some(DeviceId(50)));
        assert_eq!(firefox.volume, 50);
        assert_eq!(state.cards[0].description, "USB Headset");
    }

    /// A new sequence number starts a fresh list; so does an index that does
    /// not follow; indexes that skip but advance, as an `enum_params` reply's
    /// do, stay one list.
    #[test]
    fn a_fresh_param_list_starts_on_a_new_sequence_or_a_falling_index() {
        let mut last = None;
        assert!(Batch::starts_afresh(&mut last, 0, 0));
        assert!(!Batch::starts_afresh(&mut last, 0, 1));
        assert!(Batch::starts_afresh(&mut last, 7, 2), "an enum reply");
        assert!(!Batch::starts_afresh(&mut last, 7, 3));
        assert!(
            !Batch::starts_afresh(&mut last, 7, 5),
            "skipping is still one list"
        );
        assert!(
            Batch::starts_afresh(&mut last, 7, 0),
            "a resend under the same seq"
        );
    }

    #[test]
    fn a_monitor_stream_and_a_video_node_are_not_shown() {
        assert_eq!(
            role("Stream/Input/Audio", &props(&[("stream.monitor", "true")])),
            None
        );
        assert_eq!(role("Video/Source", &props(&[])), None);
        assert_eq!(
            role("Audio/Source", &props(&[])),
            Some(Role::Device(Direction::Input))
        );
    }
}
