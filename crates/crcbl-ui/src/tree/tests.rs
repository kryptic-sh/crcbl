//! The tree's identity, store, invalidation, hit testing and emission, each
//! held to the behaviour it exists for.

use glam::Vec2;

use super::*;
use crate::draw_list::{DrawCommand, DrawList};

const fn px(value: f32) -> LengthAuto {
    LengthAuto::Px(value)
}

/// A `width` × `height` block with nothing else set.
const fn sized(width: f32, height: f32) -> NodeStyle {
    NodeStyle {
        width: px(width),
        height: px(height),
        ..NodeStyle::DEFAULT
    }
}

/// One frame: begin at `pointer`, build, lay out at the origin with unbounded
/// space.
pub(super) fn frame(ui: &mut Ui, pointer: PointerInput, build: impl FnOnce(&mut Ui)) {
    ui.begin_frame(pointer);
    build(ui);
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );
}

pub(super) fn idle() -> PointerInput {
    PointerInput::hovering(Vec2::splat(-1.0))
}

pub(super) fn cache_is_empty(ui: &Ui, key: NodeKey) -> bool {
    ui.store.by_key(key).expect("stored").cache.is_empty()
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A page built by the same code, frame after frame.
fn page(ui: &mut Ui) -> Vec<NodeKey> {
    let mut keys = Vec::new();
    let root = ui.block("", &[], |ui| {
        for _ in 0..3 {
            keys.push(ui.block("", &sized(10.0, 10.0).declarations(), |_| {}).key);
        }
        keys.push(ui.span("", "label", &[]).key);
        keys.push(
            ui.block("#named", &sized(5.0, 5.0).declarations(), |_| {})
                .key,
        );
    });
    keys.insert(0, root.key);
    keys
}

/// **A rebuild from the same code gives every node the key it had**, and no
/// two nodes of one frame share one — including three built by one line of a
/// loop, which the call site's occurrence count tells apart.
#[test]
fn a_rebuild_gives_every_node_the_key_it_had_last_frame() {
    let mut ui = Ui::new();
    let mut first = Vec::new();
    frame(&mut ui, idle(), |ui| first = page(ui));
    let mut second = Vec::new();
    frame(&mut ui, idle(), |ui| second = page(ui));

    assert_eq!(first, second);
    let mut unique = first.clone();
    unique.sort_by_key(|key| key.0);
    unique.dedup();
    assert_eq!(
        unique.len(),
        first.len(),
        "two nodes shared a key: {first:?}"
    );
    assert!(ui.duplicate_keys().is_empty());
}

/// Builds `rows` as a list, keyed by name when `keyed`, and scrolls the row
/// named `scrolled` if it is given. Returns each row's scroll offset as read
/// back inside it.
fn list(ui: &mut Ui, rows: &[&str], keyed: bool, scrolled: Option<&str>) -> Vec<(String, Vec2)> {
    let mut seen = Vec::new();
    ui.block("", &[], |ui| {
        for &row in rows {
            let body = |ui: &mut Ui| {
                if scrolled == Some(row) {
                    ui.set_scroll_offset(Vec2::new(0.0, 7.0));
                }
                seen.push((row.to_owned(), ui.scroll_offset()));
            };
            if keyed {
                ui.block_keyed(row, "", &sized(10.0, 10.0).declarations(), body);
            } else {
                ui.block("", &sized(10.0, 10.0).declarations(), body);
            }
        }
    });
    seen
}

/// **A keyed row keeps its key and its stored state when the list reorders**,
/// and an unkeyed one does not — the hazard the keyed builder exists for.
#[test]
fn a_keyed_row_keeps_its_state_when_the_list_reorders_and_a_call_site_row_does_not() {
    for keyed in [true, false] {
        let mut ui = Ui::new();
        frame(&mut ui, idle(), |ui| {
            list(ui, &["a", "b", "c"], keyed, Some("b"));
        });
        let mut after = Vec::new();
        frame(&mut ui, idle(), |ui| {
            after = list(ui, &["c", "a", "b"], keyed, None);
        });
        let scrolled: Vec<&str> = after
            .iter()
            .filter(|(_, offset)| *offset != Vec2::ZERO)
            .map(|(row, _)| row.as_str())
            .collect();
        if keyed {
            assert_eq!(scrolled, ["b"], "the scroll did not follow row b");
        } else {
            assert_eq!(
                scrolled,
                ["a"],
                "by call site the second row's state belongs to whatever is second"
            );
        }
    }
}

/// **A duplicate key is reported once per frame per key, and both nodes are
/// still laid out as two**: three blocks sharing one id make one warning, not
/// two, and not a panic.
#[test]
fn a_duplicate_key_warns_once_per_frame_and_every_node_still_lays_out() {
    let logs = crcbl_core::log::capture();
    let mut ui = Ui::new();
    let mut keys = Vec::new();
    for round in 0..2 {
        keys.clear();
        frame(&mut ui, idle(), |ui| {
            ui.block("", &[], |ui| {
                for _ in 0..3 {
                    keys.push(
                        ui.block("#same", &sized(10.0, 10.0).declarations(), |_| {})
                            .key,
                    );
                }
            });
        });
        assert_eq!(ui.duplicate_keys().len(), 1, "frame {round}");
        let warnings = logs
            .records()
            .into_iter()
            .filter(|record| record.message.contains("share the key"))
            .count();
        assert_eq!(warnings, round + 1, "one warning per frame, frame {round}");
    }
    let rects: Vec<_> = keys
        .iter()
        .map(|&key| ui.rect(key).expect("laid out"))
        .collect();
    assert_eq!(
        rects.iter().map(|(min, _)| min.x).collect::<Vec<_>>(),
        [0.0, 10.0, 20.0],
        "the duplicates were not laid out as three boxes"
    );
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// **A node a frame did not build is gone after that frame's layout**, and
/// comes back with nothing it had.
#[test]
fn a_node_no_frame_built_is_pruned_with_its_state() {
    let mut ui = Ui::new();
    let mut gone = None;
    let build = |ui: &mut Ui, both: bool, gone: &mut Option<NodeKey>, scroll: bool| {
        ui.block("", &[], |ui| {
            ui.block("", &sized(10.0, 10.0).declarations(), |_| {});
            if both {
                *gone = Some(
                    ui.block("#b", &sized(10.0, 10.0).declarations(), |ui| {
                        if scroll {
                            ui.set_scroll_offset(Vec2::ONE);
                        }
                        assert_eq!(ui.scroll_offset() == Vec2::ONE, scroll);
                    })
                    .key,
                );
            }
        });
    };
    frame(&mut ui, idle(), |ui| build(ui, true, &mut gone, true));
    assert_eq!(ui.len(), 3);
    frame(&mut ui, idle(), |ui| build(ui, false, &mut gone, false));
    assert_eq!(ui.len(), 2, "the unbuilt node was not pruned");
    assert_eq!(ui.rect(gone.expect("built")), None);
    // Built again, it starts over: the scroll it had is not there.
    frame(&mut ui, idle(), |ui| build(ui, true, &mut gone, false));
    assert_eq!(ui.len(), 3);
}

// ---------------------------------------------------------------------------
// Hit testing
// ---------------------------------------------------------------------------

/// A 50 × 50 block `left` pixels into a root, and whether it was hovered.
fn moving(ui: &mut Ui, left: f32) -> bool {
    let style = NodeStyle {
        margin: Edges {
            left: px(left),
            ..Edges::all(px(0.0))
        },
        ..sized(50.0, 50.0)
    };
    let mut hovered = false;
    ui.block("", &[], |ui| {
        hovered = ui.block("", &style.declarations(), |_| {}).hovered;
    });
    hovered
}

/// **Hover is tested against last frame's rectangle**: a block that moves
/// away this frame is still hovered where it was, and is hovered where it went
/// only on the frame after.
#[test]
fn hover_is_resolved_against_last_frames_rectangle() {
    let mut ui = Ui::new();
    let mut hovered = false;
    frame(
        &mut ui,
        PointerInput::hovering(Vec2::new(25.0, 25.0)),
        |ui| {
            hovered = moving(ui, 0.0);
        },
    );
    assert!(!hovered, "nothing was laid out before the first frame");

    frame(
        &mut ui,
        PointerInput::hovering(Vec2::new(25.0, 25.0)),
        |ui| {
            hovered = moving(ui, 100.0);
        },
    );
    assert!(hovered, "last frame's rect was under the pointer");

    frame(
        &mut ui,
        PointerInput::hovering(Vec2::new(25.0, 25.0)),
        |ui| {
            hovered = moving(ui, 100.0);
        },
    );
    assert!(!hovered, "the block left that spot a frame ago");

    frame(
        &mut ui,
        PointerInput::hovering(Vec2::new(125.0, 25.0)),
        |ui| {
            hovered = moving(ui, 100.0);
        },
    );
    assert!(hovered, "the block is here now");
}

/// A 100 × 100 parent with a 20 × 20 child at its top-left, and each one's
/// response.
fn nested(ui: &mut Ui) -> (Response, Response) {
    let mut child = None;
    let parent = ui.block("#parent", &sized(100.0, 100.0).declarations(), |ui| {
        child = Some(ui.block("#child", &sized(20.0, 20.0).declarations(), |_| {}));
    });
    (parent, child.expect("built"))
}

fn press(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: true,
        released: false,
    }
}

fn release(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: false,
        released: true,
    }
}

