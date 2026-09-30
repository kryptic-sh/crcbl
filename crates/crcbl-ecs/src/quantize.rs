//! Per-component quantization for replication: how many bits each replicated
//! field is worth on the wire.
//!
//! # Where this sits
//!
//! [`SystemTrait::replicate`](crate::SystemTrait::replicate) writes each
//! entity's component bytes, and the server diffs those bytes against the
//! client's acked baseline — "changed" is compared in encoded space
//! (`docs/notes/simulation.md`). Quantizing *inside* `replicate` is therefore
//! quantizing before the diff: a change smaller than a field's quantum encodes
//! to the same bytes and ships nothing, where quantizing after the diff would
//! ship every sub-quantum wobble as a change. The server's determinism hash
//! reads the simulation's own state through
//! [`SystemTrait::hash_state`](crate::SystemTrait::hash_state) and never sees
//! these bytes: quantization is a wire concern, not a sim concern.
//!
//! It lives in the ECS rather than in `crcbl-net` because that is where the
//! replication hook and the per-type declarations beside it
//! ([`ComponentHash`](crate::ComponentHash)) live, and because the components
//! that declare a schema belong to crates — physics first — that must not
//! depend on the network stack and its cryptography.
//!
//! # The vocabulary
//!
//! A component declares its wire form as a schema: a list of [`Field`]s, each
//! with a [`Codec`], in wire order.
//!
//! * [`Codec::Exact`] — the identity codec: the `f64`'s eight bytes,
//!   little-endian. A schema of nothing else encodes byte-for-byte as the
//!   unquantized component would, which is what anything undeclared ships.
//! * [`Codec::Fixed`] — [`Fixed`] point over a declared range at a declared bit
//!   count; sector-local positions are the use it was built for.
//! * [`Codec::Half`] — IEEE 754-2008 binary16 ([`to_binary16`]).
//! * [`Codec::Rotation`] — a unit quaternion as its [`SmallestThree`].
//!
//! Fields are bit-packed least significant bit first with no alignment between
//! them, and the last byte is padded with zero bits; the decoder refuses a
//! payload of any other length or with a padding bit set, so every accepted
//! payload is the one encoding of its values.
//!
//! # Bounds: refuse, never clamp
//!
//! Every value a schema accepts decodes within its codec's error bound, and a
//! value no code of the codec can represent that closely — outside a [`Fixed`]
//! range, past binary16's finite range, not finite, a zero quaternion — makes
//! [`encode_values`] fail with [`QuantizeError::Unrepresentable`]. Clamping
//! would put an entity somewhere it is not and say nothing; a refusal lets the
//! component fall back to a form that can carry the value (see
//! `crcbl_phys::Transform::encode_wire`).

use std::f64::consts::FRAC_1_SQRT_2;
use std::fmt;

// ---------------------------------------------------------------------------
// Fixed point
// ---------------------------------------------------------------------------

/// Fixed point over a half-open range `[min, max)` at `bits` bits per value.
///
/// Code `k` stands for `min + k * quantum`, where `quantum = (max - min) /
/// 2^bits` and `k` runs from `0` to `2^bits - 1`, so `max` itself is one
/// quantum past the last code — the same half-open convention as
/// `crcbl_core::WorldPos`'s local offset. A range whose width is a power of two
/// gives a power-of-two quantum, and then every step of the arithmetic is exact.
///
/// A value encodes to the nearest code (ties round up) and is accepted only if
/// that code exists, so the decoded value is never more than half a quantum
/// from the one encoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fixed {
    min: f64,
    quantum: f64,
    bits: u32,
}

impl Fixed {
    /// The widest code a [`Fixed`] may declare, so every code fits a `u32`.
    pub const MAX_BITS: u32 = 32;

    /// Fixed point over `[min, max)` at `bits` bits.
    ///
    /// # Panics
    ///
    /// Panics — at compile time, in a `const` — if `bits` is zero or above
    /// [`Self::MAX_BITS`], or if `min` and `max` are not finite with `min <
    /// max`.
    #[must_use]
    pub const fn new(min: f64, max: f64, bits: u32) -> Self {
        assert!(
            bits >= 1 && bits <= Self::MAX_BITS,
            "a fixed-point code needs between 1 and 32 bits"
        );
        assert!(
            min.is_finite() && max.is_finite() && min < max,
            "a fixed-point range needs finite bounds with min < max"
        );
        Self {
            min,
            quantum: (max - min) / (1u64 << bits) as f64,
            bits,
        }
    }

