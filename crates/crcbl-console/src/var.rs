//! The two shapes a console variable takes: one that **is** its own storage,
//! and one whose storage is somewhere else.
//!
//! [`ConVar`] is Source's model — a typed atomic cell, so the code that owns a
//! knob reads it with `r_ao_view.get_bool()` and never polls the console.
//! [`Binding`] is the other half: a variable the console can print and set whose
//! value lives in a settings stack, a renderer or a game, reached through the
//! host state a [`Context`](crate::Context) carries. [`Var`] is the pair, and is
//! what the registry actually stores.

use std::any::Any;
use std::fmt;
use std::ops::BitOr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicUsize, Ordering};

use crate::value::{Fault, Kind, Value};

/// How the engine treats a variable, as a set of bits.
///
/// Hand-rolled rather than `bitflags`, because this crate has no dependencies
/// at all — and the set is small enough that the derive would buy nothing but
/// the arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Flags(u32);

impl Flags {
    /// No flags: an ordinary debug variable, not persisted, freely settable.
    pub const NONE: Self = Self(0);
    /// Persisted through the settings stack, and written when `save` is run.
    pub const ARCHIVE: Self = Self(1 << 0);
    /// Prints, and refuses a set — a device fact, or a settings key nothing
    /// reads yet.
    pub const READ_ONLY: Self = Self(1 << 1);
    /// Reserved: a value the simulation reads.
    ///
    /// Nothing sets one yet. Plan decision 9 is the rule that lands with the
    /// first one: a `SIM` variable travels as a transport `Command`, is applied
    /// on a tick boundary and is recorded by the replay stream, because a
    /// variable that changes what the simulation computes would otherwise break
    /// same-binary determinism.
    pub const SIM: Self = Self(1 << 2);

    /// Every flag with a name, in the order [`Display`](fmt::Display) prints
    /// them.
    const NAMED: [(Self, &'static str); 3] = [
        (Self::ARCHIVE, "ARCHIVE"),
        (Self::READ_ONLY, "READ_ONLY"),
        (Self::SIM, "SIM"),
    ];

    /// Both sets' bits.
    ///
    /// A `const fn` rather than only [`BitOr`], because a `static` initializer
    /// cannot call a trait method: `convar!`'s `#[flags(A | B)]` expands to a
    /// chain of these.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no flag is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits, for a caller that stores them.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl BitOr for Flags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// Prints the set names separated by `, `, and nothing at all when empty.
impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (flag, name) in Flags::NAMED {
            if !self.contains(flag) {
                continue;
            }
            if !first {
                f.write_str(", ")?;
            }
            f.write_str(name)?;
            first = false;
        }
        Ok(())
    }
}

/// A console variable's storage: one typed atomic per [`Kind`].
///
/// Typed rather than a `Mutex<Value>` so the owning code reads its own knob at
/// the cost of one relaxed atomic load. [`Kind::Text`] has no arm, which is what
/// makes a text `ConVar` unconstructible — see [`ConVar`].
#[derive(Debug)]
enum Cell {
    Bool(AtomicBool),
    Int(AtomicI64),
    /// An `f32`'s bits, because there is no `AtomicF32`.
    Float(AtomicU32),
    /// An index into the [`Kind::Enum`] set beside it.
    Enum(AtomicUsize),
}

/// A variable that **is** its own storage.
///
/// Declared with [`convar!`](crate::convar) beside the code it controls, listed
/// once in that crate's [`Table`](crate::Table), and read directly by that code:
/// `r_ao_view.get_bool()` is a relaxed atomic load, not a registry lookup.
///
/// **There is no text `ConVar`.** Every constructor takes a kind that fits an
/// atomic, so no path builds one with [`Kind::Text`]; a text variable is a
/// [`Binding`] instead. Plan decision 1.
///
/// The cells are read and written [`Relaxed`](Ordering::Relaxed): a console
/// variable orders nothing but itself, and a reader that saw the old value one
/// frame longer is what setting a knob mid-frame means anyway.
#[derive(Debug)]
pub struct ConVar {
    name: &'static str,
    help: &'static str,
    kind: Kind,
    flags: Flags,
    default: Value,
    cell: Cell,
}

