use super::*;
use crcbl_core::rand::hash_unit;

/// Values each property test draws.
const SAMPLES: u64 = 20_000;

/// A seed per test, so the draws differ between them.
const SEED: u64 = 0x51_5541_4E54;

/// `2^exponent`, exactly.
fn pow2(exponent: i32) -> f64 {
    f64::from_bits(((exponent + F64_EXPONENT_BIAS) as u64) << F64_MANTISSA_BITS)
}

// ── Half float ──────────────────────────────────────────────────────────────

/// **Known binary16 bit patterns**, from the format's definition rather than
/// from this encoder: each row is a value whose binary16 is written out in
/// IEEE 754-2008's terms (sign, exponent biased by 15, ten significand bits).
#[test]
fn half_float_matches_known_bit_patterns() {
    let table: &[(f64, u16, &str)] = &[
        (1.0, 0x3C00, "one"),
        (-2.0, 0xC000, "minus two"),
        (0.5, 0x3800, "a half"),
        (65504.0, 0x7BFF, "the largest finite binary16"),
        (pow2(-14), 0x0400, "the smallest normal"),
        (1023.0 * pow2(-24), 0x03FF, "the largest subnormal"),
        (pow2(-24), 0x0001, "the smallest subnormal"),
        (0.0, 0x0000, "zero"),
        (-0.0, 0x8000, "negative zero"),
        (f64::INFINITY, 0x7C00, "infinity"),
        (f64::NEG_INFINITY, 0xFC00, "negative infinity"),
        // Rounding, ties to even: 1 + 2^-11 is halfway between 0x3C00 and
        // 0x3C01 and goes to the even one; 1 + 3·2^-11 is halfway between
        // 0x3C01 and 0x3C02 and goes up.
        (1.0 + pow2(-11), 0x3C00, "a tie rounding down to even"),
        (1.0 + 3.0 * pow2(-11), 0x3C02, "a tie rounding up to even"),
        (1.0 + pow2(-11) + pow2(-40), 0x3C01, "just past a tie"),
        // Subnormal ties, likewise.
        (
            1.5 * pow2(-24),
            0x0002,
            "a subnormal tie rounding up to even",
        ),
        (
            2.5 * pow2(-24),
            0x0002,
            "a subnormal tie rounding down to even",
        ),
        (
            pow2(-25),
            0x0000,
            "half the smallest subnormal ties to zero",
        ),
        (3.0 * pow2(-26), 0x0001, "past half the smallest subnormal"),
        (
            1023.5 * pow2(-24),
            0x0400,
            "a tie carrying into the normals",
        ),
        // Overflow: 65520 is halfway to the next binade and ties to infinity.
        (65519.0, 0x7BFF, "below the overflow threshold"),
        (65520.0, 0x7C00, "the overflow threshold"),
        (1e10, 0x7C00, "far past the finite range"),
        (1e-30, 0x0000, "far below the smallest subnormal"),
        // Two familiar inexact constants.
        (0.1, 0x2E66, "a tenth"),
        (1.0 / 3.0, 0x3555, "a third"),
    ];
    for &(value, bits, what) in table {
        assert_eq!(
            to_binary16(value),
            bits,
            "{what}: {value:e} encoded to {:#06X}, not {bits:#06X}",
            to_binary16(value)
        );
    }

    let nan = to_binary16(f64::NAN);
    assert_eq!(nan & HALF_INFINITY, HALF_INFINITY, "NaN keeps the exponent");
    assert_ne!(nan & !HALF_SIGN & !HALF_INFINITY, 0, "and a payload");
    assert!(from_binary16(nan).is_nan());
}

#[test]
fn half_float_decodes_known_bit_patterns_exactly() {
    assert_eq!(from_binary16(0x3C00), 1.0);
    assert_eq!(from_binary16(0xC000), -2.0);
    assert_eq!(from_binary16(0x7BFF), 65504.0);
    assert_eq!(from_binary16(0x0001), pow2(-24));
    assert_eq!(from_binary16(0x03FF), 1023.0 * pow2(-24));
    assert_eq!(from_binary16(0x0400), pow2(-14));
    assert_eq!(from_binary16(0x7C00), f64::INFINITY);
    assert_eq!(from_binary16(0xFC00), f64::NEG_INFINITY);
    assert!(from_binary16(0x7E00).is_nan());
    assert!(from_binary16(0x8000).is_sign_negative());
    assert_eq!(from_binary16(0x8000), 0.0);
}