    /// Bits one value takes on the wire.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// The step between adjacent codes; a decoded value is within half of it.
    #[must_use]
    pub const fn quantum(&self) -> f64 {
        self.quantum
    }

    /// The value code `0` stands for.
    #[must_use]
    pub const fn min(&self) -> f64 {
        self.min
    }

    /// The largest code.
    #[must_use]
    pub const fn max_code(&self) -> u32 {
        ((1u64 << self.bits) - 1) as u32
    }

    /// The code nearest `value`, or `None` if `value` is not finite or is
    /// more than half a quantum outside the representable values.
    #[must_use]
    pub fn encode(&self, value: f64) -> Option<u32> {
        if !value.is_finite() {
            return None;
        }
        let code = ((value - self.min) / self.quantum + 0.5).floor();
        if !(0.0..=f64::from(self.max_code())).contains(&code) {
            return None;
        }
        Some(code as u32)
    }

    /// The value `code` stands for, or `None` past [`Self::max_code`].
    #[must_use]
    pub fn decode(&self, code: u32) -> Option<f64> {
        (code <= self.max_code()).then(|| self.min + f64::from(code) * self.quantum)
    }
}

// ---------------------------------------------------------------------------
// Half float
// ---------------------------------------------------------------------------

/// Bits of an `f64`'s stored significand.
const F64_MANTISSA_BITS: u32 = 52;
/// An `f64`'s exponent bias.
const F64_EXPONENT_BIAS: i32 = 1023;
/// The `f64` exponent field of infinities and NaNs.
const F64_EXPONENT_SPECIAL: u64 = 0x7FF;
/// Bits of a binary16's stored significand.
const HALF_MANTISSA_BITS: u32 = 10;
/// A binary16's exponent bias.
const HALF_EXPONENT_BIAS: i32 = 15;
/// The binary16 exponent field of infinities and NaNs.
const HALF_EXPONENT_SPECIAL: u16 = 0x1F;
/// A binary16's sign bit.
const HALF_SIGN: u16 = 0x8000;
/// Positive infinity as binary16.
const HALF_INFINITY: u16 = HALF_EXPONENT_SPECIAL << HALF_MANTISSA_BITS;
/// The quiet bit of a binary16 NaN: the significand's top bit.
const HALF_QUIET: u16 = 1 << (HALF_MANTISSA_BITS - 1);
/// The unbiased exponent of the largest finite binary16.
const HALF_MAX_EXPONENT: i32 = 15;
/// The unbiased exponent of the smallest normal binary16.
const HALF_MIN_EXPONENT: i32 = -14;
/// The unbiased exponent of the smallest binary16 subnormal, `2^-24`.
const HALF_SUBNORMAL_EXPONENT: i32 = HALF_MIN_EXPONENT - HALF_MANTISSA_BITS as i32;
/// The smallest binary16 subnormal as an `f64`, built from its exponent so it
/// is exact.
const HALF_SUBNORMAL_STEP: f64 =
    f64::from_bits(((HALF_SUBNORMAL_EXPONENT + F64_EXPONENT_BIAS) as u64) << F64_MANTISSA_BITS);