impl ConVar {
    /// A `bool` variable.
    #[must_use]
    pub const fn new_bool(
        name: &'static str,
        help: &'static str,
        flags: Flags,
        default: bool,
    ) -> Self {
        Self {
            name,
            help,
            kind: Kind::Bool,
            flags,
            default: Value::Bool(default),
            cell: Cell::Bool(AtomicBool::new(default)),
        }
    }

    /// An `i64` variable over an inclusive range.
    ///
    /// # Panics
    ///
    /// At compile time, when `default` is outside `min..=max` — the initializer
    /// is a constant, so the panic is a build error rather than a runtime one.
    #[must_use]
    pub const fn new_int(
        name: &'static str,
        help: &'static str,
        flags: Flags,
        min: i64,
        max: i64,
        default: i64,
    ) -> Self {
        assert!(
            min <= default && default <= max,
            "a console variable's default is outside its own range"
        );
        Self {
            name,
            help,
            kind: Kind::Int { min, max },
            flags,
            default: Value::Int(default),
            cell: Cell::Int(AtomicI64::new(default)),
        }
    }

    /// An `f32` variable over an inclusive range.
    ///
    /// # Panics
    ///
    /// At compile time, when `default` is outside `min..=max`.
    #[must_use]
    pub const fn new_float(
        name: &'static str,
        help: &'static str,
        flags: Flags,
        min: f32,
        max: f32,
        default: f32,
    ) -> Self {
        assert!(
            min <= default && default <= max,
            "a console variable's default is outside its own range"
        );
        Self {
            name,
            help,
            kind: Kind::Float { min, max },
            flags,
            default: Value::Float(default),
            cell: Cell::Float(AtomicU32::new(default.to_bits())),
        }
    }

