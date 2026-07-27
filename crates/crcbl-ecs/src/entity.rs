use crcbl_core::Handle;

/// Zero-sized marker type for entity handles.
///
/// The type parameter carried by every [`Entity`] handle. It exists purely to
/// give the handle a concrete type so the compiler can distinguish it from
/// other `Handle<…>` types without runtime cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityMarker;

/// An entity is a generational id backed by `crcbl_core::Pool<EntityMarker>`.
///
/// Entities are values: `Copy`, `Eq`, `Hash`, and cheap to store. They are
/// *not* pointers — a handle whose slot has been recycled fails to resolve
/// instead of aliasing the new occupant. See [`crcbl_core::Handle`].
pub type Entity = Handle<EntityMarker>;
