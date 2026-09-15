//! Taffy's Chrome-generated flexbox fixtures, laid out by **this engine's**
//! element tree.
//!
//! `docs/plan/07-ui-debug.md` section 2: Taffy is pinned, and an upgrade lands
//! with its fixture corpus green. Running the fixtures through `taffy::TaffyTree`
//! would prove Taffy; running them through [`Ui`] proves what this crate adds on
//! top — the traversal and cache traits over the node store, the dispatch
//! between flex containers, leaves and `display: none`, the mapping from
//! [`NodeStyle`] onto Taffy's style traits, rounding, and placing each node on
//! the screen.
//!
//! # The corpus
//!
//! `tests/fixtures/taffy/flex/` is every fixture under Taffy's `tests/xml/flex/`
//! at tag `v0.14.0` (commit `77f385683c1d698c91a23a259f87fdddf26925fb`,
//! <https://github.com/DioxusLabs/taffy>) whose inputs stay inside the subset
//! [`NodeStyle`] can express: the `border_box` and `ltr` variant of each, with
//! no text and no `<text>` leaf, no `aspect-ratio`, no `align-content`, no `baseline`, no
//! `display: block` or `grid`, no `overflow: scroll` or per-axis overflow, and
//! rounding on. Copied byte for byte; Taffy's MIT licence is beside them in
//! `tests/fixtures/taffy/LICENSE`.
//!
//! **A divergence this engine makes is written into the fixture it diverges
//! on**: the expectation changed to what the engine does, with an XML comment
//! above it saying why. None of the files carries one today.
//!
//! # The reader
//!
//! The fixtures are machine-generated, and the reader below takes exactly
//! their shape — elements, double-quoted attributes, self-closing tags and
//! comments — and panics on anything else, so a fixture it cannot read fails
//! loudly instead of being skipped. The workspace has no XML parser, and the
//! one Taffy's own harness uses would be a new dependency for a test.

use std::fs;
use std::path::{Path, PathBuf};

use crcbl_ui::FontAtlas;
use crcbl_ui::PointerInput;
use crcbl_ui::tree::{
    Align, Available, AvailableSpace, Display, FlexDirection, FlexWrap, Justify, Length,
    LengthAuto, NodeKey, NodeStyle, Overflow, Position, Ui,
};
use glam::Vec2;

/// How far a laid-out edge may sit from Chrome's, in pixels — Taffy's own
/// harness's tolerance, for Chrome's sub-pixel serialisation.
const TOLERANCE: f32 = 0.1;

/// The categories the corpus must cover, each matched against fixture names:
/// row and column, wrap, grow, shrink and basis, align and justify, gap, min and
/// max, absolute and percent.
const CATEGORIES: &[&str] = &[
    "row", "column", "wrap", "grow", "shrink", "basis", "align", "justify", "gap", "min", "max",
    "absolute", "percent",
];

// ---------------------------------------------------------------------------
// Reading a fixture
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Element>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn child(&self, name: &str) -> &Element {
        self.children
            .iter()
            .find(|child| child.name == name)
            .unwrap_or_else(|| panic!("<{}> has no <{name}>", self.name))
    }
}