/// **A press latches on the topmost node, whose ancestors stay hovered; a
/// release over it clicks it, and a release elsewhere clicks nothing** —
/// `UiState`'s capture, driven by the tree's own hit test.
#[test]
fn a_press_captures_the_topmost_node_and_only_a_release_over_it_clicks() {
    let inside = Vec2::new(10.0, 10.0);

    let mut ui = Ui::new();
    let mut seen = None;
    frame(&mut ui, idle(), |ui| seen = Some(nested(ui)));

    frame(&mut ui, press(inside), |ui| seen = Some(nested(ui)));
    let (parent, child) = seen.expect("built");
    assert!(child.pressed && child.hovered, "{child:?}");
    assert!(!parent.pressed && parent.hovered, "{parent:?}");

    frame(&mut ui, release(inside), |ui| seen = Some(nested(ui)));
    let (parent, child) = seen.expect("built");
    assert!(child.clicked, "{child:?}");
    assert!(!parent.clicked, "{parent:?}");

    // Pressed on one of two siblings, dragged onto the other, released there.
    let siblings = |ui: &mut Ui| {
        let mut pair = Vec::new();
        ui.block("", &[], |ui| {
            pair.push(ui.block("#a", &sized(20.0, 20.0).declarations(), |_| {}));
            pair.push(ui.block("#b", &sized(20.0, 20.0).declarations(), |_| {}));
        });
        pair
    };
    let (on_a, on_b) = (Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
    let mut pair = Vec::new();
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| pair = siblings(ui));
    frame(&mut ui, press(on_a), |ui| pair = siblings(ui));
    frame(&mut ui, press(on_b), |ui| pair = siblings(ui));
    assert!(pair[0].pressed, "the capture let go on a drag: {pair:?}");
    assert!(
        !pair[1].hovered,
        "a node outside the capture lit up under a drag: {pair:?}"
    );
    frame(&mut ui, release(on_b), |ui| pair = siblings(ui));
    assert!(
        !pair[0].clicked && !pair[1].clicked,
        "a press and a release on two nodes clicked one: {pair:?}"
    );
    frame(&mut ui, press(on_b), |ui| pair = siblings(ui));
    assert!(
        pair[1].pressed,
        "the release did not free the capture: {pair:?}"
    );
}