    /// A variable holding one name from a fixed set.
    ///
    /// # Panics
    ///
    /// At compile time, when `default` is not one of `values` — which is the
    /// whole reason the default is given as a name rather than as an index.
    #[must_use]
    pub const fn new_enum(
        name: &'static str,
        help: &'static str,
        flags: Flags,
        values: &'static [&'static str],
        default: &'static str,
    ) -> Self {
        let index = index_of(values, default);
        Self {
            name,
            help,
            kind: Kind::Enum(values),
            flags,
            default: Value::Enum(values[index]),
            cell: Cell::Enum(AtomicUsize::new(index)),
        }
    }

    /// The name a person types.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The one-line help, from the declaration's doc comment.
    #[must_use]
    pub const fn help(&self) -> &'static str {
        self.help
    }

    /// The domain.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// How the engine treats it.
    #[must_use]
    pub const fn flags(&self) -> Flags {
        self.flags
    }

    /// What it was declared holding.
    #[must_use]
    pub const fn default(&self) -> &Value {
        &self.default
    }

    /// The current value, boxed into a [`Value`].
    ///
    /// The printing path. Code that owns the knob uses the typed getter beside
    /// this one and never allocates a `Value` at all.
    #[must_use]
    pub fn get(&self) -> Value {
        match (&self.cell, self.kind) {
            (Cell::Bool(cell), _) => Value::Bool(cell.load(Ordering::Relaxed)),
            (Cell::Int(cell), _) => Value::Int(cell.load(Ordering::Relaxed)),
            (Cell::Float(cell), _) => Value::Float(f32::from_bits(cell.load(Ordering::Relaxed))),
            (Cell::Enum(cell), Kind::Enum(values)) => {
                Value::Enum(values[cell.load(Ordering::Relaxed)])
            }
            // Unreachable by construction: `new_enum` is the only constructor
            // that builds an enum cell, and it builds `Kind::Enum` beside it.
            (Cell::Enum(_), _) => unreachable!("an enum cell without an enum kind"),
        }
    }

    /// The value as a `bool`.
    ///
    /// # Panics
    ///
    /// When the variable is not a [`Kind::Bool`], naming it. A getter of the
    /// wrong type is a programming error in the code that declared the variable,
    /// not something a person typed, so it aborts rather than returning a
    /// `Result` every call site would `unwrap`.
    #[must_use]
    pub fn get_bool(&self) -> bool {
        match &self.cell {
            Cell::Bool(cell) => cell.load(Ordering::Relaxed),
            _ => panic!("console variable `{}` is not a bool", self.name),
        }
    }

    /// The value as an `i64`.
    ///
    /// # Panics
    ///
    /// When the variable is not a [`Kind::Int`], on [`get_bool`](Self::get_bool)'s
    /// terms.
    #[must_use]
    pub fn get_i64(&self) -> i64 {
        match &self.cell {
            Cell::Int(cell) => cell.load(Ordering::Relaxed),
            _ => panic!("console variable `{}` is not an int", self.name),
        }
    }

    /// The value as an `f32`.
    ///
    /// # Panics
    ///
    /// When the variable is not a [`Kind::Float`], on [`get_bool`](Self::get_bool)'s
    /// terms.
    #[must_use]
    pub fn get_f32(&self) -> f32 {
        match &self.cell {
            Cell::Float(cell) => f32::from_bits(cell.load(Ordering::Relaxed)),
            _ => panic!("console variable `{}` is not a float", self.name),
        }
    }

    /// The value as the name it currently holds.
    ///
    /// # Panics
    ///
    /// When the variable is not a [`Kind::Enum`], on [`get_bool`](Self::get_bool)'s
    /// terms.
    #[must_use]
    pub fn get_enum(&self) -> &'static str {
        match (&self.cell, self.kind) {
            (Cell::Enum(cell), Kind::Enum(values)) => values[cell.load(Ordering::Relaxed)],
            _ => panic!("console variable `{}` is not an enum", self.name),
        }
    }

    /// Set it, refusing a kind mismatch, a value outside the range, and a
    /// [`Flags::READ_ONLY`] variable.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming the variable and what was wrong with the value.
    pub fn set(&self, value: &Value) -> Result<(), Fault> {
        if self.flags.contains(Flags::READ_ONLY) {
            return Err(read_only(self.name));
        }
        self.kind.check(self.name, value)?;
        match (&self.cell, value) {
            (Cell::Bool(cell), Value::Bool(v)) => cell.store(*v, Ordering::Relaxed),
            (Cell::Int(cell), Value::Int(v)) => cell.store(*v, Ordering::Relaxed),
            (Cell::Float(cell), Value::Float(v)) => cell.store(v.to_bits(), Ordering::Relaxed),
            (Cell::Enum(cell), Value::Enum(v)) => {
                let Kind::Enum(values) = self.kind else {
                    unreachable!("an enum cell without an enum kind")
                };
                let index = values
                    .iter()
                    .position(|candidate| *candidate == *v)
                    .expect("`Kind::check` already refused a name outside the set");
                cell.store(index, Ordering::Relaxed);
            }
            // Unreachable: `Kind::check` above pairs the kind with the value,
            // and every constructor pairs the kind with the cell.
            _ => unreachable!("`Kind::check` admitted a value the cell cannot hold"),
        }
        Ok(())
    }
}

/// A variable whose storage is somewhere else.
///
/// The shape every settings key takes in slice 2: a `get`/`set` pair over the
/// host state a [`Context`](crate::Context) carries, so the console can print
/// and write a value it does not own. The host is `&dyn Any` because this crate
/// depends on nothing and so cannot name what the engine keeps there; the pair
/// of functions is written by the crate that *can*.
#[derive(Clone, Copy)]
pub struct Binding {
    name: &'static str,
    help: &'static str,
    kind: Kind,
    flags: Flags,
    get: fn(&dyn Any) -> Value,
    set: fn(&mut dyn Any, &Value) -> Result<(), Fault>,
}

impl Binding {
    /// A binding over `get`/`set`, which the declaring crate writes.
    #[must_use]
    pub const fn new(
        name: &'static str,
        help: &'static str,
        kind: Kind,
        flags: Flags,
        get: fn(&dyn Any) -> Value,
        set: fn(&mut dyn Any, &Value) -> Result<(), Fault>,
    ) -> Self {
        Self {
            name,
            help,
            kind,
            flags,
            get,
            set,
        }
    }

    /// The name a person types.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The one-line help.
    #[must_use]
    pub const fn help(&self) -> &'static str {
        self.help
    }

    /// The domain.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// How the engine treats it.
    #[must_use]
    pub const fn flags(&self) -> Flags {
        self.flags
    }

    /// Read the value out of `host`.
    #[must_use]
    pub fn get(&self, host: &dyn Any) -> Value {
        (self.get)(host)
    }

    /// Write the value into `host`, refusing what [`ConVar::set`] refuses.
    ///
    /// The kind and range are checked **here** rather than left to the binding's
    /// own `set`, so a binding written by another crate cannot forget to.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming the variable, from the checks or from the binding.
    pub fn set(&self, host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
        if self.flags.contains(Flags::READ_ONLY) {
            return Err(read_only(self.name));
        }
        self.kind.check(self.name, value)?;
        (self.set)(host, value)
    }
}

