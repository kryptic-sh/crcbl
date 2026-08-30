//! The declarative macros a crate declares its console entries with.
//!
//! Declarative and not a proc-macro, which plan 52 declined for the reason the
//! workspace declined one before: an attribute cannot register anything these
//! cannot, and it would put `syn` and `quote` in the build of every crate that
//! owns a knob.
//!
//! ## The name is the ident
//!
//! Both macros take the console name from the ident, through `stringify!`, so
//! there is one spelling and not two. The expansion allows a lower-case static,
//! because the ident **is** the name a person types — `r_ao_view`, Source's
//! spelling — and a `SCREAMING_CASE` static would either be a second spelling to
//! keep in step or a name nobody would type. Either case compiles, and
//! [`Registry::lookup`](crate::Registry::lookup) matches names without regard to
//! case, so a variable declared `R_AO_VIEW` still answers to `r_ao_view`.

/// Declares a console variable that is its own storage.
///
/// The doc comment is the help. The type is the kind: `bool`, `i64` and `f32`
/// carry an inclusive range written `in MIN..=MAX`, and an enum is written as
/// `&'static str` with the set after `one_of`. Flags are optional and go in a
/// `#[flags(...)]` attribute under the doc comment.
///
/// **There is deliberately no text form.** A `String` in a `static` needs a lock
/// and an allocation the engine's statics do not want, so a text variable exists
/// only as a [`Binding`](crate::Binding) — plan decision 1. A declaration whose
/// type is `String` or a bare `&'static str` matches no rule here and does not
/// compile.
///
/// ```
/// crcbl_console::convar! {
///     /// Draw the ambient-occlusion channel as grey instead of the shaded frame.
///     pub static r_ao_view: bool = false;
/// }
///
/// crcbl_console::convar! {
///     /// How many samples the anisotropic filter takes.
///     #[flags(ARCHIVE)]
///     pub static anisotropic_filtering: i64 in 1..=16 = 1;
/// }
///
/// crcbl_console::convar! {
///     /// The fraction of the frame the renderer draws at.
///     #[flags(ARCHIVE | READ_ONLY)]
///     pub static render_scale: f32 in 0.25..=2.0 = 1.0;
/// }
///
/// crcbl_console::convar! {
///     /// Which edge-antialiasing pass runs.
///     pub static antialiasing: &'static str one_of ["none", "fxaa", "smaa"] = "none";
/// }
///
/// assert_eq!(r_ao_view.name(), "r_ao_view");
/// assert_eq!(
///     r_ao_view.help(),
///     "Draw the ambient-occlusion channel as grey instead of the shaded frame."
/// );
/// assert!(!r_ao_view.get_bool());
/// assert_eq!(anisotropic_filtering.flags(), crcbl_console::Flags::ARCHIVE);
/// assert_eq!(antialiasing.get_enum(), "none");
/// ```
///
/// A text variable does not compile:
///
/// ```compile_fail
/// crcbl_console::convar! {
///     /// The player's name.
///     pub static player_name: &'static str = "anon";
/// }
/// ```
#[macro_export]
macro_rules! convar {
    (
        $(#[doc = $help:literal])+
        $(#[flags($($flag:ident)|+)])?
        $vis:vis static $name:ident: bool = $default:expr;
    ) => {
        #[allow(non_upper_case_globals, reason = "the ident is the console name")]
        $vis static $name: $crate::ConVar = $crate::ConVar::new_bool(
            stringify!($name),
            $crate::__help(concat!($($help),+)),
            $crate::Flags::NONE $($(.union($crate::Flags::$flag))+)?,
            $default,
        );
    };
    (
        $(#[doc = $help:literal])+
        $(#[flags($($flag:ident)|+)])?
        $vis:vis static $name:ident: i64 in $min:literal ..= $max:literal = $default:expr;
    ) => {
        #[allow(non_upper_case_globals, reason = "the ident is the console name")]
        $vis static $name: $crate::ConVar = $crate::ConVar::new_int(
            stringify!($name),
            $crate::__help(concat!($($help),+)),
            $crate::Flags::NONE $($(.union($crate::Flags::$flag))+)?,
            $min,
            $max,
            $default,
        );
    };
    (
        $(#[doc = $help:literal])+
        $(#[flags($($flag:ident)|+)])?
        $vis:vis static $name:ident: f32 in $min:literal ..= $max:literal = $default:expr;
    ) => {
        #[allow(non_upper_case_globals, reason = "the ident is the console name")]
        $vis static $name: $crate::ConVar = $crate::ConVar::new_float(
            stringify!($name),
            $crate::__help(concat!($($help),+)),
            $crate::Flags::NONE $($(.union($crate::Flags::$flag))+)?,
            $min,
            $max,
            $default,
        );
    };
    (
        $(#[doc = $help:literal])+
        $(#[flags($($flag:ident)|+)])?
        $vis:vis static $name:ident: &'static str one_of [$($value:literal),+ $(,)?]
            = $default:literal;
    ) => {
        #[allow(non_upper_case_globals, reason = "the ident is the console name")]
        $vis static $name: $crate::ConVar = $crate::ConVar::new_enum(
            stringify!($name),
            $crate::__help(concat!($($help),+)),
            $crate::Flags::NONE $($(.union($crate::Flags::$flag))+)?,
            &[$($value),+],
            $default,
        );
    };
}

/// Declares a console command.
///
/// The doc comment is the help and the ident is the name. The two parameters are
/// named by the declaration, so a body that wants neither writes them with a
/// leading underscore the way any unused binding is written.
///
/// ```
/// crcbl_console::concommand! {
///     /// Print the arguments back.
///     pub fn shout(cx, args) {
///         cx.print(args.join(" ").to_uppercase());
///         Ok(())
///     }
/// }
///
/// let registry = crcbl_console::Registry::gather(&[
///     crcbl_console::table![cmd shout],
/// ])
/// .expect("no duplicate names");
/// let mut host = ();
/// let mut cx = crcbl_console::Context::new(&registry, &mut host);
/// registry.execute(&mut cx, "shout hello there").expect("shout runs");
/// assert_eq!(cx.lines(), ["HELLO THERE"]);
/// ```
#[macro_export]
macro_rules! concommand {
    (
        $(#[doc = $help:literal])+
        $vis:vis fn $name:ident($cx:ident, $args:ident) $body:block
    ) => {
        #[allow(non_upper_case_globals, reason = "the ident is the console name")]
        $vis static $name: $crate::ConCommand = {
            fn run(
                $cx: &mut $crate::Context<'_>,
                $args: &[&str],
            ) -> ::core::result::Result<(), $crate::Fault> $body

            $crate::ConCommand::new(
                stringify!($name),
                $crate::__help(concat!($($help),+)),
                run,
            )
        };
    };
}

/// Builds a crate's [`Table`](crate::Table) from the entries it declared.
///
/// A bare name is a [`ConVar`](crate::ConVar); `bind NAME` is a
/// [`Binding`](crate::Binding) and `cmd NAME` is a
/// [`ConCommand`](crate::ConCommand). The three may be written in any order.
///
/// ```
/// crcbl_console::convar! {
///     /// Draw the ambient-occlusion channel as grey.
///     pub static r_ao_view: bool = false;
/// }
/// crcbl_console::concommand! {
///     /// Say nothing, loudly.
///     pub fn nop(_cx, _args) { Ok(()) }
/// }
///
/// pub fn console_table() -> crcbl_console::Table {
///     crcbl_console::table![r_ao_view, cmd nop]
/// }
///
/// assert_eq!(console_table().len(), 2);
/// ```
#[macro_export]
macro_rules! table {
    // The accumulators come first so a `@collect` head never falls through to
    // the entry rule and recurses forever.
    (@collect ($($var:expr,)*) ($($bound:expr,)*) ($($cmd:expr,)*)) => {{
        static VARS: &[&$crate::ConVar] = &[$($var,)*];
        static BINDINGS: &[&$crate::Binding] = &[$($bound,)*];
        static COMMANDS: &[&$crate::ConCommand] = &[$($cmd,)*];
        $crate::Table::new(VARS, BINDINGS, COMMANDS)
    }};
    (@collect ($($var:expr,)*) ($($bound:expr,)*) ($($cmd:expr,)*)
        cmd $entry:path $(, $($rest:tt)*)?) => {
        $crate::table!(@collect ($($var,)*) ($($bound,)*) ($($cmd,)* &$entry,) $($($rest)*)?)
    };
    (@collect ($($var:expr,)*) ($($bound:expr,)*) ($($cmd:expr,)*)
        bind $entry:path $(, $($rest:tt)*)?) => {
        $crate::table!(@collect ($($var,)*) ($($bound,)* &$entry,) ($($cmd,)*) $($($rest)*)?)
    };
    (@collect ($($var:expr,)*) ($($bound:expr,)*) ($($cmd:expr,)*)
        $entry:path $(, $($rest:tt)*)?) => {
        $crate::table!(@collect ($($var,)* &$entry,) ($($bound,)*) ($($cmd,)*) $($($rest)*)?)
    };
    ($($entry:tt)*) => {
        $crate::table!(@collect () () () $($entry)*)
    };
}
