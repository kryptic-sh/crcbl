//! Linux and FreeBSD: fontconfig, `dlopen`ed — [`Source::Fontconfig`] — and
//! a walk of the font directories when it cannot be loaded —
//! [`Source::Scan`].
//!
//! # fontconfig
//!
//! A lookup is what `fc-match` does: the query becomes a pattern (family,
//! weight on fontconfig's own scale through `FcWeightFromOpenType`, slant),
//! `FcConfigSubstitute` and `FcDefaultSubstitute` apply the system's and the
//! user's configuration to it, and `FcFontMatch` picks the face, or
//! `FcFontSort` orders every face for the fallbacks, each result prepared
//! with `FcFontRenderPrepare` as `fc-match --sort` prepares it. The
//! configuration and its font cache are loaded once per
//! [`SystemFonts`](crate::SystemFonts), on its first lookup, by
//! `FcInitLoadConfigAndFonts`.
//!
//! **`FcFontMatch` always matches something**, the default face for a family
//! nothing installs. So [`Platform::find`] keeps its answer only when the face
//! belongs to a family the substituted pattern holds with a strong binding:
//! the family asked for, and what configuration put in its place strongly —
//! `<alias binding="same">`, as the metric-compatible aliases do (Arial to
//! Liberation Sans), or an `<edit binding="strong">`. The weak defaults
//! configuration appends for every pattern are what a miss would otherwise
//! return. A fontconfig without `FcPatternGetWithBinding` keeps only the
//! family asked for. `sans-serif`, `serif` and `monospace` keep whatever
//! fontconfig chose, every face being one of those; `emoji` is held to the
//! strong families like a named family, since fontconfig's configuration
//! binds its emoji fonts to it that way, and what it chooses on a system
//! with none is the default text face.
//!
//! A variable font's named instance is reported as its face — the index's
//! low 16 bits, where fontconfig puts the instance number above them — with
//! the instance's weight and style, though reading the face gives its
//! default instance.
//!
//! # Without fontconfig
//!
//! When `libfontconfig.so.1` cannot be loaded, a lookup walks
//! `$XDG_DATA_HOME/fonts` (else `~/.local/share/fonts`), `~/.fonts`,
//! `/usr/share/fonts` and `/usr/local/share/fonts` once and matches faces by
//! name table, as macOS does. **That path ignores fontconfig's rules
//! entirely** — aliases, preferences, rejected fonts — since reading them
//! would be writing fontconfig.
//!
//! # FFI
//!
//! `dlopen`, not `#[link]`, for the reason `crcbl-shell` gives for
//! libxkbcommon: a machine without the library must still get a working
//! program, here one that walks directories, rather than a process that
//! dies in `ld.so`. Every fontconfig object is owned by a type here that
//! destroys it on drop, and every string read out of a pattern is copied
//! before the pattern can go.