/// Parses one generated fixture. See the module docs for what it accepts.
fn parse(source: &str) -> Element {
    let mut rest = source;
    let mut stack: Vec<Element> = vec![Element {
        name: String::new(),
        attributes: Vec::new(),
        children: Vec::new(),
    }];
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(comment) = rest.strip_prefix("<!--") {
            let end = comment.find("-->").expect("an unterminated comment");
            rest = &comment[end + 3..];
        } else if let Some(close) = rest.strip_prefix("</") {
            let end = close.find('>').expect("an unterminated closing tag");
            let name = close[..end].trim();
            let element = stack.pop().expect("a closing tag with nothing open");
            assert_eq!(element.name, name, "</{name}> closes <{}>", element.name);
            stack
                .last_mut()
                .expect("the document element is inside the root")
                .children
                .push(element);
            rest = &close[end + 1..];
        } else if let Some(open) = rest.strip_prefix('<') {
            let end = open.find('>').expect("an unterminated tag");
            let (body, self_closing) = match open[..end].strip_suffix('/') {
                Some(body) => (body, true),
                None => (&open[..end], false),
            };
            let (name, mut attributes_text) =
                body.split_once(char::is_whitespace).unwrap_or((body, ""));
            let mut attributes = Vec::new();
            loop {
                attributes_text = attributes_text.trim_start();
                if attributes_text.is_empty() {
                    break;
                }
                let (key, after) = attributes_text
                    .split_once("=\"")
                    .unwrap_or_else(|| panic!("an attribute that is not key=\"value\": {body}"));
                let (value, after) = after
                    .split_once('"')
                    .unwrap_or_else(|| panic!("an unterminated attribute value: {body}"));
                attributes.push((key.to_owned(), value.to_owned()));
                attributes_text = after;
            }
            let element = Element {
                name: name.to_owned(),
                attributes,
                children: Vec::new(),
            };
            if self_closing {
                stack
                    .last_mut()
                    .expect("an element is inside the root")
                    .children
                    .push(element);
            } else {
                stack.push(element);
            }
            rest = &open[end + 1..];
        } else {
            panic!(
                "text content, which no fixture in this corpus has: {:?}",
                &rest[..rest.len().min(40)]
            );
        }
    }
    let mut root = stack.pop().expect("the root");
    assert!(stack.is_empty(), "an element was never closed");
    assert_eq!(root.children.len(), 1, "a fixture is one <test>");
    root.children.remove(0)
}

fn length(value: &str) -> Length {
    if let Some(px) = value.strip_suffix("px") {
        Length::Px(px.parse().expect("a pixel length"))
    } else if let Some(percent) = value.strip_suffix('%') {
        Length::Percent(percent.parse::<f32>().expect("a percentage") / 100.0)
    } else {
        panic!("not a length: {value}")
    }
}

fn length_auto(value: &str) -> LengthAuto {
    if value == "auto" {
        return LengthAuto::Auto;
    }
    match length(value) {
        Length::Px(px) => LengthAuto::Px(px),
        Length::Percent(fraction) => LengthAuto::Percent(fraction),
    }
}

fn align(value: &str) -> Option<Align> {
    Some(match value {
        "auto" => return None,
        "start" => Align::Start,
        "end" => Align::End,
        "flex-start" => Align::FlexStart,
        "flex-end" => Align::FlexEnd,
        "center" => Align::Center,
        "stretch" => Align::Stretch,
        other => panic!("align {other} is outside the subset"),
    })
}

