//! The executable's icon on its windows: the class's, at the system DPI,
//! and each top-level window's own, at its DPI.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use std::rc::Rc;

use super::ffi::{self, Handle};

/// The class's icon at the side the system metric `metric` gives at the
/// system DPI, or null when the executable carries none, which leaves the
/// system's default icon.
///
/// The fallback a window shows until [`Icons`] gives it its own at its DPI,
/// and what a panel or a popup keeps. Loaded at the class's registration,
/// which happens once and is never undone, so the icon lives as long as the
/// process and is never destroyed.
pub(super) fn class_icon(source: Option<&IconSource>, metric: i32) -> Handle {
    // SAFETY: `GetSystemMetrics` takes an index and returns a number.
    let side = unsafe { ffi::GetSystemMetrics(metric) };
    source
        .and_then(|source| source.load(side))
        .map_or(ptr::null_mut(), OwnedIcon::leak)
}

/// **The icon Explorer shows for the executable's file**: its first icon group
/// in resource-directory order (see [`first_icon_group`]), looked up once, from
/// which an image of any size is loaded. A program that embeds an icon gets it
/// on its windows' captions and taskbar buttons without telling the shell
/// anything.
#[derive(Debug)]
pub(super) struct IconSource {
    module: Handle,
    group: ResourceName,
}

impl IconSource {
    /// `module`'s first icon group, or `None` when it has none.
    ///
    /// # Safety
    ///
    /// `module` is a loaded module, and stays loaded for as long as the
    /// source does: every [`load`](Self::load) reads its resources.
    pub(super) unsafe fn of(module: Handle) -> Option<Self> {
        first_icon_group(module).map(|group| Self { module, group })
    }

    /// The group's image for a square of `side` pixels, or `None`, logged,
    /// when it would not load.
    fn load(&self, side: i32) -> Option<OwnedIcon> {
        // SAFETY: `module` is loaded, as `of` requires, and `group` is the
        // integer id or the NUL-terminated string `first_icon_group` read out
        // of its resources, kept alive for the call. Flags zero: a new icon of
        // exactly that size, picked from the group's images, which the caller
        // owns.
        let icon = unsafe {
            ffi::LoadImageW(
                self.module,
                self.group.as_ptr(),
                ffi::value::IMAGE_ICON,
                side,
                side,
                0,
            )
        };
        let icon = NonNull::new(icon).map(OwnedIcon);
        if icon.is_none() {
            // SAFETY: reads this thread's last error, which the call above set.
            let error = unsafe { ffi::GetLastError() };
            crcbl_core::log::debug!(
                "the executable's icon would not load at {side}px (Win32 error {error}); its \
                 windows keep the icon they have"
            );
        }
        icon
    }
}

/// An icon `LoadImageW` made without `LR_SHARED`, destroyed when this is
/// dropped. Neither `Clone` nor `Copy`, so each is destroyed once.
#[derive(Debug)]
struct OwnedIcon(NonNull<c_void>);

impl OwnedIcon {
    fn raw(&self) -> Handle {
        self.0.as_ptr()
    }

    /// Gives up ownership without destroying the icon, for one that lives as
    /// long as the process.
    fn leak(self) -> Handle {
        let icon = self.raw();
        core::mem::forget(self);
        icon
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: the icon was made by `LoadImageW` without `LR_SHARED`, so it
        // is ours to destroy, and this runs once: the handle leaves `self`
        // only through `leak`, which skips this.
        if unsafe { ffi::DestroyIcon(self.raw()) } == 0 {
            // SAFETY: reads this thread's last error, which the call above set.
            let error = unsafe { ffi::GetLastError() };
            crcbl_core::log::warn!("DestroyIcon failed with Win32 error {error}");
        }
    }
}

/// The sides of the big and small icons at one DPI, as the system scales
/// [`SM_CX_ICON`](ffi::value::SM_CX_ICON) and
/// [`SM_CX_SM_ICON`](ffi::value::SM_CX_SM_ICON) for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IconSides {
    big: i32,
    small: i32,
}