use core::ffi::{CStr, c_char, c_int, c_void};
use core::ptr::{self, NonNull};
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::face::{self, same_family};
use crate::scan::Catalog;
use crate::{Defaults, FontMatch, FontQuery, FontStyle, Generic, Source, ordered};

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_NOW`, the same on Linux and FreeBSD.
const RTLD_NOW: c_int = 2;

/// The sonames tried, in order: the ABI's, then the development link a
/// system without the versioned name might still have.
const SONAMES: [&CStr; 2] = [c"libfontconfig.so.1", c"libfontconfig.so"];

/// `FcMatchPattern`.
const MATCH_PATTERN: c_int = 0;
/// `FcResultMatch`.
const RESULT_MATCH: c_int = 0;
/// `FcTypeString`.
const TYPE_STRING: c_int = 3;
/// `FcValueBindingWeak`.
const BINDING_WEAK: c_int = 0;
/// `FcTrue`.
const FC_TRUE: c_int = 1;
/// `FC_SLANT_ROMAN`.
const SLANT_ROMAN: c_int = 0;
/// `FC_SLANT_ITALIC`.
const SLANT_ITALIC: c_int = 100;
/// `FC_SLANT_OBLIQUE`.
const SLANT_OBLIQUE: c_int = 110;
/// The bits of `FC_INDEX` that number the face; fontconfig puts a variable
/// font's named instance above them.
const FACE_INDEX_MASK: c_int = 0xFFFF;

/// `FC_FAMILY`.
const FAMILY: &CStr = c"family";
/// `FC_FILE`.
const FILE: &CStr = c"file";
/// `FC_INDEX`.
const INDEX: &CStr = c"index";
/// `FC_WEIGHT`.
const WEIGHT: &CStr = c"weight";
/// `FC_SLANT`.
const SLANT: &CStr = c"slant";

/// Opaque `FcConfig`.
#[repr(C)]
struct FcConfig {
    _opaque: [u8; 0],
}
/// Opaque `FcPattern`.
#[repr(C)]
struct FcPattern {
    _opaque: [u8; 0],
}
/// Opaque `FcCharSet`, only ever passed as null.
#[repr(C)]
struct FcCharSet {
    _opaque: [u8; 0],
}
/// `FcFontSet`, whose fields are public in `fontconfig.h`.
#[repr(C)]
struct FcFontSet {
    nfont: c_int,
    /// The allocated length, which only fontconfig reads.
    _sfont: c_int,
    fonts: *mut *mut FcPattern,
}
/// `FcValue`: a type tag and a union of a pointer, an integer, a boolean and
/// a double. Only the string member is read, and only when the tag says it
/// is one; the double sizes and aligns the union as C does.
#[repr(C)]
struct FcValue {
    kind: c_int,
    u: FcValueUnion,
}
#[repr(C)]
union FcValueUnion {
    s: *const u8,
    _int: c_int,
    _double: f64,
}

/// The C prototypes this module asserts about fontconfig, each copied from
/// `fontconfig/fontconfig.h`; the ABI is stable across 2.x.
mod prototype {
    use super::{FcCharSet, FcConfig, FcFontSet, FcPattern, FcValue, c_char, c_int};

    /// `FcConfig *FcInitLoadConfigAndFonts(void)`
    pub type InitLoadConfigAndFonts = unsafe extern "C" fn() -> *mut FcConfig;
    /// `void FcConfigDestroy(FcConfig *)`
    pub type ConfigDestroy = unsafe extern "C" fn(*mut FcConfig);
    /// `FcPattern *FcPatternCreate(void)`
    pub type PatternCreate = unsafe extern "C" fn() -> *mut FcPattern;
    /// `void FcPatternDestroy(FcPattern *)`
    pub type PatternDestroy = unsafe extern "C" fn(*mut FcPattern);
    /// `FcBool FcPatternAddString(FcPattern *, const char *object, const
    /// FcChar8 *s)`
    pub type PatternAddString =
        unsafe extern "C" fn(*mut FcPattern, *const c_char, *const u8) -> c_int;
    /// `FcBool FcPatternAddInteger(FcPattern *, const char *object, int i)`
    pub type PatternAddInteger =
        unsafe extern "C" fn(*mut FcPattern, *const c_char, c_int) -> c_int;
    /// `FcResult FcPatternGetString(const FcPattern *, const char *object,
    /// int n, FcChar8 **s)`
    pub type PatternGetString =
        unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut *mut u8) -> c_int;
    /// `FcResult FcPatternGetInteger(const FcPattern *, const char *object,
    /// int n, int *i)`
    pub type PatternGetInteger =
        unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut c_int) -> c_int;
    /// `FcResult FcPatternGetWithBinding(const FcPattern *, const char
    /// *object, int id, FcValue *v, FcValueBinding *b)`
    pub type PatternGetWithBinding = unsafe extern "C" fn(
        *const FcPattern,
        *const c_char,
        c_int,
        *mut FcValue,
        *mut c_int,
    ) -> c_int;
    /// `FcBool FcConfigSubstitute(FcConfig *, FcPattern *, FcMatchKind)`
    pub type ConfigSubstitute = unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, c_int) -> c_int;
    /// `void FcDefaultSubstitute(FcPattern *)`
    pub type DefaultSubstitute = unsafe extern "C" fn(*mut FcPattern);
    /// `FcPattern *FcFontMatch(FcConfig *, FcPattern *, FcResult *)`
    pub type FontMatch =
        unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, *mut c_int) -> *mut FcPattern;
    /// `FcFontSet *FcFontSort(FcConfig *, FcPattern *, FcBool trim,
    /// FcCharSet **csp, FcResult *)`
    pub type FontSort = unsafe extern "C" fn(
        *mut FcConfig,
        *mut FcPattern,
        c_int,
        *mut *mut FcCharSet,
        *mut c_int,
    ) -> *mut FcFontSet;
    /// `FcPattern *FcFontRenderPrepare(FcConfig *, FcPattern *pat,
    /// FcPattern *font)`
    pub type FontRenderPrepare =
        unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, *mut FcPattern) -> *mut FcPattern;
    /// `void FcFontSetDestroy(FcFontSet *)`
    pub type FontSetDestroy = unsafe extern "C" fn(*mut FcFontSet);
    /// `int FcWeightFromOpenType(int)` and `int FcWeightToOpenType(int)`
    pub type WeightConvert = unsafe extern "C" fn(c_int) -> c_int;
}

