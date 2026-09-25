//! A [`Binding`]'s stable text form, both ways — what a player's saved rebinds
//! are written in.
//!
//! [`Binding`]'s `Display` prints it and its `FromStr` reads it back, so a
//! binding round-trips through a settings file whatever holds the file. Like
//! [`KeyCode::as_str`], which every key here is spelled with, **it is a
//! serialization format, not a display string**: a rebind menu shows the
//! keysym or the pad glyph, never this.
//!
//! | Binding                | Text                                   |
//! | ---------------------- | -------------------------------------- |
//! | `Key`                  | `KeyR`                                 |
//! | `Chord`                | `Alt+KeyR` (`Shift`, `Control`, `Alt`, `Super`) |
//! | `ScrollChord`          | `ControlLeft+Scroll`                   |
//! | `MouseButton`          | `Mouse:Left`, `Mouse:Back`, `Mouse:9`  |
//! | `MouseMotion`          | `MouseMotion`                          |
//! | `MouseScroll`          | `MouseScroll`                          |
//! | `PointerPosition`      | `Pointer:X`, `Pointer:Y`               |
//! | `KeyAxis`              | `KeyAxis:KeyS,KeyW` (negative, positive) |
//! | `Wasd`                 | `Wasd:KeyW,KeyS,KeyA,KeyD` (up, down, left, right) |
//! | `Virtual`              | `Virtual:` and the control's id, verbatim |
//! | `PadButton`            | `Pad:South`, by [`PadButton`] variant  |
//! | `PadDpad`              | `Pad:Dpad`                             |
//! | `PadStick`             | `PadStick:Left>0.2` (the dead zone)    |
//! | `PadTrigger`           | `PadTrigger:Right>0.25` (the threshold) |
//!
//! A dead zone or threshold prints as Rust's shortest `f32` form, which reads
//! back to the same bits. Parsing checks the *spelling* only: a threshold
//! outside `0.0..1.0` parses, and [`ActionMap::rebind`] refuses it as it
//! refuses one built in code. A name this build does not know is a
//! [`BindingParseError`], never a panic, so a file written by a newer build
//! costs the one binding and not the load.
//!
//! [`ActionMap::rebind`]: crate::ActionMap::rebind

use core::fmt;
use core::str::FromStr;

use crcbl_core::input::{KeyCode, PointerButton};

use crate::{Binding, Modifier, PadButton, PointerAxis, Stick, Trigger};

/// Why a string is not a [`Binding`]'s text form — see the
/// [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingParseError {
    /// The text that did not parse.
    pub text: String,
    /// What was wrong with it.
    pub reason: &'static str,
}

impl fmt::Display for BindingParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a binding: {}", self.text, self.reason)
    }
}

impl std::error::Error for BindingParseError {}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => f.write_str(key.as_str()),
            Self::Chord { modifier, key } => {
                write!(f, "{}+{}", modifier_name(*modifier), key.as_str())
            }
            Self::ScrollChord { held } => write!(f, "{}+Scroll", held.as_str()),
            Self::MouseButton(button) => match button {
                PointerButton::Other(index) => write!(f, "Mouse:{index}"),
                named => write!(f, "Mouse:{}", pointer_button_name(*named)),
            },
            Self::MouseMotion => f.write_str("MouseMotion"),
            Self::MouseScroll => f.write_str("MouseScroll"),
            Self::PointerPosition { axis } => match axis {
                PointerAxis::X => f.write_str("Pointer:X"),
                PointerAxis::Y => f.write_str("Pointer:Y"),
            },
            Self::KeyAxis { negative, positive } => {
                write!(f, "KeyAxis:{},{}", negative.as_str(), positive.as_str())
            }
            Self::Wasd {
                up,
                down,
                left,
                right,
            } => write!(
                f,
                "Wasd:{},{},{},{}",
                up.as_str(),
                down.as_str(),
                left.as_str(),
                right.as_str()
            ),
            Self::Virtual(id) => write!(f, "Virtual:{id}"),
            Self::PadButton(button) => write!(f, "Pad:{}", pad_button_name(*button)),
            Self::PadDpad => f.write_str("Pad:Dpad"),
            Self::PadStick { stick, deadzone } => {
                write!(f, "PadStick:{}>{deadzone}", stick_name(*stick))
            }
            Self::PadTrigger { trigger, threshold } => {
                write!(f, "PadTrigger:{}>{threshold}", trigger_name(*trigger))
            }
        }
    }
}

