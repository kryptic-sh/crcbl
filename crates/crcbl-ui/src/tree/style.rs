//! What a node is laid out and painted with.
//!
//! [`NodeStyle`] is a plain struct of the CSS subset
//! UI section 2 (`docs/notes/tooling.md`) names, and it **is** the style Taffy
//! reads: it implements `taffy`'s `CoreStyle`, `FlexboxContainerStyle` and
//! `FlexboxItemStyle` itself, so layout never converts it into a
//! `taffy::Style` and there is no second copy of any value to drift. The value
//! types here are this crate's rather than Taffy's, so a caller names no type
//! from a 0.x dependency and a Taffy upgrade changes the mapping below rather
//! than every call site.
//!
//! # Layout fields and paint fields
//!
//! The two halves are hashed apart. [`NodeStyle::layout_hash`] folds in every
//! field that can move a box and none that only colours one, which is what
//! lets a paint-only change — a hover colour — leave every layout cache in the
//! tree alone.

use core::hash::Hasher;

use taffy::{
    AlignContent, AlignItems, BoxGenerationMode, CoreStyle, Dimension, FlexboxContainerStyle,
    FlexboxItemStyle, LengthPercentage, LengthPercentageAuto, Point, Rect, Size,
};

use super::focus::Direction;
use crate::draw_list::CornerRadii;
use crate::font::layout::TextAlign;
use crate::font::{FamilyName, Font, FontFamily};
use crate::widget::NATURAL_FONT_SIZE;

/// A length that cannot be `auto`: padding and gaps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// Pixels.
    Px(f32),
    /// A fraction of the containing block's width (for padding, on both axes,
    /// as CSS resolves it) or of the gap's own axis: **`0.5` is 50%**, the
    /// convention Taffy and the Chrome-generated fixtures share.
    Percent(f32),
}

/// A length that may be `auto`: sizes, margins, offsets and the flex basis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LengthAuto {
    /// Pixels.
    Px(f32),
    /// A fraction of the containing block: **`0.5` is 50%**; see
    /// [`Length::Percent`].
    Percent(f32),
    /// Decided by the layout: content-sized for a size, free space for a
    /// margin, and no offset for an inset.
    Auto,
}

/// One value per side of a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges<T> {
    /// The top side.
    pub top: T,
    /// The right side.
    pub right: T,
    /// The bottom side.
    pub bottom: T,
    /// The left side.
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// The same value on every side.
    #[must_use]
    pub const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// The four sides, clockwise from the top.
    const fn sides(self) -> [T; 4] {
        [self.top, self.right, self.bottom, self.left]
    }

    /// Each side through `map`, into Taffy's left-right-top-bottom order.
    fn to_rect<U>(self, map: impl Fn(T) -> U) -> Rect<U> {
        Rect {
            left: map(self.left),
            right: map(self.right),
            top: map(self.top),
            bottom: map(self.bottom),
        }
    }
}

/// Whether a node generates a box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Display {
    /// A flex container, and a box in its parent's flow.
    #[default]
    Flex,
    /// No box: the node and everything under it take no space, draw nothing
    /// and are never hit.
    None,
}

/// `flex-direction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    /// Children left to right.
    #[default]
    Row,
    /// Children top to bottom.
    Column,
    /// Children right to left.
    RowReverse,
    /// Children bottom to top.
    ColumnReverse,
}

/// `flex-wrap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    /// One line, whatever it overflows.
    #[default]
    NoWrap,
    /// As many lines as the main size needs, stacked along the cross axis.
    Wrap,
    /// As many lines as needed, stacked the other way.
    WrapReverse,
}

/// `align-items` and `align-self`: where a child sits on the cross axis.
///
/// No `baseline`: nothing in the subset has a baseline to align to until real
/// fonts do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    /// The cross axis's start.
    Start,
    /// The cross axis's end.
    End,
    /// The flex-relative start, which a `wrap-reverse` container flips.
    FlexStart,
    /// The flex-relative end.
    FlexEnd,
    /// Centred.
    Center,
    /// Stretched to the line, unless the child has a cross size.
    Stretch,
}