/// Every fontconfig entry point this module uses.
struct Lib {
    init_load_config_and_fonts: prototype::InitLoadConfigAndFonts,
    config_destroy: prototype::ConfigDestroy,
    pattern_create: prototype::PatternCreate,
    pattern_destroy: prototype::PatternDestroy,
    pattern_add_string: prototype::PatternAddString,
    pattern_add_integer: prototype::PatternAddInteger,
    pattern_get_string: prototype::PatternGetString,
    pattern_get_integer: prototype::PatternGetInteger,
    /// `None` in a fontconfig older than the function; see the module docs
    /// for what [`Platform::find`] keeps then.
    pattern_get_with_binding: Option<prototype::PatternGetWithBinding>,
    config_substitute: prototype::ConfigSubstitute,
    default_substitute: prototype::DefaultSubstitute,
    font_match: prototype::FontMatch,
    font_sort: prototype::FontSort,
    font_render_prepare: prototype::FontRenderPrepare,
    font_set_destroy: prototype::FontSetDestroy,
    weight_from_open_type: prototype::WeightConvert,
    weight_to_open_type: prototype::WeightConvert,
}

static LIB: OnceLock<Option<Lib>> = OnceLock::new();

fn load() -> Option<&'static Lib> {
    LIB.get_or_init(load_uncached).as_ref()
}

