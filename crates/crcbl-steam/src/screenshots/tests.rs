//! The screenshot library over the fake.

use super::*;
use crate::{
    AppId, EResult, SteamEvent,
    client::init_on,
    testing::{self, FakeMsg, FakeScreenshots},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeScreenshots) -> R) -> R {
    testing::script(|s| f(&mut s.screenshots))
}

#[test]
fn hooking_triggering_and_asking_reach_steam() {
    let steam = steam();
    let screenshots = steam.screenshots();
    assert!(!screenshots.hooked());
    screenshots.hook(true);
    assert!(screenshots.hooked());
    screenshots.trigger();
    screenshots.hook(false);
    assert!(!screenshots.hooked());
    assert_eq!(fake(|f| f.triggered), 1);
}

/// **The pixels are sized before the call**: exactly `3 × width × height`
/// bytes reach Steam, as a copy.
#[test]
fn a_screenshot_of_the_right_size_is_written_whole() {
    let steam = steam();
    fake(|f| f.handle = 42);
    let rgb: Vec<u8> = (0..2 * 3 * 3).collect();
    assert_eq!(steam.screenshots().write(&rgb, 3, 2), Ok(ScreenshotId(42)));
    assert_eq!(fake(|f| f.written.clone()), [(rgb, 18, 3, 2)]);
}

/// A buffer that does not add up, a zero or oversized dimension, or a size
/// whose product overflows is refused, and Steam reads nothing.
#[test]
fn pixels_that_do_not_add_up_are_refused_before_the_call() {
    let steam = steam();
    fake(|f| f.handle = 42);
    let screenshots = steam.screenshots();
    let pixels = SteamError::OutOfRange("screenshot pixels");
    assert_eq!(screenshots.write(&[0; 17], 3, 2), Err(pixels.clone()));
    assert_eq!(screenshots.write(&[0; 19], 3, 2), Err(pixels.clone()));
    assert_eq!(screenshots.write(&[], 0, 2), Err(pixels.clone()));
    assert_eq!(screenshots.write(&[0; 3], 65_536, 65_536), Err(pixels));
    assert_eq!(
        screenshots.write(&[0; 3], u32::MAX, 1),
        Err(SteamError::OutOfRange("screenshot size"))
    );
    assert!(fake(|f| f.written.is_empty()));
}

#[test]
fn an_invalid_handle_is_a_refusal() {
    let steam = steam();
    assert_eq!(
        steam.screenshots().write(&[1, 2, 3], 1, 1),
        Err(SteamError::Refused("WriteScreenshot"))
    );
}

#[test]
fn tags_and_locations_reach_steam_and_a_refusal_is_an_error() {
    let steam = steam();
    let screenshots = steam.screenshots();
    screenshots
        .set_location(ScreenshotId(7), "The Pit")
        .unwrap();
    screenshots
        .tag_user(ScreenshotId(7), SteamId(testing::STEAM_ID))
        .unwrap();
    assert_eq!(fake(|f| f.locations.clone()), [(7, "The Pit".to_owned())]);
    assert_eq!(fake(|f| f.tags.clone()), [(7, testing::STEAM_ID)]);
    assert_eq!(
        screenshots.set_location(ScreenshotId(7), "a\0b"),
        Err(SteamError::InteriorNul("location"))
    );
    testing::script(|s| s.refuse = true);
    assert_eq!(
        screenshots.set_location(ScreenshotId(7), "x"),
        Err(SteamError::Refused("SetLocation"))
    );
    assert_eq!(
        screenshots.tag_user(ScreenshotId(7), SteamId(1)),
        Err(SteamError::Refused("TagUser"))
    );
}

#[test]
fn the_screenshot_callbacks_decode_to_events() {
    let mut steam = steam();
    let mut ready = 9_u32.to_le_bytes().to_vec();
    ready.extend_from_slice(&1_i32.to_le_bytes());
    testing::script(|s| {
        s.queue.push_back(FakeMsg::payload(2302, vec![0]));
        s.queue.push_back(FakeMsg::payload(2301, ready));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [
            SteamEvent::ScreenshotRequested,
            SteamEvent::ScreenshotReady {
                screenshot: ScreenshotId(9),
                result: EResult::OK,
            },
        ]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}