/// **Every binary16 decodes and re-encodes to itself.** All 65 536 patterns,
/// NaNs checked as NaNs, since the round trip keeps a NaN's payload but the
/// comparison has to say so explicitly.
#[test]
fn every_binary16_round_trips() {
    for bits in 0..=u16::MAX {
        let value = from_binary16(bits);
        if value.is_nan() {
            assert!(from_binary16(to_binary16(value)).is_nan(), "{bits:#06X}");
            assert_eq!(to_binary16(value), bits | HALF_QUIET, "{bits:#06X}");
        } else {
            assert_eq!(to_binary16(value), bits, "{bits:#06X}");
        }
    }
}

/// **A finite value decodes within half the gap to its binary16 neighbour.**
/// The gap is read off the format itself — the next pattern up — so the
/// bound does not trust the encoder under test.
#[test]
fn half_float_error_is_at_most_half_a_step() {
    for i in 0..SAMPLES {
        let magnitude = 65504.0 * hash_unit(SEED, i).powi(8);
        let value = if i % 2 == 0 { magnitude } else { -magnitude };
        let bits = to_binary16(value);
        let decoded = from_binary16(bits);
        let unsigned = bits & !HALF_SIGN;
        let step = if unsigned == 0x7BFF {
            from_binary16(0x7BFF) - from_binary16(0x7BFE)
        } else {
            from_binary16(unsigned + 1) - from_binary16(unsigned)
        };
        let below = if unsigned == 0 {
            step
        } else {
            from_binary16(unsigned) - from_binary16(unsigned - 1)
        };
        // The value may sit on either side of the code it rounded to.
        let bound = step.max(below) / 2.0;
        assert!(
            (decoded - value).abs() <= bound,
            "{value:e} decoded to {decoded:e}, off by more than {bound:e}"
        );
    }
}

// ── Fixed point ─────────────────────────────────────────────────────────────

/// A sector-local position axis as physics declares one: `±4096 m` at 24 bits,
/// a quantum of `2^-11 m`.
const POSITION: Fixed = Fixed::new(-4096.0, 4096.0, 24);

/// A range whose quantum is not a power of two, so rounding happens in the
/// arithmetic too.
const AWKWARD: Fixed = Fixed::new(-3.3, 7.1, 13);

#[test]
fn a_power_of_two_range_has_a_power_of_two_quantum() {
    assert_eq!(POSITION.quantum(), pow2(-11));
    assert_eq!(POSITION.max_code(), (1 << 24) - 1);
    assert_eq!(Fixed::new(0.0, 1.0, 32).max_code(), u32::MAX);
}

/// **The range's edges, value by value**: what is representable exactly, the
/// last value accepted on each side, and the first refused.
#[test]
fn fixed_point_accepts_within_half_a_quantum_of_a_code_and_refuses_past_it() {
    let q = POSITION.quantum();
    let max = POSITION.min() + q * f64::from(POSITION.max_code());

    assert_eq!(POSITION.encode(-4096.0), Some(0), "min is code 0");
    assert_eq!(POSITION.decode(0), Some(-4096.0));
    assert_eq!(POSITION.encode(max), Some(POSITION.max_code()));
    assert_eq!(POSITION.decode(POSITION.max_code()), Some(4096.0 - q));
    assert_eq!(
        POSITION.encode(0.0),
        Some(1 << 23),
        "zero is the middle code"
    );

    // Below min: half a quantum still rounds to code 0; past it does not.
    assert_eq!(POSITION.encode(-4096.0 - q / 2.0), Some(0));
    assert_eq!(POSITION.encode(-4096.0 - q / 2.0 - q / 64.0), None);
    // Above the last code: half a quantum would round to a code that does
    // not exist, so it is refused, and anything short of it is not.
    assert_eq!(POSITION.encode(max + q / 2.0), None);
    assert_eq!(
        POSITION.encode(4096.0),
        None,
        "max itself is past the range"
    );
    assert_eq!(
        POSITION.encode(max + q / 2.0 - q / 64.0),
        Some(POSITION.max_code())
    );

    for refused in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300] {
        assert_eq!(POSITION.encode(refused), None, "{refused}");
    }
    assert_eq!(Fixed::new(0.0, 1.0, 4).decode(16), None, "past max_code");
}

