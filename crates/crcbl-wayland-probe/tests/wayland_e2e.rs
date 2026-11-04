//! The outside path against a real compositor.
//!
//! `#[ignore]`d because it needs one: `crates/crcbl-shell/tests/run-wayland-e2e.sh`
//! runs it under headless sway, and on a desktop it reads the session's own
//! outputs.

#![cfg(any(target_os = "linux", target_os = "freebsd"))]

#[test]
#[ignore = "needs a compositor with zwlr_output_manager_v1; run-wayland-e2e.sh provides one"]
fn the_output_manager_describes_every_head_and_its_current_mode() {
    let heads = crcbl_wayland_probe::read_heads().expect("the compositor describes its outputs");
    assert!(!heads.is_empty(), "a compositor drives at least one output");
    for head in &heads {
        assert!(
            !head.name.is_empty(),
            "every head names its connector: {head:?}"
        );
        if head.enabled {
            let current = head
                .current_mode
                .expect("an enabled head is driven at a mode");
            let mode = &head.modes[current];
            assert!(
                mode.width > 0 && mode.height > 0,
                "the current mode has a size: {head:?}"
            );
        }
    }
    assert!(
        heads.iter().any(|head| head.enabled),
        "at least one output is on"
    );
}
