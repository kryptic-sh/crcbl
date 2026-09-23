//! The on-screen keyboards over the fake.

use super::*;
use crate::{
    AppId,
    client::init_on,
    testing::{self, FakeKeyboard, FakeMsg},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn keyboard<R>(f: impl FnOnce(&mut FakeKeyboard) -> R) -> R {
    testing::script(|s| f(&mut s.keyboard))
}

/// A `GamepadTextInputDismissed_t` payload, little-endian: submitted, three
/// bytes of padding, the length, the app.
fn dismissed(submitted: bool, length: u32, app: u32) -> FakeMsg {
    let mut bytes = vec![u8::from(submitted), 0, 0, 0];
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&app.to_le_bytes());
    FakeMsg::payload(714, bytes)
}

/// Pumps one message and drains what it queued.
fn pump(steam: &mut Steam, msg: FakeMsg) -> Vec<SteamEvent> {
    testing::script(|s| s.queue.push_back(msg));
    steam.pump();
    steam.events().collect()
}

const REQUEST: TextInputRequest<'static> = TextInputRequest {
    mode: TextInputMode::Password,
    lines: TextInputLines::Multiple,
    description: "Squad name",
    max_chars: 24,
    existing: "Crucible",
};

#[test]
fn the_full_screen_keyboard_is_shown_with_the_request_as_given() {
    let steam = steam();
    steam.utils().show_text_input(&REQUEST).unwrap();
    assert_eq!(
        keyboard(|k| k.shown.clone()),
        [(1, 1, "Squad name".to_owned(), 24, "Crucible".to_owned())]
    );
    steam
        .utils()
        .show_text_input(&TextInputRequest {
            mode: TextInputMode::Normal,
            lines: TextInputLines::Single,
            ..REQUEST
        })
        .unwrap();
    assert_eq!(keyboard(|k| (k.shown[1].0, k.shown[1].1)), (0, 0));
}

#[test]
fn a_nul_in_either_string_is_refused_before_the_call() {
    let steam = steam();
    for (request, argument) in [
        (
            TextInputRequest {
                description: "a\0b",
                ..REQUEST
            },
            "description",
        ),
        (
            TextInputRequest {
                existing: "a\0b",
                ..REQUEST
            },
            "existing",
        ),
    ] {
        assert_eq!(
            steam.utils().show_text_input(&request),
            Err(SteamError::InteriorNul(argument))
        );
    }
    assert!(keyboard(|k| k.shown.is_empty()));
}

#[test]
fn a_keyboard_steam_will_not_show_or_dismiss_is_an_error_naming_the_call() {
    let steam = steam();
    testing::script(|s| s.refuse = true);
    let utils = steam.utils();
    assert_eq!(
        utils.show_text_input(&REQUEST),
        Err(SteamError::Refused("ShowGamepadTextInput"))
    );
    assert_eq!(
        utils.show_floating_keyboard(FloatingKeyboardMode::Email, TextField::default()),
        Err(SteamError::Refused("ShowFloatingGamepadTextInput"))
    );
    assert_eq!(
        utils.dismiss_text_input(),
        Err(SteamError::Refused("DismissGamepadTextInput"))
    );
    assert_eq!(
        utils.dismiss_floating_keyboard(),
        Err(SteamError::Refused("DismissFloatingGamepadTextInput"))
    );
}

#[test]
fn the_floating_keyboard_is_placed_beside_the_field() {
    let steam = steam();
    let field = TextField {
        x: 40,
        y: 300,
        width: 400,
        height: 32,
    };
    for (mode, raw) in [
        (FloatingKeyboardMode::SingleLine, 0),
        (FloatingKeyboardMode::MultipleLines, 1),
        (FloatingKeyboardMode::Email, 2),
        (FloatingKeyboardMode::Numeric, 3),
    ] {
        steam.utils().show_floating_keyboard(mode, field).unwrap();
        assert_eq!(
            keyboard(|k| k.floating.last().copied()),
            Some((raw, 40, 300, 400, 32))
        );
    }
    steam.utils().dismiss_floating_keyboard().unwrap();
    steam.utils().dismiss_text_input().unwrap();
    assert_eq!(
        keyboard(|k| k.dismissed.clone()),
        ["DismissFloatingGamepadTextInput", "DismissGamepadTextInput"]
    );
}

