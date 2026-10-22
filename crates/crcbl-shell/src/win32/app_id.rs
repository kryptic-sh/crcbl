//! [`WindowDesc::app_id`](crate::WindowDesc::app_id) as the window's
//! Application User Model ID: what decides taskbar grouping on Windows and
//! which shortcut a window is matched to — Win32's `WM_CLASS`.
//!
//! **Per window, never per process.** `SetCurrentProcessExplicitAppUserModelID`
//! would set it for every window of the process, including a host
//! application's own when the engine is embedded in one, and has to be called
//! before any UI appears. The window's property store (`System.AppUserModel.ID`
//! through `SHGetPropertyStoreForWindow`) labels this window alone, and a
//! window-level ID overrides a process-level one, so a host's own choice is
//! untouched.
//!
//! **Cleared before the window is destroyed.** `SHGetPropertyStoreForWindow`'s
//! documentation: "A window's properties must be removed before the window is
//! closed. If this is not done, the resources used by those properties are not
//! returned to the system." [`clear`] does that by setting the property to
//! `VT_EMPTY`.
//!
//! The property store is a COM object; its interface is declared here by hand,
//! in this backend's FFI style, rather than through the `windows` crate, which
//! the backend does not use. `SetValue` stores a value on the window at once,
//! so `Commit` is never called.

use core::ffi::c_void;
use core::ptr;

use super::ffi::{self, Handle};

/// The longest ID Windows accepts, in characters.
const MAX_LEN: usize = 128;

/// Whether `app_id` is an ID Windows accepts: present, at most [`MAX_LEN`]
/// characters, and without spaces. The documented form is
/// `CompanyName.ProductName.SubProduct.VersionInformation`; only these two
/// limits are stated as rules.
#[must_use]
pub(super) fn is_valid(app_id: &str) -> bool {
    !app_id.is_empty() && app_id.encode_utf16().count() <= MAX_LEN && !app_id.contains(' ')
}

/// Labels `hwnd` with `app_id`. Answers whether the property was set, which
/// is when [`clear`] must run before the window is destroyed.
///
/// An empty `app_id` is the default and labels nothing. One Windows would not
/// accept is not set, with a warning: refusing the window over its taskbar
/// grouping would make a descriptor that works on every other backend fail
/// here.
pub(super) fn set(hwnd: Handle, app_id: &str) -> bool {
    if app_id.is_empty() {
        return false;
    }
    if !is_valid(app_id) {
        crcbl_core::log::warn!(
            "win32: app_id {app_id:?} is not an AppUserModelID (at most {MAX_LEN} characters, \
             no spaces); the window is not labelled with it"
        );
        return false;
    }
    let Some(wide) = ffi::wide(app_id) else {
        // `create_window` refuses a NUL before the window exists.
        return false;
    };
    let value = PropVariant {
        vt: VT_LPWSTR,
        reserved: [0; 3],
        value: PropValue {
            pointer: wide.as_ptr(),
        },
    };
    // `SetValue` copies the string, so `wide` only has to outlive the call.
    store_value(hwnd, &value, "set")
}

/// Removes the label [`set`] put on `hwnd`, as the window's property store
/// requires before the window is destroyed.
pub(super) fn clear(hwnd: Handle) {
    let empty = PropVariant {
        vt: VT_EMPTY,
        reserved: [0; 3],
        value: PropValue { padding: [0; 2] },
    };
    store_value(hwnd, &empty, "clear");
}

/// Writes `value` as `hwnd`'s `System.AppUserModel.ID`, logging a refusal.
fn store_value(hwnd: Handle, value: &PropVariant, what: &str) -> bool {
    let mut store: *mut c_void = ptr::null_mut();
    // SAFETY: `hwnd` is a live window of this thread; the IID names the
    // interface `store` receives, and `store` is a live out-pointer.
    let got = unsafe { SHGetPropertyStoreForWindow(hwnd, &IID_IPROPERTY_STORE, &raw mut store) };
    if got < 0 || store.is_null() {
        crcbl_core::log::warn!(
            "win32: no property store for a window ({got:#010x}); could not {what} its app_id"
        );
        return false;
    }
    // SAFETY: `store` is an `IPropertyStore` the call above returned with one
    // reference for us. Its first field is the vtable, laid out as
    // `PropertyStoreVtbl` declares, and both pointers stay valid until the
    // `Release` below, which gives that reference back exactly once.
    let stored = unsafe {
        let vtbl = *store.cast::<*const PropertyStoreVtbl>();
        let stored = ((*vtbl).set_value)(store, &PKEY_APP_USER_MODEL_ID, value);
        ((*vtbl).release)(store);
        stored
    };
    if stored < 0 {
        crcbl_core::log::warn!(
            "win32: the window's property store refused to {what} its app_id ({stored:#010x})"
        );
    }
    stored >= 0
}