/// `justify-content`: how free space on the main axis is shared out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Justify {
    /// Packed at the main axis's start.
    Start,
    /// Packed at its end.
    End,
    /// Packed at the flex-relative start, which a reversed direction flips.
    FlexStart,
    /// Packed at the flex-relative end.
    FlexEnd,
    /// Packed in the middle.
    Center,
    /// First and last flush with the edges, the rest spaced evenly.
    SpaceBetween,
    /// Half a gap before the first and after the last.
    SpaceAround,
    /// The same gap everywhere, edges included.
    SpaceEvenly,
}

/// `position`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Position {
    /// In the parent's flow, nudged by its offsets.
    #[default]
    Relative,
    /// Out of the flow, placed by its offsets against the parent's padding
    /// box.
    Absolute,
}

/// `overflow`, on both axes at once.
///
/// `scroll` reserves no scrollbar gutter — nothing draws a scrollbar yet, and
/// Taffy's gutter is `scrollbar_width`, which this style leaves at zero — so it
/// lays out exactly as `hidden` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Overflow {
    /// Children may draw outside the box.
    #[default]
    Visible,
    /// Children are clipped to the padding box, and the box's automatic
    /// minimum size as a flex item is zero rather than its content. A scroll
    /// offset the caller sets is applied as given.
    Hidden,
    /// As `Hidden`, and the box is a scroll container the tree manages: its
    /// offset is clamped to how far its content reaches, and it scrolls the
    /// focused node inside it into view — see [`crate::tree`]'s focus docs.
    Scroll,
}

impl Overflow {
    /// Whether children are clipped to the padding box.
    #[must_use]
    pub const fn clips(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll)
    }
}

/// `line-height`: the pitch between a text span's lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    /// The font's own: ascent to descent plus its line gap. The bitmap font's
    /// is its fixed [`crate::text::LINE_HEIGHT`] at the span's scale.
    #[default]
    Normal,
    /// This many times the font size. Inherited as the number, as CSS does, so
    /// a child with a larger font gets a larger pitch.
    Multiple(f32),
    /// This many pixels.
    Px(f32),
}

impl LineHeight {
    /// The pitch in pixels at `font_size`, given what `normal` is there.
    #[must_use]
    pub fn resolve(self, font_size: f32, normal: f32) -> f32 {
        match self {
            Self::Normal => normal,
            Self::Multiple(factor) => factor * font_size,
            Self::Px(px) => px,
        }
    }

    /// A tag and the bits of its number, for hashing and interning;
    /// [`LineHeight::from_bits`] is the way back.
    pub(crate) fn bits(self) -> (u8, u32) {
        match self {
            Self::Normal => (0, 0),
            Self::Multiple(factor) => (1, factor.to_bits()),
            Self::Px(px) => (2, px.to_bits()),
        }
    }

    /// The value [`LineHeight::bits`] made `bits` from.
    pub(crate) fn from_bits((tag, bits): (u8, u32)) -> Self {
        match tag {
            1 => Self::Multiple(f32::from_bits(bits)),
            2 => Self::Px(f32::from_bits(bits)),
            _ => Self::Normal,
        }
    }
}

/// An id a `nav-*` property names, hashed: what a node's `#id` is compared
/// against when a directional move follows the property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavId(pub(crate) u64);

impl NavId {
    /// The id `name` — without its `#`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(name, &mut hasher);
        Self(hasher.finish())
    }
}

/// A picture a stylesheet's `url()` names, hashed: what [`Ui::set_image`]
/// binds to a registered [`AtlasImage`](crate::image::AtlasImage).
///
/// A name, never a path — see [`crate::style`]'s image notes.
///
/// [`Ui::set_image`]: crate::tree::Ui::set_image
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageName(pub(crate) u64);