/// `value` as IEEE 754-2008 binary16, rounded to nearest, ties to even.
///
/// Transcribed from the binary16 interchange format (IEEE 754-2008 §3.6: one
/// sign bit, a 5-bit exponent biased by 15, a 10-bit significand) and its
/// default rounding attribute, roundTiesToEven (§4.3.1). Rounding reads the
/// `f64`'s bits directly rather than going through `f32`, since rounding twice
/// can land a tie on the wrong side. Magnitudes from 65520 (halfway past the
/// largest finite binary16, 65504) round to infinity, as §7.4 says an overflow
/// does under that attribute; magnitudes at or below `2^-25` round to a signed
/// zero; NaN stays NaN, quieted, with the top of its payload kept.
#[must_use]
pub fn to_binary16(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 48) as u16) & HALF_SIGN;
    let exponent_field = (bits >> F64_MANTISSA_BITS) & F64_EXPONENT_SPECIAL;
    let mantissa = bits & ((1 << F64_MANTISSA_BITS) - 1);

    if exponent_field == F64_EXPONENT_SPECIAL {
        if mantissa == 0 {
            return sign | HALF_INFINITY;
        }
        let payload = (mantissa >> (F64_MANTISSA_BITS - HALF_MANTISSA_BITS)) as u16;
        return sign | HALF_INFINITY | HALF_QUIET | payload;
    }
    if exponent_field == 0 {
        // Zero, or an `f64` subnormal: far below half the smallest binary16
        // subnormal, so a signed zero either way.
        return sign;
    }

    let exponent = exponent_field as i32 - F64_EXPONENT_BIAS;
    if exponent > HALF_MAX_EXPONENT {
        return sign | HALF_INFINITY;
    }
    if exponent < HALF_SUBNORMAL_EXPONENT - 1 {
        return sign;
    }

    let significand = mantissa | (1 << F64_MANTISSA_BITS);
    let (shift, base) = if exponent >= HALF_MIN_EXPONENT {
        // Normal: keep the top ten stored bits under the biased exponent.
        let biased = (exponent + HALF_EXPONENT_BIAS) as u64;
        let shift = F64_MANTISSA_BITS - HALF_MANTISSA_BITS;
        (shift, (biased << HALF_MANTISSA_BITS) | (mantissa >> shift))
    } else {
        // Subnormal: the whole significand, implicit bit included, in units of
        // `2^-24`.
        let shift = (F64_MANTISSA_BITS as i32 - (exponent - HALF_SUBNORMAL_EXPONENT)) as u32;
        (shift, significand >> shift)
    };
    let remainder = significand & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    let round_up = remainder > halfway || (remainder == halfway && base & 1 == 1);
    // A carry out of the significand bumps the exponent, which is exactly the
    // next binary16 up — the smallest normal after the largest subnormal, or
    // infinity after 65504.
    sign | (base + u64::from(round_up)) as u16
}

/// The value of the binary16 `bits`, exactly.
///
/// Every binary16 is representable as an `f64`, so this never rounds. A NaN
/// keeps its sign and payload.
#[must_use]
pub fn from_binary16(bits: u16) -> f64 {
    let sign = u64::from(bits & HALF_SIGN) << 48;
    let exponent_field = (bits >> HALF_MANTISSA_BITS) & HALF_EXPONENT_SPECIAL;
    let mantissa = u64::from(bits & ((1 << HALF_MANTISSA_BITS) - 1));
    let shift = F64_MANTISSA_BITS - HALF_MANTISSA_BITS;

    let magnitude = match exponent_field {
        0 => {
            // Zero or subnormal: `mantissa * 2^-24`, exact in an `f64`.
            let value = mantissa as f64 * HALF_SUBNORMAL_STEP;
            return f64::from_bits(sign | value.to_bits());
        }
        HALF_EXPONENT_SPECIAL => (F64_EXPONENT_SPECIAL << F64_MANTISSA_BITS) | (mantissa << shift),
        _ => {
            let exponent = i32::from(exponent_field) - HALF_EXPONENT_BIAS + F64_EXPONENT_BIAS;
            ((exponent as u64) << F64_MANTISSA_BITS) | (mantissa << shift)
        }
    };
    f64::from_bits(sign | magnitude)
}

// ---------------------------------------------------------------------------
// Smallest three
// ---------------------------------------------------------------------------

/// A unit quaternion as its three smallest components, after Glenn Fiedler,
/// "Snapshot Compression" (Gaffer On Games, 2015).
///
/// `q` and `-q` are the same rotation, so the encoder flips the sign to make
/// the largest component positive; then that component is
/// `sqrt(1 - a² - b² - c²)` of the other three and need not be sent. Two bits
/// say which one it was. The other three are each within `±1/√2` — a
/// component larger than that would be the largest — and are sent as signed
/// fixed point at `bits` bits.
///
/// Unlike [`Fixed`], the component range is closed and centred: codes run
/// from `0` to `2^bits - 2`, with the middle code standing for exactly zero
/// and the ends for exactly `±1/√2`, so an identity rotation round-trips
/// exactly. The last code, `2^bits - 1`, is never written and the decoder
/// refuses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmallestThree {
    bits: u32,
}