/// `GUID`, as `IPropertyStore`'s calls take it.
#[repr(C)]
pub(super) struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// `PROPERTYKEY`: a property set's id and the property's number in it.
#[repr(C)]
pub(super) struct PropertyKey {
    fmtid: Guid,
    pid: u32,
}

/// `IID_IPropertyStore`, `{886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}`.
const IID_IPROPERTY_STORE: Guid = Guid {
    data1: 0x886d_8eeb,
    data2: 0x8cf2,
    data3: 0x4446,
    data4: [0x8d, 0x02, 0xcd, 0xba, 0x1d, 0xbd, 0xcf, 0x99],
};

/// `PKEY_AppUserModel_ID`: `{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}`, 5.
pub(super) const PKEY_APP_USER_MODEL_ID: PropertyKey = PropertyKey {
    fmtid: Guid {
        data1: 0x9f4c_2855,
        data2: 0x9f79,
        data3: 0x4b39,
        data4: [0xa8, 0xd0, 0xe1, 0xd4, 0x2d, 0xe1, 0xd5, 0xf3],
    },
    pid: 5,
};

/// `VT_EMPTY`: no value, which removes the property.
const VT_EMPTY: u16 = 0;
/// `VT_LPWSTR`: a pointer to a NUL-terminated UTF-16 string.
const VT_LPWSTR: u16 = 31;

/// `PROPVARIANT`: a type tag, three reserved words, then the value. The value
/// is a union two pointers wide, which is what makes the whole 24 bytes on a
/// 64-bit target and 16 on a 32-bit one, as the SDK's.
#[repr(C)]
pub(super) struct PropVariant {
    vt: u16,
    reserved: [u16; 3],
    value: PropValue,
}

/// `PROPVARIANT`'s value: here only ever a string pointer or nothing.
#[repr(C)]
union PropValue {
    pointer: *const u16,
    padding: [usize; 2],
}

/// `IPropertyStore`'s vtable: `IUnknown`'s three methods, then its own five,
/// in declaration order. Only `Release` and `SetValue` are called.
#[repr(C)]
struct PropertyStoreVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_count: usize,
    get_at: usize,
    get_value: usize,
    set_value:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *const PropVariant) -> i32,
    commit: usize,
}

#[link(name = "shell32")]
unsafe extern "system" {
    // Answers an `HRESULT` and the window's property store, which the caller
    // releases.
    fn SHGetPropertyStoreForWindow(hwnd: Handle, riid: *const Guid, ppv: *mut *mut c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_id_windows_accepts_is_short_and_has_no_spaces() {
        assert!(is_valid("Kryptic.Crcbl.Sandbox"));
        assert!(is_valid(&"a".repeat(MAX_LEN)));
        assert!(!is_valid(&"a".repeat(MAX_LEN + 1)), "one past the limit");
        assert!(!is_valid("Kryptic Crcbl"), "a space");
        assert!(!is_valid(""), "nothing");
    }

    /// The hand-declared layouts are the SDK's: the `windows` crate's
    /// definitions are the witness, as generated from Windows' own metadata.
    #[test]
    fn the_property_layouts_are_the_sdks() {
        use windows::Win32::Foundation::PROPERTYKEY;
        use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
        assert_eq!(size_of::<PropVariant>(), size_of::<PROPVARIANT>());
        assert_eq!(align_of::<PropVariant>(), align_of::<PROPVARIANT>());
        assert_eq!(size_of::<PropertyKey>(), size_of::<PROPERTYKEY>());
        assert_eq!(core::mem::offset_of!(PropVariant, value), 8);
        let key = windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        assert_eq!(key.pid, PKEY_APP_USER_MODEL_ID.pid);
        let ours = &PKEY_APP_USER_MODEL_ID.fmtid;
        assert_eq!(
            key.fmtid,
            windows::core::GUID::from_values(ours.data1, ours.data2, ours.data3, ours.data4)
        );
        assert_eq!(
            <windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore as windows::core::Interface>::IID,
            windows::core::GUID::from_values(
                IID_IPROPERTY_STORE.data1,
                IID_IPROPERTY_STORE.data2,
                IID_IPROPERTY_STORE.data3,
                IID_IPROPERTY_STORE.data4,
            )
        );
    }
}
