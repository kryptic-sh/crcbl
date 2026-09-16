//! [`Reflect`] for the types an inspector meets before it meets a component:
//! the scalars, `String`, fixed-size arrays, and the scalar-layout half of
//! `glam`.
//!
//! # What is here, and what is not
//!
//! * **Scalars** — `bool`, `i8`–`i64` and `isize`, `u8`–`u64` and `usize`, `f32`
//!   and `f64`, `String`. `i128`/`u128` are **not** here: [`Value`] widens the
//!   integers to 64 bits, so a 128-bit leaf could not round-trip, and no
//!   component in this workspace holds one. `char` is not here either — an
//!   inspector has no widget for one that a one-character [`Value::Text`] would
//!   not serve better, and nothing needs it yet.
//! * **`[T; N]`** as a [`Kind::List`], which is the shape real components hold a
//!   position in: `apps/breakout/src/scene.rs`'s `Brick` and
//!   `apps/puppet/src/map.rs`'s `Surface` both spell one `[f64; 3]`.
//!   `Vec<T>`, `Option<T>` and the maps are **not** here: each of them needs a
//!   way to *change the shape* — push, clear, take `None` to `Some` — which is
//!   the same missing mechanism enum-variant switching needs, and neither has a
//!   caller yet.
//! * **`glam`'s scalar-layout vectors** — [`glam::Vec2`], [`glam::Vec3`],
//!   [`glam::DVec2`], [`glam::DVec3`], [`glam::DVec4`] and [`glam::DQuat`],
//!   each as a [`Kind::Struct`] of its named components.
//!
//! **[`glam::Vec4`], [`glam::Quat`] and [`glam::Vec3A`] are deliberately
//! absent**, and the reason is glam's representation rather than a decision
//! here: on every target with a SIMD path — SSE2, NEON, wasm SIMD, `core::simd`
//! — those three are a newtype around one 128-bit register (`pub struct
//! Vec4(pub(crate) __m128)`), their `x`/`y`/`z`/`w` are *methods*, and there is
//! no `&mut f32` inside them to hand back. Reaching them needs a by-value
//! element accessor beside the by-reference one, which is a second mechanism in
//! the trait for three types. `glam::DQuat` is here because f64 has no SIMD path
//! and it is a plain `{ x, y, z, w }` — so the boundary is exactly "does glam
//! store this as named fields", not "is this a rotation".
//!
//! A rotation is also the case where raw reflection is the wrong answer: Unreal
//! and Godot both edit one as three Euler angles, which is a per-type override
//! in the inspector rather than four quaternion rows.
//!
//! The matrices and the affine types are absent for a plainer reason: a
//! property panel does not edit a `Mat4` cell by cell.

use crate::{Field, Kind, Reflect, SetError, Value, ValueKind};

/// The [`SetError::Kind`] for a leaf of `type_name` handed the wrong kind.
fn mismatch(type_name: &'static str, expected: ValueKind, actual: &Value) -> SetError {
    SetError::Kind {
        type_name,
        expected,
        actual: actual.kind(),
    }
}

// ---------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------

/// The seven methods every leaf answers the same way: no fields, no elements,
/// no variant, and the two `Any` casts.
macro_rules! leaf_shape {
    ($ty:ty, $vkind:expr) => {
        fn kind(&self) -> Kind {
            Kind::Leaf($vkind)
        }

        fn fields(&self) -> &'static [Field] {
            &[]
        }

        fn field(&self, _index: usize) -> Option<&dyn Reflect> {
            None
        }

        fn field_mut(&mut self, _index: usize) -> Option<&mut dyn Reflect> {
            None
        }

        fn variant(&self) -> Option<&'static str> {
            None
        }

        fn as_any(&self) -> &dyn core::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
            self
        }

        fn type_name(&self) -> &'static str {
            stringify!($ty)
        }
    };
}

impl Reflect for bool {
    fn get(&self) -> Option<Value> {
        Some(Value::Bool(*self))
    }

    fn set(&mut self, value: &Value) -> Result<(), SetError> {
        match value {
            Value::Bool(v) => {
                *self = *v;
                Ok(())
            }
            other => Err(mismatch("bool", ValueKind::Bool, other)),
        }
    }

