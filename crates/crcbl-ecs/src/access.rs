//! What a system declares its tick touches beyond its own arrays — an
//! [`Access`] — and the [`Conflict`]s the [`Schedule`](crate::Schedule)
//! derives from those declarations.
//!
//! # The seam
//!
//! [`SystemTrait::tick`](crate::SystemTrait::tick) is handed `&mut self` and
//! the timestep and nothing else, so a ticking system cannot reach another
//! system's arrays, the entity pool or the world: the borrow checker gives it
//! none of them. Code that does reach across systems —
//! [`World::system_mut`](crate::World::system_mut), a game module's tick —
//! runs between schedule runs with the whole world borrowed, and cannot
//! overlap a tick. What two ticks *can* both touch is state both systems hold,
//! and the one kind of it the schedule is shown is a [`Shared`] resource
//! registered with it ([`Schedule::share`](crate::Schedule::share)). So that is
//! the whole vocabulary: named shared resources, each read or written.
//!
//! A system that holds some other handle — an `Arc<Mutex<_>>`, a channel —
//! couples through something no declaration describes and no assert sees.
//! [`Shared`] is the route that is checked; anything else is outside it.
//!
//! # Checked in debug builds
//!
//! While the schedule runs a system's tick, that system's declaration is the
//! thread's *running* one, and every [`Shared::read`] and [`Shared::write`]
//! checks against it, panicking with the system's and the resource's names on
//! an access the declaration does not cover. Without that check a conflict
//! graph is built from claims nothing tests. Release builds compile the check
//! out. Outside a tick — set-up, a game module, tooling — nothing is running
//! and nothing is checked: that code holds the world exclusively, so it cannot
//! race a system.

use std::collections::BTreeSet;
use std::fmt;

#[cfg(debug_assertions)]
use std::cell::RefCell;
#[cfg(debug_assertions)]
use std::sync::Arc;

#[cfg(doc)]
use crate::Shared;

/// The [`Shared`] resources a system's tick reads and writes, by name.
///
/// Built from [`Access::none`]:
///
/// ```rust
/// # use crcbl_ecs::Access;
/// let access = Access::none().reads("wind").writes("score");
/// assert_eq!(access.read_names().collect::<Vec<_>>(), ["wind"]);
/// assert_eq!(access.write_names().collect::<Vec<_>>(), ["score"]);
/// ```
///
/// A write covers a read of the same resource, so a name declared both ways is
/// a write. The names are kept sorted, which is what makes the conflicts the
/// schedule derives independent of the order a system declared them in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Access {
    /// Read and never written.
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
}

impl Access {
    /// Touches no shared resource: the answer of a system whose tick works on
    /// its own arrays alone.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            reads: BTreeSet::new(),
            writes: BTreeSet::new(),
        }
    }

    /// This access, also reading the resource called `name`.
    #[must_use]
    pub fn reads(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if !self.writes.contains(&name) {
            self.reads.insert(name);
        }
        self
    }

    /// This access, also writing the resource called `name`.
    #[must_use]
    pub fn writes(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.reads.remove(&name);
        self.writes.insert(name);
        self
    }

    /// The resources read and not written, in name order.
    pub fn read_names(&self) -> impl Iterator<Item = &str> {
        self.reads.iter().map(String::as_str)
    }

    /// The resources written, in name order.
    pub fn write_names(&self) -> impl Iterator<Item = &str> {
        self.writes.iter().map(String::as_str)
    }

    /// Whether this access touches nothing shared.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.reads.is_empty() && self.writes.is_empty()
    }

    /// Every resource read or written, in name order.
    pub(crate) fn touched(&self) -> impl Iterator<Item = &str> {
        self.reads.union(&self.writes).map(String::as_str)
    }

    /// Whether a read of `name` is declared — by a read or by a write.
    fn allows_read(&self, name: &str) -> bool {
        self.reads.contains(name) || self.writes.contains(name)
    }

    /// Whether a write of `name` is declared.
    fn allows_write(&self, name: &str) -> bool {
        self.writes.contains(name)
    }
}

/// How two systems' declarations collide on one resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictKind {
    /// Both write it.
    WriteWrite,
    /// One reads it and the other writes it.
    ReadWrite,
}

/// Two systems whose ticks touch one resource in a way that cannot overlap:
/// `after` must run after `before`, as it was registered.
///
/// Indices are schedule positions — registration order — rather than names,
/// which nothing requires to be unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Conflict {
    /// The system registered first.
    pub before: usize,
    /// The system registered later, which waits for `before`.
    pub after: usize,
    /// The resource both touch.
    pub resource: String,
    /// How they touch it.
    pub kind: ConflictKind,
}

/// Appends to `out` every conflict between the system at `before`, declaring
/// `earlier`, and the later one at `after`, declaring `later`, in resource
/// name order. Read against read is no conflict.
pub(crate) fn conflicts_between(
    before: usize,
    earlier: &Access,
    after: usize,
    later: &Access,
    out: &mut Vec<Conflict>,
) {
    for resource in earlier.touched() {
        if !later.allows_read(resource) {
            continue;
        }
        let kind = match (earlier.allows_write(resource), later.allows_write(resource)) {
            (true, true) => ConflictKind::WriteWrite,
            (true, false) | (false, true) => ConflictKind::ReadWrite,
            (false, false) => continue,
        };
        out.push(Conflict {
            before,
            after,
            resource: resource.to_owned(),
            kind,
        });
    }
}