impl ImageName {
    /// The name `name`, as the `url()` that names it spells it.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(name, &mut hasher);
        Self(hasher.finish())
    }
}

/// One side of `border-image-width`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderImageWidth {
    /// This many pixels.
    Px(f32),
    /// This many times the side's `border-width`; the initial value is `1`.
    Multiple(f32),
}

impl BorderImageWidth {
    /// The band's width in pixels on a side whose `border-width` is `border`.
    #[must_use]
    pub fn resolve(self, border: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Multiple(factor) => factor * border,
        }
    }
}

/// `border-image-source`, `border-image-slice` and `border-image-width`: a
/// picture cut into nine and drawn over the border box in place of the border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderImage {
    /// The picture; `None` draws the border as `border-color` instead.
    pub source: Option<ImageName>,
    /// Where the picture is cut, inward from each edge in **texels**.
    pub slice: Edges<f32>,
    /// Whether the middle of the picture is drawn over the padding box too:
    /// the `fill` keyword.
    pub fill: bool,
    /// How wide each band is drawn.
    pub width: Edges<BorderImageWidth>,
}

impl BorderImage {
    /// The initial value: no picture, no fill and a width of `1`. The slice
    /// is `0` where CSS's is `100%`: the subset takes no percentage, and with
    /// no picture the slice cuts nothing either way.
    pub const NONE: Self = Self {
        source: None,
        slice: Edges::all(0.0),
        fill: false,
        width: Edges::all(BorderImageWidth::Multiple(1.0)),
    };
}

/// `nav-up`, `nav-right`, `nav-down` and `nav-left`: where a directional move
/// from the node goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NavTarget {
    /// Wherever the spatial search finds.
    #[default]
    Auto,
    /// Nowhere: the move leaves focus where it is.
    None,
    /// The first focusable node in tree order whose `#id` this is; the spatial
    /// search when there is none.
    Id(NavId),
}

/// `nav-wrap`, on a container: which axes a directional move that finds
/// nothing inside it wraps round to the container's far side on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NavWrap {
    /// Neither: the move leaves the container, or stays put.
    #[default]
    None,
    /// Left and right, as a carousel.
    Horizontal,
    /// Up and down.
    Vertical,
    /// Both, as a grid.
    Both,
}

impl NavWrap {
    /// Whether a move along the horizontal axis (`horizontal`) or the
    /// vertical one wraps.
    #[must_use]
    pub const fn wraps(self, horizontal: bool) -> bool {
        match self {
            Self::None => false,
            Self::Horizontal => horizontal,
            Self::Vertical => !horizontal,
            Self::Both => true,
        }
    }
}

