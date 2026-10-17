//! The item table: what an item is, and the file it is written in.

use serde::{Deserialize, Serialize};

use crate::shape::Shape;

/// A word an item carries and a grid can be built to accept — `helmet`,
/// `optic`, `medical`.
///
/// The number is an index into the [`Catalog`]'s tag table, not a hash: two
/// catalogues number their tags differently, so a `Tag` is only meaningful
/// beside the catalogue it came from. [`Catalog::tag`] is how a caller gets one
/// from the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tag(pub u16);

/// An item's index in the [`Catalog`], in the order the file lists them.
///
/// This is a position and nothing more, so it moves when the file is edited.
/// [`Catalog::key`] is the id a save writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemId(pub u16);

/// Everything one kind of item is.
///
/// Definitions are immutable and shared: a hundred bandages are one `ItemDef`
/// and a hundred [`Stack`](crate::Stack)s pointing at it.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemDef {
    name: String,
    shape: Shape,
    tags: Vec<Tag>,
    stack_max: u16,
    weight_g: u32,
    letter: char,
    colour: [f32; 4],
}

impl ItemDef {
    /// The name the file gives it, which is also what [`Catalog::key`] hashes.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The footprint at [`Rotation::Deg0`](crate::Rotation::Deg0).
    #[must_use]
    pub const fn shape(&self) -> Shape {
        self.shape
    }

    /// The tags it carries, in file order.
    #[must_use]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Whether it carries `tag`, which is what a filtered grid asks.
    #[must_use]
    pub fn has_tag(&self, tag: Tag) -> bool {
        self.tags.contains(&tag)
    }

    /// The most of it one stack holds. `1` means it does not stack.
    #[must_use]
    pub const fn stack_max(&self) -> u16 {
        self.stack_max
    }

    /// What one of it weighs, in grams.
    #[must_use]
    pub const fn weight_g(&self) -> u32 {
        self.weight_g
    }

    /// The character a panel draws in its cells until there is an icon.
    ///
    /// This and [`colour`](Self::colour) are the placeholder cell, and they are
    /// in the definition rather than in a game's own table because the
    /// catalogue is the one file that already names every item. `crcbl icon
    /// bake` is what replaces them; `docs/plan/34-inventory.md` has that.
    #[must_use]
    pub const fn letter(&self) -> char {
        self.letter
    }

    /// The linear RGBA the placeholder cell is drawn in.
    #[must_use]
    pub const fn colour(&self) -> [f32; 4] {
        self.colour
    }
}

/// Every item a session knows about, plus the tag vocabulary they use.
///
/// Built from one RON document and then read-only. Lookups are linear scans in
/// file order — there is no `HashMap` in this crate, for the reason the crate
/// docs give, and a catalogue is tens of entries rather than thousands.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "CatalogFile", into = "CatalogFile")]
pub struct Catalog {
    items: Vec<ItemDef>,
    tags: Vec<String>,
}

impl Catalog {
    /// Parses one RON document.
    ///
    /// # Errors
    ///
    /// [`CatalogError`] if the text is not RON, or is RON that is not a
    /// catalogue: an unknown field, a footprint past [`MAX_SHAPE`](crate::MAX_SHAPE),
    /// a missing bracket. The error names the line and the column ron stopped
    /// at and carries ron's own message, which is what holds the offending
    /// field's name.
    pub fn from_ron(text: &str) -> Result<Self, CatalogError> {
        ron::from_str(text).map_err(CatalogError::from_spanned)
    }

