//! The native protocol's commands, and the layouts of the replies the mixer
//! reads.
//!
//! The command numbers are PulseAudio's `pulsecore/native-common.h` enum, read
//! from `module-protocol-pulse`'s `commands.h` (which mirrors it) by compiling a
//! program that printed them. Each layout is the server's, field by field and
//! version by version, as `module-protocol-pulse`'s `pulse-server.c` writes it
//! (`fill_sink_info`, `fill_source_info`, `fill_sink_input_info`,
//! `fill_source_output_info`, `fill_card_info`, `do_get_server_info`) and as
//! PulseAudio's `protocol-native.c` writes the same.

use std::collections::BTreeMap;

use super::tagstruct::{DecodeError, Reader};

pub(crate) const ERROR: u32 = 0;
pub(crate) const REPLY: u32 = 2;
pub(crate) const AUTH: u32 = 8;
pub(crate) const SET_CLIENT_NAME: u32 = 9;
pub(crate) const GET_SERVER_INFO: u32 = 20;
pub(crate) const GET_SINK_INFO_LIST: u32 = 22;
pub(crate) const GET_SOURCE_INFO_LIST: u32 = 24;
pub(crate) const GET_SINK_INPUT_INFO_LIST: u32 = 30;
pub(crate) const GET_SOURCE_OUTPUT_INFO_LIST: u32 = 32;
pub(crate) const SUBSCRIBE: u32 = 35;
pub(crate) const SET_SINK_VOLUME: u32 = 36;
pub(crate) const SET_SINK_INPUT_VOLUME: u32 = 37;
pub(crate) const SET_SOURCE_VOLUME: u32 = 38;
pub(crate) const SET_SINK_MUTE: u32 = 39;
pub(crate) const SET_SOURCE_MUTE: u32 = 40;
pub(crate) const SET_DEFAULT_SINK: u32 = 44;
pub(crate) const SET_DEFAULT_SOURCE: u32 = 45;
pub(crate) const SUBSCRIBE_EVENT: u32 = 66;
pub(crate) const MOVE_SINK_INPUT: u32 = 67;
pub(crate) const MOVE_SOURCE_OUTPUT: u32 = 68;
pub(crate) const SET_SINK_INPUT_MUTE: u32 = 69;
pub(crate) const GET_CARD_INFO_LIST: u32 = 89;
pub(crate) const SET_CARD_PROFILE: u32 = 90;
pub(crate) const SET_SINK_PORT: u32 = 96;
pub(crate) const SET_SOURCE_PORT: u32 = 97;
pub(crate) const SET_SOURCE_OUTPUT_VOLUME: u32 = 98;
pub(crate) const SET_SOURCE_OUTPUT_MUTE: u32 = 99;

/// The protocol version this client speaks: PulseAudio 15's, which
/// `pipewire-pulse` speaks too. The connection uses the lower of this and the
/// server's.
pub(crate) const VERSION: u32 = 35;

/// `PA_ERR_ACCESS`, the error a server answers a client it does not admit
/// with: one whose cookie does not match and whose user is not its own.
pub(crate) const ERR_ACCESS: u32 = 1;

/// `PA_INVALID_INDEX`: no index.
pub(crate) const NO_INDEX: u32 = u32::MAX;

/// `PA_VOLUME_NORM`: the volume that is 100 %.
pub(crate) const VOLUME_NORM: u32 = 0x1_0000;

/// The facility bits of a subscription mask and event (`PA_SUBSCRIPTION_*`).
pub(crate) mod facility {
    pub(crate) const SINK: u32 = 0;
    pub(crate) const SOURCE: u32 = 1;
    pub(crate) const SINK_INPUT: u32 = 2;
    pub(crate) const SOURCE_OUTPUT: u32 = 3;
    pub(crate) const SERVER: u32 = 7;
    pub(crate) const CARD: u32 = 9;
    /// The low bits of an event that name its facility.
    pub(crate) const MASK: u32 = 0x0f;
}

/// A port: `pa_sink_port_info` and its source and card counterparts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Port {
    pub(crate) name: String,
    pub(crate) description: String,
    /// `PA_PORT_AVAILABLE_*`: 0 unknown, 1 no, 2 yes.
    pub(crate) available: u32,
}

/// A sink or a source.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeviceInfo {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) volumes: Vec<u32>,
    pub(crate) muted: bool,
    /// For a source, the sink it monitors, or [`NO_INDEX`].
    pub(crate) monitor_of: u32,
    pub(crate) props: BTreeMap<String, String>,
    pub(crate) card: u32,
    pub(crate) ports: Vec<Port>,
    pub(crate) active_port: Option<String>,
}

