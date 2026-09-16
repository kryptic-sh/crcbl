//! The three claims `docs/plan/56-wind.md`'s "What each rung is checked by"
//! makes about rung W1's CPU half.
//!
//! * **CPU determinism** — "two samplings of the same field at the same tick
//!   are bit-identical; a replay of a body pushed by wind lands in the same
//!   pose".
//! * **Gust coherence** — "two consumers a known distance apart along the wind
//!   see the same gust arrive separated by distance over gust speed — the
//!   property per-object scrolling loses".
//! * **Calm means calm** — "a zero-intensity texel moves nothing".
//!
//! The fourth, CPU–GPU agreement, needs a device and lives in
//! `crates/crcbl/tests/render_e2e/wind.rs`.

use std::path::Path;

use crcbl_assets::MemorySource;
use crcbl_phys::WindQuery;
use crcbl_wind::{
    Beaufort, DirectionLayer, IntensityLayer, LayerGrid, ScrollOffset, UNITS_PER_METRE, Weather,
    WindField, load_direction_layer, load_intensity_layer,
};
use glam::{DVec2, DVec3};

// The generator the committed layers came out of, for the constants that say
// what those layers describe — where the hollow is, and at what scale each
// layer was drawn. Only part of it is used from here, and the other part is
// used from `authored_layers.rs`: one module compiled into two test binaries,
// each reading the half it needs.
#[allow(dead_code)]
#[path = "../tools/authored.rs"]
mod authored;

use authored::{
    DIRECTION_METRES_PER_TEXEL, INTENSITY_METRES_PER_TEXEL, INTENSITY_TEXELS, SHELTER_CENTRE,
};

/// The tick the whole suite runs at.
const DT: f64 = 1.0 / 60.0;

/// The committed pair, loaded through the asset seam the way a game would.
fn authored_field(speed: Beaufort) -> WindField {
    let mut source = MemorySource::new();
    source
        .insert(
            Path::new("wind/direction.png"),
            include_bytes!("../assets/direction.png").to_vec(),
        )
        .expect("a legal asset key");
    source
        .insert(
            Path::new("wind/intensity.png"),
            include_bytes!("../assets/intensity.png").to_vec(),
        )
        .expect("a legal asset key");
    let direction = load_direction_layer(
        &source,
        Path::new("wind/direction.png"),
        DIRECTION_METRES_PER_TEXEL,
    )
    .expect("a direction layer");
    let intensity = load_intensity_layer(
        &source,
        Path::new("wind/intensity.png"),
        INTENSITY_METRES_PER_TEXEL,
    )
    .expect("an intensity layer");
    let mut weather = Weather::from_beaufort(DVec2::new(1.0, 0.0), speed).expect("a direction");
    weather.set_gust(0.4, 24.0).expect("a real gust");
    WindField::new(weather, direction, intensity)
}

/// A field whose layers are one texel each, so the only thing that varies over
/// space is the gust front.
fn uniform_field(amplitude: f64, wavelength: f64, gust_speed: f64) -> WindField {
    let direction = DirectionLayer::from_rgba8(
        LayerGrid::new(1, 1, 8.0).expect("a grid"),
        &[255, 128, 0, 255],
    )
    .expect("one texel");
    let intensity = IntensityLayer::from_rgba8(
        LayerGrid::new(1, 1, 2.0).expect("a grid"),
        &[255, 0, 0, 255],
    )
    .expect("one texel");
    let mut weather = Weather::new(DVec2::X, 6.0).expect("a direction");
    weather.gust_speed = gust_speed;
    weather
        .set_gust(amplitude, wavelength)
        .expect("a real gust");
    WindField::new(weather, direction, intensity)
}