/// The bound on every component but the largest of a unit quaternion.
const SMALLEST_THREE_RANGE: f64 = FRAC_1_SQRT_2;
/// Bits naming which component was dropped.
const SMALLEST_THREE_INDEX_BITS: u32 = 2;

impl SmallestThree {
    /// The narrowest component code: three codes, `-1/√2`, `0` and `1/√2`.
    pub const MIN_BITS: u32 = 2;
    /// The widest component code, so every code fits a `u32`.
    pub const MAX_BITS: u32 = 32;

    /// Smallest-three at `bits` bits per sent component.
    ///
    /// # Panics
    ///
    /// Panics — at compile time, in a `const` — if `bits` is outside
    /// [`Self::MIN_BITS`]`..=`[`Self::MAX_BITS`].
    #[must_use]
    pub const fn new(bits: u32) -> Self {
        assert!(
            bits >= Self::MIN_BITS && bits <= Self::MAX_BITS,
            "a smallest-three component needs between 2 and 32 bits"
        );
        Self { bits }
    }

    /// Bits per sent component.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Bits one quaternion takes on the wire: the index and three components.
    #[must_use]
    pub const fn encoded_bits(&self) -> u32 {
        SMALLEST_THREE_INDEX_BITS + 3 * self.bits
    }

    /// The step between adjacent component codes. Each sent component decodes
    /// within half of it; the dropped one, rebuilt from the other three, within
    /// about three times that (the derivation is beside the test that holds it
    /// to a bound).
    #[must_use]
    pub fn quantum(&self) -> f64 {
        2.0 * SMALLEST_THREE_RANGE / self.max_code() as f64
    }

    /// The largest code written: `2^bits - 2`.
    #[must_use]
    pub const fn max_code(&self) -> u32 {
        ((1u64 << self.bits) - 2) as u32
    }

    /// The code standing for zero.
    const fn centre(&self) -> u32 {
        self.max_code() / 2
    }

    /// `(index of the dropped component, the other three's codes)` for the
    /// quaternion `[x, y, z, w]`, normalised first.
    ///
    /// Returns `None` if a component is not finite or the length is zero. The
    /// largest component is the first of equal magnitude, so equal inputs
    /// always give equal codes.
    #[must_use]
    pub fn encode(&self, quaternion: [f64; 4]) -> Option<(u8, [u32; 3])> {
        let length = quaternion.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !length.is_finite() || length == 0.0 {
            return None;
        }
        let mut largest = 0;
        for (i, component) in quaternion.iter().enumerate() {
            if component.abs() > quaternion[largest].abs() {
                largest = i;
            }
        }
        let scale = if quaternion[largest] < 0.0 {
            -1.0 / length
        } else {
            1.0 / length
        };
        let quantum = self.quantum();
        let mut codes = [0u32; 3];
        let others = (0..4).filter(|&i| i != largest);
        for (code, i) in codes.iter_mut().zip(others) {
            // Mathematically within the range already; the clamp only removes
            // the normalisation's rounding, never a real excursion.
            let value = (quaternion[i] * scale).clamp(-SMALLEST_THREE_RANGE, SMALLEST_THREE_RANGE);
            let offset = (value / quantum + 0.5).floor() as i64;
            *code = (i64::from(self.centre()) + offset).clamp(0, i64::from(self.max_code())) as u32;
        }
        Some((largest as u8, codes))
    }