fn load_uncached() -> Option<Lib> {
    let handle = SONAMES.iter().find_map(|soname| {
        // SAFETY: `soname` is a NUL-terminated literal and `RTLD_NOW` a valid
        // flag. `dlopen` returns a handle or null and takes ownership of
        // nothing.
        let handle = unsafe { dlopen(soname.as_ptr(), RTLD_NOW) };
        (!handle.is_null()).then_some(handle)
    })?;

    /// Resolves one symbol, or null.
    fn raw(handle: *mut c_void, name: &CStr) -> *mut c_void {
        // SAFETY: `handle` is a live `dlopen` handle, never closed, and
        // `name` is NUL-terminated.
        unsafe { dlsym(handle, name.as_ptr()) }
    }

    /// Resolves one symbol as `$ty`, or gives up on the whole library.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            let raw = raw(handle, $name);
            if raw.is_null() {
                return None;
            }
            // SAFETY: `$ty` is the prototype `fontconfig.h` declares for this
            // symbol, and the handle is never closed, so the function lives
            // as long as the `'static` table holding it.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    let get_with_binding = raw(handle, c"FcPatternGetWithBinding");
    Some(Lib {
        init_load_config_and_fonts: symbol!(
            c"FcInitLoadConfigAndFonts",
            prototype::InitLoadConfigAndFonts
        ),
        config_destroy: symbol!(c"FcConfigDestroy", prototype::ConfigDestroy),
        pattern_create: symbol!(c"FcPatternCreate", prototype::PatternCreate),
        pattern_destroy: symbol!(c"FcPatternDestroy", prototype::PatternDestroy),
        pattern_add_string: symbol!(c"FcPatternAddString", prototype::PatternAddString),
        pattern_add_integer: symbol!(c"FcPatternAddInteger", prototype::PatternAddInteger),
        pattern_get_string: symbol!(c"FcPatternGetString", prototype::PatternGetString),
        pattern_get_integer: symbol!(c"FcPatternGetInteger", prototype::PatternGetInteger),
        // SAFETY: as `symbol!`, for the one function that may be missing.
        pattern_get_with_binding: (!get_with_binding.is_null()).then(|| unsafe {
            core::mem::transmute::<*mut c_void, prototype::PatternGetWithBinding>(get_with_binding)
        }),
        config_substitute: symbol!(c"FcConfigSubstitute", prototype::ConfigSubstitute),
        default_substitute: symbol!(c"FcDefaultSubstitute", prototype::DefaultSubstitute),
        font_match: symbol!(c"FcFontMatch", prototype::FontMatch),
        font_sort: symbol!(c"FcFontSort", prototype::FontSort),
        font_render_prepare: symbol!(c"FcFontRenderPrepare", prototype::FontRenderPrepare),
        font_set_destroy: symbol!(c"FcFontSetDestroy", prototype::FontSetDestroy),
        weight_from_open_type: symbol!(c"FcWeightFromOpenType", prototype::WeightConvert),
        weight_to_open_type: symbol!(c"FcWeightToOpenType", prototype::WeightConvert),
    })
}

/// A loaded configuration, destroyed on drop.
///
/// Only [`Config::load`] makes one, from a non-null `FcInitLoadConfigAndFonts`
/// result this value alone owns.
struct Config {
    lib: &'static Lib,
    raw: NonNull<FcConfig>,
}

// SAFETY: fontconfig's objects may be used from any thread, though not from
// two at once; the one `Config` is only reached through `Fontconfig`'s
// mutex, which gives each lookup it alone.
unsafe impl Send for Config {}

impl Config {
    fn load(lib: &'static Lib) -> Option<Self> {
        // SAFETY: no arguments; the result is a new configuration or null.
        let raw = unsafe { (lib.init_load_config_and_fonts)() };
        NonNull::new(raw).map(|raw| Self { lib, raw })
    }

    /// `query` as a pattern, with configuration applied: what `FcFontMatch`
    /// and `FcFontSort` take. `None` for a family holding a NUL, which no
    /// font names.
    fn pattern(&mut self, query: &FontQuery) -> Option<Pattern> {
        let lib = self.lib;
        let family = CString::new(query.family.as_str()).ok()?;
        // SAFETY: no arguments; the result is a new pattern or null, owned
        // by the `Pattern` from here.
        let pattern = Pattern::adopt(lib, unsafe { (lib.pattern_create)() })?;
        let slant = match query.style {
            FontStyle::Normal => SLANT_ROMAN,
            FontStyle::Italic => SLANT_ITALIC,
            FontStyle::Oblique => SLANT_OBLIQUE,
        };
        let raw = pattern.raw.as_ptr();
        // SAFETY: `raw` is the live pattern `pattern` owns; the object names
        // are NUL-terminated literals and `family` a NUL-terminated string,
        // which fontconfig copies. `self.raw` is the live configuration.
        unsafe {
            let weight = (lib.weight_from_open_type)(c_int::from(query.weight));
            let added = (lib.pattern_add_string)(raw, FAMILY.as_ptr(), family.as_ptr().cast()) != 0
                && (lib.pattern_add_integer)(raw, WEIGHT.as_ptr(), weight) != 0
                && (lib.pattern_add_integer)(raw, SLANT.as_ptr(), slant) != 0
                && (lib.config_substitute)(self.raw.as_ptr(), raw, MATCH_PATTERN) != 0;
            if !added {
                return None;
            }
            (lib.default_substitute)(raw);
        }
        Some(pattern)
    }