/// A sink input or a source output.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StreamInfo {
    pub(crate) index: u32,
    pub(crate) name: Option<String>,
    /// The sink or source it is on, or [`NO_INDEX`].
    pub(crate) device: u32,
    pub(crate) volumes: Vec<u32>,
    pub(crate) muted: bool,
    pub(crate) props: BTreeMap<String, String>,
}

/// A card.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CardInfo {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) profiles: Vec<Port>,
    pub(crate) active_profile: Option<String>,
    pub(crate) props: BTreeMap<String, String>,
}

/// `do_get_server_info`'s answer, as far as the mixer needs it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ServerInfo {
    pub(crate) default_sink: Option<String>,
    pub(crate) default_source: Option<String>,
}

pub(crate) fn server_info(reader: &mut Reader<'_>) -> Result<ServerInfo, DecodeError> {
    for _ in 0..4 {
        reader.skip()?; // name, version, user name, host name
    }
    reader.skip()?; // sample spec
    Ok(ServerInfo {
        default_sink: reader.str_tag()?,
        default_source: reader.str_tag()?,
    })
}

/// One `fill_sink_info` or `fill_source_info` entry: the two share a layout.
pub(crate) fn device_info(
    reader: &mut Reader<'_>,
    version: u32,
) -> Result<DeviceInfo, DecodeError> {
    let index = reader.u32_tag()?;
    let name = reader.str_tag()?.unwrap_or_default();
    let description = reader.str_tag()?.unwrap_or_default();
    reader.skip()?; // sample spec
    reader.skip()?; // channel map
    reader.skip()?; // module index
    let volumes = reader.volumes_tag()?;
    let muted = reader.bool_tag()?;
    let monitor_of = reader.u32_tag()?; // a sink's monitor source, a source's sink
    reader.skip()?; // that one's name
    reader.skip()?; // latency
    reader.skip()?; // driver
    reader.skip()?; // flags
    let mut props = BTreeMap::new();
    if version >= 13 {
        props = reader.props_tag()?;
        reader.skip()?; // requested latency
    }
    let mut card = NO_INDEX;
    if version >= 15 {
        reader.skip()?; // base volume
        reader.skip()?; // state
        reader.skip()?; // volume steps
        card = reader.u32_tag()?;
    }
    let mut ports = Vec::new();
    let mut active_port = None;
    if version >= 16 {
        let count = reader.u32_tag()?;
        for _ in 0..count {
            let name = reader.str_tag()?.unwrap_or_default();
            let description = reader.str_tag()?.unwrap_or_default();
            reader.skip()?; // priority
            let available = if version >= 24 { reader.u32_tag()? } else { 0 };
            if version >= 34 {
                reader.skip()?; // availability group
                reader.skip()?; // type
            }
            ports.push(Port {
                name,
                description,
                available,
            });
        }
        active_port = reader.str_tag()?;
    }
    if version >= 21 {
        let formats = reader.u8_tag()?;
        for _ in 0..formats {
            reader.skip()?;
        }
    }
    Ok(DeviceInfo {
        index,
        name,
        description,
        volumes,
        muted,
        monitor_of,
        props,
        card,
        ports,
        active_port,
    })
}

/// One `fill_sink_input_info` entry.
pub(crate) fn sink_input_info(
    reader: &mut Reader<'_>,
    version: u32,
) -> Result<StreamInfo, DecodeError> {
    let index = reader.u32_tag()?;
    let name = reader.str_tag()?;
    reader.skip()?; // module index
    reader.skip()?; // client index
    let device = reader.u32_tag()?;
    reader.skip()?; // sample spec
    reader.skip()?; // channel map
    let volumes = reader.volumes_tag()?;
    for _ in 0..4 {
        reader.skip()?; // latency, sink latency, resample method, driver
    }
    let muted = if version >= 11 {
        reader.bool_tag()?
    } else {
        false
    };
    let props = if version >= 13 {
        reader.props_tag()?
    } else {
        BTreeMap::new()
    };
    if version >= 19 {
        reader.skip()?; // corked
    }
    if version >= 20 {
        reader.skip()?; // has volume
        reader.skip()?; // volume writable
    }
    if version >= 21 {
        reader.skip()?; // format
    }
    Ok(StreamInfo {
        index,
        name,
        device,
        volumes,
        muted,
        props,
    })
}

/// One `fill_source_output_info` entry.
pub(crate) fn source_output_info(
    reader: &mut Reader<'_>,
    version: u32,
) -> Result<StreamInfo, DecodeError> {
    let index = reader.u32_tag()?;
    let name = reader.str_tag()?;
    reader.skip()?; // module index
    reader.skip()?; // client index
    let device = reader.u32_tag()?;
    reader.skip()?; // sample spec
    reader.skip()?; // channel map
    for _ in 0..4 {
        reader.skip()?; // latency, source latency, resample method, driver
    }
    let props = if version >= 13 {
        reader.props_tag()?
    } else {
        BTreeMap::new()
    };
    if version >= 19 {
        reader.skip()?; // corked
    }
    let (mut volumes, mut muted) = (Vec::new(), false);
    if version >= 22 {
        volumes = reader.volumes_tag()?;
        muted = reader.bool_tag()?;
        reader.skip()?; // has volume
        reader.skip()?; // volume writable
        reader.skip()?; // format
    }
    Ok(StreamInfo {
        index,
        name,
        device,
        volumes,
        muted,
        props,
    })
}