    /// Writes the catalogue back out, deterministically.
    ///
    /// **Byte-identical for equal catalogues, on every platform.** Items print
    /// in the order they are held, fields in struct order, and the newline is
    /// pinned to `\n` rather than left to [`ron::ser::PrettyConfig`]'s default
    /// of `\r\n` on Windows.
    ///
    /// # Panics
    ///
    /// Never, in practice: a catalogue is strings, integers, floats and
    /// sequences, and ron's serializer has no failing path over those. The
    /// `Result` it returns exists for maps with non-string keys and for
    /// `Serialize` implementations that raise their own errors, and a catalogue
    /// has neither.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, Self::pretty())
            .expect("a Catalog has no serializer path that can fail")
    }

    /// The one writer configuration, so nothing that later writes a catalogue
    /// can disagree with [`to_ron`](Self::to_ron) about what a file looks like.
    fn pretty() -> ron::ser::PrettyConfig {
        ron::ser::PrettyConfig::new()
            .new_line("\n")
            .indentor("    ")
            .struct_names(true)
    }

    /// The definition at `id`, or `None` if the id is not this catalogue's.
    #[must_use]
    pub fn get(&self, id: ItemId) -> Option<&ItemDef> {
        self.items.get(id.0 as usize)
    }

    /// The id of the first item named `name`.
    ///
    /// Names are not checked for uniqueness — a catalogue with two `bandage`
    /// entries parses, and this finds the first.
    #[must_use]
    pub fn id_of(&self, name: &str) -> Option<ItemId> {
        self.items
            .iter()
            .position(|item| item.name == name)
            .and_then(|index| u16::try_from(index).ok())
            .map(ItemId)
    }

    /// The tag named `name`, if any item in this catalogue carries it.
    #[must_use]
    pub fn tag(&self, name: &str) -> Option<Tag> {
        self.tags
            .iter()
            .position(|tag| tag == name)
            .and_then(|index| u16::try_from(index).ok())
            .map(Tag)
    }

    /// The tag's name, which is what a refusal is reported to a player as.
    #[must_use]
    pub fn tag_name(&self, tag: Tag) -> Option<&str> {
        self.tags.get(tag.0 as usize).map(String::as_str)
    }

    /// The stable id of `id`: the FNV-1a hash of its name.
    ///
    /// [`ItemId`] is a position and moves when the file is edited, so a save
    /// writes this instead and reads it back through [`by_key`](Self::by_key).
    /// FNV-1a ([`crcbl_core::hash`]) because it is defined over bytes rather
    /// than over a `Hasher` whose output Rust does not promise across releases,
    /// and because a collision costs an item, not a security property.
    #[must_use]
    pub fn key(&self, id: ItemId) -> Option<u32> {
        self.get(id)
            .map(|item| crcbl_core::hash::fnv1a_32(item.name.as_bytes()))
    }

    /// The item whose [`key`](Self::key) is `key`.
    #[must_use]
    pub fn by_key(&self, key: u32) -> Option<ItemId> {
        self.items
            .iter()
            .position(|item| crcbl_core::hash::fnv1a_32(item.name.as_bytes()) == key)
            .and_then(|index| u16::try_from(index).ok())
            .map(ItemId)
    }

    /// Every definition, in file order.
    pub fn items(&self) -> impl Iterator<Item = (ItemId, &ItemDef)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| Some((ItemId(u16::try_from(index).ok()?), item)))
    }

    /// How many items the catalogue holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the catalogue holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// What a catalogue looks like on disk, which is deliberately not what it looks
/// like in memory.
///
/// The one difference is tags: a file writes them as the words a person typed
/// and this interns them into the table [`Tag`] indexes, in first-appearance
/// order over items in file order. That order is a function of the file alone,
/// so a catalogue written back out reproduces the file it was read from, and a
/// hand-written data file never spells a tag as a number.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Catalog", deny_unknown_fields)]
struct CatalogFile {
    items: Vec<ItemFile>,
}

/// One item's line in the file. See [`CatalogFile`].
#[derive(Serialize, Deserialize)]
#[serde(rename = "Item", deny_unknown_fields)]
struct ItemFile {
    name: String,
    shape: Shape,
    /// Omitted rather than written as `[]` is accepted on the way in; the
    /// writer always spells it, so a file it produced round-trips exactly.
    #[serde(default)]
    tags: Vec<String>,
    stack_max: u16,
    weight_g: u32,
    letter: char,
    colour: [f32; 4],
}