/// Prints the descriptor without the two function pointers, whose addresses say
/// nothing a reader wants.
impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Binding")
            .field("name", &self.name)
            .field("help", &self.help)
            .field("kind", &self.kind)
            .field("flags", &self.flags)
            .finish_non_exhaustive()
    }
}

/// A variable of either shape, which is what the registry stores.
#[derive(Clone, Copy, Debug)]
pub enum Var {
    /// A [`ConVar`]: its own storage.
    Static(&'static ConVar),
    /// A [`Binding`]: storage elsewhere.
    Bound(&'static Binding),
}

impl Var {
    /// The name a person types.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Static(var) => var.name(),
            Self::Bound(bound) => bound.name(),
        }
    }

    /// The one-line help.
    #[must_use]
    pub const fn help(self) -> &'static str {
        match self {
            Self::Static(var) => var.help(),
            Self::Bound(bound) => bound.help(),
        }
    }

    /// The domain.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::Static(var) => var.kind(),
            Self::Bound(bound) => bound.kind(),
        }
    }

    /// How the engine treats it.
    #[must_use]
    pub const fn flags(self) -> Flags {
        match self {
            Self::Static(var) => var.flags(),
            Self::Bound(bound) => bound.flags(),
        }
    }

    /// What it was declared holding, when it was declared with one.
    ///
    /// A [`Binding`]'s value belongs to whatever owns the storage — a settings
    /// stack has its own defaults — so there is nothing here to report.
    #[must_use]
    pub const fn default(self) -> Option<&'static Value> {
        match self {
            Self::Static(var) => Some(var.default()),
            Self::Bound(_) => None,
        }
    }

    /// The current value. `host` is ignored by a [`ConVar`], which is its own
    /// storage.
    #[must_use]
    pub fn get(self, host: &dyn Any) -> Value {
        match self {
            Self::Static(var) => var.get(),
            Self::Bound(bound) => bound.get(host),
        }
    }

    /// Set it. `host` is ignored by a [`ConVar`].
    ///
    /// # Errors
    ///
    /// A [`Fault`], on [`ConVar::set`]'s terms.
    pub fn set(self, host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
        match self {
            Self::Static(var) => var.set(value),
            Self::Bound(bound) => bound.set(host, value),
        }
    }
}

/// The one spelling of the read-only refusal, so both shapes give the same one.
fn read_only(name: &str) -> Fault {
    Fault::new(format!("`{name}` is read-only"))
}

/// Where `name` sits in `values`, at compile time.
///
/// `str` has no `const` comparison of its own, so this walks the bytes. It is a
/// `const fn` so that [`ConVar::new_enum`] can take the default as the name the
/// declaration writes and still refuse a name outside the set — as a build
/// error, before anything runs.
const fn index_of(values: &'static [&'static str], name: &str) -> usize {
    let mut i = 0;
    while i < values.len() {
        if str_eq(values[i], name) {
            return i;
        }
        i += 1;
    }
    panic!("a console variable's default is not one of its own values");
}

