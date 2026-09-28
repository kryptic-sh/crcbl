//! The catalogue: every key the engine defines, read or merely named, with the
//! console's kind and help line for each.

use crcbl_audio::mixer::Bus;
use crcbl_console::{Flags, Kind};
use crcbl_render::MIN_RENDER_SCALE;

use super::engine_audio::AUDIO_NAMESPACE;
use super::engine_video::{
    ANISOTROPIC_FILTERING_KEY, ANTIALIASING_KEY, ANTIALIASING_NAMES, FRAME_LIMIT_CEILING,
    FRAME_LIMIT_KEY, MAX_ANISOTROPIC_FILTERING, MAX_UI_SCALE, MIN_UI_SCALE, RENDER_SCALE_KEY,
    SHADOW_FILTER_KEY, SHADOW_FILTER_NAMES, SSAO_BENT_NORMALS_KEY, SSAO_BLUR_PASSES_KEY,
    SSAO_SLICES_KEY, UI_SCALE_KEY, VIDEO_NAMESPACE, effect_keys,
};

#[cfg(doc)]
use super::{VIDEO_KEYS, audio_gains};
#[cfg(doc)]
use crcbl_console::Binding;
#[cfg(doc)]
use crcbl_store::settings::SettingsStack;

/// Whether anything in this workspace reads a key yet.
///
/// The distinction the settings sample's exit criteria are written against:
/// "any key with no reader is labelled as such". A screen that offers a control
/// which silently does nothing is worse than one that says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    /// A reader in this module answers it, so writing it changes a frame or a
    /// mix.
    Read,
    /// Named by the display catalogue in `docs/notes/backends.md` and read by
    /// nothing.
    ///
    /// Named anyway, and now rather than later, because a key named late is a
    /// file every existing player has already written — catalogue rule 2 in
    /// `docs/notes/simulation.md`'s persistence rules.
    Named,
}

/// One key the engine's settings catalogue defines.
///
/// **`PartialEq` and not `Eq`**, since [`Kind`] holds floats.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogueKey {
    /// The dotted key, as it is written in `settings.toml` and passed to
    /// [`SettingsStack::get`].
    pub key: String,
    /// The name the console types, which is the key without its namespace.
    ///
    /// The spelling debug-console decision 2 (`docs/notes/tooling.md`) fixes
    /// for a settings-backed variable: `antialiasing`, not
    /// `engine.video.antialiasing` and not `r_antialiasing`, because a bare key
    /// is what the user typed in the example the console's plan was written
    /// against. It is `&'static str` where
    /// [`key`](Self::key) is a `String` because that is what it comes from —
    /// [`VIDEO_KEYS`] and its siblings — and because
    /// [`Binding`] needs a name that outlives the call.
    pub name: &'static str,
    /// What the key accepts, as the console's own domain type.
    ///
    /// **Was prose until debug-console decision 3** (`docs/notes/tooling.md`).
    /// A string could say "1 to 16" while the setter clamped to something else,
    /// and nothing could tell; a [`Kind`] is what a value is coerced and
    /// range-checked through, so
    /// `every_numeric_kind_agrees_with_the_setter_that_writes_it` can hold the
    /// two together. The prose that was here is [`help`](Self::help).
    pub kind: Kind,
    /// What the key is for, in one line — the prose the domain used to carry,
    /// minus whatever [`kind`](Self::kind) now states exactly.
    pub help: &'static str,
    /// Whether a reader answers it; see [`KeyStatus`].
    pub status: KeyStatus,
}

/// The help line every [`VIDEO_KEYS`] switch wears.
///
/// One line for six keys because it is one sentence about six keys — the name
/// is what says which effect, and a line per switch would be six copies of the
/// same clause. [`catalogue`] and the switch's own [`Binding`] read this
/// constant rather than each spelling it.
pub(super) const EFFECT_HELP: &str = "whether the player allows this effect; absent allows it";

