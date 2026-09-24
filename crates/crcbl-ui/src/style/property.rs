//! Properties by name, and the CSS grammar of each one's value.
//!
//! Every property [`NodeStyle`] has, and nothing it does not:
//!
//! | property | value |
//! |---|---|
//! | `display` | `flex` \| `none` |
//! | `position` | `relative` \| `absolute` |
//! | `top` `right` `bottom` `left`, `inset` (1–4) | length, `auto` |
//! | `width` `height` `min-width` `min-height` | length, `auto` |
//! | `max-width` `max-height` | length, `auto`, `none` |
//! | `margin` (1–4), `margin-*` | length, `auto` |
//! | `padding` (1–4), `padding-*` | length |
//! | `border-width` (1–4), `border-*-width` | px |
//! | `overflow` | `visible` \| `hidden` \| `scroll` |
//! | `flex-direction` | `row` \| `column` \| `row-reverse` \| `column-reverse` |
//! | `flex-wrap` | `nowrap` \| `wrap` \| `wrap-reverse` |
//! | `justify-content` | `normal` `start` `end` `flex-start` `flex-end` `center` `space-between` `space-around` `space-evenly` |
//! | `align-items`, `align-self` | `normal` (`auto` for `align-self`) `stretch` `start` `end` `flex-start` `flex-end` `center` |
//! | `flex` | `none` \| `auto` \| grow, shrink?, basis? \| basis |
//! | `flex-grow` `flex-shrink` | number |
//! | `flex-basis` | length, `auto` |
//! | `gap` (1–2), `row-gap` `column-gap` | length |
//! | `background` `background-color` `border-color` `color` | colour |
//! | `border-radius` (1–4), `border-*-*-radius` | px |
//! | `font-size` | px |
//! | `font-family` | a comma-separated list; the first of `bitmap`, `sans-serif` and `"Atkinson Hyperlegible"` in it is used, after the first registered font named ahead of it |
//! | `line-height` | `normal` \| number \| px |
//! | `text-align` | `left` \| `start` \| `center` \| `right` \| `end` |
//! | `outline-width` | px |
//! | `outline-offset` | px, either sign |
//! | `outline-color` | colour |
//! | `outline` | `none` \| a px width and a colour, in either order |
//! | `nav-up` `nav-right` `nav-down` `nav-left` | `auto` \| `none` \| `#id` |
//! | `nav-wrap` | `none` \| `horizontal` \| `vertical` \| `both` |
//! | `background-image` | `none` \| an image |
//! | `border-image-source` | `none` \| an image |
//! | `border-image-slice` | 1–4 numbers, and `fill` before or after them |
//! | `border-image-width` | 1–4 of px or number |
//! | `border-image-repeat` | `stretch`, once or twice |
//! | `border-image` | source, slice with an optional `/ width`, and repeat, in any order |
//!
//! A **length** is `<n>px`, `<n>%` or a unitless `0`. A **colour** is `#rgb`,
//! `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` in either the comma or the
//! space syntax, `color(srgb …)` or `color(srgb-linear …)` with an optional
//! `/ alpha`, `transparent`, or one of CSS's named colours (cssparser's
//! table). Every property also takes `initial` and `unset`.
//!
//! # Images are names, not files
//!
//! An **image** is `url(name)` or `url("name")`, and the name is looked up in
//! what the application bound with
//! [`Ui::set_image`](crate::tree::Ui::set_image) — a picture it registered in
//! an [`ImageAtlas`](crate::image::ImageAtlas) — when the node is drawn. Nothing
//! is ever loaded from a path, so a sheet cannot reach the file system, and a
//! name nothing bound draws nothing. A `background-image` is stretched over the
//! padding box, as `background-size: 100% 100%; background-repeat: no-repeat`
//! would place it: the draw list has a stretched quad and no tiling, so those
//! two properties are not in the subset.
//!
//! `border-image` is CSS Backgrounds 3's, cut down to what a nine-slice frame
//! needs. **The slice is in texels** and takes no percentage; `fill` draws the
//! middle. **The width** is a px length or a multiple of the side's
//! `border-width`, whose initial `1` makes the frame's bands the border itself —
//! so a frame drawn with `border-width` also lays its content out inside the
//! bands. `border-image-repeat` takes only `stretch`, the one mode the draw
//! list's nine-slice draws; `border-image-outset` and a percentage or `auto`
//! width are not in the subset. While a source is set the border is the
//! picture and `border-color` is not drawn, as CSS specifies.
//!
//! **Colours are sRGB, and the draw list is linear light**, so a colour is
//! decoded through the sRGB transfer function on the way in — `#808080` is
//! `0.2158`, the value that draws as `#808080` on the sRGB swapchain — and its
//! alpha is not.
//!
//! `flex: <grow>` and `flex: <grow> <shrink>` set a basis of `0px`, the
//! specification's zero; Chrome serialises it as `0%`, which resolves the same
//! against a definite container.
//!
//! `outline` has no style keyword: the ring is solid, and `none` is a zero
//! width. `nav-*` names an id the way a selector does, `#` and all; the
//! `nav-wrap` property is this engine's, not CSS's.
//!
//! # Registered fonts are found at layout, not here
//!
//! A sheet is parsed without knowing which fonts an application will register
//! with [`Ui::register_font`](crate::tree::Ui::register_font), so a
//! `font-family` list becomes two values: the first of the built-in families
//! it names, and the first other name ahead of that one — case-folded, see
//! [`FamilyName`]. The tree draws in the font registered under the name while
//! there is one, and in the built-in family otherwise. So a name nothing
//! registered draws exactly as an unknown name always did:
//! `font-family: Roboto, sans-serif` draws in `sans-serif` until `Roboto` is
//! registered.
//!
//! A list naming **no** built-in family — `font-family: Roboto` — is valid,
//! and while nothing is registered under the name it leaves the span in the
//! family it had without the declaration, as an invalid declaration would: the
//! family it inherited, or an earlier rule's. What used to be a parse
//! diagnostic for it is a warning instead: the first text span built in a name
//! nothing is registered under warns, once per tree and name, with or without
//! a built-in family after it. A list naming neither — only generic families
//! such as `serif`, CSS-wide keywords, or unquoted names starting with one — is
//! still invalid. Only the first non-built-in name is kept: in
//! `font-family: Roboto, Inter, sans-serif`, `Inter` is never looked up.
//! `line-height` takes no percentage.
//!
//! Not in the subset: `opacity` — the draw list has no group opacity to give
//! it, and multiplying each command's alpha is not what `opacity` means where
//! children overlap — `inherit`, `!important`, `em` and every other unit.

use cssparser::color::{parse_hash_color, parse_named_color};
use cssparser::{ParseError, Parser, Token, match_ignore_ascii_case};

use super::value::{Corners, Declaration, Sides};
use crate::font::is_reserved_family;
use crate::tree::{
    Align, BorderImage, BorderImageWidth, Direction, Display, Edges, FamilyName, FlexDirection,
    FlexWrap, FontFamily, ImageName, Justify, Length, LengthAuto, LineHeight, NavId, NavTarget,
    NavWrap, NodeStyle, Overflow, Position, TextAlign,
};

/// A property a stylesheet can name, longhand or shorthand.
///
/// One enum rather than two tables, so that [`Property::parse`] and
/// [`Property::copy`] — what `initial` and `unset` do — are each an exhaustive
/// match the compiler holds to the same list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Property {
    Display,
    Position,
    /// `inset` for `Sides::All`, else `top`, `right`, `bottom` or `left`.
    Inset(Sides),
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    Margin(Sides),
    Padding(Sides),
    BorderWidth(Sides),
    Overflow,
    FlexDirection,
    FlexWrap,
    JustifyContent,
    AlignItems,
    AlignSelf,
    Flex,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    Gap,
    RowGap,
    ColumnGap,
    Background,
    BorderColor,
    BorderRadius(Corners),
    Color,
    FontSize,
    FontFamily,
    LineHeight,
    TextAlign,
    Outline,
    OutlineWidth,
    OutlineColor,
    OutlineOffset,
    Nav(Direction),
    NavWrap,
    BackgroundImage,
    BorderImage,
    BorderImageSource,
    BorderImageSlice,
    BorderImageWidth,
    BorderImageRepeat,
}