    /// `FcFontMatch`'s face for `pattern`, prepared.
    fn best(&mut self, pattern: &Pattern) -> Option<Pattern> {
        let mut result = 0;
        // SAFETY: both objects are live; `result` is a valid place. The
        // match is a new pattern or null, owned by the `Pattern` from here.
        let matched =
            unsafe { (self.lib.font_match)(self.raw.as_ptr(), pattern.raw.as_ptr(), &mut result) };
        Pattern::adopt(self.lib, matched)
    }

    /// `FcFontSort`'s faces for `pattern`, trimmed to those adding coverage,
    /// each prepared against it.
    fn sorted(&mut self, pattern: &Pattern) -> Vec<Pattern> {
        let mut result = 0;
        // SAFETY: both objects are live; a null charset pointer asks for no
        // coverage set back, and `result` is a valid place. The set is new
        // or null, owned by the `FontSet` from here.
        let set = unsafe {
            (self.lib.font_sort)(
                self.raw.as_ptr(),
                pattern.raw.as_ptr(),
                FC_TRUE,
                ptr::null_mut(),
                &mut result,
            )
        };
        let Some(set) = NonNull::new(set).map(|raw| FontSet { lib: self.lib, raw }) else {
            return Vec::new();
        };
        set.fonts()
            .iter()
            .filter_map(|&font| {
                // SAFETY: the configuration, the pattern and `font` (held by
                // `set`, alive for this closure) are live; the result is a
                // new pattern or null, owned by the `Pattern` from here.
                let prepared = unsafe {
                    (self.lib.font_render_prepare)(self.raw.as_ptr(), pattern.raw.as_ptr(), font)
                };
                Pattern::adopt(self.lib, prepared)
            })
            .collect()
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        // SAFETY: `self.raw` came from `FcInitLoadConfigAndFonts` and nothing
        // else destroys it (see `Config`).
        unsafe { (self.lib.config_destroy)(self.raw.as_ptr()) };
    }
}

/// A pattern this value owns, destroyed on drop.
///
/// Only [`Pattern::adopt`] makes one, from a non-null pattern fontconfig
/// handed over.
struct Pattern {
    lib: &'static Lib,
    raw: NonNull<FcPattern>,
}

impl Pattern {
    /// Takes ownership of `raw`, a pattern fontconfig returned as the
    /// caller's to destroy, or null.
    fn adopt(lib: &'static Lib, raw: *mut FcPattern) -> Option<Self> {
        NonNull::new(raw).map(|raw| Self { lib, raw })
    }

    /// The `n`th string value of `object`, copied out.
    fn string(&self, object: &CStr, n: c_int) -> Option<Vec<u8>> {
        let mut value = ptr::null_mut();
        // SAFETY: the pattern is live; `object` is NUL-terminated and
        // `value` a valid place. On a match `value` points at a
        // NUL-terminated string the pattern owns, copied before `self`'s
        // borrow ends.
        unsafe {
            let result =
                (self.lib.pattern_get_string)(self.raw.as_ptr(), object.as_ptr(), n, &mut value);
            (result == RESULT_MATCH && !value.is_null())
                .then(|| CStr::from_ptr(value.cast()).to_bytes().to_vec())
        }
    }

    /// The first integer value of `object`. fontconfig converts a double to
    /// one, as a prepared pattern's weight may be.
    fn integer(&self, object: &CStr) -> Option<c_int> {
        let mut value = 0;
        // SAFETY: the pattern is live; `object` is NUL-terminated and
        // `value` a valid place.
        let result = unsafe {
            (self.lib.pattern_get_integer)(self.raw.as_ptr(), object.as_ptr(), 0, &mut value)
        };
        (result == RESULT_MATCH).then_some(value)
    }

