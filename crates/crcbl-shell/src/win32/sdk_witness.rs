//! A hand-declared Win32 structure checked against the `windows` crate's
//! declaration of the same structure, which is generated from Windows' own
//! metadata rather than written by hand.
//!
//! Test-only and Windows-only: the `windows` crate is a Windows
//! dev-dependency, and the backend never links it — these tests only read its
//! types. The test programs that hand-declare structures of their own include
//! this file by path, since they cannot reach a `pub(crate)` module.
//!
//! [`witness!`] takes one row per field of ours, naming the SDK field it must
//! match and how:
//!
//! - `is` — the same type at the same offset. Where the `windows` crate wraps
//!   a primitive in a newtype — a handle (`HWND(*mut c_void)`), `BOOL(i32)`,
//!   `WPARAM(usize)`, a flag type (`WNDCLASS_STYLES(u32)`) or a C enum
//!   (`DISPLAYCONFIG_ROTATION(i32)`) — the path ends in `.0`, and the
//!   comparison is with the newtype's inner primitive: that is what the C
//!   structure stores, and where a signedness mistake would show.
//! - `nested` — a structure inside a structure: the same offset, and our
//!   field's type must itself have been witnessed against the SDK field's.
//! - `width` — the same offset and size only, for a function pointer, whose
//!   SDK signature is spelled in newtypes no hand declaration shares.
//! - `after` — bytes the C structure leaves unnamed: our field must begin
//!   where the named SDK field ends.
//!
//! A path may run through a union, which is how each arm of a union the Rust
//! side flattens into one field is checked: the field's row names one arm,
//! and an `also` block repeats the field once for each other arm.
//!
//! The type checks are compile-time: a row whose types disagree does not
//! build. They project each field through a raw pointer inside a closure that
//! is never called, which is why each projection is `unsafe` and why none of
//! them ever runs.

/// `Self` was checked field by field against the SDK's `Sdk` by a
/// [`witness!`] test. What a `nested` row requires of its field's type.
pub(crate) trait Witnessed<Sdk> {}

/// One test asserting a hand declaration's size, alignment and every field
/// against the `windows` crate's declaration of the same structure; see the
/// module docs for the rows.
///
/// `struct` also names every field of ours in a pattern with no `..`, so a
/// field added without a row fails to compile. `union` cannot (a union
/// pattern names one field); `packed` is a structure the SDK declares under
/// `#pragma pack(1)`, whose alignment is asserted to be the SDK's 1 rather
/// than equal to ours.
///
/// The caller has [`Witnessed`] and this macro in scope.
macro_rules! witness {
    (
        $test:ident: $shape:tt $ours:ident => $sdk:ident {
            $($field:ident: $row:ident $($path:tt).+;)+
        }
        $(also { $($also:ident: $also_row:ident $($also_path:tt).+;)+ })?
    ) => {
        impl Witnessed<$sdk> for $ours {}

        #[test]
        fn $test() {
            witness!(@shape $shape $ours $sdk $($field)+);
            $(witness!(@row $row $ours $sdk $field $($path).+);)+
            $($(witness!(@row $also_row $ours $sdk $also $($also_path).+);)+)?
        }
    };

    (@shape struct $ours:ident $sdk:ident $($field:ident)+) => {
        let _every_field_has_a_row: fn($ours) = |value| {
            let $ours { $($field: _),+ } = value;
        };
        witness!(@size $ours $sdk);
        witness!(@align $ours $sdk);
    };
    (@shape union $ours:ident $sdk:ident $($field:ident)+) => {
        witness!(@size $ours $sdk);
        witness!(@align $ours $sdk);
    };
    (@shape packed $ours:ident $sdk:ident $($field:ident)+) => {
        let _every_field_has_a_row: fn($ours) = |value| {
            let $ours { $($field: _),+ } = value;
        };
        witness!(@size $ours $sdk);
        assert_eq!(
            align_of::<$sdk>(),
            1,
            concat!(stringify!($sdk), " is byte-packed")
        );
    };
    (@size $ours:ident $sdk:ident) => {
        assert_eq!(
            size_of::<$ours>(),
            size_of::<$sdk>(),
            concat!("size of ", stringify!($ours), " against ", stringify!($sdk))
        );
    };
    (@align $ours:ident $sdk:ident) => {
        assert_eq!(
            align_of::<$ours>(),
            align_of::<$sdk>(),
            concat!("alignment of ", stringify!($ours), " against ", stringify!($sdk))
        );
    };

    (@row is $ours:ident $sdk:ident $field:ident $($path:tt).+) => {{
        fn same_type<A, B, T>(_ours: fn(*const A) -> *const T, _sdk: fn(*const B) -> *const T) {}
        same_type(
            |value: *const $ours| unsafe { &raw const (*value).$field },
            |value: *const $sdk| unsafe { &raw const (*value).$($path).+ },
        );
        witness!(@offset $ours $sdk $field $($path).+);
    }};
    (@row nested $ours:ident $sdk:ident $field:ident $($path:tt).+) => {{
        fn witnessed<A, B, T: Witnessed<U>, U>(
            _ours: fn(*const A) -> *const T,
            _sdk: fn(*const B) -> *const U,
        ) {
        }
        witnessed(
            |value: *const $ours| unsafe { &raw const (*value).$field },
            |value: *const $sdk| unsafe { &raw const (*value).$($path).+ },
        );
        witness!(@offset $ours $sdk $field $($path).+);
    }};
    (@row width $ours:ident $sdk:ident $field:ident $($path:tt).+) => {{
        fn width<A, T>(_select: fn(*const A) -> *const T) -> usize {
            size_of::<T>()
        }
        assert_eq!(
            width(|value: *const $ours| unsafe { &raw const (*value).$field }),
            width(|value: *const $sdk| unsafe { &raw const (*value).$($path).+ }),
            concat!("width of ", stringify!($ours), "::", stringify!($field))
        );
        witness!(@offset $ours $sdk $field $($path).+);
    }};
    (@row after $ours:ident $sdk:ident $field:ident $($path:tt).+) => {{
        fn width<A, T>(_select: fn(*const A) -> *const T) -> usize {
            size_of::<T>()
        }
        assert_eq!(
            core::mem::offset_of!($ours, $field),
            core::mem::offset_of!($sdk, $($path).+)
                + width(|value: *const $sdk| unsafe { &raw const (*value).$($path).+ }),
            concat!(
                stringify!($ours), "::", stringify!($field),
                " begins where ", stringify!($sdk), "::", stringify!($($path).+), " ends"
            )
        );
    }};
    (@offset $ours:ident $sdk:ident $field:ident $($path:tt).+) => {
        assert_eq!(
            core::mem::offset_of!($ours, $field),
            core::mem::offset_of!($sdk, $($path).+),
            concat!(
                "offset of ", stringify!($ours), "::", stringify!($field),
                " against ", stringify!($sdk), "::", stringify!($($path).+)
            )
        );
    };
}

pub(crate) use witness;