/// Everything one node is laid out and painted with.
///
/// A plain struct: UI rung 4's cascade is what produces one from a
/// stylesheet. [`NodeStyle::DEFAULT`] is CSS's initial
/// value for every layout field but `display`, which is `flex` here because the
/// subset has no other layout mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeStyle {
    // -- layout ---------------------------------------------------------------
    /// `display`.
    pub display: Display,
    /// `position`.
    pub position: Position,
    /// `top`, `right`, `bottom` and `left`.
    pub inset: Edges<LengthAuto>,
    /// `width`, of the border box.
    pub width: LengthAuto,
    /// `height`, of the border box.
    pub height: LengthAuto,
    /// `min-width`.
    pub min_width: LengthAuto,
    /// `min-height`.
    pub min_height: LengthAuto,
    /// `max-width`.
    pub max_width: LengthAuto,
    /// `max-height`.
    pub max_height: LengthAuto,
    /// `margin`.
    pub margin: Edges<LengthAuto>,
    /// `padding`.
    pub padding: Edges<Length>,
    /// `border-width`, in pixels. Laid out on every side as given; see
    /// [`crate::tree`]'s emission notes for how it is painted.
    pub border: Edges<f32>,
    /// `overflow`.
    pub overflow: Overflow,
    /// `flex-direction`.
    pub flex_direction: FlexDirection,
    /// `flex-wrap`.
    pub flex_wrap: FlexWrap,
    /// `justify-content`; `None` is `normal`, which packs at the start.
    pub justify_content: Option<Justify>,
    /// `align-items`; `None` is `normal`, which stretches.
    pub align_items: Option<Align>,
    /// `align-self`; `None` is `auto`, which takes the parent's `align-items`.
    pub align_self: Option<Align>,
    /// `flex-grow`.
    pub flex_grow: f32,
    /// `flex-shrink`.
    pub flex_shrink: f32,
    /// `flex-basis`.
    pub flex_basis: LengthAuto,
    /// `column-gap`: between children on a row.
    pub column_gap: Length,
    /// `row-gap`: between children in a column, and between wrapped lines.
    pub row_gap: Length,

    // -- paint ----------------------------------------------------------------
    /// What the border box is filled with. Nothing is drawn while its alpha is
    /// zero.
    pub background: [f32; 4],
    /// What the border is drawn in.
    pub border_color: [f32; 4],
    /// Corner radii; all zero is a square box.
    pub radii: CornerRadii,
    /// What a text span is drawn in, and what an image span is tinted by.
    pub color: [f32; 4],
    /// A text span's size in pixels, on [`crate::draw_list::DrawCommand::Text`]'s
    /// terms.
    pub font_size: f32,
    /// Which built-in font a text span draws in: the first built-in family its
    /// `font-family` list names. Inherited.
    pub font_family: FontFamily,
    /// The family its `font-family` list names ahead of its first built-in
    /// one, if any: a span draws in the font
    /// [`Ui::register_font`](crate::tree::Ui::register_font) registered under
    /// it, and in `font_family` while none is. Inherited.
    pub family_name: Option<FamilyName>,
    /// The pitch of a text span's lines in the parsed font; the bitmap font's
    /// is fixed. Inherited.
    pub line_height: LineHeight,
    /// Where a text span's lines sit across its content box. Inherited.
    pub text_align: TextAlign,
    /// `outline-width`, in pixels: a ring drawn outside the border box that
    /// takes no space. Nothing is drawn while it is zero.
    pub outline_width: f32,
    /// `outline-color`. Nothing is drawn while its alpha is zero.
    pub outline_color: [f32; 4],
    /// `outline-offset`, in pixels: how far outside the border box the ring
    /// starts; negative draws it inside.
    pub outline_offset: f32,
    /// `background-image`: a picture stretched over the padding box, drawn
    /// over `background`.
    pub background_image: Option<ImageName>,
    /// `border-image-*`: a nine-sliced picture over the border box, drawn in
    /// place of `border-color` while its source is set.
    pub border_image: BorderImage,

    // -- navigation -----------------------------------------------------------
    /// `nav-up`.
    pub nav_up: NavTarget,
    /// `nav-right`.
    pub nav_right: NavTarget,
    /// `nav-down`.
    pub nav_down: NavTarget,
    /// `nav-left`.
    pub nav_left: NavTarget,
    /// `nav-wrap`.
    pub nav_wrap: NavWrap,
}

impl NodeStyle {
    /// The initial style: see the type's docs.
    pub const DEFAULT: Self = Self {
        display: Display::Flex,
        position: Position::Relative,
        inset: Edges::all(LengthAuto::Auto),
        width: LengthAuto::Auto,
        height: LengthAuto::Auto,
        min_width: LengthAuto::Auto,
        min_height: LengthAuto::Auto,
        max_width: LengthAuto::Auto,
        max_height: LengthAuto::Auto,
        margin: Edges::all(LengthAuto::Px(0.0)),
        padding: Edges::all(Length::Px(0.0)),
        border: Edges::all(0.0),
        overflow: Overflow::Visible,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        justify_content: None,
        align_items: None,
        align_self: None,
        flex_grow: 0.0,
        flex_shrink: 1.0,
        flex_basis: LengthAuto::Auto,
        column_gap: Length::Px(0.0),
        row_gap: Length::Px(0.0),
        background: [0.0; 4],
        border_color: [0.0; 4],
        radii: CornerRadii::uniform(0.0),
        color: [1.0; 4],
        font_size: NATURAL_FONT_SIZE,
        font_family: FontFamily::Bitmap,
        family_name: None,
        line_height: LineHeight::Normal,
        text_align: TextAlign::Left,
        outline_width: 0.0,
        outline_color: [0.0; 4],
        outline_offset: 0.0,
        background_image: None,
        border_image: BorderImage::NONE,
        nav_up: NavTarget::Auto,
        nav_right: NavTarget::Auto,
        nav_down: NavTarget::Auto,
        nav_left: NavTarget::Auto,
        nav_wrap: NavWrap::None,
    };

