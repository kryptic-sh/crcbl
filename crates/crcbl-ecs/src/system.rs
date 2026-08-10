use std::collections::HashMap;
use std::fmt;
use std::hash::Hasher;

use crate::component_hash::ComponentHash;
use crate::entity::Entity;

/// Stub struct for debug-draw context.
///
/// Empty for now; this is the hook point for future visualization tooling.
/// Systems with a debug-draw callback receive a `&DebugCtx` each tick so they
/// can push lines, labels, or shapes to a shared draw list without depending
/// on any renderer crate.
#[derive(Debug)]
pub struct DebugCtx;

/// Callback type for per-system debug drawing.
///
/// Set via [`System::set_debug_draw`]; called once per tick with a
/// [`&DebugCtx`](DebugCtx).
pub type DebugDrawFn = Box<dyn FnMut(&DebugCtx)>;

// ---------------------------------------------------------------------------
// Object-safe trait
// ---------------------------------------------------------------------------

/// Object-safe trait every system in the [`Schedule`](crate::Schedule) must
/// implement.
///
/// The concrete [`System<T>`] implements this automatically; custom systems
/// (e.g. systems that tick multiple arrays at once) implement it directly.
pub trait SystemTrait {
    /// Human-readable name for the inspector / debug overlay.
    fn name(&self) -> &str;

    /// Called once per tick by the schedule.
    ///
    /// `dt` is the schedule's fixed timestep in seconds — the real tick
    /// period, not an assumed one. A system that integrates over time must
    /// use it, or its simulated speed becomes a function of the tick rate.
    /// See [`World::set_tick_dt`](crate::World::set_tick_dt).
    fn tick(&mut self, dt: f64);

    /// Number of entities currently registered with this system.
    fn entity_count(&self) -> usize;

    /// Remove every entity listed in `dead` from this system.
    ///
    /// Called once per tick, after `tick`, with the set of entities that were
    /// despawned during this tick.
    fn sweep(&mut self, dead: &[Entity]);

    /// Called once per tick so the system can emit debug-draw primitives.
    ///
    /// The default implementation is a no-op; override this rather than
    /// setting a callback if the system needs access to its own data.
    fn debug_draw(&mut self, ctx: &DebugCtx);

    /// Hash the system's component state into `hasher`.
    ///
    /// The default implementation is a no-op.  Override this to feed
    /// per-entity component data into a determinism hash (used by
    /// `crcbl-server`'s sim-hash harness).
    fn hash_state(&self, _hasher: &mut dyn Hasher) {}

    /// Whether this system's `SystemTrait::hash_state` contributes
    /// component data to the determinism hash.
    ///
    /// Returns `false` by default — systems that override `hash_state`
    /// must also override this to return `true`, otherwise the
    /// determinism harness will warn that they are not contributing.
    /// The concrete [`System<T>`] returns `true`.
    fn contributes_to_hash(&self) -> bool {
        false
    }

    /// Serialise this system's per-entity component data for replication.
    ///
    /// Appends `(entity_bits: u64, data_len: u32, data: [u8])` triples — all
    /// little-endian — to `out`, one per replicated entity. Returns `true`
    /// if any data was written.
    ///
    /// The default returns `false` (system not replicated); the server then
    /// emits only the entity count for this system. Custom systems that own
    /// their component arrays (e.g. `crcbl-phys`' `PhysicsSystem`) override
    /// this to publish real state.
    fn replicate(&self, _out: &mut Vec<u8>) -> bool {
        false
    }