    /// The unit quaternion `[x, y, z, w]` that `index` and `codes` encode, or
    /// `None` if `index` is past 3 or a code past [`Self::max_code`].
    #[must_use]
    pub fn decode(&self, index: u8, codes: [u32; 3]) -> Option<[f64; 4]> {
        let largest = usize::from(index);
        if largest > 3 || codes.iter().any(|&code| code > self.max_code()) {
            return None;
        }
        let quantum = self.quantum();
        let mut quaternion = [0.0; 4];
        let others = (0..4).filter(|&i| i != largest);
        for (&code, i) in codes.iter().zip(others) {
            quaternion[i] = (f64::from(code) - f64::from(self.centre())) * quantum;
        }
        let rest: f64 = quaternion.iter().map(|c| c * c).sum();
        quaternion[largest] = (1.0 - rest).max(0.0).sqrt();
        // Quantization leaves the four a little off unit length; the largest
        // is at least a half, so the length is never near zero.
        let length = quaternion.iter().map(|c| c * c).sum::<f64>().sqrt();
        Some(quaternion.map(|c| c / length))
    }
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

/// How one field travels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec {
    /// The identity codec: one `f64`, its eight bytes little-endian. Carries
    /// every value, NaN included, bit for bit.
    Exact,
    /// One value as [`Fixed`] point.
    Fixed(Fixed),
    /// One value as IEEE 754-2008 binary16 ([`to_binary16`]). Refuses a value
    /// that is not finite or rounds to infinity.
    Half,
    /// Four values, a quaternion's `x, y, z, w`, as a [`SmallestThree`].
    Rotation(SmallestThree),
}

/// Bits of an [`Codec::Exact`] value.
const EXACT_BITS: u32 = 64;
/// Bits of a [`Codec::Half`] value.
const HALF_BITS: u32 = 16;

impl Codec {
    /// How many of a component's values this codec consumes.
    #[must_use]
    pub const fn values(&self) -> usize {
        match self {
            Self::Rotation(_) => 4,
            Self::Exact | Self::Fixed(_) | Self::Half => 1,
        }
    }

    /// Bits this codec writes.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        match self {
            Self::Exact => EXACT_BITS,
            Self::Fixed(fixed) => fixed.bits(),
            Self::Half => HALF_BITS,
            Self::Rotation(rotation) => rotation.encoded_bits(),
        }
    }
}

/// One named field of a component's wire schema.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    /// What the field is, for error messages.
    pub name: &'static str,
    /// How it travels.
    pub codec: Codec,
}

/// How many values `schema` consumes.
#[must_use]
pub const fn value_count(schema: &[Field]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < schema.len() {
        count += schema[i].codec.values();
        i += 1;
    }
    count
}

/// Bytes a payload of `schema` takes: every field's bits, rounded up to a
/// whole byte.
#[must_use]
pub const fn encoded_len(schema: &[Field]) -> usize {
    let mut bits = 0usize;
    let mut i = 0;
    while i < schema.len() {
        bits += schema[i].codec.bits() as usize;
        i += 1;
    }
    bits.div_ceil(8)
}

/// Why a value would not encode, or a payload would not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizeError {
    /// The component supplied, or asked for, a different number of values
    /// than its schema consumes.
    ValueCount {
        /// What the schema consumes.
        expected: usize,
        /// What was supplied.
        actual: usize,
    },
    /// No code of this field's codec is within its error bound of the value.
    Unrepresentable {
        /// The field.
        field: &'static str,
    },
    /// The payload is not the schema's [`encoded_len`].
    Length {
        /// The schema's length.
        expected: usize,
        /// The payload's.
        actual: usize,
    },
    /// The payload holds a code the encoder never writes in this field, or,
    /// when the field is `"padding"`, a set padding bit.
    NonCanonical {
        /// The field.
        field: &'static str,
    },
    /// The values decoded, but the component refused them.
    Refused,
}

impl fmt::Display for QuantizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValueCount { expected, actual } => {
                write!(f, "schema consumes {expected} values, {actual} supplied")
            }
            Self::Unrepresentable { field } => {
                write!(f, "field {field} cannot represent its value")
            }
            Self::Length { expected, actual } => {
                write!(f, "payload is {actual} bytes, schema wants {expected}")
            }
            Self::NonCanonical { field } => write!(f, "field {field} holds a code never written"),
            Self::Refused => f.write_str("the component refused the decoded values"),
        }
    }
}

impl std::error::Error for QuantizeError {}

/// Packs values least significant bit first.
struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    pending: u128,
    pending_bits: u32,
}

impl BitWriter<'_> {
    /// Append the low `bits` bits of `value`; `bits` is at most 64.
    fn write(&mut self, value: u64, bits: u32) {
        let masked = if bits == 64 {
            value
        } else {
            value & ((1 << bits) - 1)
        };
        self.pending |= u128::from(masked) << self.pending_bits;
        self.pending_bits += bits;
        while self.pending_bits >= 8 {
            self.out.push(self.pending as u8);
            self.pending >>= 8;
            self.pending_bits -= 8;
        }
    }

    /// Flush the last partial byte, its high bits zero.
    fn finish(self) {
        if self.pending_bits > 0 {
            self.out.push(self.pending as u8);
        }
    }
}

