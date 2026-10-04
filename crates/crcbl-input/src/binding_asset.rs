//! A game's default actions as a RON file: [`ActionMap::from_ron`] reads one,
//! [`ActionMap::to_ron`] writes one.
//!
//! ```
//! use crcbl_input::ActionMap;
//!
//! let map = ActionMap::from_ron(
//!     r#"[
//!     (action: "move", kind: Axis2, keyboard: ["Wasd:KeyW,KeyS,KeyA,KeyD"], gamepad: ["PadStick:Left>0.2"]),
//!     (action: "jump", kind: Button, keyboard: ["Space"], gamepad: ["Pad:South"], patterns: [Hold(400, "jump_charge")]),
//!     (action: "jump_charge", kind: Button),
//! ]"#,
//! )?;
//! assert_eq!(map.emits("jump", crcbl_input::Pattern::Hold), Some("jump_charge"));
//! # Ok::<(), crcbl_input::BindingAssetError>(())
//! ```
//!
//! # The schema
//!
//! A list of records, one per action, declared in file order:
//!
//! - `action` — the name, unique across the file.
//! - `kind` — `Button`, `Axis1` or `Axis2`, the [`ActionKind`].
//! - `context` — the context it is declared in; left out, it is
//!   [`GAMEPLAY_CONTEXT`].
//! - `keyboard`, `mouse`, `gamepad`, `touch` — its bindings, by the kind of
//!   [`Device`] each listens to ([`Binding::device`]; `mouse` is
//!   [`Device::Pointer`]'s list). Each binding is written in its **text form**
//!   (`binding_text.rs`), the one a player's saved rebinds use, so a file, a
//!   rebind and this asset cannot spell a binding two ways.
//! - `patterns` — `Tap(ms, "name")`, `Hold(ms, "name")`,
//!   `DoubleTap(tap_ms, window_ms, "name")` and
//!   `DoubleTapOnRelease(tap_ms, window_ms, "name")`, at most one tap, one
//!   hold and one double tap, timed in whole milliseconds. Each **emits** the
//!   named action when it fires (`emit.rs`), and that action must be a
//!   `Button` declared in the same file.
//! - `pad_chords_outrank` — `true` lets the action's pad chords take their
//!   button from the contexts above it ([`ActionMap::set_pad_chords_outrank`]);
//!   left out, it is `false`.
//!
//! Every field but `action` and `kind` may be left out.
//!
//! # The device lists are presentation
//!
//! An action's bindings are one flat list, and nothing downstream can tell
//! which binding spoke (`docs/notes/simulation.md`, _What the deleted 19-input
//! plan left behind_). The four lists are only how a file groups them: they are
//! flattened into [`ActionDecl::bindings`] in the fixed order
//! [`DEVICE_LISTS`] gives — keyboard, mouse, gamepad, touch — whatever order
//! the file writes the fields in. A binding in a list that is not its device's
//! is refused by name, so a key cannot hide under `gamepad`.
//!
//! # Defaults, not the player's choices
//!
//! What a file declares is each action's **defaults**, the ones a player's
//! rebinds are a diff over (`overrides.rs`): [`ActionMap::overrides`] and
//! [`ActionMap::apply_overrides`] work on a loaded map as on one declared in
//! code, and [`ActionMap::to_ron`] writes the defaults whatever the player
//! rebound.
//!
//! # Refused at the boundary
//!
//! A file is untrusted input, and everything wrong with it is a
//! [`BindingAssetError`] naming the line and column and what was wrong — never
//! a panic, and never a map with the bad record left out. A refusal of a record
//! as a whole (a duplicate name, a binding in the wrong list, a pattern naming
//! an action the file does not declare) points at the end of that record.

use std::collections::HashMap;
use std::fmt;

use serde::de::{self, DeserializeSeed, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::{
    ActionDecl, ActionKind, ActionMap, ActionMapError, Binding, BindingParseError, Device,
    DoubleTap, GAMEPLAY_CONTEXT, Hold, Pattern, Tap,
};

/// The devices a record groups its bindings by, in the order their lists are
/// flattened into one binding list.
pub const DEVICE_LISTS: [Device; 4] = [
    Device::Keyboard,
    Device::Pointer,
    Device::Gamepad,
    Device::Touch,
];

/// The field a device's bindings are written under.
#[must_use]
pub const fn list_name(device: Device) -> &'static str {
    match device {
        Device::Keyboard => "keyboard",
        Device::Pointer => "mouse",
        Device::Gamepad => "gamepad",
        Device::Touch => "touch",
    }
}

