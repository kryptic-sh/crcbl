//! [`Shared`]: state more than one system holds, under a name the schedule
//! knows it by.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// A named handle to state that more than one system — or a system and the
/// code driving the world — holds: the seam a system's
/// [`Access`](crate::Access) declaration describes.
///
/// Cloning clones the handle, not the value. Register it with the schedule
/// ([`World::share`](crate::World::share)) before registering a system whose
/// declaration names it; the name is the resource's identity there.
///
/// In debug builds [`read`](Self::read) and [`write`](Self::write) check the
/// declaration of the system whose tick is running, and panic on an access it
/// does not declare — see the [`access`](crate::Access) docs for why that check
/// is the part a conflict graph cannot do without. Outside a tick they check
/// nothing.
///
/// ```rust
/// use crcbl_ecs::*;
///
/// struct Mover { wind: Shared<f32>, x: f32 }
///
/// impl SystemTrait for Mover {
///     fn name(&self) -> &str { "mover" }
///     fn access(&self) -> Access { Access::none().reads("wind") }
///     fn tick(&mut self, dt: f64) { self.x += *self.wind.read() * dt as f32; }
///     fn entity_count(&self) -> usize { 0 }
///     fn sweep(&mut self, _dead: &[Entity]) {}
///     fn debug_draw(&mut self, _ctx: &DebugCtx) {}
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
///
/// let wind = Shared::new("wind", 2.0_f32);
/// let mut world = World::new();
/// world.share(&wind).expect("the first resource of that name");
/// world.register_system(Box::new(Mover { wind: wind.clone(), x: 0.0 }));
/// world.tick_with_dt(0.5);
/// assert_eq!(world.system_mut::<Mover>().map(|mover| mover.x), Some(1.0));
/// ```
pub struct Shared<T> {
    inner: Arc<Inner<T>>,
}

struct Inner<T> {
    name: String,
    value: RwLock<T>,
}

impl<T> Shared<T> {
    /// A resource called `name` holding `value`.
    #[must_use]
    pub fn new(name: impl Into<String>, value: T) -> Self {
        Self {
            inner: Arc::new(Inner {
                name: name.into(),
                value: RwLock::new(value),
            }),
        }
    }

    /// The name a declaration knows this resource by.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Borrows the value to read.
    ///
    /// A poisoned lock hands back the value as the panicking holder left it:
    /// that panic is the one worth reporting, and a second one here would bury
    /// it.
    ///
    /// # Panics
    ///
    /// In debug builds, inside a tick whose system's declaration neither reads
    /// nor writes this resource.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        #[cfg(debug_assertions)]
        crate::access::check(self.name(), false);
        self.inner
            .value
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Borrows the value to write, with a poisoned lock treated as
    /// [`read`](Self::read) treats it.
    ///
    /// The lock is [`RwLock`]'s, so asking for it while this thread still
    /// holds a borrow of the same resource may deadlock or panic.
    ///
    /// # Panics
    ///
    /// In debug builds, inside a tick whose system's declaration does not
    /// write this resource.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        #[cfg(debug_assertions)]
        crate::access::check(self.name(), true);
        self.inner
            .value
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field("name", &self.inner.name)
            .finish_non_exhaustive()
    }
}