/// Inside a block's closure, `hovered`, `pressed` and `clicked` read that
/// block, and outside every block they read false.
#[test]
fn the_ui_queries_read_the_innermost_open_block() {
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        nested(ui);
    });
    ui.begin_frame(press(Vec2::new(50.0, 50.0)));
    let mut inside = (false, false);
    ui.block("#parent", &sized(100.0, 100.0).declarations(), |ui| {
        ui.block("#child", &sized(20.0, 20.0).declarations(), |_| {});
        inside = (ui.hovered(), ui.pressed());
    });
    assert_eq!(inside, (true, true));
    assert!(!ui.hovered() && !ui.pressed() && !ui.clicked());
}

// ---------------------------------------------------------------------------
// Cache invalidation
// ---------------------------------------------------------------------------

struct Keys {
    root: NodeKey,
    left: NodeKey,
    leaf: NodeKey,
    right: NodeKey,
}

/// `root > (left > leaf), right`, with `leaf` as wide as `leaf_width` and
/// `right` filled with `right_color`.
fn invalidation_tree(ui: &mut Ui, leaf_width: f32, right_color: [f32; 4]) -> Keys {
    let mut left = None;
    let mut leaf = None;
    let mut right = None;
    let root = ui.block("#root", &[], |ui| {
        left = Some(
            ui.block("#left", &[], |ui| {
                leaf = Some(
                    ui.block("#leaf", &sized(leaf_width, 10.0).declarations(), |_| {})
                        .key,
                );
            })
            .key,
        );
        let style = NodeStyle {
            background: right_color,
            ..sized(30.0, 10.0)
        };
        right = Some(ui.block("#right", &style.declarations(), |_| {}).key);
    });
    Keys {
        root: root.key,
        left: left.expect("built"),
        leaf: leaf.expect("built"),
        right: right.expect("built"),
    }
}