impl FromStr for Binding {
    type Err = BindingParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let fail = |reason| BindingParseError {
            text: text.to_owned(),
            reason,
        };
        match text {
            "MouseMotion" => return Ok(Self::MouseMotion),
            "MouseScroll" => return Ok(Self::MouseScroll),
            "Pad:Dpad" => return Ok(Self::PadDpad),
            _ => {}
        }
        if let Some((kind, rest)) = text.split_once(':') {
            return match kind {
                "Mouse" => rest
                    .parse()
                    .map(PointerButton::Other)
                    .ok()
                    .or_else(|| pointer_button_named(rest))
                    .map(Self::MouseButton)
                    .ok_or_else(|| fail("no such mouse button")),
                "Pointer" => match rest {
                    "X" => Ok(Self::PointerPosition {
                        axis: PointerAxis::X,
                    }),
                    "Y" => Ok(Self::PointerPosition {
                        axis: PointerAxis::Y,
                    }),
                    _ => Err(fail("a pointer axis is X or Y")),
                },
                "KeyAxis" => match keys::<2>(rest) {
                    Some([negative, positive]) => Ok(Self::KeyAxis { negative, positive }),
                    None => Err(fail("a key axis is two key names")),
                },
                "Wasd" => match keys::<4>(rest) {
                    Some([up, down, left, right]) => Ok(Self::Wasd {
                        up,
                        down,
                        left,
                        right,
                    }),
                    None => Err(fail("a wasd binding is four key names")),
                },
                "Virtual" => Ok(Self::Virtual(rest.to_owned())),
                "Pad" => PadButton::ALL
                    .into_iter()
                    .find(|button| pad_button_name(*button) == rest)
                    .map(Self::PadButton)
                    .ok_or_else(|| fail("no such pad button")),
                "PadStick" => {
                    let (stick, deadzone) = named_level(rest, stick_named)
                        .ok_or_else(|| fail("a stick is Left or Right, then > and a dead zone"))?;
                    Ok(Self::PadStick { stick, deadzone })
                }
                "PadTrigger" => {
                    let (trigger, threshold) =
                        named_level(rest, trigger_named).ok_or_else(|| {
                            fail("a trigger is Left or Right, then > and a threshold")
                        })?;
                    Ok(Self::PadTrigger { trigger, threshold })
                }
                _ => Err(fail("no such binding kind")),
            };
        }
        if let Some((first, second)) = text.split_once('+') {
            if second == "Scroll" {
                return KeyCode::from_name(first)
                    .map(|held| Self::ScrollChord { held })
                    .ok_or_else(|| fail("no such key before +Scroll"));
            }
            let modifier = modifier_named(first).ok_or_else(|| fail("no such modifier"))?;
            let key = KeyCode::from_name(second).ok_or_else(|| fail("no such key"))?;
            return Ok(Self::Chord { modifier, key });
        }
        KeyCode::from_name(text)
            .map(Self::Key)
            .ok_or_else(|| fail("no such key"))
    }
}

/// `N` comma-separated key names.
fn keys<const N: usize>(text: &str) -> Option<[KeyCode; N]> {
    let mut out = [KeyCode::KeyA; N];
    let mut names = text.split(',');
    for slot in &mut out {
        *slot = KeyCode::from_name(names.next()?)?;
    }
    names.next().is_none().then_some(out)
}

