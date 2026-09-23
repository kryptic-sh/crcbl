//! The fake `ISteamInput`: controllers scripted by the test, action handles
//! handed out by name, and every call that matters recorded.

use std::{
    collections::HashMap,
    ffi::{CStr, c_char, c_void},
};

use super::{accessor, script};
use crate::ffi::{
    ISteamInput, InputActionSetHandle, InputAnalogActionHandle, InputDigitalActionHandle,
    InputHandle,
    manifest::InputFns,
    structs::{InputAnalogActionData, InputDigitalActionData},
};

/// One scripted controller.
#[derive(Debug, Clone, Default)]
pub(crate) struct FakePad {
    /// What `GetInputTypeForHandle` answers.
    pub(crate) input_type: i32,
    /// The digital actions held, by name.
    pub(crate) held: Vec<&'static str>,
    /// Each analog action's `(x, y)`, by name; one not listed reads centred.
    pub(crate) analog: HashMap<&'static str, (f32, f32)>,
    /// Actions Steam reports as inactive (unbound in the active set), by
    /// name.
    pub(crate) inactive: Vec<&'static str>,
}

/// What the fake Steam Input answers, and what it has seen.
#[derive(Debug, Default)]
pub(crate) struct FakeInput {
    /// `Init` answers `false`.
    pub(crate) refuse_init: bool,
    /// `SetInputActionManifestFilePath` answers `false`.
    pub(crate) refuse_manifest: bool,
    /// Every handle lookup answers `0` while this is set, as Steam may
    /// before a configuration loads.
    pub(crate) not_ready: bool,
    /// Action names no handle is ever answered for.
    pub(crate) missing: Vec<&'static str>,
    /// The controllers, by `InputHandle_t`.
    pub(crate) pads: HashMap<InputHandle, FakePad>,
    /// Every name a handle was answered for; handle `n` is `names[n - 1]`.
    pub(crate) names: Vec<String>,
    /// Every call, by name, in order: `Init`, `SetInputActionManifestFilePath`,
    /// `EnableDeviceCallbacks`, `RunFrame`, `Shutdown`.
    pub(crate) log: Vec<&'static str>,
    /// Every lookup made, by name, including those answered `0`.
    pub(crate) lookups: Vec<String>,
    /// The manifest path Steam was given.
    pub(crate) manifest: Option<String>,
    /// What `Init` was passed for `bExplicitlyCallRunFrame`.
    pub(crate) explicit_run_frame: Option<bool>,
    /// Every `(controller, set)` `ActivateActionSet` was given.
    pub(crate) activated: Vec<(InputHandle, InputActionSetHandle)>,
}

/// The input group of the fake library.
pub(super) const FNS: InputFns = InputFns {
    accessor: fake_input_accessor,
    init: fake_init,
    shutdown: fake_shutdown,
    set_input_action_manifest_file_path: fake_set_manifest,
    run_frame: fake_run_frame,
    enable_device_callbacks: fake_enable_device_callbacks,
    get_action_set_handle: fake_handle,
    activate_action_set: fake_activate_action_set,
    get_digital_action_handle: fake_handle,
    get_digital_action_data: fake_digital,
    get_analog_action_handle: fake_handle,
    get_analog_action_data: fake_analog,
    get_input_type_for_handle: fake_input_type,
};

unsafe extern "C" fn fake_input_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::INPUT.accessor)
}

unsafe extern "C" fn fake_init(_: *mut ISteamInput, explicit_run_frame: bool) -> bool {
    script(|s| {
        s.input.log.push("Init");
        s.input.explicit_run_frame = Some(explicit_run_frame);
        !s.input.refuse_init
    })
}

unsafe extern "C" fn fake_shutdown(_: *mut ISteamInput) -> bool {
    script(|s| s.input.log.push("Shutdown"));
    true
}

unsafe extern "C" fn fake_set_manifest(_: *mut ISteamInput, path: *const c_char) -> bool {
    // SAFETY: the caller passes a NUL-terminated string.
    let path = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    script(|s| {
        s.input.log.push("SetInputActionManifestFilePath");
        s.input.manifest = Some(path);
        !s.input.refuse_manifest
    })
}

unsafe extern "C" fn fake_run_frame(_: *mut ISteamInput, _: bool) {
    script(|s| s.input.log.push("RunFrame"));
}

unsafe extern "C" fn fake_enable_device_callbacks(_: *mut ISteamInput) {
    script(|s| s.input.log.push("EnableDeviceCallbacks"));
}

/// Every handle lookup — set, digital and analog alike: `0` while not ready
/// or for a missing name, else the name's place in [`FakeInput::names`],
/// from 1.
unsafe extern "C" fn fake_handle(_: *mut ISteamInput, name: *const c_char) -> u64 {
    // SAFETY: the caller passes a NUL-terminated string.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    script(|s| {
        let input = &mut s.input;
        input.lookups.push(name.clone());
        if input.not_ready || input.missing.iter().any(|missing| *missing == name) {
            return 0;
        }
        let index = input
            .names
            .iter()
            .position(|known| *known == name)
            .unwrap_or_else(|| {
                input.names.push(name);
                input.names.len() - 1
            });
        u64::try_from(index + 1).unwrap()
    })
}

unsafe extern "C" fn fake_activate_action_set(
    _: *mut ISteamInput,
    controller: InputHandle,
    set: InputActionSetHandle,
) {
    script(|s| s.input.activated.push((controller, set)));
}

/// The name a handle was answered for.
fn name_of(input: &FakeInput, handle: u64) -> String {
    let index = usize::try_from(handle).unwrap() - 1;
    input.names[index].clone()
}

unsafe extern "C" fn fake_digital(
    _: *mut ISteamInput,
    controller: InputHandle,
    action: InputDigitalActionHandle,
) -> InputDigitalActionData {
    script(|s| {
        let name = name_of(&s.input, action);
        let pad = &s.input.pads[&controller];
        InputDigitalActionData {
            state: u8::from(pad.held.contains(&name.as_str())),
            active: u8::from(!pad.inactive.contains(&name.as_str())),
        }
    })
}

unsafe extern "C" fn fake_analog(
    _: *mut ISteamInput,
    controller: InputHandle,
    action: InputAnalogActionHandle,
) -> InputAnalogActionData {
    script(|s| {
        let name = name_of(&s.input, action);
        let pad = &s.input.pads[&controller];
        let (x, y) = pad.analog.get(name.as_str()).copied().unwrap_or_default();
        InputAnalogActionData {
            mode: 0,
            x,
            y,
            active: u8::from(!pad.inactive.contains(&name.as_str())),
        }
    })
}

unsafe extern "C" fn fake_input_type(_: *mut ISteamInput, controller: InputHandle) -> i32 {
    script(|s| {
        s.input
            .pads
            .get(&controller)
            .map_or(0, |pad| pad.input_type)
    })
}
