//! The layout assertion every hand-written FFI structure in this crate is
//! checked with. Test-only.

/// The width of the field `select` picks out, read from its type alone.
pub(crate) fn field_size<T, F>(_select: fn(&T) -> &F) -> usize {
    size_of::<F>()
}

/// Asserts a structure's size and every field's offset and width — the Win32
/// shell's `assert_layout!` (`crcbl-shell`'s `win32::ffi` tests), which a test
/// in another crate cannot reach. The pattern names every field with no `..`,
/// so a field added without a row fails to compile.
macro_rules! assert_layout {
    ($ty:ident, $size:expr, { $($field:ident: $offset:expr, $width:expr;)+ }) => {{
        let _every_field_has_a_row: fn($ty) = |value| {
            let $ty { $($field: _),+ } = value;
        };
        assert_eq!(size_of::<$ty>(), $size, concat!("size of ", stringify!($ty)));
        $(
            assert_eq!(
                core::mem::offset_of!($ty, $field),
                $offset,
                concat!("offset of ", stringify!($ty), "::", stringify!($field))
            );
            assert_eq!(
                $crate::ffi_layout::field_size(|value: &$ty| &value.$field),
                $width,
                concat!("width of ", stringify!($ty), "::", stringify!($field))
            );
        )+
    }};
}

pub(crate) use assert_layout;