    leaf_shape!(bool, ValueKind::Bool);
}

impl Reflect for String {
    fn get(&self) -> Option<Value> {
        Some(Value::Text(self.clone()))
    }

    fn set(&mut self, value: &Value) -> Result<(), SetError> {
        match value {
            Value::Text(v) => {
                self.clone_from(v);
                Ok(())
            }
            other => Err(mismatch("String", ValueKind::Text, other)),
        }
    }

    leaf_shape!(String, ValueKind::Text);
}

/// The integer leaves whose widening is a `From`: everything narrower than the
/// 64-bit arm of [`Value`], plus that arm itself.
///
/// A write goes through `TryFrom`, so a [`Value::Int`] of `300` into an `i8` is
/// a [`SetError::Range`] naming the type rather than a truncation.
macro_rules! int_leaf {
    ($($ty:ty => $wide:ty, $arm:path, $vkind:expr);* $(;)?) => {
        $(
            impl Reflect for $ty {
                fn get(&self) -> Option<Value> {
                    Some($arm(<$wide>::from(*self)))
                }

                fn set(&mut self, value: &Value) -> Result<(), SetError> {
                    match value {
                        $arm(v) => {
                            *self = <$ty>::try_from(*v).map_err(|_| SetError::Range {
                                type_name: stringify!($ty),
                                value: value.clone(),
                            })?;
                            Ok(())
                        }
                        other => Err(mismatch(stringify!($ty), $vkind, other)),
                    }
                }

                leaf_shape!($ty, $vkind);
            }
        )*
    };
}

int_leaf! {
    i8 => i64, Value::Int, ValueKind::Int;
    i16 => i64, Value::Int, ValueKind::Int;
    i32 => i64, Value::Int, ValueKind::Int;
    i64 => i64, Value::Int, ValueKind::Int;
    u8 => u64, Value::UInt, ValueKind::UInt;
    u16 => u64, Value::UInt, ValueKind::UInt;
    u32 => u64, Value::UInt, ValueKind::UInt;
    u64 => u64, Value::UInt, ValueKind::UInt;
}

/// The pointer-width integers, read at a **fixed** 64-bit width rather than at
/// the platform's.
///
/// The same widening `crcbl_ecs::ComponentHash` does, for the same reason: a
/// browser build's `usize` is 32 bits and a native one's is 64, and a leaf that
/// changed kind with the target would make a saved edit command mean two
/// different things. `as` rather than `From` because no `From<usize> for u64`
/// exists; it is lossless on every target this engine builds for, all of which
/// have a pointer no wider than 64 bits.
macro_rules! pointer_width_leaf {
    ($($ty:ty => $wide:ty, $arm:path, $vkind:expr);* $(;)?) => {
        $(
            impl Reflect for $ty {
                fn get(&self) -> Option<Value> {
                    Some($arm(*self as $wide))
                }

                fn set(&mut self, value: &Value) -> Result<(), SetError> {
                    match value {
                        $arm(v) => {
                            *self = <$ty>::try_from(*v).map_err(|_| SetError::Range {
                                type_name: stringify!($ty),
                                value: value.clone(),
                            })?;
                            Ok(())
                        }
                        other => Err(mismatch(stringify!($ty), $vkind, other)),
                    }
                }

                leaf_shape!($ty, $vkind);
            }
        )*
    };
}

pointer_width_leaf! {
    isize => i64, Value::Int, ValueKind::Int;
    usize => u64, Value::UInt, ValueKind::UInt;
}

impl Reflect for f64 {
    fn get(&self) -> Option<Value> {
        Some(Value::Float(*self))
    }

    fn set(&mut self, value: &Value) -> Result<(), SetError> {
        match value {
            Value::Float(v) => {
                if !v.is_finite() {
                    return Err(SetError::NotFinite {
                        type_name: "f64",
                        value: *v,
                    });
                }
                *self = *v;
                Ok(())
            }
            other => Err(mismatch("f64", ValueKind::Float, other)),
        }
    }

    leaf_shape!(f64, ValueKind::Float);
}

impl Reflect for f32 {
    fn get(&self) -> Option<Value> {
        Some(Value::Float(f64::from(*self)))
    }

