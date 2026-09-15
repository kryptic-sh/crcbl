//! Typed property values: what a declaration sets on a [`NodeStyle`].
//!
//! [`Declaration`] is one property set to one typed value — what a stylesheet
//! declaration becomes once parsed, and what a builder's inline override is
//! written in. The CSS each one parses from is [`super::property`]'s.

use crate::draw_list::CornerRadii;
use crate::tree::{
    Align, BorderImageWidth, Direction, Display, Edges, FlexDirection, FlexWrap, FontFamily,
    ImageName, Justify, Length, LengthAuto, LineHeight, NavTarget, NavWrap, NodeStyle, Overflow,
    Position, TextAlign,
};

/// Which sides of a box a declaration sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sides {
    /// All four.
    All,
    /// The top.
    Top,
    /// The right.
    Right,
    /// The bottom.
    Bottom,
    /// The left.
    Left,
}

/// Which corners of a box a declaration rounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Corners {
    /// All four.
    All,
    /// The top-left.
    TopLeft,
    /// The top-right.
    TopRight,
    /// The bottom-right.
    BottomRight,
    /// The bottom-left.
    BottomLeft,
}

/// One property set to one typed value: what a stylesheet's declaration
/// becomes, and what a builder's inline override is written in.
///
/// Shorthands expand to these, so `padding: 1px 2px` is four
/// [`Declaration::Padding`]s. See the module docs for the CSS each one
/// parses from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Declaration {
    /// `display`.
    Display(Display),
    /// `position`.
    Position(Position),
    /// `top`, `right`, `bottom`, `left`.
    Inset(Sides, LengthAuto),
    /// `width`.
    Width(LengthAuto),
    /// `height`.
    Height(LengthAuto),
    /// `min-width`.
    MinWidth(LengthAuto),
    /// `min-height`.
    MinHeight(LengthAuto),
    /// `max-width`.
    MaxWidth(LengthAuto),
    /// `max-height`.
    MaxHeight(LengthAuto),
    /// `margin-*`.
    Margin(Sides, LengthAuto),
    /// `padding-*`.
    Padding(Sides, Length),
    /// `border-*-width`, in pixels.
    BorderWidth(Sides, f32),
    /// `overflow`.
    Overflow(Overflow),
    /// `flex-direction`.
    FlexDirection(FlexDirection),
    /// `flex-wrap`.
    FlexWrap(FlexWrap),
    /// `justify-content`; `None` is `normal`.
    JustifyContent(Option<Justify>),
    /// `align-items`; `None` is `normal`.
    AlignItems(Option<Align>),
    /// `align-self`; `None` is `auto`.
    AlignSelf(Option<Align>),
    /// `flex-grow`.
    FlexGrow(f32),
    /// `flex-shrink`.
    FlexShrink(f32),
    /// `flex-basis`.
    FlexBasis(LengthAuto),
    /// `column-gap`.
    ColumnGap(Length),
    /// `row-gap`.
    RowGap(Length),
    /// `background-color`, in linear light.
    Background([f32; 4]),
    /// `border-color`, in linear light.
    BorderColor([f32; 4]),
    /// `border-*-radius`, in pixels.
    BorderRadius(Corners, f32),
    /// `color`, in linear light. Inherited.
    Color([f32; 4]),
    /// `font-size`, in pixels. Inherited.
    FontSize(f32),
    /// `font-family`. Inherited.
    FontFamily(FontFamily),
    /// `line-height`. Inherited.
    LineHeight(LineHeight),
    /// `text-align`. Inherited.
    TextAlign(TextAlign),
    /// `outline-width`, in pixels.
    OutlineWidth(f32),
    /// `outline-color`, in linear light.
    OutlineColor([f32; 4]),
    /// `outline-offset`, in pixels.
    OutlineOffset(f32),
    /// `background-image`; `None` is `none`.
    BackgroundImage(Option<ImageName>),
    /// `border-image-source`; `None` is `none`.
    BorderImageSource(Option<ImageName>),
    /// One side of `border-image-slice`, in texels.
    BorderImageSlice(Sides, f32),
    /// `border-image-slice`'s `fill` keyword.
    BorderImageFill(bool),
    /// One side of `border-image-width`.
    BorderImageWidth(Sides, BorderImageWidth),
    /// `nav-up`, `nav-right`, `nav-down` or `nav-left`.
    Nav(Direction, NavTarget),
    /// `nav-wrap`.
    NavWrap(NavWrap),
}