/// A pattern's time in the file is whole milliseconds; the map times it in
/// seconds.
const MS_PER_SECOND: f32 = 1000.0;

impl ActionMap {
    /// A map declaring every action a binding asset lists, in file order, with
    /// its patterns attached — see the [module docs](self) for the schema.
    ///
    /// # Errors
    /// [`BindingAssetError`] for text that is not RON, RON that is not the
    /// schema (an unknown field or kind, a missing `action`), or a schema that
    /// is not a usable map; [`AssetRefusal`] names each.
    pub fn from_ron(text: &str) -> Result<Self, BindingAssetError> {
        // A first pass for every action's name and kind, so a pattern can name
        // an action declared further down and still be refused where it is
        // written rather than at the end of the file.
        let headers: Vec<Header> = ron::from_str(text).map_err(BindingAssetError::parse)?;
        let mut kinds = HashMap::with_capacity(headers.len());
        for header in headers {
            kinds.entry(header.action).or_insert(header.kind);
        }
        let mut map = Self::new();
        let mut refusal = None;
        let read = ron::Options::default().from_str_seed(
            text,
            Records {
                kinds: &kinds,
                map: &mut map,
                refusal: &mut refusal,
            },
        );
        match read {
            Ok(()) => Ok(map),
            Err(error) => Err(match refusal {
                Some(refusal) => BindingAssetError::at(&error, refusal),
                None => BindingAssetError::parse(error),
            }),
        }
    }

    /// Every action's declaration as a binding asset, in declaration order, in
    /// the canonical form: fields in the schema's order, empty lists left out,
    /// four-space indent, `\n` newlines on every platform, and a final
    /// newline. Reading a canonical file and writing it back gives the same
    /// bytes.
    ///
    /// Writes each action's **defaults**, not what a player rebound them to —
    /// see the [module docs](self). An emitted action named on a pattern that
    /// is not attached is not written, since it does nothing.
    ///
    /// # Errors
    /// [`AssetWriteError`] for a map the schema cannot say: one whose reading
    /// back would differ from it.
    ///
    /// # Panics
    /// Never, in practice: a record is strings, integers, enums and
    /// sequences, and ron's serializer has no failing path over those.
    pub fn to_ron(&self) -> Result<String, AssetWriteError> {
        let records = self
            .slots
            .iter()
            .map(|slot| self.record(slot))
            .collect::<Result<Vec<_>, _>>()?;
        let pretty = ron::ser::PrettyConfig::new()
            .new_line("\n")
            .indentor("    ")
            // Lists of records one per line, and everything inside a record
            // on that record's lines: two levels, then compact.
            .depth_limit(2);
        let mut text = ron::ser::to_string_pretty(&records, pretty)
            .expect("a binding asset has no serializer path that can fail");
        text.push('\n');
        Ok(text)
    }