/// Unpacks what [`BitWriter`] packed.
struct BitReader<'a> {
    data: &'a [u8],
    next: usize,
    pending: u128,
    pending_bits: u32,
}

impl BitReader<'_> {
    /// The next `bits` bits, at most 64. The caller has checked the length.
    fn read(&mut self, bits: u32) -> u64 {
        while self.pending_bits < bits {
            self.pending |= u128::from(self.data[self.next]) << self.pending_bits;
            self.next += 1;
            self.pending_bits += 8;
        }
        let value = if bits == 64 {
            self.pending as u64
        } else {
            (self.pending as u64) & ((1 << bits) - 1)
        };
        self.pending >>= bits;
        self.pending_bits -= bits;
        value
    }
}

/// Append `values` to `out` as `schema` says.
///
/// On an error `out` is left as it was, so a caller can write another form in
/// its place.
///
/// # Errors
///
/// [`QuantizeError::ValueCount`] if `values` is not [`value_count`] long;
/// [`QuantizeError::Unrepresentable`] for the first value its codec cannot
/// carry within its bound (see the [module docs](self)).
pub fn encode_values(
    schema: &[Field],
    values: &[f64],
    out: &mut Vec<u8>,
) -> Result<(), QuantizeError> {
    let expected = value_count(schema);
    if values.len() != expected {
        return Err(QuantizeError::ValueCount {
            expected,
            actual: values.len(),
        });
    }
    let start = out.len();
    let result = write_fields(schema, values, out);
    if result.is_err() {
        out.truncate(start);
    }
    result
}

fn write_fields(schema: &[Field], values: &[f64], out: &mut Vec<u8>) -> Result<(), QuantizeError> {
    let mut writer = BitWriter {
        out,
        pending: 0,
        pending_bits: 0,
    };
    // `values` is exactly `value_count(schema)` long, so every field's slice
    // is in bounds.
    let mut cursor = 0;
    for field in schema {
        let taken = &values[cursor..cursor + field.codec.values()];
        cursor += taken.len();
        let unrepresentable = QuantizeError::Unrepresentable { field: field.name };
        match field.codec {
            Codec::Exact => writer.write(taken[0].to_bits(), EXACT_BITS),
            Codec::Fixed(fixed) => {
                let code = fixed.encode(taken[0]).ok_or(unrepresentable)?;
                writer.write(u64::from(code), fixed.bits());
            }
            Codec::Half => {
                let half = to_binary16(taken[0]);
                if !taken[0].is_finite() || half & !HALF_SIGN == HALF_INFINITY {
                    return Err(unrepresentable);
                }
                writer.write(u64::from(half), HALF_BITS);
            }
            Codec::Rotation(rotation) => {
                let quaternion = [taken[0], taken[1], taken[2], taken[3]];
                let (index, codes) = rotation.encode(quaternion).ok_or(unrepresentable)?;
                writer.write(u64::from(index), SMALLEST_THREE_INDEX_BITS);
                for code in codes {
                    writer.write(u64::from(code), rotation.bits());
                }
            }
        }
    }
    writer.finish();
    Ok(())
}