/// A `<div>`'s attributes as a [`NodeStyle`]. Any attribute the subset has no
/// field for panics, so a fixture outside it cannot pass by being half read.
fn style(div: &Element) -> NodeStyle {
    let mut style = NodeStyle::DEFAULT;
    for (key, value) in &div.attributes {
        let value = value.as_str();
        match key.as_str() {
            "direction" => assert_eq!(value, "ltr", "the corpus is left to right"),
            "display" => {
                style.display = match value {
                    "flex" => Display::Flex,
                    "none" => Display::None,
                    other => panic!("display {other} is outside the subset"),
                }
            }
            "position" => {
                style.position = match value {
                    "relative" => Position::Relative,
                    "absolute" => Position::Absolute,
                    other => panic!("position {other}"),
                }
            }
            "top" => style.inset.top = length_auto(value),
            "right" => style.inset.right = length_auto(value),
            "bottom" => style.inset.bottom = length_auto(value),
            "left" => style.inset.left = length_auto(value),
            "width" => style.width = length_auto(value),
            "height" => style.height = length_auto(value),
            "min-width" => style.min_width = length_auto(value),
            "min-height" => style.min_height = length_auto(value),
            "max-width" => style.max_width = length_auto(value),
            "max-height" => style.max_height = length_auto(value),
            "margin-top" => style.margin.top = length_auto(value),
            "margin-right" => style.margin.right = length_auto(value),
            "margin-bottom" => style.margin.bottom = length_auto(value),
            "margin-left" => style.margin.left = length_auto(value),
            "padding-top" => style.padding.top = length(value),
            "padding-right" => style.padding.right = length(value),
            "padding-bottom" => style.padding.bottom = length(value),
            "padding-left" => style.padding.left = length(value),
            "border-top" | "border-right" | "border-bottom" | "border-left" => {
                let Length::Px(px) = length(value) else {
                    panic!("a percentage border is outside the subset");
                };
                match key.as_str() {
                    "border-top" => style.border.top = px,
                    "border-right" => style.border.right = px,
                    "border-bottom" => style.border.bottom = px,
                    _ => style.border.left = px,
                }
            }
            "overflow-x" | "overflow-y" => {
                style.overflow = match value {
                    "visible" => Overflow::Visible,
                    "hidden" => Overflow::Hidden,
                    other => panic!("overflow {other} is outside the subset"),
                }
            }
            "flex-direction" => {
                style.flex_direction = match value {
                    "row" => FlexDirection::Row,
                    "column" => FlexDirection::Column,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    other => panic!("flex-direction {other}"),
                }
            }
            "flex-wrap" => {
                style.flex_wrap = match value {
                    "nowrap" => FlexWrap::NoWrap,
                    "wrap" => FlexWrap::Wrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    other => panic!("flex-wrap {other} is outside the subset"),
                }
            }
            "align-items" => style.align_items = align(value),
            "align-self" => style.align_self = align(value),
            "justify-content" => {
                style.justify_content = Some(match value {
                    "start" => Justify::Start,
                    "end" => Justify::End,
                    "flex-start" => Justify::FlexStart,
                    "flex-end" => Justify::FlexEnd,
                    "center" => Justify::Center,
                    "space-between" => Justify::SpaceBetween,
                    "space-around" => Justify::SpaceAround,
                    "space-evenly" => Justify::SpaceEvenly,
                    other => panic!("justify-content {other} is outside the subset"),
                });
            }
            "flex-grow" => style.flex_grow = value.parse().expect("a number"),
            "flex-shrink" => style.flex_shrink = value.parse().expect("a number"),
            "flex-basis" => style.flex_basis = length_auto(value),
            "column-gap" => style.column_gap = length(value),
            "row-gap" => style.row_gap = length(value),
            other => panic!("{other} is outside the subset"),
        }
    }
    style
}

fn available(value: Option<&str>) -> Available {
    match value {
        None | Some("max-content") => Available::MaxContent,
        Some("min-content") => Available::MinContent,
        Some(px) => match length(px) {
            Length::Px(px) => Available::Definite(px),
            Length::Percent(_) => panic!("a percentage viewport"),
        },
    }
}

// ---------------------------------------------------------------------------
// Running one
// ---------------------------------------------------------------------------

/// A built node's key, beside its children's.
struct Built {
    key: NodeKey,
    children: Vec<Built>,
}

fn build(ui: &mut Ui, div: &Element, index: usize) -> Built {
    assert_eq!(
        div.name, "div",
        "a <{}> leaf is outside the corpus",
        div.name
    );
    let mut children = Vec::new();
    let response = ui.block_keyed(index, &style(div), |ui| {
        for (index, child) in div.children.iter().enumerate() {
            children.push(build(ui, child, index));
        }
    });
    Built {
        key: response.key,
        children,
    }
}