    /// One slot as its record.
    fn record(&self, slot: &crate::ActionSlot) -> Result<ActionFile, AssetWriteError> {
        let name = &slot.decl.name;
        if slot.repeat.is_some() {
            return Err(AssetWriteError::Repeat(name.clone()));
        }
        let mut file = ActionFile {
            action: name.clone(),
            kind: slot.decl.kind.into(),
            context: self.contexts[slot.context].clone(),
            keyboard: Vec::new(),
            mouse: Vec::new(),
            gamepad: Vec::new(),
            touch: Vec::new(),
            patterns: Vec::new(),
            pad_chords_outrank: slot.pad_chords_outrank,
        };
        for binding in &slot.defaults {
            file.list_mut(binding.device()).push(binding.to_string());
        }
        let read_back = DEVICE_LISTS
            .iter()
            .flat_map(|device| file.list(*device))
            .map(String::as_str);
        if !read_back.eq(slot.defaults.iter().map(ToString::to_string)) {
            return Err(AssetWriteError::MixedLists(name.clone()));
        }
        let emits = |pattern| {
            self.emits(name, pattern)
                .map(str::to_owned)
                .ok_or_else(|| AssetWriteError::NoEmit {
                    action: name.clone(),
                    pattern,
                })
        };
        let ms = |seconds, pattern| {
            whole_ms(seconds).ok_or_else(|| AssetWriteError::NotWholeMilliseconds {
                action: name.clone(),
                pattern,
            })
        };
        if let Some(tap) = self.tap(name) {
            let time = ms(tap.time(), Pattern::Tap)?;
            file.patterns
                .push(PatternFile::Tap(time, emits(Pattern::Tap)?));
        }
        if let Some(hold) = self.hold(name) {
            let time = ms(hold.time(), Pattern::Hold)?;
            file.patterns
                .push(PatternFile::Hold(time, emits(Pattern::Hold)?));
        }
        if let Some(double) = self.double_tap(name) {
            let tap_time = ms(double.tap_time(), Pattern::DoubleTap)?;
            let window = ms(double.window(), Pattern::DoubleTap)?;
            let target = emits(Pattern::DoubleTap)?;
            file.patterns.push(if double.fires_on_release() {
                PatternFile::DoubleTapOnRelease(tap_time, window, target)
            } else {
                PatternFile::DoubleTap(tap_time, window, target)
            });
        }
        Ok(file)
    }
}

/// `seconds` as whole milliseconds, if it is exactly that many: what
/// [`ActionMap::from_ron`] reads back has to be the same `f32`.
fn whole_ms(seconds: f32) -> Option<u16> {
    // `as` saturates out of range and maps NaN to zero; the comparison below
    // refuses anything it changed.
    let ms = (seconds * MS_PER_SECOND).round() as u16;
    (f32::from(ms) / MS_PER_SECOND == seconds).then_some(ms)
}

/// The patterns' file spelling.
const fn pattern_name(pattern: Pattern) -> &'static str {
    match pattern {
        Pattern::Tap => "Tap",
        Pattern::Hold => "Hold",
        Pattern::DoubleTap => "DoubleTap",
    }
}

// ---------------------------------------------------------------------------
// The file's shape
// ---------------------------------------------------------------------------

/// One action's record. Field order is the canonical order [`ActionMap::to_ron`]
/// writes.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Action", deny_unknown_fields)]
struct ActionFile {
    action: String,
    kind: KindFile,
    #[serde(default = "gameplay", skip_serializing_if = "is_gameplay")]
    context: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    keyboard: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mouse: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    gamepad: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    touch: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<PatternFile>,
    #[serde(default, skip_serializing_if = "is_false")]
    pad_chords_outrank: bool,
}

impl ActionFile {
    fn list(&self, device: Device) -> &[String] {
        match device {
            Device::Keyboard => &self.keyboard,
            Device::Pointer => &self.mouse,
            Device::Gamepad => &self.gamepad,
            Device::Touch => &self.touch,
        }
    }

    fn list_mut(&mut self, device: Device) -> &mut Vec<String> {
        match device {
            Device::Keyboard => &mut self.keyboard,
            Device::Pointer => &mut self.mouse,
            Device::Gamepad => &mut self.gamepad,
            Device::Touch => &mut self.touch,
        }
    }
}

fn gameplay() -> String {
    GAMEPLAY_CONTEXT.to_owned()
}

fn is_gameplay(context: &str) -> bool {
    context == GAMEPLAY_CONTEXT
}

fn is_false(flag: &bool) -> bool {
    !flag
}

/// The first pass's view of a record: what a pattern needs to know about the
/// action it emits. Everything else is skipped here and checked in the second.
#[derive(Deserialize)]
#[serde(rename = "Action")]
struct Header {
    action: String,
    kind: KindFile,
}

/// [`ActionKind`]'s file spelling.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "Kind")]
enum KindFile {
    Button,
    Axis1,
    Axis2,
}

impl From<ActionKind> for KindFile {
    fn from(kind: ActionKind) -> Self {
        match kind {
            ActionKind::Button => Self::Button,
            ActionKind::Axis1 => Self::Axis1,
            ActionKind::Axis2 => Self::Axis2,
        }
    }
}