/// [`ANTIALIASING_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
pub(super) const ANTIALIASING_HELP: &str =
    "which resolve the frame gets; absent leaves the game's own tier";

/// [`SHADOW_FILTER_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
pub(super) const SHADOW_FILTER_HELP: &str =
    "which shadow filter the frame uses; absent uses the shipped filter";
pub(super) const SSAO_SLICES_HELP: &str = "SSAO horizon slices; absent uses the shipped value";
pub(super) const SSAO_BLUR_PASSES_HELP: &str = "SSAO blur passes; absent uses the shipped value";
pub(super) const SSAO_BENT_NORMALS_HELP: &str =
    "whether SSAO writes bent normals; absent uses the shipped value";

/// [`RENDER_SCALE_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
pub(super) const RENDER_SCALE_HELP: &str = "fraction of the surface extent the frame is drawn at";

/// [`ANISOTROPIC_FILTERING_KEY`]'s help line, for [`catalogue`] and its
/// [`Binding`].
pub(super) const ANISOTROPIC_FILTERING_HELP: &str = "how the base-colour page is filtered; the low end is off, and the \
     device's own ceiling clamps it";

/// [`UI_SCALE_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
pub(super) const UI_SCALE_HELP: &str = "a multiplier over the UI's base scale: the window's scale factor, or a \
     game's fit-to-reference factor";

/// [`FRAME_LIMIT_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
pub(super) const FRAME_LIMIT_HELP: &str =
    "frames a second the loop is held under; zero is unlimited";

/// The domain of every `[engine.audio]` gain: what [`audio_gains`] clamps to.
pub(super) const GAIN_KIND: Kind = Kind::Float { min: 0.0, max: 1.0 };

/// The help line every `[engine.audio]` gain wears, on [`EFFECT_HELP`]'s terms.
pub(super) const GAIN_HELP: &str = "the bus gain; absent is unity";

/// What a [`KeyStatus::Named`] key's [`Binding`] carries: the settings stack is
/// still its storage, and nothing may write it.
///
/// [`Flags::READ_ONLY`] is the console half of [`KeyStatus::Named`] —
/// debug-console decision 3, so `help` lists the whole catalogue instead of
/// hiding the part of it no frame reads.
pub(super) const NAMED_FLAGS: Flags = Flags::ARCHIVE.union(Flags::READ_ONLY);

/// The help line of each [`NAMED_VIDEO_KEYS`] row, in that table's order.
///
/// Its own table so the console's binding for a row and the catalogue's entry
/// for it read the same literal: a `static Binding` needs a `&'static str` in a
/// const initializer, which is what stops the sentence being written twice and
/// then edited once.
///
/// **Each one opens with what the row's [`KeyStatus::Named`] means**, in the
/// words debug-console decision 3 (`docs/notes/tooling.md`) asks the console to
/// print, because that is the fact a person reading `help` needs before the
/// rest of the line is worth anything.
pub(super) const NAMED_HELP: [&str; 7] = [
    "nothing reads this yet — how the window sits on the desktop",
    "nothing reads this yet — monitor name; absent means wherever the window is",
    "nothing reads this yet — [width, height] in device pixels, as a TOML array",
    "nothing reads this yet — how the swapchain paces presentation",
    "nothing reads this yet — a scalar multiplier applied in the tonemap pass",
    "nothing reads this yet — whether the swapchain asks for an HDR format",
    "nothing reads this yet — the vertical field of view in degrees",
];