/// `Name>level`, with the name read by `named`.
fn named_level<T>(text: &str, named: fn(&str) -> Option<T>) -> Option<(T, f32)> {
    let (name, level) = text.split_once('>')?;
    Some((named(name)?, level.parse().ok()?))
}

const fn modifier_name(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Shift => "Shift",
        Modifier::Control => "Control",
        Modifier::Alt => "Alt",
        Modifier::Super => "Super",
    }
}

fn modifier_named(name: &str) -> Option<Modifier> {
    [
        Modifier::Shift,
        Modifier::Control,
        Modifier::Alt,
        Modifier::Super,
    ]
    .into_iter()
    .find(|modifier| modifier_name(*modifier) == name)
}

/// A named [`PointerButton`]'s name; `Other` prints as its index instead.
const fn pointer_button_name(button: PointerButton) -> &'static str {
    match button {
        PointerButton::Left => "Left",
        PointerButton::Right => "Right",
        PointerButton::Middle => "Middle",
        PointerButton::Back => "Back",
        PointerButton::Forward => "Forward",
        PointerButton::Other(_) => "Other",
    }
}

fn pointer_button_named(name: &str) -> Option<PointerButton> {
    [
        PointerButton::Left,
        PointerButton::Right,
        PointerButton::Middle,
        PointerButton::Back,
        PointerButton::Forward,
    ]
    .into_iter()
    .find(|button| pointer_button_name(*button) == name)
}

const fn pad_button_name(button: PadButton) -> &'static str {
    match button {
        PadButton::South => "South",
        PadButton::East => "East",
        PadButton::West => "West",
        PadButton::North => "North",
        PadButton::LeftShoulder => "LeftShoulder",
        PadButton::RightShoulder => "RightShoulder",
        PadButton::LeftStick => "LeftStick",
        PadButton::RightStick => "RightStick",
        PadButton::Start => "Start",
        PadButton::Select => "Select",
        PadButton::DpadUp => "DpadUp",
        PadButton::DpadDown => "DpadDown",
        PadButton::DpadLeft => "DpadLeft",
        PadButton::DpadRight => "DpadRight",
        PadButton::Guide => "Guide",
    }
}

const fn stick_name(stick: Stick) -> &'static str {
    match stick {
        Stick::Left => "Left",
        Stick::Right => "Right",
    }
}

fn stick_named(name: &str) -> Option<Stick> {
    [Stick::Left, Stick::Right]
        .into_iter()
        .find(|stick| stick_name(*stick) == name)
}

const fn trigger_name(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Left => "Left",
        Trigger::Right => "Right",
    }
}