/// **Round trip within half a quantum, across the declared range** — a sweep
/// of random values plus every edge. The power-of-two range is held to the
/// exact bound; the awkward one to the bound plus the few ulps its rounded
/// subtraction and multiplication can add.
#[test]
fn fixed_point_round_trips_within_half_a_quantum() {
    for (fixed, slack) in [(POSITION, 0.0), (AWKWARD, 1e-12)] {
        let q = fixed.quantum();
        let span = q * f64::from(fixed.max_code());
        let edges = [
            fixed.min(),
            // The exact half-quantum edges are the table test's; here they
            // are approached from inside, where the awkward range's rounding
            // cannot decide them either way.
            fixed.min() - q / 2.0 + q / 64.0,
            fixed.min() + q / 2.0,
            fixed.min() + span,
            fixed.min() + span + q / 2.0 - q / 64.0,
            fixed.min() + span / 2.0,
        ];
        let draws = (0..SAMPLES).map(|i| fixed.min() + span * hash_unit(SEED ^ 1, i));
        for value in edges.into_iter().chain(draws) {
            let code = fixed
                .encode(value)
                .unwrap_or_else(|| panic!("{value} is within half a quantum of a code"));
            let decoded = fixed.decode(code).unwrap();
            assert!(
                (decoded - value).abs() <= q / 2.0 + slack,
                "{value} decoded to {decoded}, more than half of {q} away"
            );
        }
    }
}

// ── Smallest three ──────────────────────────────────────────────────────────

/// What physics sends a rotation at: 18 bits a component.
const ROTATION: SmallestThree = SmallestThree::new(18);

/// A random unit quaternion from four normal-ish draws.
fn random_quaternion(i: u64) -> [f64; 4] {
    let draw = |k: u64| hash_unit(SEED ^ 2, 4 * i + k) * 2.0 - 1.0;
    let q = [draw(0), draw(1), draw(2), draw(3)];
    let length = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    q.map(|c| c / length)
}

/// `b` flipped onto `a`'s hemisphere, since `q` and `-q` are one rotation.
fn aligned(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    if dot < 0.0 { b.map(|c| -c) } else { b }
}

#[test]
fn smallest_three_drops_the_largest_component() {
    for largest in 0..4 {
        let mut q = [0.1, -0.2, 0.3, -0.1];
        q[largest] = 0.9;
        let (index, _) = ROTATION.encode(q).unwrap();
        assert_eq!(usize::from(index), largest);
        // Negative, too: magnitude decides, not sign.
        q[largest] = -0.9;
        let (index, _) = ROTATION.encode(q).unwrap();
        assert_eq!(usize::from(index), largest);
    }
}

/// **`q` and `-q` encode identically**: the sign is chosen to make the dropped
/// component positive, so the decoder's positive square root is right.
#[test]
fn smallest_three_encodes_q_and_minus_q_alike() {
    for i in 0..1_000 {
        let q = random_quaternion(i);
        assert_eq!(ROTATION.encode(q), ROTATION.encode(q.map(|c| -c)), "{q:?}");
    }
}

#[test]
fn smallest_three_round_trips_the_identity_exactly() {
    let identity = [0.0, 0.0, 0.0, 1.0];
    let (index, codes) = ROTATION.encode(identity).unwrap();
    assert_eq!(ROTATION.decode(index, codes), Some(identity));
}

/// **Reconstruction error, bounded.** Each sent component is within half a
/// quantum `h`. The dropped one is `L = sqrt(1 - Σcᵢ²)`, so an error `δᵢ` in
/// each sent `cᵢ` moves it by about `-Σcᵢδᵢ / L`; the sent three have
/// `Σ|cᵢ| ≤ √3 · √(1 - L²)`, and `L ≥ 1/2` (the largest of four squares
/// summing to one is at least a quarter), which bounds that by `3h`.
/// Renormalising moves every component by a second-order amount on top, so
/// four half-quanta per component is the bound held here.
#[test]
fn smallest_three_reconstructs_within_its_bound() {
    let bound = 4.0 * ROTATION.quantum() / 2.0;
    let mut worst: f64 = 0.0;
    for i in 0..SAMPLES {
        let q = random_quaternion(i);
        let (index, codes) = ROTATION.encode(q).unwrap();
        let decoded = aligned(q, ROTATION.decode(index, codes).unwrap());
        let length: f64 = decoded.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((length - 1.0).abs() < 1e-12, "decoded unit length");
        for (a, b) in q.iter().zip(decoded) {
            worst = worst.max((a - b).abs());
        }
    }
    assert!(
        worst <= bound,
        "worst component error {worst:e} > {bound:e}"
    );
}