/// The `[engine.video]` rows nothing reads yet, with the kinds and the help
/// the display catalogue (`docs/notes/backends.md`) fixed for them.
///
/// Literals, unlike the rows below them, because there is nothing in the tree
/// to derive them from — that is exactly what makes them [`KeyStatus::Named`].
/// A row leaves this list by growing a reader and joining `catalogue`'s derived
/// half, so the two halves cannot both claim one key.
///
/// **Every range here is this list's own**, and that is the honest half of the
/// same fact: there is no setter to agree with, which is why
/// `every_numeric_kind_agrees_with_the_setter_that_writes_it` can only cover the
/// derived rows. A row that grows a reader takes the reader's range with it, and
/// joins the test at the same moment. Until then every one of them is
/// [`Flags::READ_ONLY`] to the console, so no range here decides anything.
///
/// `resolution` is [`Kind::Text`] rather than a pair, because it is a TOML array
/// and the console's domain type spells no array; `monitor` is text because a
/// monitor name is text.
pub(super) const NAMED_VIDEO_KEYS: [(&str, Kind, &str); 7] = [
    (
        "display_mode",
        Kind::Enum(&["windowed", "borderless"]),
        NAMED_HELP[0],
    ),
    ("monitor", Kind::Text, NAMED_HELP[1]),
    ("resolution", Kind::Text, NAMED_HELP[2]),
    (
        "present_mode",
        Kind::Enum(&["auto", "vsync", "adaptive", "off"]),
        NAMED_HELP[3],
    ),
    (
        "brightness",
        Kind::Float { min: 0.0, max: 2.0 },
        NAMED_HELP[4],
    ),
    ("hdr_output", Kind::Bool, NAMED_HELP[5]),
    (
        "fov",
        Kind::Float {
            min: 1.0,
            max: 179.0,
        },
        NAMED_HELP[6],
    ),
];

/// Every key the engine defines, read or merely named.
///
/// **Derived from the readers wherever there is a reader**, so a key cannot
/// appear here under one spelling and be read under another: the effect rows
/// come from [`VIDEO_KEYS`], the antialiasing row from [`ANTIALIASING_KEY`], the
/// scale rows from [`RENDER_SCALE_KEY`] and [`UI_SCALE_KEY`], the anisotropy row from
/// [`ANISOTROPIC_FILTERING_KEY`], and the volume rows from
/// [`Bus::settings_key`]. Only the rows with no reader are
/// written out, because there is nothing to derive them from.
///
/// The **kinds** are derived too wherever a reader fixes one: the antialiasing
/// row's set is [`ANTIALIASING_NAMES`], and every numeric row's range is the one
/// its setter clamps to, which
/// `every_numeric_kind_agrees_with_the_setter_that_writes_it` holds it to.
///
/// A `Vec` rather than a `const`: a dotted key is its namespace and its name
/// joined, and `format!` is not a const operation. The caller is a settings
/// screen or `crcbl settings list`, neither of which runs per frame.
///
/// What is **not** here is the `[game]` namespace. Those keys belong to
/// whichever game wrote them, so a key this list does not name is unknown to
/// the *engine* rather than wrong.
#[must_use]
pub fn catalogue() -> Vec<CatalogueKey> {
    let read = |namespace: &str, name: &'static str, kind, help| CatalogueKey {
        key: format!("{namespace}.{name}"),
        name,
        kind,
        help,
        status: KeyStatus::Read,
    };
    let mut keys: Vec<CatalogueKey> = effect_keys()
        .map(|(key, _)| read(VIDEO_NAMESPACE, key, Kind::Bool, EFFECT_HELP))
        .collect();
    keys.push(read(
        VIDEO_NAMESPACE,
        ANTIALIASING_KEY,
        Kind::Enum(&ANTIALIASING_NAMES),
        ANTIALIASING_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        SHADOW_FILTER_KEY,
        Kind::Enum(&SHADOW_FILTER_NAMES),
        SHADOW_FILTER_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        SSAO_SLICES_KEY,
        Kind::Int { min: 2, max: 4 },
        SSAO_SLICES_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        SSAO_BLUR_PASSES_KEY,
        Kind::Int { min: 1, max: 2 },
        SSAO_BLUR_PASSES_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        SSAO_BENT_NORMALS_KEY,
        Kind::Bool,
        SSAO_BENT_NORMALS_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        RENDER_SCALE_KEY,
        Kind::Float {
            min: MIN_RENDER_SCALE,
            max: 1.0,
        },
        RENDER_SCALE_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        ANISOTROPIC_FILTERING_KEY,
        Kind::Float {
            min: 1.0,
            max: MAX_ANISOTROPIC_FILTERING,
        },
        ANISOTROPIC_FILTERING_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        UI_SCALE_KEY,
        Kind::Float {
            min: MIN_UI_SCALE,
            max: MAX_UI_SCALE,
        },
        UI_SCALE_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        FRAME_LIMIT_KEY,
        Kind::Int {
            min: 0,
            max: FRAME_LIMIT_CEILING,
        },
        FRAME_LIMIT_HELP,
    ));
    keys.extend(NAMED_VIDEO_KEYS.map(|(key, kind, help)| CatalogueKey {
        key: format!("{VIDEO_NAMESPACE}.{key}"),
        name: key,
        kind,
        help,
        status: KeyStatus::Named,
    }));
    keys.extend(
        Bus::ALL.map(|bus| read(AUDIO_NAMESPACE, bus.settings_key(), GAIN_KIND, GAIN_HELP)),
    );
    keys
}