    /// Every family name it holds, in order.
    fn families(&self) -> Vec<String> {
        (0..)
            .map_while(|n| self.string(FAMILY, n))
            .map(|name| String::from_utf8_lossy(&name).into_owned())
            .collect()
    }

    /// The family names it holds bound more strongly than weakly — see the
    /// module docs — or `None` when this fontconfig cannot say.
    fn strong_families(&self) -> Option<Vec<String>> {
        let get = self.lib.pattern_get_with_binding?;
        let mut families = Vec::new();
        for id in 0.. {
            let mut value = FcValue {
                kind: 0,
                u: FcValueUnion { _double: 0.0 },
            };
            let mut binding = BINDING_WEAK;
            // SAFETY: the pattern is live; `FAMILY` is NUL-terminated and
            // `value` and `binding` are valid places. On a match whose tag is
            // a string, `value.u.s` points at a NUL-terminated string the
            // pattern owns, copied before `self`'s borrow ends.
            unsafe {
                if get(
                    self.raw.as_ptr(),
                    FAMILY.as_ptr(),
                    id,
                    &mut value,
                    &mut binding,
                ) != RESULT_MATCH
                {
                    break;
                }
                if value.kind == TYPE_STRING && binding != BINDING_WEAK && !value.u.s.is_null() {
                    let name = CStr::from_ptr(value.u.s.cast()).to_bytes();
                    families.push(String::from_utf8_lossy(name).into_owned());
                }
            }
        }
        Some(families)
    }

    /// The face this matched or sorted pattern names, or `None` when it
    /// names no file, or a file of a format this crate does not read.
    fn to_match(&self) -> Option<FontMatch> {
        let path = PathBuf::from(OsStr::from_bytes(&self.string(FILE, 0)?));
        if !face::is_font_file(&path) {
            return None;
        }
        let index = u32::try_from(self.integer(INDEX).unwrap_or(0) & FACE_INDEX_MASK).ok()?;
        let family = self.families().into_iter().next()?;
        let weight = self.integer(WEIGHT).map_or(crate::NORMAL_WEIGHT, |weight| {
            // SAFETY: a pure conversion of an integer.
            let weight = unsafe { (self.lib.weight_to_open_type)(weight) };
            u16::try_from(weight)
                .ok()
                .filter(|weight| (1..=1000).contains(weight))
                .unwrap_or(crate::NORMAL_WEIGHT)
        });
        let style = match self.integer(SLANT).unwrap_or(SLANT_ROMAN) {
            SLANT_ROMAN => FontStyle::Normal,
            SLANT_OBLIQUE => FontStyle::Oblique,
            _ => FontStyle::Italic,
        };
        Some(FontMatch {
            path,
            index,
            family,
            weight,
            style,
        })
    }
}

impl Drop for Pattern {
    fn drop(&mut self) {
        // SAFETY: `self.raw` was handed over as ours to destroy and nothing
        // else destroys it (see `Pattern`).
        unsafe { (self.lib.pattern_destroy)(self.raw.as_ptr()) };
    }
}

/// A font set this value owns, destroyed, with the references it holds, on
/// drop.
struct FontSet {
    lib: &'static Lib,
    raw: NonNull<FcFontSet>,
}

impl FontSet {
    /// Its patterns, which it owns and which live as long as it does.
    fn fonts(&self) -> &[*mut FcPattern] {
        // SAFETY: the set is live, and `fonts` holds `nfont` patterns (or is
        // null with none) for as long as the set does, which the returned
        // borrow of `self` keeps.
        unsafe {
            let set = self.raw.as_ref();
            match usize::try_from(set.nfont) {
                Ok(count) if count > 0 && !set.fonts.is_null() => {
                    core::slice::from_raw_parts(set.fonts, count)
                }
                _ => &[],
            }
        }
    }
}

impl Drop for FontSet {
    fn drop(&mut self) {
        // SAFETY: `self.raw` came from `FcFontSort`, which hands the set to
        // the caller, and nothing else destroys it.
        unsafe { (self.lib.font_set_destroy)(self.raw.as_ptr()) };
    }
}