impl IconSides {
    fn at(dpi: u32) -> Self {
        // SAFETY: `GetSystemMetricsForDpi` takes an index and a DPI by value
        // and returns a number.
        let side = |metric| unsafe { ffi::GetSystemMetricsForDpi(metric, dpi) };
        Self {
            big: side(ffi::value::SM_CX_ICON),
            small: side(ffi::value::SM_CX_SM_ICON),
        }
    }
}

/// A window's own icons, at its DPI, over the class's at the system DPI.
///
/// The class's icons are loaded once at the system DPI, so on a monitor at
/// another scale the system would stretch them; these are loaded at the sizes
/// the window's DPI asks for and set with `WM_SETICON`, and again whenever
/// those sizes change. Only windows with a caption or a taskbar button have
/// them, and only when the executable carries an icon.
///
/// # Ownership
///
/// An icon set with `WM_SETICON` stays the sender's, and the system does not
/// destroy it with the window. Each one here is destroyed when it is
/// replaced, after its replacement is set, and the last ones when this is
/// dropped, which is after the window is destroyed: the shell keeps a window's
/// state until then on every path (`destroy_window` and the shell's own drop
/// destroy the window first, and a window the system destroyed is already
/// gone when its state is removed).
#[derive(Debug)]
pub(super) struct Icons {
    source: Rc<IconSource>,
    /// The sides last loaded, or `None` before the first load.
    sides: Option<IconSides>,
    big: Option<OwnedIcon>,
    small: Option<OwnedIcon>,
}

impl Icons {
    /// Gives `hwnd` the source's icons at `dpi`.
    pub(super) fn new(hwnd: Handle, source: Rc<IconSource>, dpi: u32) -> Self {
        let mut icons = Self {
            source,
            sides: None,
            big: None,
            small: None,
        };
        icons.set_dpi(hwnd, dpi);
        icons
    }

    /// Gives `hwnd` the source's icons at `dpi`, unless the ones it has are
    /// already the sizes `dpi` asks for.
    ///
    /// A slot whose icon would not load keeps the one it has, and is not
    /// tried again at the same size.
    pub(super) fn set_dpi(&mut self, hwnd: Handle, dpi: u32) {
        let sides = IconSides::at(dpi);
        if !needs_load(self.sides, sides) {
            return;
        }
        self.sides = Some(sides);
        for (slot, side, held) in [
            (ffi::value::ICON_BIG, sides.big, &mut self.big),
            (ffi::value::ICON_SMALL, sides.small, &mut self.small),
        ] {
            let Some(icon) = self.source.load(side) else {
                continue;
            };
            // SAFETY: `hwnd` is a live window of this thread, and `icon` is
            // kept in `held` for as long as it is set. The answer is the icon
            // the slot had, which is the one `held` drops below or none, and
            // is not needed.
            unsafe {
                ffi::SendMessageW(hwnd, ffi::msg::SET_ICON, slot, icon.raw() as ffi::Lparam);
            }
            // The icon replaced is no longer on the window, so dropping it here
            // destroys nothing in use.
            *held = Some(icon);
        }
    }
}

/// Whether a window whose icons were last loaded at `loaded` needs new ones
/// for `wanted`: on the first load, and when the sizes changed. A DPI change
/// that keeps both sides loads nothing.
fn needs_load(loaded: Option<IconSides>, wanted: IconSides) -> bool {
    loaded != Some(wanted)
}

/// A resource's name: an integer id or a string, as Win32 names one.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ResourceName {
    Id(u16),
    /// NUL-terminated UTF-16.
    Named(Vec<u16>),
}

impl ResourceName {
    /// The name as Win32 takes it: an id as `MAKEINTRESOURCEW` spells it, a
    /// string by address, valid while `self` is.
    fn as_ptr(&self) -> *const u16 {
        match self {
            Self::Id(id) => usize::from(*id) as *const u16,
            Self::Named(name) => name.as_ptr(),
        }
    }
}