    /// Folds every layout field into `state`, and no paint or navigation
    /// field.
    ///
    /// Floats go in by their bits, so `0.0` and `-0.0` hash apart — a spurious
    /// relayout at worst, never a missed one. The text fields that size a
    /// span — its font, size and line height — are not here: they move a text
    /// span's box through its content hash, which the tree folds them into.
    pub fn layout_hash(&self, state: &mut impl Hasher) {
        fn length(state: &mut impl Hasher, value: Length) {
            match value {
                Length::Px(px) => {
                    state.write_u8(0);
                    state.write_u32(px.to_bits());
                }
                Length::Percent(fraction) => {
                    state.write_u8(1);
                    state.write_u32(fraction.to_bits());
                }
            }
        }
        fn length_auto(state: &mut impl Hasher, value: LengthAuto) {
            match value {
                LengthAuto::Px(px) => {
                    state.write_u8(0);
                    state.write_u32(px.to_bits());
                }
                LengthAuto::Percent(fraction) => {
                    state.write_u8(1);
                    state.write_u32(fraction.to_bits());
                }
                LengthAuto::Auto => state.write_u8(2),
            }
        }
        state.write_u8(self.display as u8);
        state.write_u8(self.position as u8);
        for side in self.inset.sides() {
            length_auto(state, side);
        }
        for size in [
            self.width,
            self.height,
            self.min_width,
            self.min_height,
            self.max_width,
            self.max_height,
            self.flex_basis,
        ] {
            length_auto(state, size);
        }
        for side in self.margin.sides() {
            length_auto(state, side);
        }
        for side in self.padding.sides() {
            length(state, side);
        }
        for side in self.border.sides() {
            state.write_u32(side.to_bits());
        }
        state.write_u8(self.overflow as u8);
        state.write_u8(self.flex_direction as u8);
        state.write_u8(self.flex_wrap as u8);
        state.write_u8(
            self.justify_content
                .map_or(u8::MAX, |justify| justify as u8),
        );
        state.write_u8(self.align_items.map_or(u8::MAX, |align| align as u8));
        state.write_u8(self.align_self.map_or(u8::MAX, |align| align as u8));
        state.write_u32(self.flex_grow.to_bits());
        state.write_u32(self.flex_shrink.to_bits());
        length(state, self.column_gap);
        length(state, self.row_gap);
    }
}

impl NodeStyle {
    /// The `nav-*` property for a move in `direction`.
    #[must_use]
    pub const fn nav(&self, direction: Direction) -> NavTarget {
        match direction {
            Direction::Up => self.nav_up,
            Direction::Right => self.nav_right,
            Direction::Down => self.nav_down,
            Direction::Left => self.nav_left,
        }
    }

    /// The `nav-*` property for a move in `direction`, to set.
    pub const fn nav_mut(&mut self, direction: Direction) -> &mut NavTarget {
        match direction {
            Direction::Up => &mut self.nav_up,
            Direction::Right => &mut self.nav_right,
            Direction::Down => &mut self.nav_down,
            Direction::Left => &mut self.nav_left,
        }
    }