/// **A layout change clears the changed node's cache and its ancestors' and
/// nobody else's; a paint change clears none** — and the layout that follows
/// matches one computed from nothing.
#[test]
fn a_style_change_clears_the_node_and_its_ancestors_and_a_sibling_keeps_its_cache() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        invalidation_tree(ui, 10.0, red);
    });

    // Rebuilt unchanged: every cache survives the build.
    ui.begin_frame(idle());
    let keys = invalidation_tree(&mut ui, 10.0, red);
    for key in [keys.root, keys.left, keys.leaf, keys.right] {
        assert!(
            !cache_is_empty(&ui, key),
            "{key:?} lost its cache to a no-op rebuild"
        );
    }
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );

    // The leaf widens.
    ui.begin_frame(idle());
    let keys = invalidation_tree(&mut ui, 25.0, red);
    for key in [keys.leaf, keys.left, keys.root] {
        assert!(cache_is_empty(&ui, key), "{key:?} kept a stale cache");
    }
    assert!(
        !cache_is_empty(&ui, keys.right),
        "the sibling's cache was cleared by a change that was not its own"
    );
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );
    let right = ui.rect(keys.right).expect("laid out");
    assert_eq!(
        right.0.x, 25.0,
        "the sibling did not move for its neighbour"
    );

    let mut fresh = Ui::new();
    frame(&mut fresh, idle(), |ui| {
        invalidation_tree(ui, 25.0, red);
    });
    for key in [keys.root, keys.left, keys.leaf, keys.right] {
        assert_eq!(
            ui.rect(key),
            fresh.rect(key),
            "the cached layout of {key:?} is stale"
        );
    }

    // Only a colour changes.
    ui.begin_frame(idle());
    let keys = invalidation_tree(&mut ui, 25.0, [0.0, 0.0, 1.0, 1.0]);
    for key in [keys.root, keys.left, keys.leaf, keys.right] {
        assert!(!cache_is_empty(&ui, key), "a paint change cleared {key:?}");
    }
}