/// What the catalogue says about `key`, or `None` for one it does not name.
///
/// The lookup a settings screen and `crcbl settings list` both make, and the
/// reason [`catalogue`] is a list rather than a map: the whole catalogue scanned
/// once per key in a player's file is not worth a hash, and the order is what a
/// screen renders in.
#[must_use]
pub fn catalogued(key: &str) -> Option<CatalogueKey> {
    catalogue().into_iter().find(|entry| entry.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_console::Value;
    use crcbl_render::{Antialiasing, shadow::Filter};

    use crate::settings::tests::stack_from;

    /// **Every key a reader answers is in the catalogue, under the spelling
    /// the reader uses.**
    ///
    /// The failure this exists for is a catalogue that drifts: a screen offers
    /// `engine.video.ssao`, the reader looks for `ambient_occlusion`, and the
    /// player's choice lands in a key nothing will ever read. Asserted against
    /// the reader tables themselves rather than a second list, so a key renamed
    /// in one place fails here rather than in a player's file.
    #[test]
    fn every_key_with_a_reader_is_catalogued_as_read() {
        let read: Vec<String> = catalogue()
            .into_iter()
            .filter(|entry| entry.status == KeyStatus::Read)
            .map(|entry| entry.key)
            .collect();

        let mut wanted: Vec<String> = effect_keys()
            .map(|(key, _)| format!("{VIDEO_NAMESPACE}.{key}"))
            .collect();
        wanted.push(format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{SSAO_SLICES_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{SSAO_BLUR_PASSES_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{SSAO_BENT_NORMALS_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}"));
        wanted.extend(Bus::ALL.map(|bus| format!("{AUDIO_NAMESPACE}.{}", bus.settings_key())));

        for key in &wanted {
            assert!(read.contains(key), "`{key}` has a reader and no entry");
        }
        assert_eq!(
            read.len(),
            wanted.len(),
            "the catalogue calls something read that no reader answers: {read:?}"
        );
    }

    /// **A key is named once**, so a lookup cannot be ambiguous and a screen
    /// cannot draw one control twice.
    ///
    /// The way this breaks is a row keeping its `Named` entry after it grows a
    /// reader, which would put the same key in the list under both statuses.
    #[test]
    fn no_key_appears_in_the_catalogue_twice() {
        let mut seen: Vec<String> = catalogue().into_iter().map(|entry| entry.key).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(before, seen.len(), "a key is catalogued twice: {seen:?}");
    }

    /// **A key the catalogue names is a key a stack can be asked for**, which
    /// is the claim a screen depends on and the one a stray space or a wrong
    /// namespace would break silently.
    #[test]
    fn every_catalogued_key_is_a_usable_dotted_key() {
        let stack = stack_from("");
        for entry in catalogue() {
            assert!(
                entry.key.starts_with("engine.")
                    && !entry.key.contains(' ')
                    && entry.key.split('.').count() == 3,
                "`{}` is not a two-level engine key",
                entry.key
            );
            assert!(
                !stack.contains(&entry.key),
                "an empty file answered `{}`",
                entry.key
            );
            assert!(!entry.help.is_empty(), "`{}` has no help", entry.key);
            assert!(
                entry.key.ends_with(entry.name) && !entry.name.contains('.'),
                "`{}` does not end in its bare console name `{}`",
                entry.key,
                entry.name
            );
        }
    }

    /// **A key nothing defines is not catalogued**, which is what lets a caller
    /// tell a typo from a `[game]` key.
    #[test]
    fn a_key_the_engine_does_not_define_is_not_catalogued() {
        assert!(
            catalogued("engine.video.shadow").is_none(),
            "a typo matched"
        );
        assert!(
            catalogued("game.difficulty").is_none(),
            "a game key matched"
        );
        assert_eq!(
            catalogued(&format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"))
                .expect("the scale is catalogued")
                .status,
            KeyStatus::Read,
        );
        assert_eq!(
            catalogued(&format!("{VIDEO_NAMESPACE}.display_mode"))
                .expect("the display mode is catalogued")
                .status,
            KeyStatus::Named,
            "a key with no reader must not claim to have one",
        );
    }

    /// **The catalogue names the tier key once, with every rung the reader
    /// takes, and names no `smaa` key at all.**
    ///
    /// The kind is [`ANTIALIASING_NAMES`], which is built from
    /// [`Antialiasing::ALL`] — so this asserts the derivation actually arrived
    /// rather than re-deriving it: a rung added to the enum and a set that did
    /// not follow is a screen offering a control it will not describe. The
    /// `smaa` half is the retired key — a catalogue that still named it would
    /// have `crcbl settings list` reporting a row nothing reads as one the
    /// engine defines.
    #[test]
    fn the_antialiasing_domain_names_every_rung_the_reader_takes() {
        let key = format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}");
        let entry = catalogued(&key).expect("the tier is catalogued");
        assert_eq!(entry.status, KeyStatus::Read);
        let Kind::Enum(values) = entry.kind else {
            panic!("the tier is an enum, not {:?}", entry.kind)
        };
        assert_eq!(values.len(), Antialiasing::ALL.len());
        for tier in Antialiasing::ALL {
            assert!(
                values.contains(&tier.name()),
                "{tier:?} is missing from the set {values:?}",
            );
            assert_eq!(entry.kind.parse(tier.name()), Ok(Value::Enum(tier.name())));
        }
        assert_eq!(
            catalogue().iter().filter(|row| row.key == key).count(),
            1,
            "the tier is catalogued twice",
        );
        assert!(
            catalogued(&format!("{VIDEO_NAMESPACE}.smaa")).is_none(),
            "the retired key is still catalogued",
        );
    }

    /// **The catalogue names the shadow-filter key once, with every filter the
    /// reader takes.**
    #[test]
    fn the_shadow_filter_domain_names_every_filter_the_reader_takes() {
        let key = format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}");
        let entry = catalogued(&key).expect("the filter is catalogued");
        assert_eq!(entry.status, KeyStatus::Read);
        let Kind::Enum(values) = entry.kind else {
            panic!("the filter is an enum, not {:?}", entry.kind)
        };
        assert_eq!(values.len(), Filter::ALL.len());
        for filter in Filter::ALL {
            assert!(
                values.contains(&filter.label()),
                "{filter:?} is missing from {values:?}"
            );
            assert_eq!(
                entry.kind.parse(filter.label()),
                Ok(Value::Enum(filter.label()))
            );
        }
        assert_eq!(catalogue().iter().filter(|row| row.key == key).count(), 1);
    }
}
