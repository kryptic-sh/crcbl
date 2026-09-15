//! Styles through the tree: the cascade's order, inheritance, `var()`, the
//! resolve counter's skips, layout caches under paint-only changes, and
//! reloading — each held to the behaviour it exists for.

use std::time::Duration;

use glam::Vec2;

use super::tests::{cache_is_empty, frame, idle};
use super::*;
use crate::style::{Declaration, STYLESHEET_POLL_INTERVAL, Severity, StyleStats};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The style `key` was built with this frame.
fn style_of(ui: &Ui, key: NodeKey) -> NodeStyle {
    ui.nodes
        .iter()
        .find(|node| node.key == key)
        .expect("built this frame")
        .style
}

fn hover(at: Vec2) -> PointerInput {
    PointerInput::hovering(at)
}

// ---------------------------------------------------------------------------
// The cascade
// ---------------------------------------------------------------------------

/// **`default.css` loses to any app rule, a higher tier beats a later lower
/// one, a later rule wins within a tier, and inline beats everything** — each
/// against a control that shows the losing rule would otherwise apply.
#[test]
fn the_cascade_orders_origin_then_tier_then_source_and_inline_wins() {
    let app = "
        readout { flex-direction: row-reverse; }
        span { position: relative; }
        #pick { width: 10px; }
        .pick { width: 20px; }
        .a { flex-wrap: wrap-reverse; height: 1px; }
        .b { height: 2px; }
        block { flex-wrap: wrap; }
    ";
    let build = |ui: &mut Ui| {
        let mut keys = Vec::new();
        ui.block("", &[], |ui| {
            keys.push(ui.block("readout", &[], |_| {}).key);
            keys.push(ui.span(".readout-reading", "1", &[]).key);
            keys.push(ui.block("#pick.pick", &[], |_| {}).key);
            keys.push(ui.block(".a.b", &[], |_| {}).key);
            keys.push(
                ui.block(".b.a", &[Declaration::Height(LengthAuto::Px(5.0))], |_| {})
                    .key,
            );
            keys.push(ui.block(".pick", &[], |_| {}).key);
            keys.push(ui.block("", &[], |_| {}).key);
        });
        keys
    };

    let mut plain = Ui::new();
    let mut keys = Vec::new();
    frame(&mut plain, idle(), |ui| keys = build(ui));
    assert_eq!(
        style_of(&plain, keys[0]).flex_direction,
        FlexDirection::Column
    );
    assert_eq!(style_of(&plain, keys[1]).position, Position::Absolute);

    let mut ui = Ui::new();
    ui.add_stylesheet("app.css", app);
    frame(&mut ui, idle(), |ui| keys = build(ui));
    assert_eq!(
        style_of(&ui, keys[0]).flex_direction,
        FlexDirection::RowReverse,
        "default.css's type rule beat the app's"
    );
    assert_eq!(
        style_of(&ui, keys[1]).position,
        Position::Relative,
        "default.css's class rule beat an app type rule: origin must come before tier"
    );
    assert_eq!(
        style_of(&ui, keys[5]).width,
        LengthAuto::Px(20.0),
        "the class rule applies alone"
    );
    assert_eq!(
        style_of(&ui, keys[2]).width,
        LengthAuto::Px(10.0),
        "a later class rule beat an id rule"
    );
    let ab = style_of(&ui, keys[3]);
    assert_eq!(
        ab.height,
        LengthAuto::Px(2.0),
        "the earlier of two class rules won"
    );
    assert_eq!(
        ab.flex_wrap,
        FlexWrap::WrapReverse,
        "a later type rule beat a class rule"
    );
    assert_eq!(
        style_of(&ui, keys[4]).height,
        LengthAuto::Px(5.0),
        "a rule beat inline"
    );
    assert_eq!(
        style_of(&ui, keys[6]).flex_wrap,
        FlexWrap::Wrap,
        "the type rule applies alone"
    );
}