impl From<KindFile> for ActionKind {
    fn from(kind: KindFile) -> Self {
        match kind {
            KindFile::Button => Self::Button,
            KindFile::Axis1 => Self::Axis1,
            KindFile::Axis2 => Self::Axis2,
        }
    }
}

/// One pattern, its times in milliseconds and the action it emits.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Pattern")]
enum PatternFile {
    Tap(u16, String),
    Hold(u16, String),
    DoubleTap(u16, u16, String),
    DoubleTapOnRelease(u16, u16, String),
}

impl PatternFile {
    const fn pattern(&self) -> Pattern {
        match self {
            Self::Tap(..) => Pattern::Tap,
            Self::Hold(..) => Pattern::Hold,
            Self::DoubleTap(..) | Self::DoubleTapOnRelease(..) => Pattern::DoubleTap,
        }
    }

    fn emits(&self) -> &str {
        match self {
            Self::Tap(_, emits)
            | Self::Hold(_, emits)
            | Self::DoubleTap(_, _, emits)
            | Self::DoubleTapOnRelease(_, _, emits) => emits,
        }
    }

    /// This pattern's times as the map counts them, or `None` for a time of
    /// zero.
    fn timed(&self) -> Option<Timed> {
        let seconds = |ms: u16| f32::from(ms) / MS_PER_SECOND;
        Some(match *self {
            Self::Tap(ms, _) => Timed::Tap(Tap::new(seconds(ms))?),
            Self::Hold(ms, _) => Timed::Hold(Hold::new(seconds(ms))?),
            Self::DoubleTap(tap, window, _) => {
                Timed::DoubleTap(DoubleTap::new(seconds(tap), seconds(window))?)
            }
            Self::DoubleTapOnRelease(tap, window, _) => {
                Timed::DoubleTap(DoubleTap::new(seconds(tap), seconds(window))?.on_release())
            }
        })
    }
}

/// A pattern whose times have been checked, waiting for every action to be
/// declared before it is attached.
enum Timed {
    Tap(Tap),
    Hold(Hold),
    DoubleTap(DoubleTap),
}

impl Timed {
    /// Attach this pattern to `action`, emitting `emits`.
    fn attach(self, map: &mut ActionMap, action: &str, emits: &str) -> Result<(), ActionMapError> {
        let pattern = match self {
            Self::Tap(tap) => {
                map.set_tap(action, Some(tap))?;
                Pattern::Tap
            }
            Self::Hold(hold) => {
                map.set_hold(action, Some(hold))?;
                Pattern::Hold
            }
            Self::DoubleTap(double) => {
                map.set_double_tap(action, Some(double))?;
                Pattern::DoubleTap
            }
        };
        map.set_emits(action, pattern, Some(emits))
    }
}

// ---------------------------------------------------------------------------
// The second pass
// ---------------------------------------------------------------------------

/// The second pass over the file's list: each record checked and declared as
/// it is read, so a refusal carries the position ron is at.
struct Records<'a> {
    /// Every action's kind, from the first pass.
    kinds: &'a HashMap<String, KindFile>,
    map: &'a mut ActionMap,
    /// What refused the file, for [`ActionMap::from_ron`] to report: serde
    /// carries a visitor's error only as a message.
    refusal: &'a mut Option<AssetRefusal>,
}