/// One `fill_card_info` entry.
pub(crate) fn card_info(reader: &mut Reader<'_>, version: u32) -> Result<CardInfo, DecodeError> {
    let index = reader.u32_tag()?;
    let name = reader.str_tag()?.unwrap_or_default();
    reader.skip()?; // module index
    reader.skip()?; // driver
    let count = reader.u32_tag()?;
    let mut profiles = Vec::new();
    for _ in 0..count {
        let name = reader.str_tag()?.unwrap_or_default();
        let description = reader.str_tag()?.unwrap_or_default();
        reader.skip()?; // sinks
        reader.skip()?; // sources
        reader.skip()?; // priority
        // A profile's `available` is a yes/no, 1/0; written as a port's
        // `PA_PORT_AVAILABLE_*` here so both read alike: 1 no, 2 yes.
        let available = if version >= 29 {
            if reader.u32_tag()? == 0 { 1 } else { 2 }
        } else {
            0
        };
        profiles.push(Port {
            name,
            description,
            available,
        });
    }
    let active_profile = reader.str_tag()?;
    let props = reader.props_tag()?;
    if version >= 26 {
        let ports = reader.u32_tag()?;
        for _ in 0..ports {
            for _ in 0..4 {
                reader.skip()?; // name, description, priority, available
            }
            reader.skip()?; // direction
            reader.skip()?; // proplist
            let profiles = reader.u32_tag()?;
            for _ in 0..profiles {
                reader.skip()?;
            }
            if version >= 27 {
                reader.skip()?; // latency offset
            }
            if version >= 34 {
                reader.skip()?; // availability group
                reader.skip()?; // type
            }
        }
    }
    Ok(CardInfo {
        index,
        name,
        profiles,
        active_profile,
        props,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::pulse::tagstruct::Writer;

    /// A sink entry written field by field as `fill_sink_info` writes it at
    /// version 35 reads back with its volume, ports and active port, and the
    /// reader ends exactly at the entry's end.
    #[test]
    fn a_sink_entry_at_version_35_reads_whole() {
        let mut w = Writer::default();
        w.u32(61)
            .str(Some("alsa_output.usb"))
            .str(Some("USB Headset"));
        let mut bytes = w.into_bytes();
        bytes.extend_from_slice(&[b'a', 3, 2, 0, 0, 0xbb, 0x80]); // s16le, 2 ch, 48 kHz
        bytes.extend_from_slice(&[b'm', 2, 1, 2]);
        let mut w = Writer::default();
        w.u32(NO_INDEX)
            .volumes(VOLUME_NORM / 2, 2)
            .bool(false)
            .u32(62)
            .str(Some("monitor"));
        bytes.extend(w.into_bytes());
        bytes.push(b'U');
        bytes.extend_from_slice(&0u64.to_be_bytes());
        let mut w = Writer::default();
        w.str(Some("PipeWire"))
            .u32(0)
            .props(&[("device.form_factor", "headset")]);
        bytes.extend(w.into_bytes());
        bytes.push(b'U');
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(b'V');
        bytes.extend_from_slice(&VOLUME_NORM.to_be_bytes());
        let mut w = Writer::default();
        w.u32(0).u32(65_537).u32(48).u32(1); // state, steps, card, n_ports
        w.str(Some("analog-output"))
            .str(Some("Analog Output"))
            .u32(100)
            .u32(2);
        w.str(None).u32(0); // availability group, type
        w.str(Some("analog-output"));
        bytes.extend(w.into_bytes());
        bytes.extend_from_slice(&[b'B', 0]); // no formats

        let mut reader = Reader::new(&bytes);
        let sink = device_info(&mut reader, VERSION).unwrap();
        assert!(reader.is_empty(), "the entry ends where it should");
        assert_eq!((sink.index, sink.name.as_str()), (61, "alsa_output.usb"));
        assert_eq!(sink.volumes, [VOLUME_NORM / 2, VOLUME_NORM / 2]);
        assert_eq!(sink.card, 48);
        assert_eq!(sink.ports.len(), 1);
        assert_eq!(sink.ports[0].available, 2);
        assert_eq!(sink.active_port.as_deref(), Some("analog-output"));
        assert_eq!(
            sink.props.get("device.form_factor").map(String::as_str),
            Some("headset")
        );
    }
}