/// A declaration the schedule refused, or a resource it would not register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    /// `system`'s [`SystemTrait::access`](crate::SystemTrait::access) names a
    /// resource no [`Schedule::share`](crate::Schedule::share) registered.
    UnknownResource {
        /// The declaring system's name.
        system: String,
        /// The name it declared.
        resource: String,
    },
    /// A resource of this name is already registered with the schedule.
    DuplicateResource {
        /// The name registered twice.
        resource: String,
    },
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownResource { system, resource } => write!(
                f,
                "system `{system}` declares shared resource `{resource}`, which is not \
                 registered with its schedule"
            ),
            Self::DuplicateResource { resource } => write!(
                f,
                "a shared resource named `{resource}` is already registered with this schedule"
            ),
        }
    }
}

impl std::error::Error for AccessError {}

/// A registered system's name and declaration, as the schedule recorded them
/// at registration.
#[derive(Debug)]
pub(crate) struct Declared {
    /// The name the debug-build check reports; only that check reads it, so a
    /// build without it does not keep it.
    #[cfg(debug_assertions)]
    pub(crate) system: String,
    pub(crate) access: Access,
}

#[cfg(debug_assertions)]
thread_local! {
    /// The declaration of the system whose tick this thread is running.
    static RUNNING: RefCell<Option<Arc<Declared>>> = const { RefCell::new(None) };
}

/// Makes a declaration the thread's running one until dropped, then puts back
/// whichever was running before — on unwinding too, so a tick that panics
/// leaves no stale declaration to judge the code that catches it.
#[cfg(debug_assertions)]
pub(crate) struct Running {
    previous: Option<Arc<Declared>>,
}

#[cfg(debug_assertions)]
impl Running {
    pub(crate) fn enter(declared: &Arc<Declared>) -> Self {
        let previous = RUNNING.with(|running| running.replace(Some(Arc::clone(declared))));
        Self { previous }
    }
}

#[cfg(debug_assertions)]
impl Drop for Running {
    fn drop(&mut self) {
        let previous = self.previous.take();
        RUNNING.with(|running| *running.borrow_mut() = previous);
    }
}

/// Panics if a system's tick is running on this thread and its declaration
/// does not cover this access to `resource`.
///
/// # Panics
///
/// On exactly that, naming the system and the resource.
#[cfg(debug_assertions)]
pub(crate) fn check(resource: &str, write: bool) {
    RUNNING.with_borrow(|running| {
        let Some(declared) = running else {
            return;
        };
        let (allowed, verb) = if write {
            (declared.access.allows_write(resource), "wrote")
        } else {
            (declared.access.allows_read(resource), "read")
        };
        assert!(
            allowed,
            "system `{}` {verb} shared resource `{resource}` without declaring that access",
            declared.system
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflicts(earlier: &Access, later: &Access) -> Vec<(String, ConflictKind)> {
        let mut out = Vec::new();
        conflicts_between(0, earlier, 1, later, &mut out);
        out.into_iter()
            .map(|conflict| (conflict.resource, conflict.kind))
            .collect()
    }

    #[test]
    fn two_writers_of_one_resource_conflict() {
        let writer = Access::none().writes("score");
        assert_eq!(
            conflicts(&writer, &writer),
            [("score".to_owned(), ConflictKind::WriteWrite)]
        );
    }

    /// Both directions: the earlier system reading and the later writing, and
    /// the other way round, are the same conflict.
    #[test]
    fn a_reader_and_a_writer_of_one_resource_conflict_either_way_round() {
        let reader = Access::none().reads("wind");
        let writer = Access::none().writes("wind");
        let read_write = [("wind".to_owned(), ConflictKind::ReadWrite)];
        assert_eq!(conflicts(&reader, &writer), read_write);
        assert_eq!(conflicts(&writer, &reader), read_write);
    }

    #[test]
    fn two_readers_of_one_resource_do_not_conflict() {
        let reader = Access::none().reads("wind");
        assert_eq!(conflicts(&reader, &reader), []);
    }

    #[test]
    fn systems_touching_different_resources_do_not_conflict() {
        let a = Access::none().writes("score");
        let b = Access::none().writes("wind");
        assert_eq!(conflicts(&a, &b), []);
    }

    /// A write covers a read of the same name, whichever was declared first,
    /// so the name is listed once, as a write.
    #[test]
    fn a_name_declared_both_ways_is_a_write() {
        for access in [
            Access::none().reads("wind").writes("wind"),
            Access::none().writes("wind").reads("wind"),
        ] {
            assert_eq!(access.read_names().count(), 0);
            assert_eq!(access.write_names().collect::<Vec<_>>(), ["wind"]);
            assert!(access.allows_read("wind"));
        }
    }

    #[test]
    fn the_refusals_name_what_was_refused() {
        let unknown = AccessError::UnknownResource {
            system: "mover".to_owned(),
            resource: "wind".to_owned(),
        };
        let message = unknown.to_string();
        assert!(message.contains("`mover`"), "{message}");
        assert!(message.contains("`wind`"), "{message}");

        let duplicate = AccessError::DuplicateResource {
            resource: "wind".to_owned(),
        };
        assert!(duplicate.to_string().contains("`wind`"));
    }
}