    /// A text span's line pitch in `font`, the parsed font its family names.
    #[must_use]
    pub fn text_line_height(&self, font: &Font) -> f32 {
        self.line_height.resolve(
            self.font_size,
            font.metrics().normal_line_height(self.font_size),
        )
    }
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// The mapping onto Taffy's style traits
// ---------------------------------------------------------------------------

const fn length_percentage(value: Length) -> LengthPercentage {
    match value {
        Length::Px(px) => LengthPercentage::length(px),
        Length::Percent(fraction) => LengthPercentage::percent(fraction),
    }
}

const fn length_percentage_auto(value: LengthAuto) -> LengthPercentageAuto {
    match value {
        LengthAuto::Px(px) => LengthPercentageAuto::length(px),
        LengthAuto::Percent(fraction) => LengthPercentageAuto::percent(fraction),
        LengthAuto::Auto => LengthPercentageAuto::auto(),
    }
}

const fn dimension(value: LengthAuto) -> Dimension {
    match value {
        LengthAuto::Px(px) => Dimension::length(px),
        LengthAuto::Percent(fraction) => Dimension::percent(fraction),
        LengthAuto::Auto => Dimension::auto(),
    }
}

const fn align_items(value: Align) -> AlignItems {
    match value {
        Align::Start => AlignItems::START,
        Align::End => AlignItems::END,
        Align::FlexStart => AlignItems::FLEX_START,
        Align::FlexEnd => AlignItems::FLEX_END,
        Align::Center => AlignItems::CENTER,
        Align::Stretch => AlignItems::STRETCH,
    }
}

const fn justify_content(value: Justify) -> AlignContent {
    match value {
        Justify::Start => AlignContent::START,
        Justify::End => AlignContent::END,
        Justify::FlexStart => AlignContent::FLEX_START,
        Justify::FlexEnd => AlignContent::FLEX_END,
        Justify::Center => AlignContent::CENTER,
        Justify::SpaceBetween => AlignContent::SPACE_BETWEEN,
        Justify::SpaceAround => AlignContent::SPACE_AROUND,
        Justify::SpaceEvenly => AlignContent::SPACE_EVENLY,
    }
}

impl CoreStyle for NodeStyle {
    /// Names grid lines and areas, which the subset has none of; `String`
    /// because Taffy's trait bound asks for an owned string type.
    type CustomIdent = String;

    fn box_generation_mode(&self) -> BoxGenerationMode {
        match self.display {
            Display::Flex => BoxGenerationMode::Normal,
            Display::None => BoxGenerationMode::None,
        }
    }

    fn overflow(&self) -> Point<taffy::Overflow> {
        let overflow = match self.overflow {
            Overflow::Visible => taffy::Overflow::Visible,
            Overflow::Hidden => taffy::Overflow::Hidden,
            Overflow::Scroll => taffy::Overflow::Scroll,
        };
        Point {
            x: overflow,
            y: overflow,
        }
    }

    fn position(&self) -> taffy::Position {
        match self.position {
            Position::Relative => taffy::Position::Relative,
            Position::Absolute => taffy::Position::Absolute,
        }
    }

    fn inset(&self) -> Rect<LengthPercentageAuto> {
        self.inset.to_rect(length_percentage_auto)
    }

    fn size(&self) -> Size<Dimension> {
        Size {
            width: dimension(self.width),
            height: dimension(self.height),
        }
    }

    fn min_size(&self) -> Size<LengthPercentageAuto> {
        Size {
            width: length_percentage_auto(self.min_width),
            height: length_percentage_auto(self.min_height),
        }
    }

    fn max_size(&self) -> Size<LengthPercentageAuto> {
        Size {
            width: length_percentage_auto(self.max_width),
            height: length_percentage_auto(self.max_height),
        }
    }

    fn margin(&self) -> Rect<LengthPercentageAuto> {
        self.margin.to_rect(length_percentage_auto)
    }

    fn padding(&self) -> Rect<LengthPercentage> {
        self.padding.to_rect(length_percentage)
    }