impl From<CatalogFile> for Catalog {
    fn from(file: CatalogFile) -> Self {
        let mut tags: Vec<String> = Vec::new();
        let items = file
            .items
            .into_iter()
            .map(|item| ItemDef {
                name: item.name,
                shape: item.shape,
                tags: item
                    .tags
                    .into_iter()
                    .map(|name| {
                        let index =
                            tags.iter()
                                .position(|held| *held == name)
                                .unwrap_or_else(|| {
                                    tags.push(name);
                                    tags.len() - 1
                                });
                        Tag(u16::try_from(index).unwrap_or(u16::MAX))
                    })
                    .collect(),
                stack_max: item.stack_max,
                weight_g: item.weight_g,
                letter: item.letter,
                colour: item.colour,
            })
            .collect();
        Self { items, tags }
    }
}

impl From<Catalog> for CatalogFile {
    fn from(catalog: Catalog) -> Self {
        let names = catalog.tags;
        Self {
            items: catalog
                .items
                .into_iter()
                .map(|item| ItemFile {
                    name: item.name,
                    shape: item.shape,
                    tags: item
                        .tags
                        .into_iter()
                        .map(|tag| names.get(tag.0 as usize).cloned().unwrap_or_default())
                        .collect(),
                    stack_max: item.stack_max,
                    weight_g: item.weight_g,
                    letter: item.letter,
                    colour: item.colour,
                })
                .collect(),
        }
    }
}

/// Why a string is not a [`Catalog`].
///
/// ron's own [`SpannedError`](ron::error::SpannedError) reduced to the three
/// things a person fixing the file needs, so a caller reporting it does not
/// have to depend on ron's types. This is the shape
/// `crcbl_render::stack::StackError` already has, and for the same reason.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}, column {column}: {message}")]
pub struct CatalogError {
    line: usize,
    column: usize,
    message: String,
}

impl CatalogError {
    /// Where the parser was when it gave up: the **start** of the span, which
    /// for an unknown field is the field's own name rather than the end of the
    /// document.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The column of [`line`](Self::line), 1-based as ron counts it.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// What ron said, without the position it says it at.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    fn from_spanned(error: ron::error::SpannedError) -> Self {
        Self {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalogue in the shape `to_ron` writes: struct names on, four-space
    /// indent, every field spelled, a trailing comma on every entry.
    const CANONICAL: &str = r###"Catalog(
    items: [
        Item(
            name: "bandage",
            shape: [
                "#",
            ],
            tags: [
                "medical",
            ],
            stack_max: 5,
            weight_g: 30,
            letter: 'b',
            colour: (0.9, 0.2, 0.2, 1.0),
        ),
        Item(
            name: "helmet",
            shape: [
                "##",
                "##",
            ],
            tags: [
                "helmet",
            ],
            stack_max: 1,
            weight_g: 1200,
            letter: 'h',
            colour: (0.3, 0.4, 0.5, 1.0),
        ),
    ],
)"###;