fn trigger_named(name: &str) -> Option<Trigger> {
    [Trigger::Left, Trigger::Right]
        .into_iter()
        .find(|trigger| trigger_name(*trigger) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of every variant, and every name each one can carry.
    fn every_binding() -> Vec<Binding> {
        let mut all = vec![
            Binding::MouseMotion,
            Binding::MouseScroll,
            Binding::PointerPosition {
                axis: PointerAxis::X,
            },
            Binding::PointerPosition {
                axis: PointerAxis::Y,
            },
            Binding::KeyAxis {
                negative: KeyCode::KeyS,
                positive: KeyCode::KeyW,
            },
            Binding::Wasd {
                up: KeyCode::ArrowUp,
                down: KeyCode::ArrowDown,
                left: KeyCode::ArrowLeft,
                right: KeyCode::ArrowRight,
            },
            Binding::Virtual("fire".to_owned()),
            Binding::Virtual("odd:id+with>marks,".to_owned()),
            Binding::Virtual(String::new()),
            Binding::PadDpad,
            Binding::MouseButton(PointerButton::Other(9)),
        ];
        all.extend(KeyCode::ALL.iter().map(|key| Binding::Key(*key)));
        all.extend(
            KeyCode::ALL
                .iter()
                .map(|key| Binding::ScrollChord { held: *key }),
        );
        for modifier in [
            Modifier::Shift,
            Modifier::Control,
            Modifier::Alt,
            Modifier::Super,
        ] {
            all.push(Binding::Chord {
                modifier,
                key: KeyCode::Tab,
            });
        }
        all.extend(
            [
                PointerButton::Left,
                PointerButton::Right,
                PointerButton::Middle,
                PointerButton::Back,
                PointerButton::Forward,
            ]
            .map(Binding::MouseButton),
        );
        all.extend(PadButton::ALL.map(Binding::PadButton));
        for (stick, trigger) in [(Stick::Left, Trigger::Left), (Stick::Right, Trigger::Right)] {
            for level in [0.0, 0.1, 0.25, 1.0 / 3.0, 0.999_999_9] {
                all.push(Binding::PadStick {
                    stick,
                    deadzone: level,
                });
                all.push(Binding::PadTrigger {
                    trigger,
                    threshold: level,
                });
            }
        }
        all
    }

    #[test]
    fn every_binding_reads_back_as_itself() {
        let all = every_binding();
        for binding in &all {
            let text = binding.to_string();
            assert_eq!(
                text.parse::<Binding>().as_ref(),
                Ok(binding),
                "{binding:?} printed as {text:?}"
            );
        }
        // Every variant was reached: a new one fails to compile the `match`
        // in `Display`, and this makes sure the list tests each one we have.
        let kinds: std::collections::HashSet<_> = all.iter().map(std::mem::discriminant).collect();
        assert_eq!(kinds.len(), 14, "one of each Binding variant");
    }

    #[test]
    fn the_spelling_is_the_documented_one() {
        for (binding, text) in [
            (Binding::Key(KeyCode::KeyR), "KeyR"),
            (
                Binding::Chord {
                    modifier: Modifier::Alt,
                    key: KeyCode::KeyR,
                },
                "Alt+KeyR",
            ),
            (
                Binding::ScrollChord {
                    held: KeyCode::ControlLeft,
                },
                "ControlLeft+Scroll",
            ),
            (Binding::MouseButton(PointerButton::Left), "Mouse:Left"),
            (Binding::PadButton(PadButton::South), "Pad:South"),
            (
                Binding::PadTrigger {
                    trigger: Trigger::Right,
                    threshold: 0.25,
                },
                "PadTrigger:Right>0.25",
            ),
            (
                Binding::PadStick {
                    stick: Stick::Left,
                    deadzone: 0.2,
                },
                "PadStick:Left>0.2",
            ),
            (
                Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                },
                "Wasd:KeyW,KeyS,KeyA,KeyD",
            ),
        ] {
            assert_eq!(binding.to_string(), text);
        }
    }

    #[test]
    fn a_name_this_build_does_not_know_is_an_error_naming_the_text() {
        for text in [
            "",
            "KeyNotAKey",
            "Hyper+KeyR",
            "Alt+KeyNotAKey",
            "NotAKey+Scroll",
            "Mouse:Thumb3",
            "Mouse:-1",
            "Pointer:Z",
            "KeyAxis:KeyS",
            "KeyAxis:KeyS,KeyW,KeyA",
            "Wasd:KeyW,KeyS,KeyA",
            "Pad:Paddle1",
            "PadStick:Middle>0.2",
            "PadStick:Left",
            "PadTrigger:Right>much",
            "Joystick:Left",
        ] {
            let error = text.parse::<Binding>().expect_err(text);
            assert_eq!(error.text, text);
            assert!(error.to_string().contains("is not a binding"), "{error}");
        }
    }

    #[test]
    fn a_level_parses_whatever_rebind_will_refuse() {
        // The spelling is fine; the value is `rebind`'s to refuse.
        assert_eq!(
            "PadTrigger:Left>1.5".parse::<Binding>(),
            Ok(Binding::PadTrigger {
                trigger: Trigger::Left,
                threshold: 1.5
            })
        );
    }
}
