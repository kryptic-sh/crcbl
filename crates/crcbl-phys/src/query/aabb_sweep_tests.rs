use super::*;
use crate::broadphase::Segment;

#[test]
fn short_box_sweeps_keep_their_contact_time() {
    for scale in [1e300, 1e100, 1e-6, 1e-10, 1e-100, 1e-300] {
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            let target = Aabb::new(DVec3::ZERO, DVec3::splat(10.0 * scale));
            let start = DVec3::splat(2.0 * scale) - axis * (4.0 * scale);
            let hit = swept_sphere_vs_aabb(
                &Segment::new(start, start + axis * (2.0 * scale)),
                scale,
                &target,
            )
            .unwrap_or_else(|| panic!("short moving sweep missed: scale={scale} axis={axis:?}"));
            assert!((hit.t - 0.5).abs() < 1e-12);
            assert_eq!(hit.normal, -axis);
            assert!(!hit.started_inside);
            assert!(
                swept_sphere_vs_aabb(
                    &Segment::new(start, start + axis * (0.5 * scale)),
                    scale,
                    &target,
                )
                .is_none()
            );
            assert!(swept_sphere_vs_aabb(&Segment::new(start, start), scale, &target).is_none());
            let inside = start + axis * (1.5 * scale);
            let resting = swept_sphere_vs_aabb(&Segment::new(inside, inside), scale, &target)
                .expect("stationary overlap remains a contact");
            assert_eq!(resting.t, 0.0);
            assert_eq!(resting.normal, -axis);
            assert!(resting.started_inside);
        }
    }
}
