//! Making an endpoint the default, through `IPolicyConfig`.
//!
//! **Undocumented, and used on purpose.** Windows has no public API that
//! changes the default audio endpoint: the Sound control panel does it through
//! this private COM interface. The owner chose it over answering
//! [`Action::MakeDefault`](crate::Action::MakeDefault) with
//! [`MixerError::Unsupported`] on Windows (2026-10-22, `docs/backlog.md`,
//! "System audio control"), knowing a Windows update could change or remove
//! it; a class or interface that is no longer there fails `CoCreateInstance`,
//! and that answers [`MixerError::Unsupported`] rather than anything worse.
//!
//! **The declaration** is the community one, as AudioDeviceCmdlets carries it
//! (`SOURCE/IPolicyConfig.cs` and `PolicyConfigClient.cs`, checked
//! 2026-10-22): the interface's IID, the class's CLSID, and the methods in
//! vtable order. Only `SetDefaultEndpoint` is called; the methods before it
//! are declared so that it lands at its place in the vtable, with their
//! pointer arguments left untyped since nothing here calls them.

// The methods keep Windows' own names, which every other declaration of this
// interface uses, so this one can be checked against them line by line.
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Win32::Media::Audio::{ERole, eConsole, eMultimedia};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{BOOL, GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, PCWSTR};
use windows_core::interface;

use super::com;
use crate::MixerError;

/// `CPolicyConfigClient`, the class the Sound control panel makes.
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// The roles a device made default takes: console (games, system sounds,
/// most apps) and multimedia (music and video players), which together are
/// what the Sound control panel calls the "Default Device". The
/// communications role, its "Default Communication Device", is left as the
/// person set it: Windows keeps it apart so a headset can stay the device for
/// calls while music moves.
const ROLES: [ERole; 2] = [eConsole, eMultimedia];

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: BOOL, format: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(
        &self,
        device: PCWSTR,
        default: BOOL,
        default_period: *mut i64,
        minimum_period: *mut i64,
    ) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(
        &self,
        device: PCWSTR,
        effects: BOOL,
        key: *const c_void,
        value: *mut c_void,
    ) -> HRESULT;
    fn SetPropertyValue(
        &self,
        device: PCWSTR,
        effects: BOOL,
        key: *const c_void,
        value: *mut c_void,
    ) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: BOOL) -> HRESULT;
}

/// Makes the endpoint whose id string is `endpoint` the default for its
/// direction, in each of [`ROLES`]. Called on the mixer's thread, which is in
/// the multithreaded apartment.
pub(super) fn make_default(endpoint: &str) -> Result<(), MixerError> {
    // SAFETY: a CLSID constant, no outer object; the result is checked.
    let policy: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }.map_err(
            |_| {
                MixerError::Unsupported("this Windows has no IPolicyConfig to set the default with")
            },
        )?;
    let endpoint = HSTRING::from(endpoint);
    for role in ROLES {
        // SAFETY: `endpoint` is a NUL-terminated string that outlives the
        // call, and `role` one of `ERole`'s values.
        unsafe { policy.SetDefaultEndpoint(PCWSTR(endpoint.as_ptr()), role) }
            .ok()
            .map_err(|error| com::failed("making the endpoint the default", &error))?;
    }
    Ok(())
}
