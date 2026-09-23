//! The framework half: `[GCController controllers]` read into the poller's
//! plain [`Seen`] list, and the public [`GameController`] over it.

use super::ffi::{self, Id, Pool, sel};
use super::{
    AXES, BUTTONS, GameControllerError, Key, Path, Poller, Reading, Seen, Source, kind_of,
};
use crate::{GamepadEvent, PadKind};

/// The element `path` reaches from `object` by property getters, or null
/// where a step is nil or a getter the object does not have — a device
/// without that element, or a macOS older than the getter.
///
/// # Safety
///
/// `object` must be a live object or null, with a pool in scope.
unsafe fn element(object: Id, path: Path) -> Id {
    let mut current = object;
    for getter in path {
        let selector = sel(getter);
        // SAFETY: `current` is live (the caller's, or a getter's return held
        // by the pool); `respondsToSelector:` guards the send, and every
        // getter on these paths takes no argument and returns an object.
        if current.is_null() || !unsafe { ffi::responds_to(current, selector) } {
            return core::ptr::null_mut();
        }
        current = unsafe { ffi::msg(current, selector) };
    }
    current
}

/// One `GCExtendedGamepad`'s elements, row for row with the tables.
///
/// # Safety
///
/// `profile` must be a live `GCExtendedGamepad`, with a pool in scope.
unsafe fn read_profile(profile: Id) -> Reading {
    let mut reading = Reading::default();
    for (pressed, &(path, _)) in reading.pressed.iter_mut().zip(&BUTTONS) {
        // SAFETY: every button path ends at a `GCControllerButtonInput`,
        // whose `isPressed` returns `BOOL`.
        let button = unsafe { element(profile, path) };
        *pressed = !button.is_null() && unsafe { ffi::msg_bool(button, sel(c"isPressed")) };
    }
    for (value, &(path, _)) in reading.values.iter_mut().zip(&AXES) {
        // SAFETY: every axis path ends at a `GCControllerButtonInput` (a
        // trigger) or a `GCControllerAxisInput` (a stick axis), and both
        // declare `float value`.
        let input = unsafe { element(profile, path) };
        if !input.is_null() {
            *value = unsafe { ffi::msg_f32(input, sel(c"value")) };
        }
    }
    reading
}

/// A controller's family from what it says about itself.
///
/// # Safety
///
/// `controller` must be a live `GCController`, with a pool in scope.
unsafe fn kind_of_controller(controller: Id) -> PadKind {
    // SAFETY: both getters return an `NSString` or nil, and `element` guards
    // `productCategory`, which arrived in macOS 10.15.
    let category = unsafe { ffi::string(element(controller, &[c"productCategory"])) };
    let vendor = unsafe { ffi::string(element(controller, &[c"vendorName"])) };
    kind_of(category.as_deref(), vendor.as_deref())
}

/// `[GCController controllers]` as a [`Source`], holding a retain on every
/// controller it has listed so each one's address stays its own.
#[derive(Debug)]
struct Framework {
    /// The `GCController` class.
    class: Id,
    /// Every controller listed at the last read, retained, with its family.
    retained: Vec<(Id, PadKind)>,
}

impl Source for Framework {
    fn read(&mut self, into: &mut Vec<Seen>) {
        into.clear();
        let _pool = Pool::push();
        // SAFETY: `+[GCController controllers]` returns an `NSArray` of
        // `GCController`s (or nil, read as empty), valid within the pool.
        let array = unsafe { ffi::msg(self.class, sel(c"controllers")) };
        let count = if array.is_null() {
            0
        } else {
            // SAFETY: `array` is a live `NSArray`.
            unsafe { ffi::msg_usize(array, sel(c"count")) }
        };
        for index in 0..count {
            // SAFETY: `index` is below the array's count; its elements are
            // `GCController`s, whose `extendedGamepad` returns the profile or
            // nil.
            let controller = unsafe { ffi::object_at(array, index) };
            let profile = unsafe { ffi::msg(controller, sel(c"extendedGamepad")) };
            if profile.is_null() {
                continue;
            }
            let kind = match self.retained.iter().find(|&&(held, _)| held == controller) {
                Some(&(_, kind)) => kind,
                None => {
                    // SAFETY: `controller` is live; the retain is balanced by
                    // the release below or in `Drop`.
                    unsafe { ffi::msg(controller, sel(c"retain")) };
                    let kind = unsafe { kind_of_controller(controller) };
                    self.retained.push((controller, kind));
                    kind
                }
            };
            into.push(Seen {
                key: Key(controller as usize),
                kind,
                // SAFETY: `profile` is the live `GCExtendedGamepad` just read.
                reading: unsafe { read_profile(profile) },
            });
        }
        self.retained.retain(|&(held, _)| {
            let listed = into.iter().any(|seen| seen.key == Key(held as usize));
            if !listed {
                // SAFETY: `held` was retained once when first listed, and is
                // released once, here, as it leaves the list.
                unsafe { ffi::msg_void(held, sel(c"release")) };
            }
            listed
        });
    }
}

impl Drop for Framework {
    fn drop(&mut self) {
        for &(held, _) in &self.retained {
            // SAFETY: each was retained once when first listed and has not
            // been released since.
            unsafe { ffi::msg_void(held, sel(c"release")) };
        }
    }
}