fn set_sides<T: Copy>(edges: &mut Edges<T>, sides: Sides, value: T) {
    match sides {
        Sides::All => *edges = Edges::all(value),
        Sides::Top => edges.top = value,
        Sides::Right => edges.right = value,
        Sides::Bottom => edges.bottom = value,
        Sides::Left => edges.left = value,
    }
}

impl Declaration {
    /// Sets this declaration's property on `style`.
    pub fn apply(self, style: &mut NodeStyle) {
        match self {
            Self::Display(value) => style.display = value,
            Self::Position(value) => style.position = value,
            Self::Inset(sides, value) => set_sides(&mut style.inset, sides, value),
            Self::Width(value) => style.width = value,
            Self::Height(value) => style.height = value,
            Self::MinWidth(value) => style.min_width = value,
            Self::MinHeight(value) => style.min_height = value,
            Self::MaxWidth(value) => style.max_width = value,
            Self::MaxHeight(value) => style.max_height = value,
            Self::Margin(sides, value) => set_sides(&mut style.margin, sides, value),
            Self::Padding(sides, value) => set_sides(&mut style.padding, sides, value),
            Self::BorderWidth(sides, value) => set_sides(&mut style.border, sides, value),
            Self::Overflow(value) => style.overflow = value,
            Self::FlexDirection(value) => style.flex_direction = value,
            Self::FlexWrap(value) => style.flex_wrap = value,
            Self::JustifyContent(value) => style.justify_content = value,
            Self::AlignItems(value) => style.align_items = value,
            Self::AlignSelf(value) => style.align_self = value,
            Self::FlexGrow(value) => style.flex_grow = value,
            Self::FlexShrink(value) => style.flex_shrink = value,
            Self::FlexBasis(value) => style.flex_basis = value,
            Self::ColumnGap(value) => style.column_gap = value,
            Self::RowGap(value) => style.row_gap = value,
            Self::Background(value) => style.background = value,
            Self::BorderColor(value) => style.border_color = value,
            Self::BorderRadius(corners, value) => {
                let radii = &mut style.radii;
                match corners {
                    Corners::All => *radii = CornerRadii::uniform(value),
                    Corners::TopLeft => radii.top_left = value,
                    Corners::TopRight => radii.top_right = value,
                    Corners::BottomRight => radii.bottom_right = value,
                    Corners::BottomLeft => radii.bottom_left = value,
                }
            }
            Self::Color(value) => style.color = value,
            Self::FontSize(value) => style.font_size = value,
            Self::FontFamily(value) => style.font_family = value,
            Self::LineHeight(value) => style.line_height = value,
            Self::TextAlign(value) => style.text_align = value,
            Self::OutlineWidth(value) => style.outline_width = value,
            Self::OutlineColor(value) => style.outline_color = value,
            Self::OutlineOffset(value) => style.outline_offset = value,
            Self::BackgroundImage(value) => style.background_image = value,
            Self::BorderImageSource(value) => style.border_image.source = value,
            Self::BorderImageSlice(sides, value) => {
                set_sides(&mut style.border_image.slice, sides, value);
            }
            Self::BorderImageFill(value) => style.border_image.fill = value,
            Self::BorderImageWidth(sides, value) => {
                set_sides(&mut style.border_image.width, sides, value);
            }
            Self::Nav(direction, value) => *style.nav_mut(direction) = value,
            Self::NavWrap(value) => style.nav_wrap = value,
        }
    }
}