/// Compares every node's laid-out box with its expectation, relative to its
/// parent as Chrome reports it, and describes each mismatch.
fn compare(
    ui: &Ui,
    built: &Built,
    expected: &Element,
    parent: Vec2,
    path: &str,
    mismatches: &mut Vec<String>,
) {
    let (min, max) = ui.rect(built.key).expect("every built node is laid out");
    let number = |name: &str| -> f32 {
        expected
            .attribute(name)
            .unwrap_or_else(|| panic!("<node> has no {name}"))
            .parse()
            .expect("a number")
    };
    let got = [
        min.x - parent.x,
        min.y - parent.y,
        max.x - min.x,
        max.y - min.y,
    ];
    let want = [number("x"), number("y"), number("width"), number("height")];
    if got
        .iter()
        .zip(&want)
        .any(|(got, want)| (got - want).abs() >= TOLERANCE)
    {
        mismatches.push(format!(
            "{path}: laid out x y w h {got:?}, Chrome has {want:?}"
        ));
    }
    assert_eq!(
        built.children.len(),
        expected.children.len(),
        "{path}: the expectation has a different number of children"
    );
    for (index, (child, want)) in built.children.iter().zip(&expected.children).enumerate() {
        compare(ui, child, want, min, &format!("{path}/{index}"), mismatches);
    }
}

/// Lays out one fixture and returns its mismatches.
fn run(path: &Path, atlas: &FontAtlas) -> Vec<String> {
    let source = fs::read_to_string(path).expect("a readable fixture");
    let test = parse(&source);
    assert_eq!(test.name, "test");
    assert_eq!(
        test.attribute("use-rounding"),
        Some("true"),
        "the engine always rounds; an unrounded fixture is outside the corpus"
    );
    let viewport = test.child("viewport");
    let space = AvailableSpace {
        width: available(viewport.attribute("width")),
        height: available(viewport.attribute("height")),
    };
    let input = test.child("input");
    let expectations = test.child("expectations");
    assert_eq!(input.children.len(), 1, "one root");

    let mut ui = Ui::new();
    let mut mismatches = Vec::new();
    // Twice: the second frame lays out from the first frame's caches, which is
    // the path a page takes every frame after its first.
    for frame in ["first frame", "cached frame"] {
        ui.begin_frame(PointerInput::default());
        let built = build(&mut ui, &input.children[0], 0);
        ui.layout(Vec2::ZERO, space, atlas);
        let mut found = Vec::new();
        compare(
            &ui,
            &built,
            &expectations.children[0],
            Vec2::ZERO,
            frame,
            &mut found,
        );
        mismatches.extend(found);
    }
    mismatches
}

fn corpus() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/taffy/flex");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "xml"))
        .collect();
    files.sort();
    files
}

/// **Every fixture in the corpus lays out as Chrome did**, through the
/// engine's own tree, on the first frame and on a frame that reuses its
/// caches — and the corpus covers every category the subset promises.
#[test]
fn every_flexbox_fixture_lays_out_through_the_tree_as_chrome_did() {
    let atlas = FontAtlas::built_in();
    let files = corpus();
    assert!(
        files.len() >= 300,
        "only {} fixtures were found: the corpus did not load",
        files.len()
    );
    let names: Vec<String> = files
        .iter()
        .map(|path| {
            path.file_name()
                .expect("a file")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    for category in CATEGORIES {
        assert!(
            names.iter().any(|name| name.contains(category)),
            "no fixture covers {category}"
        );
    }

    let mut failed = Vec::new();
    for (path, name) in files.iter().zip(&names) {
        let mismatches = run(path, &atlas);
        if !mismatches.is_empty() {
            failed.push(format!("{name}:\n  {}", mismatches.join("\n  ")));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} fixtures diverge from Chrome:\n{}",
        failed.len(),
        files.len(),
        failed.join("\n")
    );
    eprintln!("taffy fixtures: {} laid out as Chrome did", files.len());
}

/// The reader refuses what it does not understand rather than skipping it.
#[test]
#[should_panic(expected = "text content")]
fn the_reader_refuses_text_content() {
    parse("<test><input><div>HH</div></input></test>");
}

/// A divergence comment is read past, so a fixture can carry one.
#[test]
fn the_reader_reads_past_a_comment() {
    let test =
        parse("<test name=\"a\">\n  <!-- divergence: why -->\n  <node x=\"1\" y=\"2\"/>\n</test>");
    assert_eq!(test.children.len(), 1);
    assert_eq!(test.children[0].attribute("y"), Some("2"));
}