/// The name of `module`'s first icon group, the one Explorer shows for its
/// file, or `None` when it has none.
///
/// First in resource-directory order, which is the order
/// `EnumResourceNamesW` walks: named groups alphabetically, then numbered
/// ones by id. Not resource id 1 by assumption: a program may number its icon
/// otherwise, or name it.
fn first_icon_group(module: Handle) -> Option<ResourceName> {
    /// Keeps the first name and stops the walk.
    ///
    /// # Safety
    ///
    /// `param` is the address of the `Option<ResourceName>` below, and `name`
    /// is an integer id or a NUL-terminated string valid for this call, as
    /// `EnumResourceNamesW` documents.
    unsafe extern "system" fn take_first(
        _module: Handle,
        _kind: *const u16,
        name: *mut u16,
        param: ffi::Lparam,
    ) -> ffi::Bool32 {
        let raw = name as usize;
        let name = if raw >> 16 == 0 {
            ResourceName::Id(raw as u16)
        } else {
            let mut len = 0;
            // SAFETY: a string name is NUL-terminated, so every read up to and
            // including the terminator is inside it.
            while unsafe { *name.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: the `len + 1` units just walked, terminator included.
            ResourceName::Named(unsafe { core::slice::from_raw_parts(name, len + 1) }.to_vec())
        };
        // SAFETY: `param` is the live `Option` `first_icon_group` passed, and
        // nothing else touches it while the walk runs.
        unsafe { *(param as *mut Option<ResourceName>) = Some(name) };
        0
    }

    let mut found: Option<ResourceName> = None;
    // SAFETY: `module` is a loaded module, the type is an integer resource
    // id, `take_first` has the `ENUMRESNAMEPROCW` signature, and `found`
    // outlives the call, which runs the callback only before it returns. The
    // result is not read: the walk stopped by the callback reports failure
    // too, and a module with no icon group reports it the same way.
    unsafe {
        ffi::EnumResourceNamesW(
            module,
            ffi::value::RT_GROUP_ICON as *const u16,
            take_first,
            core::ptr::from_mut(&mut found) as ffi::Lparam,
        );
    }
    found
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, HICON, ICONINFO};

    /// `shell32.dll`, mapped for its resources alone: the stand-in for an
    /// executable that embeds icons. The caller frees it.
    pub(in super::super) fn shell32() -> Handle {
        let name = ffi::wide("shell32.dll").expect("a literal");
        // SAFETY: `name` is NUL-terminated and outlives the call, and the
        // flags map the file for its resources without running any of it.
        let module = unsafe {
            ffi::LoadLibraryExW(
                name.as_ptr(),
                ptr::null_mut(),
                ffi::value::LOAD_AS_RESOURCES,
            )
        };
        assert!(!module.is_null(), "shell32.dll maps as a resource file");
        module
    }

    /// An icon's width and height in pixels, read back from its bitmaps, or
    /// `None` when the handle is not a live icon.
    pub(in super::super) fn pixel_size(icon: Handle) -> Option<(i32, i32)> {
        let mut info = ICONINFO::default();
        // SAFETY: `info` is a live out-parameter; a stale handle is refused,
        // not dereferenced.
        unsafe { GetIconInfo(HICON(icon), &raw mut info) }.ok()?;
        let mut bitmap = BITMAP::default();
        // SAFETY: `hbmColor` is the bitmap `GetIconInfo` just made, read into
        // a `BITMAP` of exactly the size passed. Both bitmaps are this test's
        // to delete.
        let read = unsafe {
            let read = GetObjectW(
                info.hbmColor.into(),
                size_of::<BITMAP>() as i32,
                Some((&raw mut bitmap).cast()),
            );
            assert!(DeleteObject(info.hbmColor.into()).as_bool());
            assert!(DeleteObject(info.hbmMask.into()).as_bool());
            read
        };
        assert_ne!(read, 0, "a colour icon's bitmap reads back");
        Some((bitmap.bmWidth, bitmap.bmHeight))
    }

    /// A test binary embeds no icon, so a program without one keeps the
    /// system's default rather than failing to register its class, and its
    /// windows get no icons of their own.
    #[test]
    fn an_executable_without_an_icon_leaves_the_class_icon_null() {
        // SAFETY: a null name asks for the executable's own module.
        let exe = unsafe { ffi::GetModuleHandleW(ptr::null()) };
        assert_eq!(first_icon_group(exe), None);
        // SAFETY: the executable's module is loaded for the whole run.
        assert!(unsafe { IconSource::of(exe) }.is_none());
        assert!(class_icon(None, ffi::value::SM_CX_ICON).is_null());
    }

    /// Its first group is id 1, the icon its file shows, and that group loads
    /// at both of the class's sizes.
    #[test]
    fn a_module_with_icons_gives_its_first_group_at_both_sizes() {
        let module = shell32();
        assert_eq!(first_icon_group(module), Some(ResourceName::Id(1)));
        // SAFETY: `module` stays mapped until after the source is dropped.
        let source = unsafe { IconSource::of(module) }.expect("shell32 has icons");
        for metric in [ffi::value::SM_CX_ICON, ffi::value::SM_CX_SM_ICON] {
            let icon = class_icon(Some(&source), metric);
            assert!(!icon.is_null(), "no icon at metric {metric}");
            // SAFETY: `icon` was just loaded without `LR_SHARED` and leaked
            // to this test, which owns it.
            unsafe { ffi::DestroyIcon(icon) };
        }
        drop(source);
        // SAFETY: `module` was loaded above and nothing borrows from it now.
        unsafe { ffi::FreeLibrary(module) };
    }

    /// The sides a window's icons are loaded at follow its DPI, not the
    /// system's: 100%, 150% and 200% of the 32 and 16 pixel defaults.
    #[test]
    fn icon_sides_scale_with_the_window_dpi() {
        for (dpi, big, small) in [(96, 32, 16), (144, 48, 24), (192, 64, 32)] {
            assert_eq!(IconSides::at(dpi), IconSides { big, small }, "at {dpi} DPI");
        }
    }

    /// Loaded at a DPI's sides, each icon is that many pixels: read back from
    /// its bitmap rather than assumed from the request.
    #[test]
    fn icons_load_at_the_pixel_size_of_each_dpi() {
        let module = shell32();
        // SAFETY: `module` stays mapped until after the source is dropped.
        let source = unsafe { IconSource::of(module) }.expect("shell32 has icons");
        for dpi in [96, 144, 192] {
            let sides = IconSides::at(dpi);
            for side in [sides.big, sides.small] {
                let icon = source.load(side).expect("shell32's icon loads");
                assert_eq!(pixel_size(icon.raw()), Some((side, side)), "at {dpi} DPI");
            }
        }
        drop(source);
        // SAFETY: `module` was loaded above and nothing borrows from it now.
        unsafe { ffi::FreeLibrary(module) };
    }

    /// Dropping an owned icon destroys it. A destroyed icon's handle is
    /// refused rather than reused at once: `USER` handles carry a reuse
    /// count in their high word.
    #[test]
    fn an_owned_icon_is_destroyed_when_dropped() {
        let module = shell32();
        // SAFETY: `module` stays mapped until after the source is dropped.
        let source = unsafe { IconSource::of(module) }.expect("shell32 has icons");
        let icon = source.load(32).expect("shell32's icon loads");
        let raw = icon.raw();
        assert!(pixel_size(raw).is_some(), "live before the drop");
        drop(icon);
        assert_eq!(pixel_size(raw), None, "destroyed by the drop");
        drop(source);
        // SAFETY: `module` was loaded above and nothing borrows from it now.
        unsafe { ffi::FreeLibrary(module) };
    }

    /// The first load always happens, a DPI change that keeps both sides
    /// loads nothing, and one that changes either side reloads.
    #[test]
    fn icons_reload_only_when_their_sides_change() {
        let at_96 = IconSides { big: 32, small: 16 };
        let at_144 = IconSides { big: 48, small: 24 };
        assert!(needs_load(None, at_96));
        assert!(!needs_load(Some(at_96), at_96));
        assert!(needs_load(Some(at_96), at_144));
        assert!(needs_load(Some(at_96), IconSides { big: 32, small: 20 }));
    }
}