/// Decode `data`, written by [`encode_values`] with `schema`, into `values`.
///
/// # Errors
///
/// [`QuantizeError::ValueCount`] if `values` is not [`value_count`] long,
/// [`QuantizeError::Length`] if `data` is not [`encoded_len`] long, and
/// [`QuantizeError::NonCanonical`] for a code the encoder never writes or a
/// set padding bit. `values` is unspecified after an error.
pub fn decode_values(
    schema: &[Field],
    data: &[u8],
    values: &mut [f64],
) -> Result<(), QuantizeError> {
    let expected = value_count(schema);
    if values.len() != expected {
        return Err(QuantizeError::ValueCount {
            expected,
            actual: values.len(),
        });
    }
    let expected_len = encoded_len(schema);
    if data.len() != expected_len {
        return Err(QuantizeError::Length {
            expected: expected_len,
            actual: data.len(),
        });
    }
    let mut reader = BitReader {
        data,
        next: 0,
        pending: 0,
        pending_bits: 0,
    };
    let mut cursor = 0;
    for field in schema {
        let non_canonical = QuantizeError::NonCanonical { field: field.name };
        let slots = &mut values[cursor..cursor + field.codec.values()];
        cursor += slots.len();
        match field.codec {
            Codec::Exact => slots[0] = f64::from_bits(reader.read(EXACT_BITS)),
            Codec::Fixed(fixed) => {
                let code = reader.read(fixed.bits()) as u32;
                slots[0] = fixed.decode(code).ok_or(non_canonical)?;
            }
            Codec::Half => slots[0] = from_binary16(reader.read(HALF_BITS) as u16),
            Codec::Rotation(rotation) => {
                let index = reader.read(SMALLEST_THREE_INDEX_BITS) as u8;
                let codes = [(); 3].map(|()| reader.read(rotation.bits()) as u32);
                slots.copy_from_slice(&rotation.decode(index, codes).ok_or(non_canonical)?);
            }
        }
    }
    if reader.pending != 0 {
        return Err(QuantizeError::NonCanonical { field: "padding" });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// A component that declares its wire form as a quantization schema.
///
/// The declaration is compile-time: [`Self::SCHEMA`] is a constant, and
/// [`Self::Values`] a fixed-size array sized to it. A component that does not
/// implement this keeps whatever bytes its system's `replicate` writes, which
/// for a plain value is the identity encoding.
///
/// ```
/// use crcbl_ecs::quantize::{self, Codec, Field, Fixed, Quantized};
///
/// /// A height above a sector's floor, to the millimetre-ish.
/// #[derive(Debug, PartialEq)]
/// struct Altitude(f64);
///
/// impl Quantized for Altitude {
///     const SCHEMA: &'static [Field] = &[Field {
///         name: "altitude",
///         codec: Codec::Fixed(Fixed::new(0.0, 1024.0, 20)),
///     }];
///     type Values = [f64; 1];
///
///     fn to_values(&self) -> [f64; 1] {
///         [self.0]
///     }
///
///     fn from_values(values: &[f64; 1]) -> Option<Self> {
///         Some(Self(values[0]))
///     }
/// }
///
/// let mut wire = Vec::new();
/// quantize::encode(&Altitude(12.5), &mut wire).unwrap();
/// assert_eq!(wire.len(), 3, "twenty bits round up to three bytes");
/// assert_eq!(quantize::decode::<Altitude>(&wire), Ok(Altitude(12.5)));
///
/// // Out of range is refused, not clamped.
/// assert!(quantize::encode(&Altitude(-1.0), &mut wire).is_err());
/// ```
pub trait Quantized: Sized {
    /// The component's fields, in wire order.
    const SCHEMA: &'static [Field];

    /// The values [`Self::SCHEMA`] consumes, in order: a `[f64; N]` with `N`
    /// its [`value_count`].
    type Values: AsRef<[f64]> + AsMut<[f64]> + Default;

    /// The component's values, in schema order.
    fn to_values(&self) -> Self::Values;

    /// The component the decoded `values` describe, or `None` if they do not
    /// describe a valid one.
    fn from_values(values: &Self::Values) -> Option<Self>;
}

/// Append `component`'s quantized form to `out`; see [`encode_values`].
///
/// # Errors
///
/// As [`encode_values`]; `out` is left as it was.
pub fn encode<T: Quantized>(component: &T, out: &mut Vec<u8>) -> Result<(), QuantizeError> {
    encode_values(T::SCHEMA, component.to_values().as_ref(), out)
}

/// The component `data` encodes; see [`decode_values`].
///
/// # Errors
///
/// As [`decode_values`], and [`QuantizeError::Refused`] when
/// [`Quantized::from_values`] rejects what decoded.
pub fn decode<T: Quantized>(data: &[u8]) -> Result<T, QuantizeError> {
    let mut values = T::Values::default();
    decode_values(T::SCHEMA, data, values.as_mut())?;
    T::from_values(&values).ok_or(QuantizeError::Refused)
}

#[cfg(test)]
mod tests;