/// A spread of sample positions that is not a lattice — a grid at the layer's
/// own pitch would only ever land on texel centres.
fn probe_points() -> Vec<DVec3> {
    (0..64)
        .map(|index| {
            let t = f64::from(index);
            DVec3::new(t * 7.3 - 90.0, t * 0.5 - 10.0, t * -11.7 + 40.0)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CPU determinism
// ---------------------------------------------------------------------------

#[test]
fn two_samplings_at_the_same_tick_are_bit_identical() {
    let mut field = authored_field(Beaufort::Strong);
    for tick in 0..90 {
        for point in probe_points() {
            let first = field.sample(point);
            let second = field.sample(point);
            assert_eq!(
                first.to_array().map(f64::to_bits),
                second.to_array().map(f64::to_bits),
                "tick {tick} answered {first} then {second} at {point}"
            );
        }
        field.advance(DT);
    }
}

/// The order samples are taken in cannot change any of them.
///
/// The field is stateless except for its one scroll offset, and the offset
/// advances on the tick rather than on a sample. A field that carried
/// per-sample state — the per-object scrolling decision 3 is written against —
/// would fail this.
#[test]
fn the_order_samples_are_taken_in_changes_none_of_them() {
    let mut field = authored_field(Beaufort::Breezy);
    for _ in 0..37 {
        field.advance(DT);
    }
    let points = probe_points();
    let forwards: Vec<DVec3> = points.iter().map(|point| field.sample(*point)).collect();
    let backwards: Vec<DVec3> = points
        .iter()
        .rev()
        .map(|point| field.sample(*point))
        .collect();
    for (index, (there, back)) in forwards.iter().zip(backwards.iter().rev()).enumerate() {
        assert_eq!(
            there.to_array().map(f64::to_bits),
            back.to_array().map(f64::to_bits),
            "point {index} answered {there} forwards and {back} backwards"
        );
    }
}

/// A body pushed by the wind lands in the same pose on a replay.
///
/// Explicit Euler over a drag that pulls the body's velocity towards the air's,
/// which is the shape rung W4's provider will have. What is asserted is the
/// whole trajectory, bit for bit, not just the last pose — a divergence that
/// healed by the end is still a divergence.
#[test]
fn a_replay_of_a_body_pushed_by_wind_lands_in_the_same_pose() {
    fn fly(field: &mut WindField) -> Vec<DVec3> {
        let mut position = DVec3::new(-31.5, 2.0, 12.25);
        let mut velocity = DVec3::ZERO;
        let mut track = Vec::with_capacity(600);
        for _ in 0..600 {
            let relative = field.sample(position) - velocity;
            velocity += relative * 0.35 * DT;
            position += velocity * DT;
            track.push(position);
            field.advance(DT);
        }
        track
    }

    let first = fly(&mut authored_field(Beaufort::Violent));
    let second = fly(&mut authored_field(Beaufort::Violent));
    assert_eq!(first.len(), second.len());
    for (tick, (a, b)) in first.iter().zip(&second).enumerate() {
        assert_eq!(
            a.to_array().map(f64::to_bits),
            b.to_array().map(f64::to_bits),
            "tick {tick}: {a} on the first run, {b} on the replay"
        );
    }
    let travelled = (first[first.len() - 1] - first[0]).length();
    assert!(
        travelled > 5.0,
        "the body has to move for the replay to mean anything; it travelled {travelled} m"
    );
}

// ---------------------------------------------------------------------------
// Gust coherence
// ---------------------------------------------------------------------------

/// Two consumers a known distance apart along the wind see the same gust,
/// separated by distance over gust speed.
///
/// The distance is `ticks × step`, read out of [`ScrollOffset::step`], because
/// that is the distance the one global offset actually covers in that many
/// ticks. A distance picked any other way — "eight metres" — is one the fixed
/// point lands a nanometre either side of, and an *exact* equality is a far
/// stronger claim than one inside a tolerance.
///
/// The layers are one texel each so that the only thing varying over space is
/// the gust: with a spatially varying intensity the downwind consumer is in a
/// different part of the field, and the claim would need a tolerance that hid
/// the thing being measured.
#[test]
fn a_gust_arrives_downwind_after_distance_over_gust_speed() {
    const TICKS: i64 = 120;
    let mut field = uniform_field(0.5, 24.0, 4.0);
    let (step_x, step_z) = ScrollOffset::step(&field.weather(), DT);
    assert_eq!(step_z, 0, "a wind towards +X scrolls along X alone");
    let distance = (step_x * TICKS) as f64 / UNITS_PER_METRE;
    assert!(
        distance > 7.0,
        "the two consumers have to be a real distance apart: {distance} m"
    );

    let upwind = DVec3::new(-40.0, 1.0, 3.0);
    let downwind = upwind + DVec3::new(distance, 0.0, 0.0);

    let arriving = field.sample(upwind);
    let mut seen = Vec::with_capacity(TICKS as usize + 1);
    for _ in 0..TICKS {
        seen.push(field.sample(downwind));
        field.advance(DT);
    }
    let arrived = field.sample(downwind);

    assert_eq!(
        arrived.to_array().map(f64::to_bits),
        arriving.to_array().map(f64::to_bits),
        "the gust that was at {upwind} arrived at {downwind} as {arrived}, not {arriving}"
    );

    // And it was not already there — otherwise the equality above is a claim
    // about a field that does not change.
    let spread = seen
        .iter()
        .map(|sample| (sample.length() - arriving.length()).abs())
        .fold(0.0f64, f64::max);
    assert!(
        spread > 0.5,
        "the downwind consumer saw the same wind all along ({spread} m/s of variation), \
         so the arrival proves nothing"
    );
}

/// The offset is one value for the world, so a consumer that has not been
/// sampled for a hundred ticks sees the same gust as one that has.
///
/// This is the property decision 3 is written for: *God of War*'s per-object
/// offsets diverged from their neighbours the moment direction or speed
/// changed, and the field here changes both mid-flight.
#[test]
fn the_offset_survives_the_weather_changing_under_it() {
    let mut field = uniform_field(0.5, 24.0, 4.0);
    let point = DVec3::new(11.0, 0.0, -4.0);

    let mut patient = Vec::new();
    for tick in 0..240 {
        if tick == 80 {
            let mut weather = field.weather();
            weather.gust_speed = 9.0;
            field.set_weather(weather);
        }
        if tick == 160 {
            let mut weather = field.weather();
            weather
                .set_direction(DVec2::new(0.6, 0.8))
                .expect("a real direction");
            field.set_weather(weather);
        }
        patient.push(field.sample(point));
        field.advance(DT);
    }
    let after = field.scroll();

    // The same schedule, sampling only at the end. One global offset means the
    // two agree; an offset accumulated per sample would not have advanced at
    // all for the second.
    let mut absent = uniform_field(0.5, 24.0, 4.0);
    for tick in 0..240 {
        if tick == 80 {
            let mut weather = absent.weather();
            weather.gust_speed = 9.0;
            absent.set_weather(weather);
        }
        if tick == 160 {
            let mut weather = absent.weather();
            weather
                .set_direction(DVec2::new(0.6, 0.8))
                .expect("a real direction");
            absent.set_weather(weather);
        }
        absent.advance(DT);
    }
    assert_eq!(absent.scroll(), after);
    assert_eq!(
        absent.sample(point).to_array().map(f64::to_bits),
        field.sample(point).to_array().map(f64::to_bits)
    );
    assert!(
        patient
            .iter()
            .any(|sample| (sample.length() - patient[0].length()).abs() > 0.5),
        "the gust has to be moving for this to be a claim about the offset"
    );
}

// ---------------------------------------------------------------------------
// Calm means calm
// ---------------------------------------------------------------------------

/// A zero-intensity texel moves nothing — exactly zero, at every tick, at every
/// gust amplitude, and through the trait a consumer reads.
#[test]
fn calm_means_calm_in_the_sheltered_hollow() {
    let extent = f64::from(INTENSITY_TEXELS) * INTENSITY_METRES_PER_TEXEL;
    let hollow = DVec3::new(SHELTER_CENTRE.0 * extent, 3.0, SHELTER_CENTRE.1 * extent);
    for preset in Beaufort::ALL {
        let mut field = authored_field(preset);
        for tick in 0..120 {
            let sample = field.sample(hollow);
            assert_eq!(
                sample,
                DVec3::ZERO,
                "{preset:?} at tick {tick} blew {sample} through a calm texel"
            );
            let query: &dyn WindQuery = &field;
            assert_eq!(query.wind_at(hollow), DVec3::ZERO);
            field.advance(DT);
        }
    }
}

/// The hollow is calm and its surroundings are not, so the test above is about
/// the hollow rather than about a field that never blows.
#[test]
fn the_field_around_the_hollow_is_not_calm() {
    let extent = f64::from(INTENSITY_TEXELS) * INTENSITY_METRES_PER_TEXEL;
    let field = authored_field(Beaufort::Strong);
    let mut strongest = 0.0f64;
    for step in 0..64 {
        let t = f64::from(step) / 64.0 * extent;
        strongest = strongest.max(field.sample(DVec3::new(t, 0.0, 0.0)).length());
    }
    assert!(
        strongest > 3.0,
        "the authored field never reaches 3 m/s at Beaufort strong: {strongest}"
    );
}

/// A layer that is zero everywhere moves nothing anywhere, whatever the
/// weather does — the general case of the hollow.
#[test]
fn an_entirely_calm_layer_moves_nothing_anywhere() {
    let direction = DirectionLayer::from_rgba8(
        LayerGrid::new(1, 1, 8.0).expect("a grid"),
        &[255, 128, 0, 255],
    )
    .expect("one texel");
    let intensity =
        IntensityLayer::from_rgba8(LayerGrid::new(1, 1, 2.0).expect("a grid"), &[0, 0, 0, 255])
            .expect("one texel");
    let mut weather = Weather::from_beaufort(DVec2::X, Beaufort::Violent).expect("a direction");
    weather.set_gust(1.0, 8.0).expect("a real gust");
    let mut field = WindField::new(weather, direction, intensity);
    for _ in 0..50 {
        for point in probe_points() {
            assert_eq!(field.sample(point), DVec3::ZERO);
        }
        field.advance(DT);
    }
}