    fn border(&self) -> Rect<LengthPercentage> {
        self.border.to_rect(LengthPercentage::length)
    }
}

impl FlexboxContainerStyle for NodeStyle {
    fn flex_direction(&self) -> taffy::FlexDirection {
        match self.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        }
    }

    fn flex_wrap(&self) -> taffy::FlexWrap {
        match self.flex_wrap {
            FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
            FlexWrap::Wrap => taffy::FlexWrap::Wrap,
            FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        }
    }

    fn gap(&self) -> Size<LengthPercentage> {
        Size {
            width: length_percentage(self.column_gap),
            height: length_percentage(self.row_gap),
        }
    }

    fn align_items(&self) -> Option<AlignItems> {
        self.align_items.map(align_items)
    }

    fn justify_content(&self) -> Option<AlignContent> {
        self.justify_content.map(justify_content)
    }
}

impl FlexboxItemStyle for NodeStyle {
    fn flex_basis(&self) -> Dimension {
        dimension(self.flex_basis)
    }

    fn flex_grow(&self) -> f32 {
        self.flex_grow
    }

    fn flex_shrink(&self) -> f32 {
        self.flex_shrink
    }

    fn align_self(&self) -> Option<AlignItems> {
        self.align_self.map(align_items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::DefaultHasher;

    fn layout_hash(style: &NodeStyle) -> u64 {
        let mut hasher = DefaultHasher::new();
        style.layout_hash(&mut hasher);
        hasher.finish()
    }

    /// **A paint field never moves the layout hash, and a layout field always
    /// does** — the split the cache invalidation rests on.
    #[test]
    fn the_layout_hash_sees_layout_fields_and_ignores_paint_fields() {
        let base = NodeStyle::DEFAULT;
        let painted = NodeStyle {
            background: [1.0, 0.0, 0.0, 1.0],
            border_color: [0.0, 1.0, 0.0, 1.0],
            radii: CornerRadii::uniform(4.0),
            color: [0.5; 4],
            outline_width: 2.0,
            outline_color: [1.0; 4],
            outline_offset: 1.0,
            background_image: Some(ImageName::new("x")),
            border_image: BorderImage {
                source: Some(ImageName::new("y")),
                slice: Edges::all(4.0),
                fill: true,
                width: Edges::all(BorderImageWidth::Px(3.0)),
            },
            nav_up: NavTarget::None,
            nav_left: NavTarget::Id(NavId::new("x")),
            nav_wrap: NavWrap::Both,
            ..base
        };
        assert_eq!(layout_hash(&base), layout_hash(&painted));

        let moved: [NodeStyle; 8] = [
            NodeStyle {
                width: LengthAuto::Px(10.0),
                ..base
            },
            NodeStyle {
                padding: Edges {
                    left: Length::Px(1.0),
                    ..base.padding
                },
                ..base
            },
            NodeStyle {
                inset: Edges {
                    bottom: LengthAuto::Percent(0.5),
                    ..base.inset
                },
                ..base
            },
            NodeStyle {
                border: Edges {
                    right: 2.0,
                    ..base.border
                },
                ..base
            },
            NodeStyle {
                display: Display::None,
                ..base
            },
            NodeStyle {
                align_self: Some(Align::Center),
                ..base
            },
            NodeStyle {
                row_gap: Length::Percent(0.1),
                ..base
            },
            NodeStyle {
                flex_shrink: 0.0,
                ..base
            },
        ];
        for style in moved {
            assert_ne!(layout_hash(&base), layout_hash(&style), "{style:?}");
        }
    }

    /// Every side lands on Taffy's side of the same name.
    #[test]
    fn edges_map_onto_the_sides_of_the_same_name() {
        let style = NodeStyle {
            border: Edges {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            ..NodeStyle::DEFAULT
        };
        let rect = CoreStyle::border(&style);
        assert_eq!(
            [rect.top, rect.right, rect.bottom, rect.left],
            [1.0, 2.0, 3.0, 4.0].map(LengthPercentage::length)
        );
    }
}
