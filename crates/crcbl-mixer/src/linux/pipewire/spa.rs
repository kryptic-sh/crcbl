//! The SPA and PipeWire numbers the client speaks in.
//!
//! Each is the value of the C constant it is named after, read from the
//! headers PipeWire 1.6.9 installs (`spa-0.2` and `pipewire-0.3`) by compiling
//! a program that printed them, so none is a transcription from memory. The
//! protocol fixes them: a server of any version reads these same numbers.

/// `SPA_TYPE_OBJECT_Props`.
pub(crate) const OBJECT_PROPS: u32 = 262_146;
/// `SPA_TYPE_OBJECT_ParamProfile`.
pub(crate) const OBJECT_PARAM_PROFILE: u32 = 262_151;
/// `SPA_TYPE_OBJECT_ParamRoute`.
pub(crate) const OBJECT_PARAM_ROUTE: u32 = 262_153;

/// `SPA_PARAM_Props`.
pub(crate) const PARAM_PROPS: u32 = 2;
/// `SPA_PARAM_EnumProfile`.
pub(crate) const PARAM_ENUM_PROFILE: u32 = 8;
/// `SPA_PARAM_Profile`.
pub(crate) const PARAM_PROFILE: u32 = 9;
/// `SPA_PARAM_EnumRoute`.
pub(crate) const PARAM_ENUM_ROUTE: u32 = 12;
/// `SPA_PARAM_Route`.
pub(crate) const PARAM_ROUTE: u32 = 13;

/// `SPA_PROP_mute`.
pub(crate) const PROP_MUTE: u32 = 65_540;
/// `SPA_PROP_channelVolumes`.
pub(crate) const PROP_CHANNEL_VOLUMES: u32 = 65_544;

/// `SPA_PARAM_PROFILE_index`.
pub(crate) const PROFILE_INDEX: u32 = 1;
/// `SPA_PARAM_PROFILE_name`.
pub(crate) const PROFILE_NAME: u32 = 2;
/// `SPA_PARAM_PROFILE_description`.
pub(crate) const PROFILE_DESCRIPTION: u32 = 3;
/// `SPA_PARAM_PROFILE_available`.
pub(crate) const PROFILE_AVAILABLE: u32 = 5;
/// `SPA_PARAM_PROFILE_save`.
pub(crate) const PROFILE_SAVE: u32 = 8;

/// `SPA_PARAM_ROUTE_index`.
pub(crate) const ROUTE_INDEX: u32 = 1;
/// `SPA_PARAM_ROUTE_direction`.
pub(crate) const ROUTE_DIRECTION: u32 = 2;
/// `SPA_PARAM_ROUTE_device`.
pub(crate) const ROUTE_DEVICE: u32 = 3;
/// `SPA_PARAM_ROUTE_name`.
pub(crate) const ROUTE_NAME: u32 = 4;
/// `SPA_PARAM_ROUTE_description`.
pub(crate) const ROUTE_DESCRIPTION: u32 = 5;
/// `SPA_PARAM_ROUTE_available`.
pub(crate) const ROUTE_AVAILABLE: u32 = 7;
/// `SPA_PARAM_ROUTE_profiles`.
pub(crate) const ROUTE_PROFILES: u32 = 9;
/// `SPA_PARAM_ROUTE_props`.
pub(crate) const ROUTE_PROPS: u32 = 10;
/// `SPA_PARAM_ROUTE_devices`.
pub(crate) const ROUTE_DEVICES: u32 = 11;
/// `SPA_PARAM_ROUTE_save`.
pub(crate) const ROUTE_SAVE: u32 = 13;

/// `SPA_DIRECTION_INPUT`.
pub(crate) const DIRECTION_INPUT: u32 = 0;
/// `SPA_DIRECTION_OUTPUT`.
pub(crate) const DIRECTION_OUTPUT: u32 = 1;
/// `SPA_PARAM_AVAILABILITY_no`.
pub(crate) const AVAILABILITY_NO: u32 = 1;