/// **A node whose children change clears itself and its ancestors**, even
/// when its own style did not move.
#[test]
fn adding_a_child_clears_the_parent_and_its_ancestors() {
    let build = |ui: &mut Ui, extra: bool| {
        let mut inner = None;
        let outer = ui.block("#outer", &[], |ui| {
            inner = Some(
                ui.block("#inner", &[], |ui| {
                    ui.block("#one", &sized(10.0, 10.0).declarations(), |_| {});
                    if extra {
                        ui.block("#two", &sized(10.0, 10.0).declarations(), |_| {});
                    }
                })
                .key,
            );
        });
        (outer.key, inner.expect("built"))
    };
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        build(ui, false);
    });
    ui.begin_frame(idle());
    let (outer, inner) = build(&mut ui, true);
    assert!(cache_is_empty(&ui, inner) && cache_is_empty(&ui, outer));
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );
    assert_eq!(ui.rect(inner).expect("laid out").1.x, 20.0);
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// **Text is measured once per (text, scale, width bucket)**, a second span of
/// the same text is answered from the cache, and a frame whose tree did not
/// change measures nothing at all.
#[test]
fn text_is_measured_once_and_an_unchanged_frame_measures_nothing() {
    let atlas = FontAtlas::built_in();
    let build = |ui: &mut Ui, text: &str| {
        ui.block("", &[], |ui| {
            ui.block("", &[], |ui| {
                ui.span("", text, &[]);
            });
            ui.block("", &[], |ui| {
                ui.span("", text, &[]);
            });
        })
    };
    let mut ui = Ui::new();
    ui.begin_frame(idle());
    let root = build(&mut ui, "HEALTH");
    ui.layout(Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
    let first = (ui.measure.misses, ui.measure.hits);
    assert!(first.0 >= 1, "nothing was measured");
    assert!(
        first.1 >= 1,
        "the second span re-measured the first one's text"
    );
    assert_eq!(
        ui.rect(root.key).expect("laid out").1.x,
        2.0 * atlas.text_width("HEALTH", 1.0),
        "the measurement is not the font's"
    );

    ui.begin_frame(idle());
    build(&mut ui, "HEALTH");
    ui.layout(Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
    assert_eq!(
        (ui.measure.misses, ui.measure.hits),
        first,
        "an unchanged tree went back to the measure callback"
    );

    ui.begin_frame(idle());
    build(&mut ui, "SHIELD!");
    ui.layout(Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
    assert!(ui.measure.misses > first.0, "new text was not measured");
    assert_eq!(
        ui.measure.texts(),
        1,
        "the old text's measurements outlived its span"
    );
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

fn emitted(ui: &Ui) -> DrawList {
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

/// **`overflow: hidden` clips exactly the block's children to its padding
/// box**, and pops the clip after them.
#[test]
fn overflow_hidden_clips_the_children_to_the_padding_box() {
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        let clipper = NodeStyle {
            overflow: Overflow::Hidden,
            border: Edges::all(2.0),
            padding: Edges::all(Length::Px(3.0)),
            background: [1.0; 4],
            ..sized(40.0, 40.0)
        };
        ui.block("", &[], |ui| {
            ui.block("", &clipper.declarations(), |ui| {
                ui.block(
                    "",
                    &NodeStyle {
                        background: [0.5; 4],
                        ..sized(100.0, 100.0)
                    }
                    .declarations(),
                    |_| {},
                );
            });
            ui.block(
                "",
                &NodeStyle {
                    background: [0.25; 4],
                    ..sized(10.0, 10.0)
                }
                .declarations(),
                |_| {},
            );
        });
    });
    let list = emitted(&ui);
    let clips = list.clips();
    assert_eq!(list.len(), 3, "{:?}", list.commands());
    assert_eq!(
        clips[0],
        ClipRect::NONE,
        "the clipping block is not clipped itself"
    );
    assert_eq!(
        clips[1],
        ClipRect {
            min: Vec2::splat(2.0),
            max: Vec2::splat(38.0)
        },
        "the child is not clipped to the padding box"
    );
    assert_eq!(clips[2], ClipRect::NONE, "the clip outlived the block");
    assert_eq!(list.clip(), ClipRect::NONE);
}

/// **A square block is a rect and an outline; a rounded one is one rounded
/// rect; uneven borders are a band per side** — and a transparent block with
/// no border draws nothing.
#[test]
fn a_block_paints_with_the_primitive_its_style_needs() {
    let fill = [0.2, 0.3, 0.4, 1.0];
    let edge = [1.0, 1.0, 0.0, 1.0];
    let cases: [(NodeStyle, &[&str]); 4] = [
        (
            NodeStyle {
                background: fill,
                border: Edges::all(1.0),
                border_color: edge,
                ..sized(20.0, 20.0)
            },
            &["Rect", "RectOutline"],
        ),
        (
            NodeStyle {
                background: fill,
                border: Edges::all(1.0),
                border_color: edge,
                radii: CornerRadii::uniform(4.0),
                ..sized(20.0, 20.0)
            },
            &["RoundedRect"],
        ),
        (
            NodeStyle {
                border: Edges {
                    top: 1.0,
                    right: 2.0,
                    bottom: 3.0,
                    left: 4.0,
                },
                border_color: edge,
                ..sized(20.0, 20.0)
            },
            &["Rect", "Rect", "Rect", "Rect"],
        ),
        (sized(20.0, 20.0), &[]),
    ];
    for (style, want) in cases {
        let mut ui = Ui::new();
        frame(&mut ui, idle(), |ui| {
            ui.block("", &style.declarations(), |_| {});
        });
        let list = emitted(&ui);
        let got: Vec<&str> = list
            .commands()
            .iter()
            .map(|command| match command {
                DrawCommand::Rect { .. } => "Rect",
                DrawCommand::RectOutline { .. } => "RectOutline",
                DrawCommand::RoundedRect { .. } => "RoundedRect",
                other => panic!("a block drew {other:?}"),
            })
            .collect();
        assert_eq!(got, want, "{style:?}");
    }
}

use crate::draw_list::CornerRadii;

/// **`display: none` draws nothing, takes no space and is never hit**, and
/// neither is anything under it.
#[test]
fn display_none_draws_nothing_takes_no_space_and_is_never_hit() {
    let hidden = NodeStyle {
        display: Display::None,
        background: [1.0; 4],
        ..sized(50.0, 50.0)
    };
    let build = |ui: &mut Ui| {
        let mut responses = Vec::new();
        ui.block("", &[], |ui| {
            responses.push(ui.block("", &hidden.declarations(), |ui| {
                ui.span("", "gone", &[]);
            }));
            responses.push(
                ui.block(
                    "",
                    &NodeStyle {
                        background: [1.0; 4],
                        ..sized(10.0, 10.0)
                    }
                    .declarations(),
                    |_| {},
                ),
            );
        });
        responses
    };
    let mut ui = Ui::new();
    let mut responses = Vec::new();
    frame(&mut ui, idle(), |ui| responses = build(ui));
    assert_eq!(emitted(&ui).len(), 1, "the hidden block drew");
    assert_eq!(ui.rect(responses[1].key).expect("laid out").0.x, 0.0);
    frame(&mut ui, PointerInput::hovering(Vec2::new(5.0, 5.0)), |ui| {
        responses = build(ui)
    });
    assert!(!responses[0].hovered && responses[1].hovered);
}

/// **A scroll offset moves a block's children, not the block**, for drawing
/// and for the hit test alike.
#[test]
fn a_scroll_offset_moves_the_children_for_drawing_and_hitting() {
    let build = |ui: &mut Ui, scroll: bool| {
        let mut child = None;
        let scroller = NodeStyle {
            overflow: Overflow::Hidden,
            flex_direction: FlexDirection::Column,
            ..sized(50.0, 50.0)
        };
        ui.block("", &scroller.declarations(), |ui| {
            if scroll {
                ui.set_scroll_offset(Vec2::new(0.0, 20.0));
            }
            ui.block("", &sized(50.0, 20.0).declarations(), |_| {});
            child = Some(
                ui.block(
                    "",
                    &NodeStyle {
                        background: [1.0; 4],
                        ..sized(50.0, 20.0)
                    }
                    .declarations(),
                    |_| {},
                ),
            );
        });
        child.expect("built")
    };
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        build(ui, true);
    });
    let list = emitted(&ui);
    let DrawCommand::Rect { min, .. } = list.commands()[0] else {
        panic!("{:?}", list.commands());
    };
    assert_eq!(
        min,
        Vec2::ZERO,
        "the second child did not scroll up into view"
    );
    let mut child = None;
    frame(
        &mut ui,
        PointerInput::hovering(Vec2::new(10.0, 5.0)),
        |ui| {
            child = Some(build(ui, false));
        },
    );
    assert!(
        child.expect("built").hovered,
        "the hit test ignored the scroll"
    );
}
