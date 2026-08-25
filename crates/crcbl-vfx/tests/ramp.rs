//! Curves and gradients: the values between the stops, and the ones outside.

use crcbl_vfx::{Curve, Gradient, RampError};
use glam::Vec4;

#[test]
fn a_ramp_clamps_at_both_ends() {
    let curve = Curve::new(vec![(0.2, 4.0), (0.8, 10.0)]).expect("two ascending stops");
    assert_eq!(
        curve.eval(-1.0),
        4.0,
        "below the first key is not the first value"
    );
    assert_eq!(curve.eval(0.2), 4.0, "the first key is not the first value");
    assert_eq!(curve.eval(0.8), 10.0, "the last key is not the last value");
    assert_eq!(
        curve.eval(9.0),
        10.0,
        "above the last key is not the last value"
    );
}

#[test]
fn a_ramp_interpolates_between_its_stops() {
    let curve = Curve::new(vec![(0.0, 0.0), (1.0, 8.0)]).expect("two ascending stops");
    assert_eq!(curve.eval(0.25), 2.0);
    assert_eq!(curve.eval(0.5), 4.0);
    assert_eq!(curve.eval(0.75), 6.0);
}

/// Three stops, because the two-stop case cannot tell a search that picks the
/// right span from one that always picks the last.
#[test]
fn a_three_stop_ramp_reads_the_span_the_sample_is_in() {
    let curve = Curve::new(vec![(0.0, 0.0), (0.5, 10.0), (1.0, 0.0)]).expect("three stops");
    assert_eq!(curve.eval(0.25), 5.0, "the first span was not read");
    assert_eq!(curve.eval(0.5), 10.0, "the middle stop was not read");
    assert_eq!(curve.eval(0.75), 5.0, "the second span was not read");
}

#[test]
fn two_stops_at_one_key_are_a_step() {
    let curve = Curve::new(vec![(0.0, 1.0), (0.5, 1.0), (0.5, 9.0), (1.0, 9.0)])
        .expect("coincident keys ascend weakly");
    assert_eq!(curve.eval(0.4), 1.0, "before the step");
    assert_eq!(curve.eval(0.5), 9.0, "a coincident pair did not step");
    assert_eq!(curve.eval(0.6), 9.0, "after the step");
}

#[test]
fn a_gradient_interpolates_every_channel() {
    let gradient = Gradient::new(vec![
        (0.0, Vec4::new(1.0, 0.0, 0.0, 1.0)),
        (1.0, Vec4::new(0.0, 1.0, 0.0, 0.0)),
    ])
    .expect("two ascending stops");
    assert_eq!(gradient.eval(0.5), Vec4::new(0.5, 0.5, 0.0, 0.5));
}

#[test]
fn a_constant_ramp_is_that_value_everywhere() {
    let curve = Curve::constant(3.5);
    assert_eq!(curve.eval(-10.0), 3.5);
    assert_eq!(curve.eval(0.5), 3.5);
    assert_eq!(curve.eval(10.0), 3.5);
    assert_eq!(curve.stops().len(), 1);
}

/// A `NaN` sample clamps to the first stop rather than falling past every
/// comparison and returning the last.
#[test]
fn a_not_a_number_sample_reads_the_first_stop() {
    let curve = Curve::new(vec![(0.0, 1.0), (1.0, 99.0)]).expect("two ascending stops");
    assert_eq!(curve.eval(f32::NAN), 1.0);
}

#[test]
fn a_ramp_refuses_stops_it_cannot_search() {
    assert_eq!(Curve::new(Vec::new()), Err(RampError::Empty));
    assert_eq!(
        Curve::new(vec![(1.0, 0.0), (0.0, 1.0)]),
        Err(RampError::Unsorted { index: 1 })
    );
    assert_eq!(
        Curve::new(vec![(0.0, 0.0), (f32::NAN, 1.0)]),
        Err(RampError::KeyNotFinite { index: 1 })
    );
    assert_eq!(
        Curve::new(vec![(f32::INFINITY, 0.0)]),
        Err(RampError::KeyNotFinite { index: 0 })
    );
}