    /// **A catalogue written out reads back the same, byte for byte.** The
    /// round trip is asserted on the *text* rather than on the parsed value,
    /// because a writer that reordered fields, printed `2` for a float, or
    /// emitted `\r\n` would still compare equal after a second parse — and the
    /// thing a data file in git needs is that the bytes do not move.
    ///
    /// The second half asserts the parsed value too, so a writer that agreed
    /// with itself while dropping a field cannot pass on the text alone.
    #[test]
    fn a_catalogue_written_out_reads_back_the_same() {
        let catalog = Catalog::from_ron(CANONICAL).expect("the canonical file parses");
        assert_eq!(catalog.to_ron(), CANONICAL, "the writer is not the file");
        assert_eq!(
            Catalog::from_ron(&catalog.to_ron()).expect("what to_ron wrote has to parse"),
            catalog,
        );
        assert!(!catalog.to_ron().contains('\r'), "the newline is pinned");

        assert_eq!(catalog.len(), 2);
        assert!(!catalog.is_empty());
        let bandage = catalog.id_of("bandage").expect("it is in the file");
        let helmet = catalog.id_of("helmet").expect("it is in the file");
        assert_eq!(bandage, ItemId(0), "file order is id order");
        assert_eq!(helmet, ItemId(1));
        assert_eq!(catalog.id_of("nothing"), None);

        let def = catalog
            .get(bandage)
            .expect("the id came from this catalogue");
        assert_eq!(def.name(), "bandage");
        assert_eq!(def.stack_max(), 5);
        assert_eq!(def.weight_g(), 30);
        assert_eq!(def.letter(), 'b');
        assert_eq!(def.colour(), [0.9, 0.2, 0.2, 1.0]);
        assert_eq!(def.shape().width(), 1);
        assert_eq!(def.shape().height(), 1);

        let medical = catalog.tag("medical").expect("the bandage carries it");
        assert_eq!(
            medical,
            Tag(0),
            "first appearance in file order is tag order"
        );
        assert_eq!(catalog.tag("helmet"), Some(Tag(1)));
        assert_eq!(catalog.tag("optic"), None, "no item carries it");
        assert_eq!(catalog.tag_name(medical), Some("medical"));
        assert!(def.has_tag(medical));
        assert_eq!(def.tags(), &[medical]);
        assert!(!def.has_tag(Tag(1)));
    }

    /// **The save's item id survives an edit to the file that the position
    /// does not.** Inserting an item ahead of `helmet` moves its [`ItemId`] and
    /// must not move its [`Catalog::key`], which is the whole reason a second
    /// id exists.
    #[test]
    fn the_stable_key_outlives_a_catalogue_edit() {
        let catalog = Catalog::from_ron(CANONICAL).expect("the canonical file parses");
        let helmet = catalog.id_of("helmet").expect("it is in the file");
        let key = catalog.key(helmet).expect("the id is this catalogue's");

        let edited = Catalog::from_ron(&CANONICAL.replace(
            "        Item(\n            name: \"bandage\",",
            "        Item(\n            name: \"splint\",\n            shape: [\n                \"#\",\n            ],\n            tags: [],\n            stack_max: 1,\n            weight_g: 90,\n            letter: 's',\n            colour: (1.0, 1.0, 1.0, 1.0),\n        ),\n        Item(\n            name: \"bandage\",",
        ))
        .expect("the edited file parses");
        let moved = edited.id_of("helmet").expect("it is still in the file");
        assert_ne!(moved, helmet, "the edit moved its position");
        assert_eq!(edited.key(moved), Some(key), "the key is not its position");
        assert_eq!(edited.by_key(key), Some(moved));
        assert_eq!(edited.by_key(key.wrapping_add(1)), None);

        assert_eq!(key, 0x8d7a_7c20, "saves hold FNV-1a of the name");
    }

    /// **A file this build cannot stand behind is refused with a position.** A
    /// caller shows the player which line to fix, so the line and the column
    /// have to be ron's and not zero.
    #[test]
    fn a_catalogue_that_is_not_one_is_refused_where_it_went_wrong() {
        assert!(Catalog::from_ron("not ron").is_err());

        let unknown = Catalog::from_ron(&CANONICAL.replace("stack_max: 5", "stackmax: 5"))
            .expect_err("an unknown field is not a catalogue");
        assert!(unknown.line() > 1, "the position is ron's, not zero");
        assert!(unknown.column() >= 1);
        assert!(
            unknown.message().contains("stackmax"),
            "ron's message names the field: {}",
            unknown.message()
        );
        assert!(
            unknown
                .to_string()
                .starts_with(&format!("line {}, column ", unknown.line())),
            "{unknown}",
        );

        let huge = Catalog::from_ron(&CANONICAL.replace("\"#\",", "\"#########\","))
            .expect_err("a footprint past the mask is not a catalogue");
        assert!(
            huge.message().contains("footprint"),
            "the shape's own refusal is what ron reports: {}",
            huge.message()
        );
    }
}