/// GameController.framework, with every listed controller's connection state.
///
/// Not `Send`: it holds controller objects. Poll it from the thread that turns
/// the main run loop — see the module docs for why that loop matters.
#[derive(Debug)]
pub struct GameController {
    framework: Framework,
    poller: Poller,
}

impl GameController {
    /// Finds the `GCController` class. Nothing is read until the first poll.
    ///
    /// # Errors
    /// [`GameControllerError::Unavailable`] if the runtime has no such class.
    pub fn new() -> Result<Self, GameControllerError> {
        let class = ffi::class(c"GCController").ok_or(GameControllerError::Unavailable)?;
        Ok(Self {
            framework: Framework {
                class,
                retained: Vec::new(),
            },
            poller: Poller::default(),
        })
    }

    /// Reads `[GCController controllers]` and calls `emit` with what changed:
    /// connections, disconnections, and snapshots that differ from the last
    /// one reported. Every listed controller is read on every call.
    pub fn poll(&mut self, mut emit: impl FnMut(GamepadEvent)) {
        self.poller.poll(&mut self.framework, &mut emit);
    }
}

/// Smoke tests against the real framework. They run only on a macOS host —
/// CI's macOS job — and need no controller: a test binary turns no run loop,
/// so the list is expected empty there, and what is checked is that the calls
/// are well-formed.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_name::kind_of_name;

    /// **`[GCController controllers]` returns an `NSArray`**, and a poll over
    /// it reports only well-formed events.
    #[test]
    fn the_controller_list_is_an_array() {
        let pads = GameController::new().expect("GameController is linked");
        let pool = Pool::push();
        // SAFETY: as `Framework::read`; `isKindOfClass:` takes a class.
        let array = unsafe { ffi::msg(pads.framework.class, sel(c"controllers")) };
        assert!(!array.is_null(), "+controllers returned nil");
        let ns_array = ffi::class(c"NSArray").expect("Foundation is loaded");
        assert!(
            unsafe { ffi::is_kind_of(array, ns_array) },
            "not an NSArray"
        );
        drop(pool);

        let mut pads = pads;
        let mut connected = Vec::new();
        pads.poll(|event| match event {
            GamepadEvent::Connected { id, .. } => connected.push(id),
            GamepadEvent::State { id, snapshot } => {
                assert!(connected.contains(&id), "a state before its connection");
                assert!(snapshot.is_finite());
            }
            GamepadEvent::Disconnected { .. } => panic!("nothing was connected yet"),
        });
    }

    /// **Every getter the tables walk exists on the class it is sent to**,
    /// so a misspelt element name fails here rather than reading as a button
    /// that is never pressed.
    #[test]
    fn every_path_names_real_getters() {
        let class = |name| ffi::class(name).unwrap_or_else(|| panic!("no class {name:?}"));
        let profile = class(c"GCExtendedGamepad");
        let pad = class(c"GCControllerDirectionPad");
        let button = class(c"GCControllerButtonInput");
        let axis = class(c"GCControllerAxisInput");
        // SAFETY: `instancesRespondToSelector:` is a class method of
        // `NSObject`, and every receiver here is a class.
        let has = |class, getter: &core::ffi::CStr| unsafe {
            ffi::instances_respond_to(class, sel(getter))
        };
        let paths = BUTTONS
            .iter()
            .map(|&(path, _)| (path, button, c"isPressed"))
            .chain(AXES.iter().map(|&(path, _)| {
                let last = if path.len() == 2 { axis } else { button };
                (path, last, c"value")
            }));
        for (path, last, reader) in paths {
            assert!(has(profile, path[0]), "GCExtendedGamepad.{:?}", path[0]);
            if let Some(&inner) = path.get(1) {
                assert!(has(pad, inner), "GCControllerDirectionPad.{inner:?}");
            }
            assert!(has(last, reader), "{path:?} then {reader:?}");
        }
    }

    /// **The framework's own category names classify as `kind_of` expects** —
    /// the one place the names `product_name` matches are read from the
    /// framework rather than written down.
    #[test]
    fn the_framework_categories_name_their_families() {
        let _pool = Pool::push();
        // SAFETY: each is an `NSString *` constant the framework exports.
        let categories = unsafe {
            [
                (ffi::GCProductCategoryDualShock4, PadKind::PlayStation),
                (ffi::GCProductCategoryDualSense, PadKind::PlayStation),
                (ffi::GCProductCategoryXboxOne, PadKind::Xbox),
                (ffi::GCProductCategorySwitchPro, PadKind::Switch),
                (ffi::GCProductCategorySwitchJoyConPair, PadKind::Switch),
                (ffi::GCProductCategoryMFi, PadKind::Generic),
            ]
        };
        for (constant, kind) in categories {
            // SAFETY: a live `NSString`, with the pool above in scope.
            let name = unsafe { ffi::string(constant) }.expect("a category name");
            assert_eq!(kind_of_name(&name), kind, "{name:?}");
            assert_eq!(kind_of(Some(&name), None), kind, "{name:?}");
        }
    }
}