/// fontconfig, loaded and configured.
struct Fontconfig {
    config: Mutex<Config>,
}

impl Fontconfig {
    fn load() -> Option<Self> {
        Config::load(load()?).map(|config| Self {
            config: Mutex::new(config),
        })
    }

    fn config(&self) -> std::sync::MutexGuard<'_, Config> {
        // A panic inside a lookup leaves the configuration as fontconfig
        // left it, which is whole: nothing here writes to it.
        self.config.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        let mut config = self.config();
        let pattern = config.pattern(query)?;
        let found = config.best(&pattern)?;
        let any_face = matches!(
            Generic::parse(&query.family),
            Some(Generic::SansSerif | Generic::Serif | Generic::Monospace)
        );
        if !any_face {
            let accepted = pattern
                .strong_families()
                .unwrap_or_else(|| vec![query.family.clone()]);
            let families = found.families();
            let kept = families
                .iter()
                .any(|family| accepted.iter().any(|wanted| same_family(family, wanted)));
            if !kept {
                return None;
            }
        }
        found.to_match()
    }

    fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        let primary = self.find(query);
        let mut config = self.config();
        let Some(pattern) = config.pattern(query) else {
            return Vec::new();
        };
        let sorted = config.sorted(&pattern);
        ordered(
            primary.as_ref(),
            sorted.iter().filter_map(Pattern::to_match),
        )
    }
}

/// What the walk answers without fontconfig: the families a fontconfig
/// configuration usually prefers, first installed wins.
const SCAN_DEFAULTS: Defaults = Defaults {
    sans_serif: &["DejaVu Sans", "Noto Sans", "Liberation Sans"],
    serif: &["DejaVu Serif", "Noto Serif", "Liberation Serif"],
    monospace: &["DejaVu Sans Mono", "Noto Sans Mono", "Liberation Mono"],
    emoji: &["Noto Color Emoji"],
    cascade: &[
        "DejaVu Sans",
        "Noto Sans",
        "Noto Sans CJK SC",
        "Noto Sans Symbols",
        "Noto Sans Symbols 2",
        "Noto Color Emoji",
    ],
};

/// The directories walked without fontconfig; see the module docs.
fn scan_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let mut dirs = Vec::new();
    dirs.extend(data_home.map(|dir| dir.join("fonts")));
    dirs.extend(home.map(|home| home.join(".fonts")));
    dirs.push(PathBuf::from("/usr/share/fonts"));
    dirs.push(PathBuf::from("/usr/local/share/fonts"));
    dirs
}

/// Where lookups go, decided on the first.
enum Backend {
    Fontconfig(Fontconfig),
    Scan(Catalog),
}

pub(crate) struct Platform {
    backend: OnceLock<Backend>,
}

impl core::fmt::Debug for Platform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Platform").finish_non_exhaustive()
    }
}

impl Platform {
    pub fn new() -> Self {
        Self {
            backend: OnceLock::new(),
        }
    }

    fn backend(&self) -> &Backend {
        self.backend.get_or_init(|| match Fontconfig::load() {
            Some(fontconfig) => Backend::Fontconfig(fontconfig),
            None => Backend::Scan(Catalog::new(scan_dirs())),
        })
    }

    pub fn source(&self) -> Source {
        match self.backend() {
            Backend::Fontconfig(_) => Source::Fontconfig,
            Backend::Scan(_) => Source::Scan,
        }
    }

    pub fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        match self.backend() {
            Backend::Fontconfig(fontconfig) => fontconfig.find(query),
            Backend::Scan(catalog) => SCAN_DEFAULTS.find(query, |family| catalog.faces_of(family)),
        }
    }

    pub fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        match self.backend() {
            Backend::Fontconfig(fontconfig) => fontconfig.fallbacks(query),
            Backend::Scan(_) => {
                SCAN_DEFAULTS.fallbacks(query, Vec::new(), |query| self.find(query))
            }
        }
    }
}