/// The failure a value parser reports; the caller words the diagnostic.
pub(crate) type Invalid = ParseError<()>;

fn invalid<T>() -> Result<T, Invalid> {
    Err(ParseError::custom(()))
}

impl Property {
    /// The property `name` spells, ignoring ASCII case.
    pub fn from_name(name: &str) -> Option<Self> {
        use Sides::{All, Bottom, Left, Right, Top};
        Some(match_ignore_ascii_case! { name,
            "display" => Self::Display,
            "position" => Self::Position,
            "inset" => Self::Inset(All),
            "top" => Self::Inset(Top),
            "right" => Self::Inset(Right),
            "bottom" => Self::Inset(Bottom),
            "left" => Self::Inset(Left),
            "width" => Self::Width,
            "height" => Self::Height,
            "min-width" => Self::MinWidth,
            "min-height" => Self::MinHeight,
            "max-width" => Self::MaxWidth,
            "max-height" => Self::MaxHeight,
            "margin" => Self::Margin(All),
            "margin-top" => Self::Margin(Top),
            "margin-right" => Self::Margin(Right),
            "margin-bottom" => Self::Margin(Bottom),
            "margin-left" => Self::Margin(Left),
            "padding" => Self::Padding(All),
            "padding-top" => Self::Padding(Top),
            "padding-right" => Self::Padding(Right),
            "padding-bottom" => Self::Padding(Bottom),
            "padding-left" => Self::Padding(Left),
            "border-width" => Self::BorderWidth(All),
            "border-top-width" => Self::BorderWidth(Top),
            "border-right-width" => Self::BorderWidth(Right),
            "border-bottom-width" => Self::BorderWidth(Bottom),
            "border-left-width" => Self::BorderWidth(Left),
            "overflow" => Self::Overflow,
            "flex-direction" => Self::FlexDirection,
            "flex-wrap" => Self::FlexWrap,
            "justify-content" => Self::JustifyContent,
            "align-items" => Self::AlignItems,
            "align-self" => Self::AlignSelf,
            "flex" => Self::Flex,
            "flex-grow" => Self::FlexGrow,
            "flex-shrink" => Self::FlexShrink,
            "flex-basis" => Self::FlexBasis,
            "gap" => Self::Gap,
            "row-gap" => Self::RowGap,
            "column-gap" => Self::ColumnGap,
            "background" | "background-color" => Self::Background,
            "border-color" => Self::BorderColor,
            "border-radius" => Self::BorderRadius(Corners::All),
            "border-top-left-radius" => Self::BorderRadius(Corners::TopLeft),
            "border-top-right-radius" => Self::BorderRadius(Corners::TopRight),
            "border-bottom-right-radius" => Self::BorderRadius(Corners::BottomRight),
            "border-bottom-left-radius" => Self::BorderRadius(Corners::BottomLeft),
            "color" => Self::Color,
            "font-size" => Self::FontSize,
            "font-family" => Self::FontFamily,
            "line-height" => Self::LineHeight,
            "text-align" => Self::TextAlign,
            "outline" => Self::Outline,
            "outline-width" => Self::OutlineWidth,
            "outline-color" => Self::OutlineColor,
            "outline-offset" => Self::OutlineOffset,
            "nav-up" => Self::Nav(Direction::Up),
            "nav-right" => Self::Nav(Direction::Right),
            "nav-down" => Self::Nav(Direction::Down),
            "nav-left" => Self::Nav(Direction::Left),
            "nav-wrap" => Self::NavWrap,
            "background-image" => Self::BackgroundImage,
            "border-image" => Self::BorderImage,
            "border-image-source" => Self::BorderImageSource,
            "border-image-slice" => Self::BorderImageSlice,
            "border-image-width" => Self::BorderImageWidth,
            "border-image-repeat" => Self::BorderImageRepeat,
            _ => return None,
        })
    }