    /// Downcast support for callers that hold `&mut dyn SystemTrait` (e.g.
    /// through [`Schedule::iter_mut`](crate::Schedule::iter_mut)) and need the
    /// concrete system type.
    /// Implementations are the one-liner `self`.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl fmt::Debug for dyn SystemTrait {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("dyn SystemTrait")
            .field("name", &self.name())
            .field("entity_count", &self.entity_count())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Concrete System<T>
// ---------------------------------------------------------------------------

/// A system that owns a dense `Vec<T>` of component data plus a sparse
/// entity→index map.
///
/// # Layout
///
/// * `data: Vec<T>` — the values, stored contiguously for cache-friendly
///   iteration.
/// * `index_to_entity: Vec<Entity>` — parallel to `data`; `index_to_entity[i]`
///   is the entity that owns `data[i]`.
/// * `entity_to_index: HashMap<Entity, usize>` — inverse of the above; answers
///   "where is this entity's data?" in O(1).
///
/// Removal uses swap-remove on both vectors and updates the moved entity's
/// mapping, so removal is O(1) but does not preserve insertion order.
pub struct System<T> {
    name: String,
    data: Vec<T>,
    entity_to_index: HashMap<Entity, usize>,
    index_to_entity: Vec<Entity>,
    debug_draw_fn: Option<DebugDrawFn>,
}

impl<T> System<T> {
    /// Creates an empty system with the given human-readable `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data: Vec::new(),
            entity_to_index: HashMap::new(),
            index_to_entity: Vec::new(),
            debug_draw_fn: None,
        }
    }

    /// The system's name (same as [`SystemTrait::name`]).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    // -- registration -------------------------------------------------------

    /// Registers `entity` with this system, storing `data`.
    ///
    /// If `entity` is already registered, its existing data is overwritten
    /// in-place. Otherwise a new entry is pushed.
    ///
    /// `entity` must be alive. A system owns no entity pool and so cannot
    /// check: attaching a handle that [`World::sweep`](crate::World::sweep)
    /// has already retired stores data that nothing will ever remove, and it
    /// counts towards [`SystemTrait::entity_count`] and
    /// [`SystemTrait::hash_state`] for the rest of the run.
    pub fn attach(&mut self, entity: Entity, data: T) {
        if let Some(&index) = self.entity_to_index.get(&entity) {
            self.data[index] = data;
        } else {
            let index = self.data.len();
            self.data.push(data);
            self.index_to_entity.push(entity);
            self.entity_to_index.insert(entity, index);
        }
    }

    /// Removes `entity` from this system, returning its data if it was
    /// registered.
    ///
    /// Uses swap-remove: O(1) but may reorder the remaining entries.
    pub fn detach(&mut self, entity: Entity) -> Option<T> {
        let index = self.entity_to_index.remove(&entity)?;
        let last = self.data.len() - 1;
        if index != last {
            self.data.swap(index, last);
            self.index_to_entity.swap(index, last);
            let moved_entity = self.index_to_entity[index];
            self.entity_to_index.insert(moved_entity, index);
        }
        self.index_to_entity.pop();
        Some(self.data.pop().unwrap())
    }

    // -- lookup ---------------------------------------------------------------

    /// Borrows the data for `entity`, or `None` if it isn't registered.
    #[must_use]
    pub fn get(&self, entity: Entity) -> Option<&T> {
        let &index = self.entity_to_index.get(&entity)?;
        Some(&self.data[index])
    }

    /// Mutably borrows the data for `entity`.
    #[must_use]
    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let &index = self.entity_to_index.get(&entity)?;
        Some(&mut self.data[index])
    }

    // -- iteration -----------------------------------------------------------

    /// Iterates all component data in storage order (contiguous, cache-friendly).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// Mutably iterates all component data in storage order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.data.iter_mut()
    }

    /// Iterates `(entity, &data)` pairs.
    pub fn iter_entities(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.index_to_entity.iter().copied().zip(self.data.iter())
    }

    // -- debug draw ----------------------------------------------------------

    /// Sets a callback invoked each tick by [`SystemTrait::debug_draw`].
    ///
    /// Pass `None` to clear a previously-set callback.
    pub fn set_debug_draw(&mut self, f: Option<DebugDrawFn>) {
        self.debug_draw_fn = f;
    }
}