    fn set(&mut self, value: &Value) -> Result<(), SetError> {
        match value {
            Value::Float(v) => {
                if !v.is_finite() {
                    return Err(SetError::NotFinite {
                        type_name: "f32",
                        value: *v,
                    });
                }
                // A saturating cast: `1e300 as f32` is `inf`, which would be a
                // corruption reported as a successful write. Refuse it by the
                // one thing that distinguishes it — a finite input that landed
                // on a non-finite result.
                let narrowed = *v as f32;
                if !narrowed.is_finite() {
                    return Err(SetError::Range {
                        type_name: "f32",
                        value: value.clone(),
                    });
                }
                *self = narrowed;
                Ok(())
            }
            other => Err(mismatch("f32", ValueKind::Float, other)),
        }
    }

    leaf_shape!(f32, ValueKind::Float);
}

// ---------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------

/// A fixed-size array is a [`Kind::List`]: `N` elements an inspector labels by
/// their index, with no [`Field`] row apiece.
///
/// `as_slice`/`as_mut_slice` rather than `self.get(index)`, because `get` is one
/// of this trait's own methods and the slice inherent would not be the one that
/// resolved.
impl<T: Reflect, const N: usize> Reflect for [T; N] {
    fn type_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    fn kind(&self) -> Kind {
        Kind::List { len: N }
    }

    fn get(&self) -> Option<Value> {
        None
    }

    fn set(&mut self, _value: &Value) -> Result<(), SetError> {
        Err(SetError::NotALeaf {
            type_name: core::any::type_name::<Self>(),
        })
    }

    fn fields(&self) -> &'static [Field] {
        &[]
    }

    fn field(&self, index: usize) -> Option<&dyn Reflect> {
        self.as_slice().get(index).map(|e| e as &dyn Reflect)
    }

    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        self.as_mut_slice()
            .get_mut(index)
            .map(|e| e as &mut dyn Reflect)
    }

    fn variant(&self) -> Option<&'static str> {
        None
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// glam
// ---------------------------------------------------------------------------

/// One of glam's scalar-layout vectors, as a [`Kind::Struct`] of its components.
///
/// The index list is spelled out per type rather than counted by the macro so
/// that [`Reflect::field`]'s arms and [`Reflect::fields`]' order cannot drift
/// apart: they are written from the same list.
macro_rules! glam_struct {
    ($($ty:ty as $name:literal { $($index:tt => $component:ident),+ $(,)? })*) => {
        $(
            impl Reflect for $ty {
                fn type_name(&self) -> &'static str {
                    $name
                }

                fn kind(&self) -> Kind {
                    Kind::Struct
                }

                fn get(&self) -> Option<Value> {
                    None
                }

                fn set(&mut self, _value: &Value) -> Result<(), SetError> {
                    Err(SetError::NotALeaf { type_name: $name })
                }

                fn fields(&self) -> &'static [Field] {
                    const FIELDS: &[Field] = &[
                        $(Field::new(stringify!($component)),)+
                    ];
                    FIELDS
                }

                fn field(&self, index: usize) -> Option<&dyn Reflect> {
                    match index {
                        $($index => Some(&self.$component as &dyn Reflect),)+
                        _ => None,
                    }
                }

                fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
                    match index {
                        $($index => Some(&mut self.$component as &mut dyn Reflect),)+
                        _ => None,
                    }
                }

                fn variant(&self) -> Option<&'static str> {
                    None
                }

                fn as_any(&self) -> &dyn core::any::Any {
                    self
                }

                fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
                    self
                }
            }
        )*
    };
}

glam_struct! {
    glam::Vec2 as "Vec2" { 0 => x, 1 => y }
    glam::Vec3 as "Vec3" { 0 => x, 1 => y, 2 => z }
    glam::DVec2 as "DVec2" { 0 => x, 1 => y }
    glam::DVec3 as "DVec3" { 0 => x, 1 => y, 2 => z }
    glam::DVec4 as "DVec4" { 0 => x, 1 => y, 2 => z, 3 => w }
    glam::DQuat as "DQuat" { 0 => x, 1 => y, 2 => z, 3 => w }
}