/// **Two components tied at `1/√2`**: the first is dropped, and the other,
/// sitting exactly on the range's edge, is still carried.
#[test]
fn smallest_three_takes_the_first_of_a_tie_and_carries_the_edge() {
    let q = [0.0, FRAC_1_SQRT_2, 0.0, FRAC_1_SQRT_2];
    let (index, codes) = ROTATION.encode(q).unwrap();
    assert_eq!(index, 1);
    assert_eq!(codes[2], ROTATION.max_code(), "w at +1/√2 is the top code");
    let decoded = ROTATION.decode(index, codes).unwrap();
    for (a, b) in q.iter().zip(decoded) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn smallest_three_refuses_what_is_not_a_rotation() {
    assert_eq!(ROTATION.encode([0.0; 4]), None);
    assert_eq!(ROTATION.encode([f64::NAN, 0.0, 0.0, 1.0]), None);
    assert_eq!(ROTATION.encode([f64::INFINITY, 0.0, 0.0, 1.0]), None);
    // The code the encoder never writes, and an index past w.
    let top = ROTATION.max_code() + 1;
    assert_eq!(ROTATION.decode(0, [top, 0, 0]), None);
    assert_eq!(ROTATION.decode(4, [0, 0, 0]), None);
}

// ── Schemas ─────────────────────────────────────────────────────────────────

const MIXED: &[Field] = &[
    Field {
        name: "x",
        codec: Codec::Fixed(POSITION),
    },
    Field {
        name: "speed",
        codec: Codec::Half,
    },
    Field {
        name: "rotation",
        codec: Codec::Rotation(ROTATION),
    },
    Field {
        name: "mass",
        codec: Codec::Exact,
    },
];

#[test]
fn a_schema_counts_its_values_and_bytes() {
    assert_eq!(value_count(MIXED), 7);
    // 24 + 16 + (2 + 3 × 18) + 64 = 160 bits.
    assert_eq!(encoded_len(MIXED), 20);
    assert_eq!(encoded_len(&[]), 0);
    let odd = [Field {
        name: "odd",
        codec: Codec::Fixed(Fixed::new(0.0, 1.0, 9)),
    }];
    assert_eq!(encoded_len(&odd), 2, "nine bits round up to two bytes");
}

/// **An all-[`Codec::Exact`] schema is the identity encoding**: the bytes are
/// each `f64`'s own, little-endian, in order — what an undeclared component
/// ships.
#[test]
fn an_exact_schema_encodes_byte_identically_to_the_raw_values() {
    let exact = [
        Field {
            name: "a",
            codec: Codec::Exact,
        },
        Field {
            name: "b",
            codec: Codec::Exact,
        },
        Field {
            name: "c",
            codec: Codec::Exact,
        },
    ];
    let values = [1.25, -0.0, f64::NAN];
    let mut wire = Vec::new();
    encode_values(&exact, &values, &mut wire).unwrap();
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(wire, raw);

    let mut decoded = [0.0; 3];
    decode_values(&exact, &wire, &mut decoded).unwrap();
    for (a, b) in values.iter().zip(decoded) {
        assert_eq!(a.to_bits(), b.to_bits(), "bit for bit, NaN included");
    }
}

#[test]
fn a_mixed_schema_round_trips_each_field_within_its_bound() {
    let q = random_quaternion(7);
    let values = [12.345, -3.75, q[0], q[1], q[2], q[3], 70.125];
    let mut wire = vec![0xEE];
    encode_values(MIXED, &values, &mut wire).unwrap();
    assert_eq!(wire[0], 0xEE, "appended after what was there");
    assert_eq!(wire.len(), 1 + encoded_len(MIXED));

    let mut decoded = [0.0; 7];
    decode_values(MIXED, &wire[1..], &mut decoded).unwrap();
    assert!((decoded[0] - values[0]).abs() <= POSITION.quantum() / 2.0);
    assert_eq!(decoded[1], -3.75, "exact in binary16");
    let rotation = aligned(q, [decoded[2], decoded[3], decoded[4], decoded[5]]);
    for (a, b) in q.iter().zip(rotation) {
        assert!((a - b).abs() <= 2.0 * ROTATION.quantum());
    }
    assert_eq!(decoded[6], 70.125);
}

/// **A refused value leaves the buffer as it was**, so the caller can write a
/// different form in its place without cleaning up a half-written one.
#[test]
fn a_refused_value_names_its_field_and_leaves_the_buffer_alone() {
    let q = [0.0, 0.0, 0.0, 1.0];
    let cases = [
        ([5000.0, 1.0, q[0], q[1], q[2], q[3], 1.0], "x"),
        ([0.0, 65520.0, q[0], q[1], q[2], q[3], 1.0], "speed"),
        ([0.0, f64::NAN, q[0], q[1], q[2], q[3], 1.0], "speed"),
        ([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], "rotation"),
    ];
    for (values, field) in cases {
        let mut wire = vec![1, 2, 3];
        assert_eq!(
            encode_values(MIXED, &values, &mut wire),
            Err(QuantizeError::Unrepresentable { field })
        );
        assert_eq!(wire, [1, 2, 3]);
    }
    assert_eq!(
        encode_values(MIXED, &[0.0; 3], &mut Vec::new()),
        Err(QuantizeError::ValueCount {
            expected: 7,
            actual: 3
        })
    );
}

#[test]
fn decode_refuses_a_payload_that_is_not_the_one_encoding() {
    let values = [1.0, 2.0, 0.0, 0.0, 0.0, 1.0, 3.0];
    let mut wire = Vec::new();
    encode_values(MIXED, &values, &mut wire).unwrap();
    let mut out = [0.0; 7];

    assert_eq!(
        decode_values(MIXED, &wire[1..], &mut out),
        Err(QuantizeError::Length {
            expected: 20,
            actual: 19
        })
    );
    assert_eq!(
        decode_values(MIXED, &wire, &mut [0.0; 6]),
        Err(QuantizeError::ValueCount {
            expected: 7,
            actual: 6
        })
    );

    // A set padding bit: nine bits of a two-byte payload leave seven spare.
    let odd = [Field {
        name: "odd",
        codec: Codec::Fixed(Fixed::new(0.0, 1.0, 9)),
    }];
    let mut one = [0.0];
    assert_eq!(decode_values(&odd, &[0xFF, 0x01], &mut one), Ok(()));
    assert_eq!(
        decode_values(&odd, &[0xFF, 0x03], &mut one),
        Err(QuantizeError::NonCanonical { field: "padding" })
    );

    // The rotation code the encoder never writes: every bit of the first
    // sent component set. It starts two bits after the rotation's index,
    // which starts at bit 24 + 16.
    let mut forged = wire.clone();
    for bit in 42..42 + ROTATION.bits() as usize {
        forged[bit / 8] |= 1 << (bit % 8);
    }
    assert_eq!(
        decode_values(MIXED, &forged, &mut out),
        Err(QuantizeError::NonCanonical { field: "rotation" })
    );
}

/// A component declaring a schema through [`Quantized`].
#[derive(Debug, PartialEq)]
struct Probe {
    height: f64,
    heading: [f64; 4],
}

impl Quantized for Probe {
    const SCHEMA: &'static [Field] = &[
        Field {
            name: "height",
            codec: Codec::Fixed(POSITION),
        },
        Field {
            name: "heading",
            codec: Codec::Rotation(ROTATION),
        },
    ];
    type Values = [f64; 5];

    fn to_values(&self) -> [f64; 5] {
        let [x, y, z, w] = self.heading;
        [self.height, x, y, z, w]
    }

    fn from_values(values: &[f64; 5]) -> Option<Self> {
        (values[0] >= 0.0).then(|| Self {
            height: values[0],
            heading: [values[1], values[2], values[3], values[4]],
        })
    }
}

#[test]
fn a_quantized_component_encodes_through_its_schema_and_can_refuse_on_decode() {
    let probe = Probe {
        height: 2.5,
        heading: [0.0, 0.0, 0.0, 1.0],
    };
    let mut wire = Vec::new();
    encode(&probe, &mut wire).unwrap();
    assert_eq!(wire.len(), encoded_len(Probe::SCHEMA));
    assert_eq!(decode::<Probe>(&wire), Ok(probe));

    let below = Probe {
        height: -2.5,
        heading: [0.0, 0.0, 0.0, 1.0],
    };
    let mut wire = Vec::new();
    encode(&below, &mut wire).unwrap();
    assert_eq!(decode::<Probe>(&wire), Err(QuantizeError::Refused));
}