impl<T: ComponentHash + 'static> SystemTrait for System<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn tick(&mut self, _dt: f64) {
        // Default: no-op. Users that need per-tick logic iterate the system
        // directly or implement `SystemTrait` on their own type.
    }

    fn entity_count(&self) -> usize {
        self.data.len()
    }

    fn sweep(&mut self, dead: &[Entity]) {
        for &entity in dead {
            self.detach(entity);
        }
    }

    fn debug_draw(&mut self, ctx: &DebugCtx) {
        if let Some(ref mut f) = self.debug_draw_fn {
            f(ctx);
        }
    }

    /// Hashes `(entity, component)` pairs in ascending entity order.
    ///
    /// Storage order is not canonical — `detach` swap-removes, so two systems
    /// holding the same logical state reach different `data` orders from
    /// different attach/detach histories. Sorting by entity makes the hash a
    /// function of the state alone.
    fn hash_state(&self, hasher: &mut dyn Hasher) {
        let mut order: Vec<usize> = (0..self.data.len()).collect();
        order.sort_unstable_by_key(|&i| self.index_to_entity[i].to_bits());
        for i in order {
            hasher.write(&self.index_to_entity[i].to_bits().to_le_bytes());
            self.data[i].hash_component(hasher);
        }
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl<T> fmt::Debug for System<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("System")
            .field("name", &self.name)
            .field("entity_count", &self.data.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn e(idx: u32) -> Entity {
        crate::test_entity(idx, 1)
    }

    #[test]
    fn a_component_attached_to_an_entity_is_what_get_answers_with() {
        let mut sys = System::<i32>::new("test");
        let ent = e(1);
        sys.attach(ent, 42);
        assert_eq!(sys.get(ent), Some(&42));
    }

    #[test]
    fn attaching_twice_to_one_entity_replaces_the_value_without_adding_a_row() {
        let mut sys = System::<i32>::new("test");
        let ent = e(1);
        sys.attach(ent, 10);
        sys.attach(ent, 99);
        assert_eq!(sys.entity_count(), 1);
        assert_eq!(sys.get(ent), Some(&99));
    }

    #[test]
    fn detach_removes_and_reorders() {
        let mut sys = System::<i32>::new("test");
        let a = e(1);
        let b = e(2);
        let c = e(3);
        sys.attach(a, 1);
        sys.attach(b, 2);
        sys.attach(c, 3);

        assert_eq!(sys.detach(b), Some(2));
        assert_eq!(sys.entity_count(), 2);
        assert_eq!(sys.get(b), None);
        // a and c must still be present
        assert!(sys.get(a).is_some());
        assert!(sys.get(c).is_some());
    }

    #[test]
    fn detach_unknown_returns_none() {
        let mut sys = System::<i32>::new("test");
        let ent = e(42);
        assert_eq!(sys.detach(ent), None);
    }

    #[test]
    fn double_detach_is_safe() {
        let mut sys = System::<i32>::new("test");
        let ent = e(1);
        sys.attach(ent, 7);
        assert_eq!(sys.detach(ent), Some(7));
        assert_eq!(sys.detach(ent), None);
        assert_eq!(sys.entity_count(), 0);
    }

    #[test]
    fn iter_yields_all_values() {
        let mut sys = System::<i32>::new("test");
        sys.attach(e(1), 10);
        sys.attach(e(2), 20);
        let mut sum = 0;
        for v in sys.iter() {
            sum += *v;
        }
        assert_eq!(sum, 30);
    }

    #[test]
    fn iter_mut_allows_mutation() {
        let mut sys = System::<i32>::new("test");
        let a = e(1);
        let b = e(2);
        sys.attach(a, 1);
        sys.attach(b, 2);
        for v in sys.iter_mut() {
            *v *= 10;
        }
        assert_eq!(sys.get(a), Some(&10));
        assert_eq!(sys.get(b), Some(&20));
    }

    #[test]
    fn iter_entities_yields_pairs() {
        let mut sys = System::<&str>::new("test");
        let a = e(10);
        let b = e(20);
        sys.attach(a, "alpha");
        sys.attach(b, "beta");

        let mut pairs: Vec<_> = sys.iter_entities().map(|(ent, d)| (ent, *d)).collect();
        pairs.sort_by_key(|(ent, _)| ent.to_bits());
        assert_eq!(pairs, vec![(a, "alpha"), (b, "beta")]);
    }

    #[test]
    fn sweep_removes_dead_entities() {
        let mut sys = System::<i32>::new("test");
        let a = e(1);
        let b = e(2);
        let c = e(3);
        sys.attach(a, 1);
        sys.attach(b, 2);
        sys.attach(c, 3);

        sys.sweep(&[b, c]);
        assert_eq!(sys.entity_count(), 1);
        assert!(sys.get(a).is_some());
        assert!(sys.get(b).is_none());
        assert!(sys.get(c).is_none());
    }

    #[test]
    fn sweep_empty_dead_list_is_noop() {
        let mut sys = System::<i32>::new("test");
        sys.attach(e(1), 42);
        sys.sweep(&[]);
        assert_eq!(sys.entity_count(), 1);
    }

    #[test]
    fn sweep_unknown_entity_is_noop() {
        let mut sys = System::<i32>::new("test");
        sys.attach(e(1), 42);
        let ghost = e(999);
        sys.sweep(&[ghost]);
        assert_eq!(sys.entity_count(), 1);
    }

    #[test]
    fn debug_draw_callback_is_called() {
        let mut sys = System::<i32>::new("test");
        let called = std::rc::Rc::new(std::cell::RefCell::new(false));
        let called2 = called.clone();
        sys.set_debug_draw(Some(Box::new(move |_ctx| {
            *called2.borrow_mut() = true;
        })));
        sys.debug_draw(&DebugCtx);
        assert!(*called.borrow());
    }

    /// The neighbour above proves the hook is called when one is set; this is
    /// the other half, and it used to be spelled as "must not panic" — which a
    /// `debug_draw` that called a *stale* callback would also satisfy.
    #[test]
    fn debug_draw_without_a_callback_calls_nothing() {
        let mut sys = System::<i32>::new("test");
        let called = std::rc::Rc::new(std::cell::RefCell::new(false));
        let called2 = called.clone();
        sys.set_debug_draw(Some(Box::new(move |_ctx| {
            *called2.borrow_mut() = true;
        })));
        sys.set_debug_draw(None);

        sys.debug_draw(&DebugCtx);
        assert!(
            !*called.borrow(),
            "a callback that has been taken away must not still run"
        );
    }

    #[test]
    fn hash_state_ignores_attach_and_detach_history() {
        use std::collections::hash_map::DefaultHasher;

        fn hash(sys: &System<i32>) -> u64 {
            let mut h = DefaultHasher::new();
            sys.hash_state(&mut h);
            h.finish()
        }

        let (a, b, c) = (e(1), e(2), e(3));

        // Same logical state, reached two ways.  The second system's `data`
        // ends up in swap-remove order, which is not ascending entity order.
        let mut direct = System::<i32>::new("s");
        direct.attach(a, 10);
        direct.attach(b, 20);
        direct.attach(c, 30);

        let mut churned = System::<i32>::new("s");
        churned.attach(c, 30);
        churned.attach(e(4), 40);
        churned.attach(a, 10);
        churned.detach(e(4));
        churned.attach(b, 20);

        assert_eq!(hash(&direct), hash(&churned));

        // The hash still tracks the state itself.
        let mut changed = System::<i32>::new("s");
        changed.attach(a, 10);
        changed.attach(b, 20);
        changed.attach(c, 31);
        assert_ne!(hash(&direct), hash(&changed));
    }

    #[test]
    fn a_system_reports_the_name_it_was_built_with() {
        let sys = System::<i32>::new("physics");
        assert_eq!(sys.name(), "physics");
    }

    #[test]
    fn a_systems_debug_output_names_it_and_reports_its_entity_count() {
        let mut sys = System::<i32>::new("render");
        sys.attach(e(7), 1);
        let s = format!("{sys:?}");
        assert!(s.contains("render"));
        assert!(s.contains("entity_count: 1"));
    }
}