impl NodeStyle {
    /// Every property of this style as a declaration, per side and per corner,
    /// so that applying them in order onto any style yields exactly this one.
    ///
    /// What a caller with a whole [`NodeStyle`] in hand passes as an inline
    /// override to take no part of the cascade.
    #[must_use]
    pub fn declarations(&self) -> Vec<Declaration> {
        use Declaration as D;
        let sides = [Sides::Top, Sides::Right, Sides::Bottom, Sides::Left];
        let edges = |edges: Edges<LengthAuto>| [edges.top, edges.right, edges.bottom, edges.left];
        let mut all = vec![
            D::Display(self.display),
            D::Position(self.position),
            D::Width(self.width),
            D::Height(self.height),
            D::MinWidth(self.min_width),
            D::MinHeight(self.min_height),
            D::MaxWidth(self.max_width),
            D::MaxHeight(self.max_height),
            D::Overflow(self.overflow),
            D::FlexDirection(self.flex_direction),
            D::FlexWrap(self.flex_wrap),
            D::JustifyContent(self.justify_content),
            D::AlignItems(self.align_items),
            D::AlignSelf(self.align_self),
            D::FlexGrow(self.flex_grow),
            D::FlexShrink(self.flex_shrink),
            D::FlexBasis(self.flex_basis),
            D::ColumnGap(self.column_gap),
            D::RowGap(self.row_gap),
            D::Background(self.background),
            D::BorderColor(self.border_color),
            D::Color(self.color),
            D::FontSize(self.font_size),
            D::FontFamily(self.font_family),
            D::LineHeight(self.line_height),
            D::TextAlign(self.text_align),
            D::OutlineWidth(self.outline_width),
            D::OutlineColor(self.outline_color),
            D::OutlineOffset(self.outline_offset),
            D::BackgroundImage(self.background_image),
            D::BorderImageSource(self.border_image.source),
            D::BorderImageFill(self.border_image.fill),
            D::NavWrap(self.nav_wrap),
        ];
        for direction in Direction::ALL {
            all.push(D::Nav(direction, self.nav(direction)));
        }
        let padding = self.padding;
        let border = self.border;
        for (at, side) in sides.into_iter().enumerate() {
            all.push(D::Inset(side, edges(self.inset)[at]));
            all.push(D::Margin(side, edges(self.margin)[at]));
            all.push(D::Padding(
                side,
                [padding.top, padding.right, padding.bottom, padding.left][at],
            ));
            all.push(D::BorderWidth(
                side,
                [border.top, border.right, border.bottom, border.left][at],
            ));
            let image = self.border_image;
            all.push(D::BorderImageSlice(
                side,
                [
                    image.slice.top,
                    image.slice.right,
                    image.slice.bottom,
                    image.slice.left,
                ][at],
            ));
            all.push(D::BorderImageWidth(
                side,
                [
                    image.width.top,
                    image.width.right,
                    image.width.bottom,
                    image.width.left,
                ][at],
            ));
        }
        let radii = self.radii;
        all.extend([
            D::BorderRadius(Corners::TopLeft, radii.top_left),
            D::BorderRadius(Corners::TopRight, radii.top_right),
            D::BorderRadius(Corners::BottomRight, radii.bottom_right),
            D::BorderRadius(Corners::BottomLeft, radii.bottom_left),
        ]);
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn px(value: f32) -> LengthAuto {
        LengthAuto::Px(value)
    }

    /// **`declarations` reproduces a style exactly** from any starting point.
    #[test]
    fn a_styles_declarations_rebuild_it_exactly() {
        let style = NodeStyle {
            inset: Edges {
                top: px(1.0),
                right: LengthAuto::Auto,
                bottom: LengthAuto::Percent(0.2),
                left: px(-3.0),
            },
            padding: Edges {
                top: Length::Px(1.0),
                right: Length::Px(2.0),
                bottom: Length::Percent(0.3),
                left: Length::Px(4.0),
            },
            border: Edges {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            radii: CornerRadii {
                top_left: 1.0,
                top_right: 2.0,
                bottom_right: 3.0,
                bottom_left: 4.0,
            },
            background: [0.1, 0.2, 0.3, 0.4],
            color: [0.5; 4],
            font_size: 20.0,
            font_family: FontFamily::Sans,
            line_height: LineHeight::Multiple(1.5),
            text_align: TextAlign::Center,
            align_self: Some(Align::Center),
            outline_width: 2.0,
            outline_color: [0.9, 0.8, 0.7, 0.6],
            outline_offset: -1.0,
            nav_up: NavTarget::None,
            nav_right: NavTarget::Id(crate::tree::NavId::new("next")),
            nav_wrap: NavWrap::Horizontal,
            background_image: Some(crate::tree::ImageName::new("sky")),
            border_image: crate::tree::BorderImage {
                source: Some(crate::tree::ImageName::new("frame")),
                slice: Edges {
                    top: 1.0,
                    right: 2.0,
                    bottom: 3.0,
                    left: 4.0,
                },
                fill: true,
                width: Edges {
                    top: BorderImageWidth::Px(5.0),
                    right: BorderImageWidth::Multiple(2.0),
                    bottom: BorderImageWidth::Px(7.0),
                    left: BorderImageWidth::Multiple(0.5),
                },
            },
            ..NodeStyle::DEFAULT
        };
        let mut rebuilt = NodeStyle {
            width: px(99.0),
            margin: Edges::all(LengthAuto::Auto),
            color: [0.0; 4],
            ..NodeStyle::DEFAULT
        };
        for declaration in style.declarations() {
            declaration.apply(&mut rebuilt);
        }
        assert_eq!(rebuilt, style);
    }
}