/// **`color` and `font-size` inherit and nothing else does; `unset` inherits,
/// `initial` does not** — and a parent's inline colour changing reaches a child
/// whose own inputs did not change.
#[test]
fn inherited_properties_reach_descendants_and_nothing_else_does() {
    let sheet = "
        .panel { color: #ff0000; font-size: 20px; background: #0000ff; padding: 3px; }
        .own { color: #00ff00; }
        .initial { color: initial; font-size: initial; }
        .reset { color: unset; }
    ";
    let build = |ui: &mut Ui, panel_inline: &[Declaration]| {
        let mut keys = Vec::new();
        ui.block(".panel", panel_inline, |ui| {
            keys.push(ui.span("", "direct", &[]).key);
            ui.block(".own", &[], |ui| keys.push(ui.span("", "own", &[]).key));
            ui.block(".initial", &[], |ui| {
                keys.push(ui.span("", "initial", &[]).key)
            });
            ui.block(".reset", &[], |ui| keys.push(ui.span("", "reset", &[]).key));
            ui.block("", &[], |ui| {
                ui.block("", &[], |ui| keys.push(ui.span("", "deep", &[]).key));
            });
        });
        keys
    };
    let mut ui = Ui::new();
    ui.add_stylesheet("inherit.css", sheet);
    let mut keys = Vec::new();
    frame(&mut ui, idle(), |ui| keys = build(ui, &[]));

    let direct = style_of(&ui, keys[0]);
    assert_eq!((direct.color, direct.font_size), (RED, 20.0));
    assert_eq!(direct.background, [0.0; 4], "background inherited");
    assert_eq!(
        direct.padding,
        NodeStyle::DEFAULT.padding,
        "padding inherited"
    );
    assert_eq!(style_of(&ui, keys[1]).color, GREEN);
    let initial = style_of(&ui, keys[2]);
    assert_eq!(
        (initial.color, initial.font_size),
        (NodeStyle::DEFAULT.color, NodeStyle::DEFAULT.font_size)
    );
    assert_eq!(style_of(&ui, keys[3]).color, RED, "`unset` did not inherit");
    assert_eq!(
        style_of(&ui, keys[4]).color,
        RED,
        "two levels down lost the colour"
    );

    frame(&mut ui, idle(), |ui| {
        keys = build(ui, &[Declaration::Color(BLUE)])
    });
    assert_eq!(
        style_of(&ui, keys[4]).color,
        BLUE,
        "a cached child kept its parent's old inherited colour"
    );
    assert_eq!(style_of(&ui, keys[1]).color, GREEN);
}

/// **`var()` substitutes an inherited or own custom property, falls back,
/// nests in a function, and a cycle or a missing variable unsets the property
/// over an earlier rule** — with a warning naming where it was written.
#[test]
fn var_resolves_through_the_tree_and_an_invalid_one_unsets_its_property() {
    let sheet = "\
.theme { --accent: #00ff00; --pad: 4px 6px; --red: 255; --loop: var(--loop); }
.card { background: var(--accent); padding: var(--pad); width: var(--missing, 30px);
  height: var(--loop, 7px); border-color: rgb(var(--red), 0, 0); margin-left: 9px; }
.dark { --accent: #000000; }
.late { margin-left: var(--nope); }
";
    let logs = crcbl_core::log::capture();
    let mut ui = Ui::new();
    ui.add_stylesheet("vars.css", sheet);
    let mut keys = Vec::new();
    frame(&mut ui, idle(), |ui| {
        ui.block(".theme", &[], |ui| {
            keys.push(ui.block(".card", &[], |_| {}).key);
            keys.push(ui.block(".card.dark.late", &[], |_| {}).key);
        });
    });
    let card = style_of(&ui, keys[0]);
    assert_eq!(
        card.background, GREEN,
        "an inherited variable did not substitute"
    );
    assert_eq!(
        card.padding,
        Edges {
            top: Length::Px(4.0),
            right: Length::Px(6.0),
            bottom: Length::Px(4.0),
            left: Length::Px(6.0)
        },
        "a shorthand did not expand after substitution"
    );
    assert_eq!(
        card.width,
        LengthAuto::Px(30.0),
        "the fallback was not used"
    );
    assert_eq!(
        card.height,
        LengthAuto::Px(7.0),
        "a cyclic variable was not invalid"
    );
    assert_eq!(
        card.border_color, RED,
        "var() inside rgb() did not substitute"
    );
    assert_eq!(card.margin.left, LengthAuto::Px(9.0));

    let dark = style_of(&ui, keys[1]);
    assert_eq!(
        dark.background, BLACK,
        "an own custom property did not shadow the inherited one"
    );
    assert_eq!(
        dark.margin.left,
        NodeStyle::DEFAULT.margin.left,
        "an invalid var() kept the earlier rule's value instead of unsetting"
    );
    let warned = logs
        .records()
        .into_iter()
        .filter(|record| {
            record
                .message
                .starts_with("vars.css:5:9: warning: `margin-left")
        })
        .count();
    assert_eq!(warned, 1, "{:#?}", logs.records());
}

// ---------------------------------------------------------------------------
// The resolve counter
// ---------------------------------------------------------------------------

struct Hovered {
    button: NodeKey,
    hint: NodeKey,
}

/// A 200-wide column: a 50 × 20 `.button`, a `.list` of three 10-high `.row`s
/// and a `.hint` — whose rules are hover on the button, a descendant rule with
/// no pseudo-class for the rows, and hover on the panel for the hint.
fn hovered_page(ui: &mut Ui) -> Hovered {
    let fixed = |width: f32, height: f32| {
        [
            Declaration::Width(LengthAuto::Px(width)),
            Declaration::Height(LengthAuto::Px(height)),
        ]
    };
    let mut button = None;
    let mut hint = None;
    ui.block(
        ".panel",
        &[Declaration::FlexDirection(FlexDirection::Column)],
        |ui| {
            button = Some(ui.block(".button", &fixed(50.0, 20.0), |_| {}).key);
            ui.block(
                ".list",
                &[Declaration::FlexDirection(FlexDirection::Column)],
                |ui| {
                    for row in 0..3 {
                        ui.block_keyed(
                            row,
                            ".row",
                            &[Declaration::Width(LengthAuto::Px(200.0))],
                            |_| {},
                        );
                    }
                },
            );
            hint = Some(ui.block(".hint", &fixed(200.0, 10.0), |_| {}).key);
        },
    );
    Hovered {
        button: button.expect("built"),
        hint: hint.expect("built"),
    }
}

/// One frame of [`hovered_page`] at `pointer`: what resolution did, and the
/// keys.
fn step(ui: &mut Ui, pointer: PointerInput) -> (StyleStats, Hovered) {
    let mut keys = None;
    frame(ui, pointer, |ui| keys = Some(hovered_page(ui)));
    (ui.style_stats(), keys.expect("built"))
}

const HOVERED_SHEET: &str = "
    .button:hover { background: #ff0000; }
    .list .row { height: 10px; }
    .panel:hover .hint { color: #00ff00; }
";

/// **A pointer move re-resolves exactly the nodes whose candidate rules test
/// what changed** — the button for its own hover, the hint for the panel's —
/// and none of the nodes whose hover also changed but whose rules never
/// mention it; and three rows of one class merge their rules once.
#[test]
fn a_hover_change_re_resolves_only_the_nodes_whose_rules_depend_on_it() {
    let mut ui = Ui::new();
    ui.add_stylesheet("hover.css", HOVERED_SHEET);
    let nowhere = idle();
    let on_row = hover(Vec2::new(100.0, 25.0));
    let on_button = hover(Vec2::new(10.0, 10.0));

    let first = step(&mut ui, nowhere).0;
    assert_eq!(first.nodes, 7);
    assert_eq!(first.resolves, 7, "a first frame resolves everything");
    assert_eq!(
        first.definitions, 2,
        "one definition for the unmatched nodes and one for the rows"
    );
    assert_eq!(
        step(&mut ui, nowhere).0.resolves,
        0,
        "an unchanged frame resolved something"
    );

    // Over a row: the panel, the list and the row are hovered now. Only the
    // hint's rule reads any of that.
    assert_eq!(
        step(&mut ui, on_row).0.resolves,
        1,
        "hovering a row re-resolved more than the hint"
    );
    let (_, keys) = step(&mut ui, on_row);
    assert_eq!(style_of(&ui, keys.hint).color, GREEN);
    assert_eq!(ui.style_stats().resolves, 0);

    // Onto the button: its own hover changes; the panel is still hovered.
    assert_eq!(
        step(&mut ui, on_button).0.resolves,
        1,
        "moving within the panel re-resolved the hint"
    );
    let keys = step(&mut ui, on_button).1;
    assert_eq!(style_of(&ui, keys.button).background, RED);
    assert_eq!(ui.style_stats().resolves, 0);

    let (away, keys) = step(&mut ui, nowhere);
    assert_eq!(
        away.resolves, 2,
        "leaving should re-resolve the button and the hint"
    );
    assert_eq!(style_of(&ui, keys.button).background, [0.0; 4]);
    assert_eq!(style_of(&ui, keys.hint).color, NodeStyle::DEFAULT.color);
}

/// **A rule that changes only paint on hover clears no layout cache; one that
/// changes a size clears that node's and its ancestors' and nobody else's.**
#[test]
fn a_paint_only_hover_keeps_every_layout_cache_and_a_size_change_clears_only_its_chain() {
    let sheet = "
        .paint { width: 50px; height: 20px; }
        .paint:hover { background: #ff0000; border-color: #00ff00; color: #0000ff; border-top-left-radius: 3px; }
        .grow { width: 40px; height: 20px; }
        .grow:hover { width: 60px; }
        .sibling { width: 30px; height: 20px; }
    ";
    let build = |ui: &mut Ui| {
        let mut keys = Vec::new();
        let root = ui
            .block("#root", &[], |ui| {
                keys.push(ui.block(".paint", &[], |_| {}).key);
                keys.push(ui.block(".grow", &[], |_| {}).key);
                keys.push(ui.block(".sibling", &[], |_| {}).key);
            })
            .key;
        keys.insert(0, root);
        keys
    };
    let atlas = FontAtlas::built_in();
    let mut ui = Ui::new();
    ui.add_stylesheet("paint.css", sheet);
    let mut keys = Vec::new();
    frame(&mut ui, idle(), |ui| keys = build(ui));
    frame(&mut ui, idle(), |ui| keys = build(ui));

    ui.begin_frame(hover(Vec2::new(10.0, 10.0)));
    keys = build(&mut ui);
    let [root, paint, grow, sibling] = keys[..] else {
        unreachable!("four nodes");
    };
    assert_eq!(
        style_of(&ui, paint).background,
        RED,
        "the hover rule did not apply"
    );
    for key in [root, paint, grow, sibling] {
        assert!(
            !cache_is_empty(&ui, key),
            "a paint-only hover cleared {key:?}'s layout cache"
        );
    }
    ui.layout(Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);

    ui.begin_frame(hover(Vec2::new(60.0, 10.0)));
    build(&mut ui);
    assert!(
        cache_is_empty(&ui, grow) && cache_is_empty(&ui, root),
        "a size change kept a stale cache"
    );
    assert!(
        !cache_is_empty(&ui, paint) && !cache_is_empty(&ui, sibling),
        "a size change cleared its siblings' caches"
    );
    ui.layout(Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
    assert_eq!(
        ui.rect(sibling).expect("laid out").0.x,
        110.0,
        "the sibling did not move"
    );
}

// ---------------------------------------------------------------------------
// Stylesheet changes
// ---------------------------------------------------------------------------

/// A file under the system's temporary directory, removed when dropped.
struct TempSheet(std::path::PathBuf);

impl TempSheet {
    fn new(test: &str, css: &str) -> Self {
        let path = std::env::temp_dir().join(format!("crcbl-ui-{test}-{}.css", std::process::id()));
        std::fs::write(&path, css).expect("the temporary sheet is written");
        Self(path)
    }

    fn write(&self, css: &str) {
        std::fs::write(&self.0, css).expect("the temporary sheet is rewritten");
    }
}

impl Drop for TempSheet {
    fn drop(&mut self) {
        // A leftover file in the temporary directory is harmless; a panic here
        // would hide the assertion that failed.
        if let Err(error) = std::fs::remove_file(&self.0) {
            eprintln!("{}: not removed: {error}", self.0.display());
        }
    }
}

fn width_of_x(ui: &mut Ui) -> LengthAuto {
    let mut key = None;
    frame(ui, idle(), |ui| key = Some(ui.block(".x", &[], |_| {}).key));
    style_of(ui, key.expect("built")).width
}

/// **A reload with a parse error keeps the last good sheet and says where the
/// error is; the next good write replaces it, one full re-resolve follows, and
/// the file is looked at no more often than the poll interval.**
#[test]
fn a_reload_keeps_the_last_good_sheet_on_a_parse_error_and_takes_the_next_good_one() {
    // Each write is a different length, so the stamp moves whatever the
    // filesystem's clock granularity.
    let file = TempSheet::new("reload", ".x { width: 10px; }");
    let logs = crcbl_core::log::capture();
    let mut ui = Ui::new();
    ui.load_stylesheet(&file.0).expect("the sheet loads");
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(10.0));
    let generation = ui.stylesheet_generation();
    assert_eq!(
        ui.poll_stylesheets(Duration::ZERO),
        0,
        "an untouched file was reloaded"
    );

    file.write(".x { width: 20px; }\n.y + .z { width: 1px; }");
    assert_eq!(
        ui.poll_stylesheets(STYLESHEET_POLL_INTERVAL),
        0,
        "a sheet with an error was taken"
    );
    assert_eq!(
        width_of_x(&mut ui),
        LengthAuto::Px(10.0),
        "the last good sheet was not kept"
    );
    assert_eq!(ui.stylesheet_generation(), generation);
    let name = file.0.display().to_string();
    let records = logs.records();
    assert!(
        records
            .iter()
            .any(|record| record.message.starts_with(&format!("{name}:2:1: error:"))),
        "the error was not reported at its file and line: {records:#?}"
    );
    assert!(
        records
            .iter()
            .any(|record| record.message.contains("keeping the last good sheet"))
    );

    file.write(".x { width: 30px; }");
    let within = STYLESHEET_POLL_INTERVAL + STYLESHEET_POLL_INTERVAL / 2;
    assert_eq!(
        ui.poll_stylesheets(within),
        0,
        "the file was looked at inside the interval"
    );
    assert_eq!(
        ui.poll_stylesheets(STYLESHEET_POLL_INTERVAL * 2),
        1,
        "the good write was not taken"
    );
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(30.0));
    assert_eq!(ui.stylesheet_generation(), generation + 1);
    assert_eq!(
        ui.style_stats().resolves,
        1,
        "the reload did not re-resolve the node"
    );
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(30.0));
    assert_eq!(ui.style_stats().resolves, 0);
}

/// **`replace_stylesheet` returns the errors that kept the old sheet**, located,
/// and takes a sheet whose only problems are warnings.
#[test]
fn replacing_a_sheet_returns_its_errors_and_takes_one_with_only_warnings() {
    let mut ui = Ui::new();
    let sheet = ui.add_stylesheet("live.css", ".x { width: 10px; }");
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(10.0));

    let errors = ui
        .replace_stylesheet(sheet, ".x { width: 20px; }\n\n  ::before { }")
        .expect_err("an error was accepted");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(
        (errors[0].line, errors[0].column, errors[0].severity),
        (3, 3, Severity::Error)
    );
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(10.0));

    ui.replace_stylesheet(sheet, ".x { width: 20px; wobble: 1px; }")
        .expect("warnings are not errors");
    assert_eq!(width_of_x(&mut ui), LengthAuto::Px(20.0));
}

/// **A malformed builder selector is reported once, and the node is still
/// built and laid out** with no selector at all.
#[test]
fn a_malformed_node_selector_warns_once_and_the_node_is_still_built() {
    let logs = crcbl_core::log::capture();
    let mut ui = Ui::new();
    ui.add_stylesheet("any.css", "* { height: 4px; }");
    let mut key = None;
    for _ in 0..3 {
        frame(&mut ui, idle(), |ui| {
            key = Some(ui.block(".a b", &[], |_| {}).key)
        });
    }
    let warnings = logs
        .records()
        .into_iter()
        .filter(|record| record.message.contains("`.a b` is not a node selector"))
        .count();
    assert_eq!(warnings, 1);
    assert_eq!(
        style_of(&ui, key.expect("built")).height,
        LengthAuto::Px(4.0)
    );
}