/// Byte equality of two `str`s, in a `const` context.
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const AA_VALUES: &[&str] = &["none", "fxaa", "smaa"];

    static A_BOOL: ConVar = ConVar::new_bool("t_bool", "a bool", Flags::NONE, false);
    static AN_INT: ConVar = ConVar::new_int("t_int", "an int", Flags::NONE, 1, 16, 4);
    static A_FLOAT: ConVar = ConVar::new_float("t_float", "a float", Flags::NONE, 0.0, 1.0, 0.5);
    static AN_ENUM: ConVar = ConVar::new_enum("t_enum", "an enum", Flags::NONE, AA_VALUES, "fxaa");
    static LOCKED: ConVar = ConVar::new_bool("t_locked", "read only", Flags::READ_ONLY, true);

    #[test]
    fn a_bool_cell_round_trips_and_a_second_reference_sees_the_new_value() {
        A_BOOL.set(&Value::Bool(true)).expect("a bool takes a bool");
        // The point of the static: the reader does not go through the handle
        // that wrote it.
        let elsewhere: &ConVar = &A_BOOL;
        assert!(elsewhere.get_bool());
        assert_eq!(elsewhere.get(), Value::Bool(true));
        A_BOOL
            .set(&Value::Bool(false))
            .expect("a bool takes a bool");
        assert!(!elsewhere.get_bool());
    }

    #[test]
    fn the_int_and_float_cells_round_trip_through_their_typed_getters() {
        AN_INT.set(&Value::Int(9)).expect("inside the range");
        assert_eq!(AN_INT.get_i64(), 9);
        assert_eq!(AN_INT.get(), Value::Int(9));
        A_FLOAT.set(&Value::Float(0.25)).expect("inside the range");
        assert_eq!(A_FLOAT.get_f32(), 0.25);
        assert_eq!(A_FLOAT.get(), Value::Float(0.25));
    }

    #[test]
    fn an_enum_cell_holds_an_index_and_reads_back_the_name() {
        AN_ENUM.set(&Value::Enum("smaa")).expect("in the set");
        assert_eq!(AN_ENUM.get_enum(), "smaa");
        assert_eq!(AN_ENUM.get(), Value::Enum("smaa"));
        assert_eq!(*AN_ENUM.default(), Value::Enum("fxaa"));
    }

    #[test]
    fn a_set_of_the_wrong_kind_is_refused_and_leaves_the_value_alone() {
        AN_INT.set(&Value::Int(4)).expect("inside the range");
        assert_eq!(
            AN_INT
                .set(&Value::Bool(true))
                .expect_err("an int is not a bool")
                .message(),
            "`t_int` is an int, not a bool"
        );
        assert_eq!(AN_INT.get_i64(), 4);
    }

    #[test]
    fn a_set_outside_the_range_is_refused_and_leaves_the_value_alone() {
        AN_INT.set(&Value::Int(4)).expect("inside the range");
        assert_eq!(
            AN_INT
                .set(&Value::Int(99))
                .expect_err("above the range")
                .message(),
            "`t_int`: 99 is outside 1..=16"
        );
        assert_eq!(AN_INT.get_i64(), 4);
    }

    #[test]
    fn a_read_only_variable_refuses_every_set() {
        assert_eq!(
            LOCKED
                .set(&Value::Bool(false))
                .expect_err("read only")
                .message(),
            "`t_locked` is read-only"
        );
        assert!(LOCKED.get_bool());
    }

    #[test]
    #[should_panic(expected = "console variable `t_int` is not a bool")]
    fn a_typed_getter_of_the_wrong_type_panics_naming_the_variable() {
        let _ = AN_INT.get_bool();
    }

    #[test]
    #[should_panic(expected = "console variable `t_bool` is not a float")]
    fn every_typed_getter_names_the_variable_it_refused() {
        let _ = A_BOOL.get_f32();
    }

    #[test]
    fn the_flag_set_is_bits_and_prints_the_names_it_holds() {
        let both = Flags::ARCHIVE | Flags::READ_ONLY;
        assert!(both.contains(Flags::ARCHIVE));
        assert!(both.contains(Flags::READ_ONLY));
        assert!(!both.contains(Flags::SIM));
        assert!(!both.is_empty());
        assert_eq!(both.to_string(), "ARCHIVE, READ_ONLY");
        assert_eq!(Flags::NONE.to_string(), "");
        assert!(Flags::NONE.is_empty());
        assert_eq!(both.bits(), Flags::ARCHIVE.union(Flags::READ_ONLY).bits());
        assert_eq!(Flags::default(), Flags::NONE);
    }

    // -- bindings ------------------------------------------------------------

    /// A stand-in for the host state slice 2 will put behind a binding.
    #[derive(Debug, Default)]
    struct FakeHost {
        gain: f32,
        label: String,
    }

    fn gain_of(host: &dyn Any) -> Value {
        Value::Float(host.downcast_ref::<FakeHost>().expect("a FakeHost").gain)
    }

    fn set_gain(host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
        let Value::Float(v) = value else {
            return Err(Fault::new("`gain` is a float"));
        };
        host.downcast_mut::<FakeHost>().expect("a FakeHost").gain = *v;
        Ok(())
    }

    fn label_of(host: &dyn Any) -> Value {
        Value::Text(
            host.downcast_ref::<FakeHost>()
                .expect("a FakeHost")
                .label
                .clone(),
        )
    }

    fn set_label(host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
        let Value::Text(v) = value else {
            return Err(Fault::new("`label` is text"));
        };
        host.downcast_mut::<FakeHost>()
            .expect("a FakeHost")
            .label
            .clone_from(v);
        Ok(())
    }

    static GAIN: Binding = Binding::new(
        "gain",
        "the master gain",
        Kind::Float { min: 0.0, max: 1.0 },
        Flags::ARCHIVE,
        gain_of,
        set_gain,
    );
    static LABEL: Binding = Binding::new(
        "label",
        "a name with spaces in it",
        Kind::Text,
        Flags::ARCHIVE,
        label_of,
        set_label,
    );
    static NAMED_ONLY: Binding = Binding::new(
        "fov",
        "nothing reads this yet",
        Kind::Float {
            min: 1.0,
            max: 179.0,
        },
        Flags::READ_ONLY,
        gain_of,
        set_gain,
    );

    #[test]
    fn a_binding_reads_and_writes_the_host_it_is_given() {
        let mut host = FakeHost::default();
        assert_eq!(GAIN.get(&host), Value::Float(0.0));
        GAIN.set(&mut host, &Value::Float(0.75))
            .expect("inside the range");
        assert_eq!(GAIN.get(&host), Value::Float(0.75));
        assert_eq!(host.gain, 0.75);
    }

    #[test]
    fn a_binding_is_the_only_shape_a_text_variable_takes() {
        let mut host = FakeHost::default();
        LABEL
            .set(&mut host, &Value::Text("two words".to_owned()))
            .expect("text takes text");
        assert_eq!(LABEL.get(&host), Value::Text("two words".to_owned()));
        assert_eq!(LABEL.kind(), Kind::Text);
    }

    #[test]
    fn a_binding_refuses_the_wrong_kind_before_it_reaches_the_host() {
        let mut host = FakeHost::default();
        assert_eq!(
            GAIN.set(&mut host, &Value::Bool(true))
                .expect_err("a float is not a bool")
                .message(),
            "`gain` is a float, not a bool"
        );
        assert_eq!(host.gain, 0.0);
    }

    #[test]
    fn a_read_only_binding_refuses_a_set_the_way_a_read_only_convar_does() {
        let mut host = FakeHost::default();
        assert_eq!(
            NAMED_ONLY
                .set(&mut host, &Value::Float(90.0))
                .expect_err("read only")
                .message(),
            "`fov` is read-only"
        );
        assert_eq!(host.gain, 0.0);
    }

    #[test]
    fn a_var_answers_for_either_shape() {
        let mut host = FakeHost::default();
        let bound = Var::Bound(&GAIN);
        let stat = Var::Static(&A_FLOAT);

        assert_eq!(bound.name(), "gain");
        assert_eq!(bound.help(), "the master gain");
        assert_eq!(bound.flags(), Flags::ARCHIVE);
        assert_eq!(bound.kind(), Kind::Float { min: 0.0, max: 1.0 });
        assert!(bound.default().is_none());

        assert_eq!(stat.name(), "t_float");
        assert_eq!(stat.default(), Some(&Value::Float(0.5)));

        bound
            .set(&mut host, &Value::Float(0.25))
            .expect("inside the range");
        assert_eq!(bound.get(&host), Value::Float(0.25));

        stat.set(&mut host, &Value::Float(0.125))
            .expect("inside the range");
        assert_eq!(stat.get(&host), Value::Float(0.125));
    }

    #[test]
    fn a_binding_debug_prints_without_the_function_pointers() {
        let printed = format!("{GAIN:?}");
        assert!(printed.contains("gain"), "{printed}");
        assert!(printed.contains("Float"), "{printed}");
    }

    #[test]
    fn const_string_equality_matches_the_runtime_one() {
        assert!(str_eq("smaa", "smaa"));
        assert!(!str_eq("smaa", "fxaa"));
        assert!(!str_eq("smaa", "smaaa"));
        assert_eq!(index_of(AA_VALUES, "smaa"), 2);
    }
}