/// **The accepted text is read into a buffer the reported length sizes**,
/// with room for its NUL, and arrives whole.
#[test]
fn accepted_text_is_read_at_the_reported_length_and_arrives_whole() {
    let mut steam = steam();
    keyboard(|k| k.text = "Crucible Squad 🛡".as_bytes().to_vec());
    let length = keyboard(|k| u32::try_from(k.text.len()).unwrap());
    let events = pump(&mut steam, dismissed(true, length, 480));
    assert_eq!(
        events,
        [SteamEvent::TextInputDismissed {
            text: Some("Crucible Squad 🛡".to_owned())
        }]
    );
    assert_eq!(keyboard(|k| k.offered.clone()), [length + 1]);
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

/// **A cancelled keyboard is `None`**, and no text is asked for.
#[test]
fn a_cancelled_keyboard_is_none_and_reads_nothing() {
    let mut steam = steam();
    keyboard(|k| k.text = b"typed then cancelled".to_vec());
    assert_eq!(
        pump(&mut steam, dismissed(false, 0, 480)),
        [SteamEvent::TextInputDismissed { text: None }]
    );
    assert!(keyboard(|k| k.offered.is_empty()));
}

/// A length past any keyboard's, or a read Steam refuses, is `None` and
/// counted — never a guess, and never a four-gigabyte allocation.
#[test]
fn text_steam_will_not_hand_over_is_none_and_counted() {
    let mut steam = steam();
    keyboard(|k| k.length = Some(u32::MAX));
    assert_eq!(
        pump(&mut steam, dismissed(true, u32::MAX, 480)),
        [SteamEvent::TextInputDismissed { text: None }]
    );
    assert!(
        keyboard(|k| k.offered.is_empty()),
        "nothing allocated for it"
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 1);

    keyboard(|k| {
        k.length = None;
        k.text = b"ok".to_vec();
        k.refuse_read = true;
    });
    assert_eq!(
        pump(&mut steam, dismissed(true, 2, 480)),
        [SteamEvent::TextInputDismissed { text: None }]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 2);
}

/// A length that does not count the NUL still reads the whole text: the
/// buffer has one byte more than the length.
#[test]
fn a_length_without_the_nul_still_reads_the_whole_text() {
    let mut steam = steam();
    keyboard(|k| k.text = b"ninechars".to_vec());
    let events = pump(&mut steam, dismissed(true, 9, 480));
    assert_eq!(
        events,
        [SteamEvent::TextInputDismissed {
            text: Some("ninechars".to_owned())
        }]
    );
}

#[test]
fn text_that_is_not_utf8_is_read_lossily_and_counted() {
    let mut steam = steam();
    keyboard(|k| k.text = b"caf\xE9".to_vec());
    assert_eq!(
        pump(&mut steam, dismissed(true, 4, 480)),
        [SteamEvent::TextInputDismissed {
            text: Some("caf\u{FFFD}".to_owned())
        }]
    );
    assert_eq!(steam.diagnostics().lossy_strings, 1);
}

#[test]
fn another_apps_dismissal_is_counted_unknown() {
    let mut steam = steam();
    assert_eq!(pump(&mut steam, dismissed(true, 0, 570)), []);
    assert_eq!(steam.diagnostics().unknown, 1);
}

#[test]
fn the_floating_keyboard_closing_is_an_event() {
    let mut steam = steam();
    assert_eq!(
        pump(&mut steam, FakeMsg::payload(738, vec![0])),
        [SteamEvent::FloatingKeyboardDismissed]
    );
}