impl Records<'_> {
    /// Check one record and declare it. Its patterns are attached once every
    /// action is declared, since one may emit an action further down.
    fn declare(&mut self, record: ActionFile) -> Result<Vec<(Timed, String)>, AssetRefusal> {
        let action = record.action.clone();
        let mut bindings = Vec::new();
        for device in DEVICE_LISTS {
            for text in record.list(device) {
                let binding =
                    text.parse::<Binding>()
                        .map_err(|error| AssetRefusal::BadBinding {
                            action: action.clone(),
                            error,
                        })?;
                if binding.device() != device {
                    return Err(AssetRefusal::WrongList {
                        action,
                        binding: text.clone(),
                        list: device,
                    });
                }
                bindings.push(binding);
            }
        }
        let mut seen = [false; Pattern::ALL.len()];
        let mut timed = Vec::with_capacity(record.patterns.len());
        for pattern in &record.patterns {
            let kind = pattern.pattern();
            let emits = pattern.emits();
            let Some(times) = pattern.timed() else {
                return Err(AssetRefusal::BadTime {
                    action,
                    pattern: kind,
                });
            };
            timed.push((times, emits.to_owned()));
            if std::mem::replace(&mut seen[kind.index()], true) {
                return Err(AssetRefusal::DuplicatePattern {
                    action,
                    pattern: kind,
                });
            }
            if emits == action {
                return Err(AssetRefusal::EmitsItself {
                    action,
                    pattern: kind,
                });
            }
            match self.kinds.get(emits) {
                Some(KindFile::Button) => {}
                Some(_) => {
                    return Err(AssetRefusal::EmitNotAButton {
                        action,
                        pattern: kind,
                        emits: emits.to_owned(),
                    });
                }
                None => {
                    return Err(AssetRefusal::UnknownEmit {
                        action,
                        pattern: kind,
                        emits: emits.to_owned(),
                    });
                }
            }
        }
        self.map
            .try_declare_in(
                &record.context,
                ActionDecl {
                    name: record.action,
                    kind: record.kind.into(),
                    bindings,
                },
            )
            .and_then(|()| {
                self.map
                    .set_pad_chords_outrank(&action, record.pad_chords_outrank)
            })
            .map_err(AssetRefusal::Declare)?;
        Ok(timed)
    }

    /// Hand `refusal` to [`ActionMap::from_ron`] and stop the parse where it is.
    fn refuse<E: de::Error>(&mut self, refusal: AssetRefusal) -> E {
        let error = E::custom(&refusal);
        *self.refusal = Some(refusal);
        error
    }
}

impl<'de> DeserializeSeed<'de> for Records<'_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Records<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a list of action records")
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<(), A::Error> {
        let mut patterns = Vec::new();
        while let Some(record) = seq.next_element::<ActionFile>()? {
            let action = record.action.clone();
            match self.declare(record) {
                Ok(timed) => patterns.push((action, timed)),
                Err(refusal) => return Err(self.refuse(refusal)),
            }
        }
        for (action, timed) in patterns {
            for (pattern, emits) in timed {
                // Every check `attach` makes was made as the record was read;
                // what it refuses anyway is reported, at the end of the list.
                if let Err(error) = pattern.attach(self.map, &action, &emits) {
                    return Err(self.refuse(AssetRefusal::Declare(error)));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a binding asset was refused, and where — see
/// [`ActionMap::from_ron`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingAssetError {
    line: usize,
    column: usize,
    refusal: AssetRefusal,
}

impl BindingAssetError {
    /// The line ron was at when it stopped, 1-based.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The column of [`line`](Self::line), 1-based as ron counts it.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// What was wrong.
    #[must_use]
    pub const fn refusal(&self) -> &AssetRefusal {
        &self.refusal
    }

    fn at(error: &ron::error::SpannedError, refusal: AssetRefusal) -> Self {
        Self {
            line: error.span.start.line,
            column: error.span.start.col,
            refusal,
        }
    }

    fn parse(error: ron::error::SpannedError) -> Self {
        let message = error.code.to_string();
        Self::at(&error, AssetRefusal::Parse(message))
    }
}

impl fmt::Display for BindingAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.refusal
        )
    }
}

impl std::error::Error for BindingAssetError {}