    /// Parses this property's value from the whole of `input` into `out`.
    ///
    /// # Errors
    ///
    /// When the value is not one this property takes. `out` may hold part of a
    /// shorthand by then; the caller discards it.
    pub fn parse<'i>(
        self,
        input: &mut Parser<'i>,
        out: &mut Vec<Declaration>,
    ) -> Result<(), Invalid> {
        use Declaration as D;
        input.parse_entirely(|input| {
            match self {
                Self::Display => out.push(D::Display(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "flex" => Display::Flex,
                        "none" => Display::None,
                        _ => return None,
                    })
                })?)),
                Self::Position => out.push(D::Position(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "relative" => Position::Relative,
                        "absolute" => Position::Absolute,
                        _ => return None,
                    })
                })?)),
                Self::Inset(sides) => {
                    four(
                        input,
                        sides,
                        |input| length_auto(input, Sign::Any),
                        D::Inset,
                        out,
                    )?;
                }
                Self::Width => out.push(D::Width(length_auto(input, Sign::NonNegative)?)),
                Self::Height => out.push(D::Height(length_auto(input, Sign::NonNegative)?)),
                Self::MinWidth => out.push(D::MinWidth(length_auto(input, Sign::NonNegative)?)),
                Self::MinHeight => out.push(D::MinHeight(length_auto(input, Sign::NonNegative)?)),
                Self::MaxWidth => out.push(D::MaxWidth(max_size(input)?)),
                Self::MaxHeight => out.push(D::MaxHeight(max_size(input)?)),
                Self::Margin(sides) => {
                    four(
                        input,
                        sides,
                        |input| length_auto(input, Sign::Any),
                        D::Margin,
                        out,
                    )?;
                }
                Self::Padding(sides) => four(input, sides, length, D::Padding, out)?,
                Self::BorderWidth(sides) => {
                    four(
                        input,
                        sides,
                        |input| px(input, Sign::NonNegative),
                        D::BorderWidth,
                        out,
                    )?;
                }
                Self::Overflow => out.push(D::Overflow(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "visible" => Overflow::Visible,
                        "hidden" => Overflow::Hidden,
                        "scroll" => Overflow::Scroll,
                        _ => return None,
                    })
                })?)),
                Self::FlexDirection => out.push(D::FlexDirection(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "row" => FlexDirection::Row,
                        "column" => FlexDirection::Column,
                        "row-reverse" => FlexDirection::RowReverse,
                        "column-reverse" => FlexDirection::ColumnReverse,
                        _ => return None,
                    })
                })?)),
                Self::FlexWrap => out.push(D::FlexWrap(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "nowrap" => FlexWrap::NoWrap,
                        "wrap" => FlexWrap::Wrap,
                        "wrap-reverse" => FlexWrap::WrapReverse,
                        _ => return None,
                    })
                })?)),
                Self::JustifyContent => out.push(D::JustifyContent(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "normal" => None,
                        "start" => Some(Justify::Start),
                        "end" => Some(Justify::End),
                        "flex-start" => Some(Justify::FlexStart),
                        "flex-end" => Some(Justify::FlexEnd),
                        "center" => Some(Justify::Center),
                        "space-between" => Some(Justify::SpaceBetween),
                        "space-around" => Some(Justify::SpaceAround),
                        "space-evenly" => Some(Justify::SpaceEvenly),
                        _ => return None,
                    })
                })?)),
                Self::AlignItems => {
                    out.push(D::AlignItems(keyword(input, |name| align(name, "normal"))?))
                }
                Self::AlignSelf => {
                    out.push(D::AlignSelf(keyword(input, |name| align(name, "auto"))?))
                }
                Self::Flex => flex(input, out)?,
                Self::FlexGrow => out.push(D::FlexGrow(number(input)?)),
                Self::FlexShrink => out.push(D::FlexShrink(number(input)?)),
                Self::FlexBasis => out.push(D::FlexBasis(length_auto(input, Sign::NonNegative)?)),
                Self::Gap => {
                    let row = length(input)?;
                    let column = if input.is_exhausted() {
                        row
                    } else {
                        length(input)?
                    };
                    out.extend([D::RowGap(row), D::ColumnGap(column)]);
                }
                Self::RowGap => out.push(D::RowGap(length(input)?)),
                Self::ColumnGap => out.push(D::ColumnGap(length(input)?)),
                Self::Background => out.push(D::Background(color(input)?)),
                Self::BorderColor => out.push(D::BorderColor(color(input)?)),
                Self::BorderRadius(Corners::All) => {
                    let [top_left, top_right, bottom_right, bottom_left] =
                        one_to_four(input, |input| px(input, Sign::NonNegative))?;
                    if [top_right, bottom_right, bottom_left]
                        .iter()
                        .all(|r| r.to_bits() == top_left.to_bits())
                    {
                        out.push(D::BorderRadius(Corners::All, top_left));
                    } else {
                        out.extend([
                            D::BorderRadius(Corners::TopLeft, top_left),
                            D::BorderRadius(Corners::TopRight, top_right),
                            D::BorderRadius(Corners::BottomRight, bottom_right),
                            D::BorderRadius(Corners::BottomLeft, bottom_left),
                        ]);
                    }
                }
                Self::BorderRadius(corner) => {
                    out.push(D::BorderRadius(corner, px(input, Sign::NonNegative)?));
                }
                Self::Color => out.push(D::Color(color(input)?)),
                Self::FontSize => out.push(D::FontSize(px(input, Sign::Positive)?)),
                Self::FontFamily => {
                    let (family, name) = font_family(input)?;
                    out.extend(family.map(D::FontFamily));
                    out.push(D::FamilyName(name));
                }
                Self::LineHeight => out.push(D::LineHeight(line_height(input)?)),
                Self::TextAlign => out.push(D::TextAlign(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "left" | "start" => TextAlign::Left,
                        "center" => TextAlign::Center,
                        "right" | "end" => TextAlign::Right,
                        _ => return None,
                    })
                })?)),
                Self::Outline => outline(input, out)?,
                Self::OutlineWidth => out.push(D::OutlineWidth(px(input, Sign::NonNegative)?)),
                Self::OutlineColor => out.push(D::OutlineColor(color(input)?)),
                Self::OutlineOffset => out.push(D::OutlineOffset(px(input, Sign::Any)?)),
                Self::Nav(direction) => out.push(D::Nav(direction, nav_target(input)?)),
                Self::NavWrap => out.push(D::NavWrap(keyword(input, |name| {
                    Some(match_ignore_ascii_case! { name,
                        "none" => NavWrap::None,
                        "horizontal" => NavWrap::Horizontal,
                        "vertical" => NavWrap::Vertical,
                        "both" => NavWrap::Both,
                        _ => return None,
                    })
                })?)),
                Self::BackgroundImage => out.push(D::BackgroundImage(image_or_none(input)?)),
                Self::BorderImageSource => out.push(D::BorderImageSource(image_or_none(input)?)),
                Self::BorderImageSlice => border_image_slice(input, out)?,
                Self::BorderImageWidth => {
                    four(
                        input,
                        Sides::All,
                        border_image_width,
                        D::BorderImageWidth,
                        out,
                    )?;
                }
                Self::BorderImageRepeat => border_image_repeat(input)?,
                Self::BorderImage => border_image(input, out)?,
            }
            Ok(())
        })
    }

    /// Copies every field this property sets from `from` to `to`: what
    /// `initial` does with [`NodeStyle::DEFAULT`] as `from`, and `unset` with
    /// the node's inherited starting point.
    pub fn copy(self, from: &NodeStyle, to: &mut NodeStyle) {
        fn sides<T: Copy>(from: Edges<T>, to: &mut Edges<T>, sides: Sides) {
            match sides {
                Sides::All => *to = from,
                Sides::Top => to.top = from.top,
                Sides::Right => to.right = from.right,
                Sides::Bottom => to.bottom = from.bottom,
                Sides::Left => to.left = from.left,
            }
        }
        match self {
            Self::Display => to.display = from.display,
            Self::Position => to.position = from.position,
            Self::Inset(which) => sides(from.inset, &mut to.inset, which),
            Self::Width => to.width = from.width,
            Self::Height => to.height = from.height,
            Self::MinWidth => to.min_width = from.min_width,
            Self::MinHeight => to.min_height = from.min_height,
            Self::MaxWidth => to.max_width = from.max_width,
            Self::MaxHeight => to.max_height = from.max_height,
            Self::Margin(which) => sides(from.margin, &mut to.margin, which),
            Self::Padding(which) => sides(from.padding, &mut to.padding, which),
            Self::BorderWidth(which) => sides(from.border, &mut to.border, which),
            Self::Overflow => to.overflow = from.overflow,
            Self::FlexDirection => to.flex_direction = from.flex_direction,
            Self::FlexWrap => to.flex_wrap = from.flex_wrap,
            Self::JustifyContent => to.justify_content = from.justify_content,
            Self::AlignItems => to.align_items = from.align_items,
            Self::AlignSelf => to.align_self = from.align_self,
            Self::Flex => {
                to.flex_grow = from.flex_grow;
                to.flex_shrink = from.flex_shrink;
                to.flex_basis = from.flex_basis;
            }
            Self::FlexGrow => to.flex_grow = from.flex_grow,
            Self::FlexShrink => to.flex_shrink = from.flex_shrink,
            Self::FlexBasis => to.flex_basis = from.flex_basis,
            Self::Gap => {
                to.row_gap = from.row_gap;
                to.column_gap = from.column_gap;
            }
            Self::RowGap => to.row_gap = from.row_gap,
            Self::ColumnGap => to.column_gap = from.column_gap,
            Self::Background => to.background = from.background,
            Self::BorderColor => to.border_color = from.border_color,
            Self::BorderRadius(corners) => match corners {
                Corners::All => to.radii = from.radii,
                Corners::TopLeft => to.radii.top_left = from.radii.top_left,
                Corners::TopRight => to.radii.top_right = from.radii.top_right,
                Corners::BottomRight => to.radii.bottom_right = from.radii.bottom_right,
                Corners::BottomLeft => to.radii.bottom_left = from.radii.bottom_left,
            },
            Self::Color => to.color = from.color,
            Self::FontSize => to.font_size = from.font_size,
            Self::FontFamily => {
                to.font_family = from.font_family;
                to.family_name = from.family_name;
            }
            Self::LineHeight => to.line_height = from.line_height,
            Self::TextAlign => to.text_align = from.text_align,
            Self::Outline => {
                to.outline_width = from.outline_width;
                to.outline_color = from.outline_color;
            }
            Self::OutlineWidth => to.outline_width = from.outline_width,
            Self::OutlineColor => to.outline_color = from.outline_color,
            Self::OutlineOffset => to.outline_offset = from.outline_offset,
            Self::Nav(direction) => *to.nav_mut(direction) = from.nav(direction),
            Self::NavWrap => to.nav_wrap = from.nav_wrap,
            Self::BackgroundImage => to.background_image = from.background_image,
            Self::BorderImage => to.border_image = from.border_image,
            Self::BorderImageSource => to.border_image.source = from.border_image.source,
            Self::BorderImageSlice => {
                to.border_image.slice = from.border_image.slice;
                to.border_image.fill = from.border_image.fill;
            }
            Self::BorderImageWidth => to.border_image.width = from.border_image.width,
            // `stretch` is the only value, so there is no field to copy.
            Self::BorderImageRepeat => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Value grammar
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sign {
    Any,
    NonNegative,
    Positive,
}

impl Sign {
    fn admits(self, value: f32) -> bool {
        value.is_finite()
            && match self {
                Self::Any => true,
                Self::NonNegative => value >= 0.0,
                Self::Positive => value > 0.0,
            }
    }
}

fn keyword<'i, T>(
    input: &mut Parser<'i>,
    pick: impl FnOnce(&str) -> Option<T>,
) -> Result<T, Invalid> {
    let name = input.expect_ident()?;
    pick(name).map_or_else(invalid, Ok)
}

fn align(name: &str, default: &str) -> Option<Option<Align>> {
    if name.eq_ignore_ascii_case(default) {
        return Some(None);
    }
    Some(Some(match_ignore_ascii_case! { name,
        "stretch" => Align::Stretch,
        "start" => Align::Start,
        "end" => Align::End,
        "flex-start" => Align::FlexStart,
        "flex-end" => Align::FlexEnd,
        "center" => Align::Center,
        _ => return None,
    }))
}

fn number(input: &mut Parser<'_>) -> Result<f32, Invalid> {
    let value = input.expect_number()?;
    if Sign::NonNegative.admits(value) {
        Ok(value)
    } else {
        invalid()
    }
}

/// `<n>px` or a unitless `0`.
fn px(input: &mut Parser<'_>, sign: Sign) -> Result<f32, Invalid> {
    let value = match *input.next()? {
        Token::Dimension {
            value, ref unit, ..
        } if unit.eq_ignore_ascii_case("px") => value,
        Token::Number { value: 0.0, .. } => 0.0,
        _ => return invalid(),
    };
    if sign.admits(value) {
        Ok(value)
    } else {
        invalid()
    }
}

/// `<n>px`, `<n>%` or `0`.
fn length(input: &mut Parser<'_>) -> Result<Length, Invalid> {
    match length_auto(input, Sign::NonNegative)? {
        LengthAuto::Px(value) => Ok(Length::Px(value)),
        LengthAuto::Percent(value) => Ok(Length::Percent(value)),
        LengthAuto::Auto => invalid(),
    }
}

/// `<n>px`, `<n>%`, `0` or `auto`.
fn length_auto(input: &mut Parser<'_>, sign: Sign) -> Result<LengthAuto, Invalid> {
    let value = match *input.next()? {
        Token::Ident(ref name) if name.eq_ignore_ascii_case("auto") => return Ok(LengthAuto::Auto),
        Token::Dimension {
            value, ref unit, ..
        } if unit.eq_ignore_ascii_case("px") => LengthAuto::Px(value),
        Token::Number { value: 0.0, .. } => LengthAuto::Px(0.0),
        // Already divided by a hundred: `50%` is `0.5`, the convention
        // `Length::Percent` shares.
        Token::Percentage { unit_value, .. } => LengthAuto::Percent(unit_value),
        _ => return invalid(),
    };
    let (LengthAuto::Px(number) | LengthAuto::Percent(number)) = value else {
        unreachable!("auto returned above");
    };
    if sign.admits(number) {
        Ok(value)
    } else {
        invalid()
    }
}

/// `max-width` and `max-height`, whose `none` is this crate's `auto`.
fn max_size(input: &mut Parser<'_>) -> Result<LengthAuto, Invalid> {
    if input
        .try_parse(|input| input.expect_ident_matching("none"))
        .is_ok()
    {
        return Ok(LengthAuto::Auto);
    }
    length_auto(input, Sign::NonNegative)
}

/// One to four values, in CSS's top, right, bottom, left order.
fn one_to_four<T: Copy>(
    input: &mut Parser<'_>,
    mut parse: impl FnMut(&mut Parser<'_>) -> Result<T, Invalid>,
) -> Result<[T; 4], Invalid> {
    let top = parse(input)?;
    let Ok(right) = input.try_parse(&mut parse) else {
        return Ok([top; 4]);
    };
    let Ok(bottom) = input.try_parse(&mut parse) else {
        return Ok([top, right, top, right]);
    };
    let Ok(left) = input.try_parse(&mut parse) else {
        return Ok([top, right, bottom, right]);
    };
    Ok([top, right, bottom, left])
}

/// A box property: one side's value, or the shorthand's one to four.
fn four<T: Copy + PartialEq>(
    input: &mut Parser<'_>,
    sides: Sides,
    parse: impl FnMut(&mut Parser<'_>) -> Result<T, Invalid>,
    declare: fn(Sides, T) -> Declaration,
    out: &mut Vec<Declaration>,
) -> Result<(), Invalid> {
    let mut parse = parse;
    if sides != Sides::All {
        out.push(declare(sides, parse(input)?));
        return Ok(());
    }
    let values = one_to_four(input, parse)?;
    if values.iter().all(|value| *value == values[0]) {
        out.push(declare(Sides::All, values[0]));
    } else {
        for (side, value) in [Sides::Top, Sides::Right, Sides::Bottom, Sides::Left]
            .into_iter()
            .zip(values)
        {
            out.push(declare(side, value));
        }
    }
    Ok(())
}

/// `flex`: `none`, `auto`, `<grow> <shrink>? <basis>?` or `<basis>`.
fn flex(input: &mut Parser<'_>, out: &mut Vec<Declaration>) -> Result<(), Invalid> {
    use Declaration as D;
    let (grow, shrink, basis) = if input
        .try_parse(|input| input.expect_ident_matching("none"))
        .is_ok()
    {
        (0.0, 0.0, LengthAuto::Auto)
    } else if input
        .try_parse(|input| input.expect_ident_matching("auto"))
        .is_ok()
    {
        (1.0, 1.0, LengthAuto::Auto)
    } else if let Ok(grow) = input.try_parse(number) {
        let shrink = input.try_parse(number).unwrap_or(1.0);
        let basis = if input.is_exhausted() {
            LengthAuto::Px(0.0)
        } else {
            length_auto(input, Sign::NonNegative)?
        };
        (grow, shrink, basis)
    } else {
        (1.0, 1.0, length_auto(input, Sign::NonNegative)?)
    };
    out.extend([
        D::FlexGrow(grow),
        D::FlexShrink(shrink),
        D::FlexBasis(basis),
    ]);
    Ok(())
}

/// `font-family`: the first built-in family in the list, and the first name
/// ahead of it that a registered font could answer to.
///
/// A name that is not built-in is skipped as it always was, and the first one
/// before any built-in family is kept besides, as the [`FamilyName`]
/// [`crate::tree::Ui::register_font`] looks fonts up by. One that is reserved —
/// a generic family, a CSS-wide keyword, or an unquoted run of words starting
/// with one — is skipped and not kept. Invalid when the list yields neither.
fn font_family(
    input: &mut Parser<'_>,
) -> Result<(Option<FontFamily>, Option<FamilyName>), Invalid> {
    let mut chosen = None;
    let mut registered = None;
    loop {
        let (family, name) = if let Ok(name) =
            input.try_parse(|input| input.expect_string().map(ToString::to_string))
        {
            let registrable = !is_reserved_family(&name);
            (named_family(&name), registrable.then_some(name))
        } else {
            let mut name = input.expect_ident()?.to_string();
            let registrable = !is_reserved_family(&name);
            let mut words = 1;
            while let Ok(word) = input.try_parse(|input| input.expect_ident_cloned()) {
                name.push(' ');
                name.push_str(&word);
                words += 1;
            }
            // The generic family is one bare identifier; quoted, or followed by
            // another word, it is a family name like any other.
            let family = if words == 1 && name.eq_ignore_ascii_case("sans-serif") {
                Some(FontFamily::Sans)
            } else {
                named_family(&name)
            };
            (family, registrable.then_some(name))
        };
        if chosen.is_none() {
            match family {
                Some(_) => chosen = family,
                None => registered = registered.or(name.map(|name| FamilyName::new(&name))),
            }
        }
        if input.is_exhausted() {
            break;
        }
        input.expect_comma()?;
    }
    if chosen.is_none() && registered.is_none() {
        return invalid();
    }
    Ok((chosen, registered))
}

/// The family a family name — quoted, or unquoted identifiers joined by
/// single spaces — names, if this engine has it.
fn named_family(name: &str) -> Option<FontFamily> {
    Some(match_ignore_ascii_case! { name,
        "bitmap" => FontFamily::Bitmap,
        "atkinson hyperlegible" => FontFamily::Sans,
        _ => return None,
    })
}

/// `outline`: `none`, or a width and a colour in either order.
fn outline(input: &mut Parser<'_>, out: &mut Vec<Declaration>) -> Result<(), Invalid> {
    use Declaration as D;
    if input
        .try_parse(|input| input.expect_ident_matching("none"))
        .is_ok()
    {
        out.extend([D::OutlineWidth(0.0), D::OutlineColor([0.0; 4])]);
        return Ok(());
    }
    let (width, colour) = if let Ok(width) = input.try_parse(|input| px(input, Sign::NonNegative)) {
        (width, color(input)?)
    } else {
        let colour = color(input)?;
        (px(input, Sign::NonNegative)?, colour)
    };
    out.extend([D::OutlineWidth(width), D::OutlineColor(colour)]);
    Ok(())
}

/// `nav-*`: `auto`, `none` or an id with its `#`.
fn nav_target(input: &mut Parser<'_>) -> Result<NavTarget, Invalid> {
    match *input.next()? {
        Token::Ident(ref name) if name.eq_ignore_ascii_case("auto") => Ok(NavTarget::Auto),
        Token::Ident(ref name) if name.eq_ignore_ascii_case("none") => Ok(NavTarget::None),
        Token::IDHash(ref id) => Ok(NavTarget::Id(NavId::new(id))),
        _ => invalid(),
    }
}

/// `line-height`: `normal`, a number or a pixel length.
fn line_height(input: &mut Parser<'_>) -> Result<LineHeight, Invalid> {
    match *input.next()? {
        Token::Ident(ref name) if name.eq_ignore_ascii_case("normal") => Ok(LineHeight::Normal),
        Token::Number { value, .. } if Sign::NonNegative.admits(value) => {
            Ok(LineHeight::Multiple(value))
        }
        Token::Dimension {
            value, ref unit, ..
        } if unit.eq_ignore_ascii_case("px") && Sign::NonNegative.admits(value) => {
            Ok(LineHeight::Px(value))
        }
        _ => invalid(),
    }
}

/// An image: `url(name)` or `url("name")`; see the module docs.
fn image(input: &mut Parser<'_>) -> Result<ImageName, Invalid> {
    let token = input.next()?.clone();
    match token {
        Token::UnquotedUrl(ref name) => Ok(ImageName::new(name)),
        Token::Function(ref name) if name.eq_ignore_ascii_case("url") => {
            input.parse_nested_block(|input| Ok(ImageName::new(&input.expect_string()?.clone())))
        }
        _ => invalid(),
    }
}

/// `none` or an image.
fn image_or_none(input: &mut Parser<'_>) -> Result<Option<ImageName>, Invalid> {
    if input
        .try_parse(|input| input.expect_ident_matching("none"))
        .is_ok()
    {
        return Ok(None);
    }
    image(input).map(Some)
}

/// `border-image-slice`: one to four numbers, with `fill` before or after.
fn border_image_slice(input: &mut Parser<'_>, out: &mut Vec<Declaration>) -> Result<(), Invalid> {
    let fill = |input: &mut Parser<'_>| {
        input
            .try_parse(|input| input.expect_ident_matching("fill"))
            .is_ok()
    };
    let before = fill(input);
    four(
        input,
        Sides::All,
        number,
        Declaration::BorderImageSlice,
        out,
    )?;
    let after = !before && fill(input);
    out.push(Declaration::BorderImageFill(before || after));
    Ok(())
}

/// One side of `border-image-width`: a number is a multiple of the border's
/// width — a unitless `0` included, as CSS reads it — and a px length is
/// pixels.
fn border_image_width(input: &mut Parser<'_>) -> Result<BorderImageWidth, Invalid> {
    if let Ok(factor) = input.try_parse(number) {
        return Ok(BorderImageWidth::Multiple(factor));
    }
    Ok(BorderImageWidth::Px(px(input, Sign::NonNegative)?))
}

/// The one `border-image-repeat` keyword the subset has.
fn stretch(input: &mut Parser<'_>) -> Result<(), Invalid> {
    Ok(input.expect_ident_matching("stretch")?)
}

/// `border-image-repeat`: `stretch` for one axis or both. It sets nothing,
/// because nothing else is accepted.
fn border_image_repeat(input: &mut Parser<'_>) -> Result<(), Invalid> {
    stretch(input)?;
    if !input.is_exhausted() {
        stretch(input)?;
    }
    Ok(())
}

/// `border-image`: a source, a slice with an optional `/ width`, and a
/// repeat, each at most once and in any order. What is left out goes back to
/// its initial value, as a shorthand does.
fn border_image(input: &mut Parser<'_>, out: &mut Vec<Declaration>) -> Result<(), Invalid> {
    use Declaration as D;
    let initial = BorderImage::NONE;
    let mut source = None;
    let mut slice = None;
    let mut repeats = 0;
    while !input.is_exhausted() {
        if source.is_none()
            && let Ok(found) = input.try_parse(image_or_none)
        {
            source = Some(found);
            continue;
        }
        if slice.is_none() {
            let mut parts = Vec::new();
            if input
                .try_parse(|input| border_image_slice(input, &mut parts))
                .is_ok()
            {
                if input.try_parse(|input| input.expect_delim('/')).is_ok() {
                    four(
                        input,
                        Sides::All,
                        border_image_width,
                        D::BorderImageWidth,
                        &mut parts,
                    )?;
                }
                slice = Some(parts);
                continue;
            }
        }
        if repeats == 0 && input.try_parse(stretch).is_ok() {
            repeats = 1;
            // A second `stretch`, straight after, names the other axis.
            if input.try_parse(stretch).is_ok() {
                repeats = 2;
            }
            continue;
        }
        return invalid();
    }
    if source.is_none() && slice.is_none() && repeats == 0 {
        return invalid();
    }
    out.push(D::BorderImageSource(source.flatten()));
    out.extend([
        D::BorderImageSlice(Sides::All, initial.slice.top),
        D::BorderImageFill(initial.fill),
        D::BorderImageWidth(Sides::All, initial.width.top),
    ]);
    out.extend(slice.unwrap_or_default());
    Ok(())
}

/// A colour, decoded to linear light; see the module docs.
fn color(input: &mut Parser<'_>) -> Result<[f32; 4], Invalid> {
    let token = input.next()?.clone();
    match token {
        Token::Hash(ref hex) | Token::IDHash(ref hex) => {
            let (r, g, b, alpha) =
                parse_hash_color(hex.as_bytes()).map_err(|()| ParseError::custom(()))?;
            Ok(from_srgb8(r, g, b, alpha))
        }
        Token::Ident(ref name) if name.eq_ignore_ascii_case("transparent") => Ok([0.0; 4]),
        Token::Ident(ref name) => {
            let (r, g, b) = parse_named_color(name).map_err(|()| ParseError::custom(()))?;
            Ok(from_srgb8(r, g, b, 1.0))
        }
        Token::Function(ref name)
            if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") =>
        {
            input.parse_nested_block(rgb)
        }
        Token::Function(ref name) if name.eq_ignore_ascii_case("color") => {
            input.parse_nested_block(color_function)
        }
        _ => invalid(),
    }
}

/// The inside of `rgb(…)`: three channels and an optional alpha, comma- or
/// space-separated, each channel `0..=255` or a percentage.
fn rgb(input: &mut Parser<'_>) -> Result<[f32; 4], Invalid> {
    fn channel(input: &mut Parser<'_>) -> Result<f32, Invalid> {
        Ok(match *input.next()? {
            Token::Number { value, .. } if value.is_finite() => (value / 255.0).clamp(0.0, 1.0),
            Token::Percentage { unit_value, .. } if unit_value.is_finite() => {
                unit_value.clamp(0.0, 1.0)
            }
            _ => return invalid(),
        })
    }
    fn alpha(input: &mut Parser<'_>) -> Result<f32, Invalid> {
        Ok(match *input.next()? {
            Token::Number { value, .. }
            | Token::Percentage {
                unit_value: value, ..
            } if value.is_finite() => value.clamp(0.0, 1.0),
            _ => return invalid(),
        })
    }
    let red = channel(input)?;
    let commas = input.try_parse(|input| input.expect_comma()).is_ok();
    let separator = |input: &mut Parser<'_>| -> Result<(), Invalid> {
        if commas {
            input.expect_comma()?;
        }
        Ok(())
    };
    let green = channel(input)?;
    separator(input)?;
    let blue = channel(input)?;
    let opacity = if input.is_exhausted() {
        1.0
    } else {
        if commas {
            input.expect_comma()?;
        } else {
            input.expect_delim('/')?;
        }
        alpha(input)?
    };
    Ok([
        srgb_to_linear(red),
        srgb_to_linear(green),
        srgb_to_linear(blue),
        opacity,
    ])
}

/// The inside of `color(…)`: `srgb` or `srgb-linear`, three channels each a
/// number in `0..=1` or a percentage, and an optional `/ alpha`.
///
/// `srgb-linear` is how a colour already in linear light — the draw list's
/// own space — is written without a round trip through eight-bit sRGB.
fn color_function(input: &mut Parser<'_>) -> Result<[f32; 4], Invalid> {
    fn unit(input: &mut Parser<'_>) -> Result<f32, Invalid> {
        Ok(match *input.next()? {
            Token::Number { value, .. }
            | Token::Percentage {
                unit_value: value, ..
            } if value.is_finite() => value.clamp(0.0, 1.0),
            _ => return invalid(),
        })
    }
    let linear = keyword(input, |name| {
        Some(match_ignore_ascii_case! { name,
            "srgb" => false,
            "srgb-linear" => true,
            _ => return None,
        })
    })?;
    let channels = [unit(input)?, unit(input)?, unit(input)?];
    let alpha = if input.is_exhausted() {
        1.0
    } else {
        input.expect_delim('/')?;
        unit(input)?
    };
    let [red, green, blue] = if linear {
        channels
    } else {
        channels.map(srgb_to_linear)
    };
    Ok([red, green, blue, alpha])
}

fn from_srgb8(r: u8, g: u8, b: u8, alpha: f32) -> [f32; 4] {
    let decode = |channel: u8| srgb_to_linear(f32::from(channel) / 255.0);
    [decode(r), decode(g), decode(b), alpha]
}

/// One sRGB-encoded channel in `0..=1` as linear light: IEC 61966-2-1's
/// transfer function, which is what an sRGB swapchain encodes back through.
fn srgb_to_linear(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::CornerRadii;

    fn parse(property: &str, css: &str) -> Result<Vec<Declaration>, ()> {
        let property = Property::from_name(property).unwrap_or_else(|| panic!("{property}"));
        let mut out = Vec::new();
        let mut input = Parser::new(css);
        property.parse(&mut input, &mut out).map_err(|_| ())?;
        Ok(out)
    }

    fn style_of(pairs: &[(&str, &str)]) -> NodeStyle {
        let mut style = NodeStyle::DEFAULT;
        for (property, css) in pairs {
            for declaration in parse(property, css).unwrap_or_else(|()| panic!("{property}: {css}"))
            {
                declaration.apply(&mut style);
            }
        }
        style
    }

    const fn px(value: f32) -> LengthAuto {
        LengthAuto::Px(value)
    }

    /// **Each shorthand expands to its sides in CSS's clockwise order** for one,
    /// two, three and four values — and a longhand after it overrides one side.
    #[test]
    fn box_shorthands_expand_clockwise_from_the_top() {
        let cases: [(&str, [f32; 4]); 4] = [
            ("1px", [1.0, 1.0, 1.0, 1.0]),
            ("1px 2px", [1.0, 2.0, 1.0, 2.0]),
            ("1px 2px 3px", [1.0, 2.0, 3.0, 2.0]),
            ("1px 2px 3px 4px", [1.0, 2.0, 3.0, 4.0]),
        ];
        for (css, [top, right, bottom, left]) in cases {
            let style = style_of(&[
                ("margin", css),
                ("padding", css),
                ("border-width", css),
                ("inset", css),
            ]);
            assert_eq!(
                style.margin,
                Edges {
                    top: px(top),
                    right: px(right),
                    bottom: px(bottom),
                    left: px(left)
                },
                "{css}"
            );
            assert_eq!(
                style.padding,
                Edges {
                    top: Length::Px(top),
                    right: Length::Px(right),
                    bottom: Length::Px(bottom),
                    left: Length::Px(left)
                },
                "{css}"
            );
            assert_eq!(
                style.border,
                Edges {
                    top,
                    right,
                    bottom,
                    left
                },
                "{css}"
            );
            assert_eq!(
                style.inset,
                Edges {
                    top: px(top),
                    right: px(right),
                    bottom: px(bottom),
                    left: px(left)
                },
                "{css}"
            );

            let style = style_of(&[("border-radius", css)]);
            assert_eq!(
                style.radii,
                CornerRadii {
                    top_left: top,
                    top_right: right,
                    bottom_right: bottom,
                    bottom_left: left
                },
                "border-radius: {css}"
            );
        }
        let style = style_of(&[("padding", "4px"), ("padding-left", "9px")]);
        assert_eq!(style.padding.left, Length::Px(9.0));
        assert_eq!(style.padding.top, Length::Px(4.0));
        let style = style_of(&[("margin", "auto 10%"), ("gap", "3px 5px")]);
        assert_eq!(
            (style.margin.top, style.margin.right),
            (LengthAuto::Auto, LengthAuto::Percent(0.1))
        );
        assert_eq!(
            (style.row_gap, style.column_gap),
            (Length::Px(3.0), Length::Px(5.0))
        );
        assert_eq!(style_of(&[("gap", "7px")]).column_gap, Length::Px(7.0));
    }

    /// **`flex` expands the way the specification's table does.**
    #[test]
    fn the_flex_shorthand_expands_per_the_specification() {
        let cases = [
            ("none", (0.0, 0.0, LengthAuto::Auto)),
            ("auto", (1.0, 1.0, LengthAuto::Auto)),
            ("2", (2.0, 1.0, px(0.0))),
            ("2 3", (2.0, 3.0, px(0.0))),
            ("2 3 10px", (2.0, 3.0, px(10.0))),
            ("2 10%", (2.0, 1.0, LengthAuto::Percent(0.1))),
            ("40px", (1.0, 1.0, px(40.0))),
            ("0 0 auto", (0.0, 0.0, LengthAuto::Auto)),
        ];
        for (css, (grow, shrink, basis)) in cases {
            let style = style_of(&[("flex", css)]);
            assert_eq!(
                (style.flex_grow, style.flex_shrink, style.flex_basis),
                (grow, shrink, basis),
                "flex: {css}"
            );
        }
    }

    /// **Every keyword and length form lands on the field it names**, and
    /// every value outside the grammar is refused.
    #[test]
    fn values_parse_into_the_field_they_name_and_bad_values_are_refused() {
        let style = style_of(&[
            ("display", "NONE"),
            ("position", "absolute"),
            ("width", "50%"),
            ("height", "0"),
            ("min-width", "auto"),
            ("max-height", "none"),
            ("max-width", "12.5px"),
            ("overflow", "hidden"),
            ("flex-direction", "column-reverse"),
            ("flex-wrap", "wrap-reverse"),
            ("justify-content", "space-evenly"),
            ("align-items", "normal"),
            ("align-self", "flex-end"),
            ("flex-basis", "auto"),
            ("font-size", "26px"),
            ("top", "-4px"),
            ("line-height", "1.25"),
            ("text-align", "CENTER"),
            ("outline-offset", "-2px"),
            ("nav-up", "#top"),
            ("nav-down", "NONE"),
            ("nav-left", "auto"),
            ("nav-wrap", "horizontal"),
        ]);
        assert_eq!(style.display, Display::None);
        assert_eq!(style.position, Position::Absolute);
        assert_eq!(style.width, LengthAuto::Percent(0.5));
        assert_eq!(style.height, px(0.0));
        assert_eq!(style.max_height, LengthAuto::Auto);
        assert_eq!(style.max_width, px(12.5));
        assert_eq!(style.overflow, Overflow::Hidden);
        assert_eq!(style.flex_direction, FlexDirection::ColumnReverse);
        assert_eq!(style.flex_wrap, FlexWrap::WrapReverse);
        assert_eq!(style.justify_content, Some(Justify::SpaceEvenly));
        assert_eq!(style.align_items, None);
        assert_eq!(style.align_self, Some(Align::FlexEnd));
        assert_eq!(style.font_size, 26.0);
        assert_eq!(style.inset.top, px(-4.0));
        assert_eq!(style.line_height, LineHeight::Multiple(1.25));
        assert_eq!(style.text_align, TextAlign::Center);
        assert_eq!(style.outline_offset, -2.0);
        assert_eq!(style.nav_up, NavTarget::Id(NavId::new("top")));
        assert_eq!(style.nav_down, NavTarget::None);
        assert_eq!(style.nav_left, NavTarget::Auto);
        assert_eq!(style.nav_right, NavTarget::Auto);
        assert_eq!(style.nav_wrap, NavWrap::Horizontal);
        assert_eq!(
            style_of(&[("overflow", "scroll")]).overflow,
            Overflow::Scroll
        );
        for css in ["3px #ff0000", "#ff0000 3px"] {
            let ring = style_of(&[("outline", css)]);
            assert_eq!(
                (ring.outline_width, ring.outline_color),
                (3.0, [1.0, 0.0, 0.0, 1.0]),
                "outline: {css}"
            );
        }
        let none = style_of(&[("outline", "2px red"), ("outline", "none")]);
        assert_eq!((none.outline_width, none.outline_color), (0.0, [0.0; 4]));

        let refused = [
            ("width", "10"),
            ("width", "-1px"),
            ("width", "10em"),
            ("padding", "auto"),
            ("padding", "-2px"),
            ("padding", "1px 2px 3px 4px 5px"),
            ("border-width", "10%"),
            ("display", "block"),
            ("overflow", "auto"),
            ("align-self", "normal"),
            ("align-items", "auto"),
            ("flex-grow", "-1"),
            ("flex", "1 2 3 4"),
            ("font-size", "0"),
            ("color", "#12"),
            ("color", "notacolour"),
            ("color", "red !important"),
            ("background", "rgb(1, 2)"),
            ("background", "rgb(1, 2 3)"),
            ("width", "1e39px"),
            ("font-family", "serif"),
            ("font-family", "\"sans-serif\""),
            ("font-family", "sans-serif bold"),
            ("font-family", "bitmap,"),
            ("font-family", "12px"),
            ("line-height", "-1"),
            ("line-height", "120%"),
            ("line-height", "2em"),
            ("text-align", "justify"),
            ("outline-width", "-1px"),
            ("outline", "2px"),
            ("outline", "red"),
            ("outline", "2px red solid"),
            ("nav-up", "top"),
            ("nav-up", "\"#top\""),
            ("nav-up", "#a #b"),
            ("nav-wrap", "wrap"),
        ];
        for (property, css) in refused {
            assert!(
                parse(property, css).is_err(),
                "`{property}: {css}` was accepted"
            );
        }
    }

    /// **`font-family` keeps the first name ahead of its built-in family** for
    /// a registered font to answer to, quoted or bare and in any case; a name
    /// after the built-in one, or a reserved one, is not kept, and a list of
    /// such a name alone sets no built-in family.
    #[test]
    fn font_family_keeps_the_first_name_ahead_of_its_built_in_family() {
        let roboto = Some(FamilyName::new("roboto"));
        let cases = [
            ("\"ROBOTO\", sans-serif", FontFamily::Sans, roboto),
            ("Roboto, Inter, bitmap", FontFamily::Bitmap, roboto),
            ("serif, roboto, sans-serif", FontFamily::Sans, roboto),
            (
                "\"sans-serif\", roboto, sans-serif",
                FontFamily::Sans,
                roboto,
            ),
            (
                "sans-serif bold, roboto, sans-serif",
                FontFamily::Sans,
                roboto,
            ),
            ("sans-serif, roboto", FontFamily::Sans, None),
            ("bitmap", FontFamily::Bitmap, None),
            (
                "Roboto Mono, sans-serif",
                FontFamily::Sans,
                Some(FamilyName::new("roboto mono")),
            ),
        ];
        for (css, family, name) in cases {
            let style = style_of(&[("font-family", css)]);
            assert_eq!(
                (style.font_family, style.family_name),
                (family, name),
                "{css}"
            );
        }
        let alone = style_of(&[("font-family", "sans-serif"), ("font-family", "roboto")]);
        assert_eq!(
            (alone.font_family, alone.family_name),
            (FontFamily::Sans, roboto)
        );
    }

    /// **`font-family` takes the first family in its list this engine has**,
    /// by generic name, quoted name or unquoted words, in any case.
    #[test]
    fn font_family_takes_the_first_family_it_has() {
        let cases = [
            ("bitmap", FontFamily::Bitmap),
            ("sans-serif", FontFamily::Sans),
            ("\"Atkinson Hyperlegible\"", FontFamily::Sans),
            ("atkinson   HYPERLEGIBLE", FontFamily::Sans),
            ("\"Helvetica Neue\", Arial, sans-serif", FontFamily::Sans),
            ("Fira Sans, bitmap, sans-serif", FontFamily::Bitmap),
            ("sans-serif, bitmap", FontFamily::Sans),
        ];
        for (css, want) in cases {
            assert_eq!(style_of(&[("font-family", css)]).font_family, want, "{css}");
        }
        let height = |css| style_of(&[("line-height", css)]).line_height;
        assert_eq!(height("normal"), LineHeight::Normal);
        assert_eq!(height("0"), LineHeight::Multiple(0.0));
        assert_eq!(height("18px"), LineHeight::Px(18.0));
        let align = |css| style_of(&[("text-align", css)]).text_align;
        assert_eq!(align("start"), TextAlign::Left);
        assert_eq!(align("end"), TextAlign::Right);
        assert_eq!(align("right"), TextAlign::Right);
    }

    /// **A colour is decoded from sRGB to linear light in every syntax**, with
    /// alpha left linear, matching IEC 61966-2-1 at known points.
    #[test]
    fn colours_decode_from_srgb_to_linear_light_in_every_syntax() {
        let close = |got: [f32; 4], want: [f32; 4]| {
            got.iter()
                .zip(want)
                .all(|(got, want)| (got - want).abs() < 1e-4)
        };
        // 128/255 through the transfer function is 0.215861; 0.5 alpha stays.
        let grey = [0.215_861, 0.215_861, 0.215_861, 1.0];
        for css in [
            "#808080",
            "#808080ff",
            "rgb(128, 128, 128)",
            "rgb(128 128 128)",
            "RGBA(128,128,128,1)",
        ] {
            let got = style_of(&[("color", css)]).color;
            assert!(close(got, grey), "{css}: {got:?}");
        }
        let cases = [
            ("#fff", [1.0, 1.0, 1.0, 1.0]),
            ("#f008", [1.0, 0.0, 0.0, 136.0 / 255.0]),
            ("white", [1.0, 1.0, 1.0, 1.0]),
            ("Black", [0.0, 0.0, 0.0, 1.0]),
            ("transparent", [0.0; 4]),
            ("rgba(255, 0, 0, 0.5)", [1.0, 0.0, 0.0, 0.5]),
            ("rgb(100% 0% 0% / 25%)", [1.0, 0.0, 0.0, 0.25]),
            // The linear segment at the dark end: 10/255 / 12.92.
            ("rgb(10, 10, 10)", [0.003_035, 0.003_035, 0.003_035, 1.0]),
        ];
        for (css, want) in cases {
            let got = style_of(&[("background", css)]).background;
            assert!(close(got, want), "{css}: {got:?} is not {want:?}");
        }
    }

    /// **An image is a name in `url()`, quoted or not, and never a path that
    /// is read**: `background-image` and `border-image-source` take it or
    /// `none`, and every other form is refused.
    #[test]
    fn images_are_names_in_url_or_none() {
        let sky = Some(ImageName::new("sky"));
        for css in ["url(sky)", "url(\"sky\")", "URL('sky')"] {
            assert_eq!(
                style_of(&[("background-image", css)]).background_image,
                sky,
                "{css}"
            );
            assert_eq!(
                style_of(&[("border-image-source", css)])
                    .border_image
                    .source,
                sky,
                "{css}"
            );
        }
        assert_ne!(
            style_of(&[("background-image", "url(Sky)")]).background_image,
            sky,
            "names are case-sensitive"
        );
        let cleared = style_of(&[
            ("background-image", "url(sky)"),
            ("background-image", "none"),
        ]);
        assert_eq!(cleared.background_image, None);
        for (property, css) in [
            ("background-image", "sky"),
            ("background-image", "\"sky\""),
            ("background-image", "url(sky) url(sea)"),
            ("background-image", "linear-gradient(red, blue)"),
            ("border-image-source", "url(a) none"),
        ] {
            assert!(
                parse(property, css).is_err(),
                "`{property}: {css}` was accepted"
            );
        }
    }

    /// **`border-image`'s longhands parse CSS's grammar for the subset**: a
    /// slice of one to four numbers with `fill` on either side, a width of px
    /// or multiples per side, and `stretch` as the only repeat — and the
    /// shorthand resets what it leaves out, in any order.
    #[test]
    fn border_image_parses_its_longhands_and_its_shorthand() {
        let style = style_of(&[
            ("border-image-slice", "1 2 3 4"),
            ("border-image-width", "2 5px 0 1.5"),
            ("border-image-repeat", "stretch stretch"),
        ]);
        let frame = style.border_image;
        assert_eq!(
            [
                frame.slice.top,
                frame.slice.right,
                frame.slice.bottom,
                frame.slice.left
            ],
            [1.0, 2.0, 3.0, 4.0]
        );
        assert!(!frame.fill);
        assert_eq!(
            [
                frame.width.top,
                frame.width.right,
                frame.width.bottom,
                frame.width.left
            ],
            [
                BorderImageWidth::Multiple(2.0),
                BorderImageWidth::Px(5.0),
                BorderImageWidth::Multiple(0.0),
                BorderImageWidth::Multiple(1.5),
            ]
        );
        for css in ["fill 4", "4 fill", "4 4 fill"] {
            let frame = style_of(&[("border-image-slice", css)]).border_image;
            assert!(frame.fill, "{css}");
            assert_eq!(frame.slice.left, 4.0, "{css}");
        }

        let set = |css: &str| {
            style_of(&[
                ("border-image-source", "url(old)"),
                ("border-image-slice", "9 fill"),
                ("border-image-width", "7px"),
                ("border-image", css),
            ])
            .border_image
        };
        let whole = set("url(frame) 4 fill / 12px stretch");
        assert_eq!(whole.source, Some(ImageName::new("frame")));
        assert_eq!((whole.slice.top, whole.fill), (4.0, true));
        assert_eq!(whole.width.right, BorderImageWidth::Px(12.0));
        let reordered = set("stretch 4 fill / 12px url(frame)");
        assert_eq!(reordered, whole, "the shorthand's parts go in any order");
        let source_only = set("url(frame)");
        assert_eq!(
            source_only,
            BorderImage {
                source: Some(ImageName::new("frame")),
                ..BorderImage::NONE
            },
            "what the shorthand leaves out goes back to its initial value"
        );
        let slice_only = set("4");
        assert_eq!(
            (
                slice_only.source,
                slice_only.slice.top,
                slice_only.width.top
            ),
            (None, 4.0, BorderImageWidth::Multiple(1.0))
        );

        for (property, css) in [
            ("border-image-slice", "25%"),
            ("border-image-slice", "-1"),
            ("border-image-slice", "fill"),
            ("border-image-slice", "fill 1 fill"),
            ("border-image-slice", "1 2 3 4 5"),
            ("border-image-width", "auto"),
            ("border-image-width", "10%"),
            ("border-image-width", "-1px"),
            ("border-image-repeat", "repeat"),
            ("border-image-repeat", "round"),
            ("border-image-repeat", "stretch space"),
            ("border-image", "url(a) 4 / 2 / 1"),
            ("border-image", "url(a) url(b)"),
            ("border-image", "4 5 6 7 8"),
            ("border-image", "repeat"),
        ] {
            assert!(
                parse(property, css).is_err(),
                "`{property}: {css}` was accepted"
            );
        }
    }

    /// **`color()` takes `srgb` through the transfer function and
    /// `srgb-linear` exactly as written** — the latter is how a linear-light
    /// colour reaches a sheet without an eight-bit round trip.
    #[test]
    fn the_color_function_decodes_srgb_and_keeps_srgb_linear() {
        assert_eq!(
            style_of(&[("color", "color(srgb-linear 1 0.94 0.55)")]).color,
            [1.0, 0.94, 0.55, 1.0]
        );
        assert_eq!(
            style_of(&[("color", "color(SRGB-LINEAR 0 0 0 / 0.66)")]).color,
            [0.0, 0.0, 0.0, 0.66]
        );
        assert_eq!(
            style_of(&[("color", "color(srgb-linear 50% 0 0 / 25%)")]).color,
            [0.5, 0.0, 0.0, 0.25]
        );
        let decoded = style_of(&[("color", "color(srgb 0.5 0.5 0.5)")]).color;
        assert!((decoded[0] - 0.214_041).abs() < 1e-4, "{decoded:?}");
        for css in [
            "color(display-p3 1 0 0)",
            "color(srgb-linear 1 0)",
            "color(srgb-linear 1 0 0 0.5)",
            "color(srgb-linear 1, 0, 0)",
        ] {
            assert!(parse("color", css).is_err(), "`color: {css}` was accepted");
        }
    }

    /// **`initial` restores exactly the fields a property owns**, shorthand or
    /// longhand, and no others.
    #[test]
    fn copying_a_property_copies_exactly_its_own_fields() {
        let set = style_of(&[
            ("padding", "5px"),
            ("margin", "5px"),
            ("flex", "3 3 9px"),
            ("border-radius", "4px"),
        ]);
        let mut style = set;
        Property::from_name("padding-left")
            .expect("known")
            .copy(&NodeStyle::DEFAULT, &mut style);
        assert_eq!(style.padding.left, Length::Px(0.0));
        assert_eq!(style.padding.top, Length::Px(5.0));
        Property::from_name("flex")
            .expect("known")
            .copy(&NodeStyle::DEFAULT, &mut style);
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (0.0, 1.0, LengthAuto::Auto)
        );
        assert_eq!(
            style.margin, set.margin,
            "copying padding and flex moved the margin"
        );
        Property::from_name("border-top-right-radius")
            .expect("known")
            .copy(&NodeStyle::DEFAULT, &mut style);
        assert_eq!((style.radii.top_right, style.radii.top_left), (0.0, 4.0));
    }
}