/// `PW_VERSION_CORE`.
pub(crate) const VERSION_CORE: i32 = 4;
/// `PW_VERSION_REGISTRY`.
pub(crate) const VERSION_REGISTRY: i32 = 3;
/// `PW_VERSION_NODE`, `PW_VERSION_DEVICE` and `PW_VERSION_METADATA`, which are
/// all 3.
pub(crate) const VERSION_OBJECT: i32 = 3;

/// The core's object id, fixed by the protocol.
pub(crate) const CORE_ID: u32 = 0;
/// The client object's id, fixed by the protocol.
pub(crate) const CLIENT_ID: u32 = 1;

/// `PW_CORE_METHOD_HELLO`.
pub(crate) const CORE_HELLO: u8 = 1;
/// `PW_CORE_METHOD_SYNC`.
pub(crate) const CORE_SYNC: u8 = 2;
/// `PW_CORE_METHOD_PONG`.
pub(crate) const CORE_PONG: u8 = 3;
/// `PW_CORE_METHOD_GET_REGISTRY`.
pub(crate) const CORE_GET_REGISTRY: u8 = 5;
/// `PW_CORE_EVENT_DONE`.
pub(crate) const CORE_EVENT_DONE: u8 = 1;
/// `PW_CORE_EVENT_PING`.
pub(crate) const CORE_EVENT_PING: u8 = 2;
/// `PW_CORE_EVENT_ERROR`.
pub(crate) const CORE_EVENT_ERROR: u8 = 3;

/// `PW_CLIENT_METHOD_UPDATE_PROPERTIES`.
pub(crate) const CLIENT_UPDATE_PROPERTIES: u8 = 2;

/// `PW_REGISTRY_METHOD_BIND`.
pub(crate) const REGISTRY_BIND: u8 = 1;
/// `PW_REGISTRY_EVENT_GLOBAL`.
pub(crate) const REGISTRY_EVENT_GLOBAL: u8 = 0;
/// `PW_REGISTRY_EVENT_GLOBAL_REMOVE`.
pub(crate) const REGISTRY_EVENT_GLOBAL_REMOVE: u8 = 1;

/// `PW_NODE_METHOD_SUBSCRIBE_PARAMS`, and the device's, which is the same.
pub(crate) const SUBSCRIBE_PARAMS: u8 = 1;
/// `PW_DEVICE_METHOD_ENUM_PARAMS`, and the node's, which is the same.
pub(crate) const ENUM_PARAMS: u8 = 2;
/// `SPA_PARAM_INFO_SERIAL`: flips each time a param's value changes.
pub(crate) const PARAM_INFO_SERIAL: i32 = 1;
/// `PW_NODE_METHOD_SET_PARAM`, and the device's, which is the same.
pub(crate) const SET_PARAM: u8 = 3;
/// `PW_NODE_EVENT_INFO`, and the device's, which is the same.
pub(crate) const EVENT_INFO: u8 = 0;
/// `PW_NODE_EVENT_PARAM`, and the device's, which is the same.
pub(crate) const EVENT_PARAM: u8 = 1;

/// `PW_METADATA_METHOD_SET_PROPERTY`.
pub(crate) const METADATA_SET_PROPERTY: u8 = 1;
/// `PW_METADATA_EVENT_PROPERTY`.
pub(crate) const METADATA_EVENT_PROPERTY: u8 = 0;

/// `PW_TYPE_INTERFACE_Node`.
pub(crate) const INTERFACE_NODE: &str = "PipeWire:Interface:Node";
/// `PW_TYPE_INTERFACE_Device`.
pub(crate) const INTERFACE_DEVICE: &str = "PipeWire:Interface:Device";
/// `PW_TYPE_INTERFACE_Metadata`.
pub(crate) const INTERFACE_METADATA: &str = "PipeWire:Interface:Metadata";
/// `PW_TYPE_INTERFACE_Link`.
pub(crate) const INTERFACE_LINK: &str = "PipeWire:Interface:Link";