/// What was wrong with a binding asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetRefusal {
    /// Not RON, or RON that is not the schema — an unknown field, an unknown
    /// `kind`, a missing `action` — in ron's own words, which name it.
    Parse(String),
    /// A binding's text is not a [`Binding`].
    BadBinding {
        /// The record's action.
        action: String,
        /// Why the text did not parse, with the text.
        error: BindingParseError,
    },
    /// A binding written in a device list that is not its device's — a key
    /// under `gamepad`.
    WrongList {
        /// The record's action.
        action: String,
        /// The binding, as written.
        binding: String,
        /// The list it was written in; [`Binding::device`] names the right one.
        list: Device,
    },
    /// Two taps, two holds or two double taps on one action.
    DuplicatePattern {
        /// The record's action.
        action: String,
        /// The pattern written twice.
        pattern: Pattern,
    },
    /// A pattern time of zero milliseconds, which would fire on every press.
    BadTime {
        /// The record's action.
        action: String,
        /// The pattern.
        pattern: Pattern,
    },
    /// A pattern that emits an action the file does not declare.
    UnknownEmit {
        /// The record's action.
        action: String,
        /// The pattern.
        pattern: Pattern,
        /// The name it emits.
        emits: String,
    },
    /// A pattern that emits an action that is not a `Button`.
    EmitNotAButton {
        /// The record's action.
        action: String,
        /// The pattern.
        pattern: Pattern,
        /// The action it emits.
        emits: String,
    },
    /// A pattern that emits the action it is on.
    EmitsItself {
        /// The record's action.
        action: String,
        /// The pattern.
        pattern: Pattern,
    },
    /// The map refused the record: [`ActionMapError::DuplicateName`] for a
    /// name declared twice, [`ActionMapError::InvalidDeadzone`] for a pad dead
    /// zone or threshold outside `0.0..1.0`.
    Declare(ActionMapError),
}

impl fmt::Display for AssetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => f.write_str(message),
            Self::BadBinding { action, error } => write!(f, "action {action:?}: {error}"),
            Self::WrongList {
                action,
                binding,
                list,
            } => write!(
                f,
                "action {action:?}: {binding:?} is not a {} binding",
                list_name(*list)
            ),
            Self::DuplicatePattern { action, pattern } => {
                let pattern = pattern_name(*pattern);
                write!(f, "action {action:?}: more than one {pattern}")
            }
            Self::BadTime { action, pattern } => {
                let pattern = pattern_name(*pattern);
                write!(f, "action {action:?}: a {pattern} of zero milliseconds")
            }
            Self::UnknownEmit {
                action,
                pattern,
                emits,
            } => {
                let pattern = pattern_name(*pattern);
                write!(
                    f,
                    "action {action:?}: {pattern} emits {emits:?}, which is not declared"
                )
            }
            Self::EmitNotAButton {
                action,
                pattern,
                emits,
            } => {
                let pattern = pattern_name(*pattern);
                write!(
                    f,
                    "action {action:?}: {pattern} emits {emits:?}, which is not a Button"
                )
            }
            Self::EmitsItself { action, pattern } => {
                let pattern = pattern_name(*pattern);
                write!(f, "action {action:?}: {pattern} emits its own action")
            }
            Self::Declare(error) => error.fmt(f),
        }
    }
}

/// Why [`ActionMap::to_ron`] cannot write a map: the schema has no way to say
/// it, so the file would read back as a different map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetWriteError {
    /// The action's defaults interleave devices — a pad button between two
    /// keys — and the file's lists would read back in [`DEVICE_LISTS`] order.
    MixedLists(String),
    /// A pattern attached with no emitted action named; the schema's patterns
    /// all emit one.
    NoEmit {
        /// The action.
        action: String,
        /// The pattern.
        pattern: Pattern,
    },
    /// A pattern time that is not a whole number of milliseconds, or more
    /// than a `u16` of them.
    NotWholeMilliseconds {
        /// The action.
        action: String,
        /// The pattern.
        pattern: Pattern,
    },
    /// A repeat schedule ([`ActionMap::set_repeat`]), which the schema does not
    /// have.
    Repeat(String),
}

impl fmt::Display for AssetWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedLists(action) => {
                write!(f, "action {action:?}: its bindings interleave devices")
            }
            Self::NoEmit { action, pattern } => {
                let pattern = pattern_name(*pattern);
                write!(f, "action {action:?}: its {pattern} emits no action")
            }
            Self::NotWholeMilliseconds { action, pattern } => {
                let pattern = pattern_name(*pattern);
                write!(
                    f,
                    "action {action:?}: its {pattern} is not a whole number of milliseconds"
                )
            }
            Self::Repeat(action) => write!(f, "action {action:?}: a repeat has no asset form"),
        }
    }
}

impl std::error::Error for AssetWriteError {}

#[cfg(test)]
mod tests;
