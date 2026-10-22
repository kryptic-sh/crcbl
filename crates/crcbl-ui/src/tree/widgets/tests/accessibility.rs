//! The tree as a screen reader reads it, and the actions a reader asks for.

use accesskit::{
    Action, ActionRequest, Node, NodeId, Rect, Role, TextPosition, TextSelection, Toggled, TreeId,
    TreeUpdate,
};

use super::*;
use crate::tree::{Response, TextInputOptions};

/// The keys of [`form`]'s widgets.
#[derive(Clone, Copy)]
struct Form {
    ok: Response,
    agree: Response,
    name: Response,
    note: Response,
    off: Response,
}

/// One frame of a form: a button, a checkbox editing `agree`, a text input
/// editing `name`, a plain label and a disabled button.
fn form(ui: &mut Ui, pointer: PointerInput, agree: &mut bool, name: &mut String) -> Form {
    frame(ui, pointer, NavInput::default(), |ui| {
        let ok = ui.button("#ok", "OK");
        let agree = ui.checkbox("#agree", "Agree", agree);
        let name = ui.text_input("#name", name);
        let note = ui.span("#note", "Plain words", &[]);
        let mut off = None;
        ui.enabled(false, |ui| off = Some(ui.button("#off", "Off")));
        Form {
            ok,
            agree,
            name,
            note,
            off: off.expect("built inside the scope"),
        }
    })
}

/// The node `key` became in `update`.
fn node(update: &TreeUpdate, key: NodeKey) -> &Node {
    update
        .nodes
        .iter()
        .find(|(id, _)| *id == NodeId(key.0))
        .map(|(_, node)| node)
        .unwrap_or_else(|| panic!("{key:?} is not in the tree"))
}

/// `key`'s laid-out rectangle as AccessKit's.
fn bounds(ui: &Ui, key: NodeKey) -> Rect {
    let (min, max) = rect(ui, key);
    Rect {
        x0: f64::from(min.x),
        y0: f64::from(min.y),
        x1: f64::from(max.x),
        y1: f64::from(max.y),
    }
}

/// A reader's `action` on `key`.
fn ask(action: Action, key: NodeKey) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: NodeId(key.0),
        data: None,
    }
}

/// **Each widget is one node with its role, its name from the text inside
/// it, its state and its laid-out bounds**, under a window whose child is the
/// page, and focus is the window while nothing is focused.
#[test]
fn widgets_become_nodes_with_their_roles_names_states_and_bounds() {
    let mut ui = Ui::new();
    let (mut agree, mut name) = (true, String::from("Ada"));
    let widgets = form(&mut ui, idle(), &mut agree, &mut name);
    let update = ui.accessibility_update();

    let root = update.tree.as_ref().expect("the whole tree").root;
    let window = &update
        .nodes
        .iter()
        .find(|(id, _)| *id == root)
        .expect("root")
        .1;
    assert_eq!(window.role(), Role::Window);
    assert_eq!(
        update.focus, root,
        "focus is not the window with nothing focused"
    );
    let page = ui.nodes[0].key;
    assert_eq!(window.children(), [NodeId(page.0)]);

    let ok = node(&update, widgets.ok.key);
    assert_eq!(ok.role(), Role::Button);
    assert_eq!(ok.label(), Some("OK"));
    assert!(
        ok.children().is_empty(),
        "a button's label span is a node too"
    );
    assert!(ok.supports_action(Action::Click) && ok.supports_action(Action::Focus));
    assert_eq!(ok.bounds(), Some(bounds(&ui, widgets.ok.key)));

    let check = node(&update, widgets.agree.key);
    assert_eq!(check.role(), Role::CheckBox);
    assert_eq!(check.label(), Some("Agree"), "the box or the mark named it");
    assert_eq!(check.toggled(), Some(Toggled::True));
    assert_eq!(check.bounds(), Some(bounds(&ui, widgets.agree.key)));

    let input = node(&update, widgets.name.key);
    assert_eq!(input.role(), Role::TextInput);
    assert_eq!(input.value(), Some("Ada"));
    assert_eq!(
        input.label(),
        None,
        "a text input took its text as its label"
    );

    let note = node(&update, widgets.note.key);
    assert_eq!(note.role(), Role::Label);
    assert_eq!(note.value(), Some("Plain words"));
    assert!(!note.supports_action(Action::Click));
    assert_eq!(note.bounds(), Some(bounds(&ui, widgets.note.key)));

    let off = node(&update, widgets.off.key);
    assert!(off.is_disabled());
    assert!(!off.supports_action(Action::Click) && !off.supports_action(Action::Focus));
}

/// **A reader's click makes the node report `clicked` on the next frame and
/// no other, and flips a checkbox as a pointer's would**; its focus moves
/// focus there on the next frame, which the update then names.
#[test]
fn a_readers_click_fires_the_next_frame_and_its_focus_moves_focus() {
    let mut ui = Ui::new();
    let (mut agree, mut name) = (true, String::new());
    let widgets = form(&mut ui, idle(), &mut agree, &mut name);

    assert!(ui.accessibility_action(&ask(Action::Click, widgets.ok.key)));
    let next = form(&mut ui, idle(), &mut agree, &mut name);
    assert!(next.ok.clicked, "a reader's click did not fire the button");
    let after = form(&mut ui, idle(), &mut agree, &mut name);
    assert!(
        !after.ok.clicked,
        "a reader's click fired for a second frame"
    );

    assert!(ui.accessibility_action(&ask(Action::Click, widgets.agree.key)));
    let flipped = form(&mut ui, idle(), &mut agree, &mut name);
    assert!(
        flipped.agree.changed && !agree,
        "a reader's click left the box"
    );
    let update = ui.accessibility_update();
    assert_eq!(
        node(&update, widgets.agree.key).toggled(),
        Some(Toggled::False)
    );

    assert!(ui.accessibility_action(&ask(Action::Focus, widgets.name.key)));
    let focused = form(&mut ui, idle(), &mut agree, &mut name);
    assert!(focused.name.focused, "a reader's focus did not move focus");
    assert_eq!(ui.focused(), Some(widgets.name.key));
    let update = ui.accessibility_update();
    assert_eq!(update.focus, NodeId(widgets.name.key.0));
    assert!(node(&update, widgets.name.key).supports_action(Action::Blur));

    assert!(ui.accessibility_action(&ask(Action::Blur, widgets.name.key)));
    assert_eq!(
        ui.focused(),
        None,
        "a reader's blur left focus where it was"
    );
}

/// **A node the tree does not hold, an action a node does not offer, and a
/// click on a disabled node are refused**, and the refused click fires
/// nothing on the next frame.
#[test]
fn an_unknown_node_or_an_unoffered_action_is_refused() {
    let mut ui = Ui::new();
    let (mut agree, mut name) = (false, String::new());
    let widgets = form(&mut ui, idle(), &mut agree, &mut name);
    let root = ui.accessibility_update().tree.expect("the whole tree").root;

    assert!(!ui.accessibility_action(&ask(Action::Click, NodeKey(0x5eed))));
    assert!(!ui.accessibility_action(&ActionRequest {
        target_node: root,
        ..ask(Action::Focus, widgets.ok.key)
    }));
    assert!(!ui.accessibility_action(&ask(Action::Expand, widgets.ok.key)));
    assert!(!ui.accessibility_action(&ask(Action::Click, widgets.note.key)));
    assert!(!ui.accessibility_action(&ask(Action::Blur, widgets.ok.key)));
    assert!(!ui.accessibility_action(&ask(Action::Click, widgets.off.key)));
    let next = form(&mut ui, idle(), &mut agree, &mut name);
    assert!(!next.off.clicked, "a disabled button fired");
}

/// **A header's row is a button named by its title, not by its toggle's
/// glyph, and expanded exactly while its body is built.**
#[test]
fn a_headers_row_is_named_by_its_title_and_expanded_while_open() {
    let mut ui = Ui::new();
    let header = |ui: &mut Ui, pointer| {
        frame(ui, pointer, NavInput::default(), |ui| {
            ui.collapsing("#more", "More", |ui| {
                ui.span("", "Hidden words", &[]);
            })
        })
    };
    let row = header(&mut ui, idle());
    let update = ui.accessibility_update();
    let closed = node(&update, row.key);
    assert_eq!(closed.role(), Role::Button);
    assert_eq!(closed.label(), Some("More"), "the toggle's glyph named it");
    assert_eq!(closed.is_expanded(), Some(false));

    assert!(ui.accessibility_action(&ask(Action::Click, row.key)));
    header(&mut ui, idle());
    let update = ui.accessibility_update();
    assert_eq!(node(&update, row.key).is_expanded(), Some(true));
}

/// **A paragraph is a paragraph of labels, one per run**, each holding its
/// run's text as built, so a reader hears the markup's words in order.
#[test]
fn a_paragraph_reads_as_its_runs_in_order() {
    let mut ui = Ui::new();
    let mut runs = Vec::new();
    let paragraph = frame(&mut ui, idle(), NavInput::default(), |ui| {
        ui.paragraph(".body", &[], |p| {
            runs.push(p.run("", "Build finished in ", &[]).key);
            runs.push(p.run("b", "4 minutes", &[]).key);
        })
    });
    let update = ui.accessibility_update();
    let body = node(&update, paragraph.key);
    assert_eq!(body.role(), Role::Paragraph);
    assert_eq!(
        body.children(),
        runs.iter().map(|key| NodeId(key.0)).collect::<Vec<_>>()
    );
    let words: Vec<_> = runs
        .iter()
        .map(|&key| {
            let run = node(&update, key);
            assert_eq!(run.role(), Role::Label);
            run.value().expect("a run holds its text").to_owned()
        })
        .collect();
    assert_eq!(words, ["Build finished in ", "4 minutes"]);
}

/// The name a screen reader hears for `node`, by AccessKit's own rule
/// (`accesskit_consumer`'s `label_comes_from_value`, which the AT-SPI and UI
/// Automation adapters both follow): a `Label` is named by its value, anything
/// else by its label.
fn reader_name(node: &Node) -> Option<&str> {
    if node.role() == Role::Label {
        node.value()
    } else {
        node.label()
    }
}

/// **A named block that still holds a widget keeps its role and is named by
/// its label**: the name stands for its text spans and the button stays.
#[test]
fn a_named_block_holding_a_widget_keeps_its_role() {
    let mut ui = Ui::new();
    let (row, button) = frame(&mut ui, idle(), NavInput::default(), |ui| {
        let mut button = None;
        let row = ui.block(".row", &[], |ui| {
            ui.span(".hl", "ga", &[]);
            ui.span(".tx", "mma", &[]);
            button = Some(ui.button("#go", "Go").key);
        });
        ui.set_accessible_label(row.key, "gamma");
        (row.key, button.expect("built"))
    });
    let update = ui.accessibility_update();
    let named = node(&update, row);
    assert_eq!(named.role(), Role::GenericContainer);
    assert_eq!(reader_name(named), Some("gamma"));
    assert_eq!(
        named.children(),
        [NodeId(button.0)],
        "only the button is left to read"
    );
}

/// **A block named with `set_accessible_label` reads as that name, not as the
/// spans it was drawn in**: "gamma" drawn as "ga" and "mma" so the match can
/// be accented is one label, and without the name it is the two pieces.
#[test]
fn a_named_block_reads_as_its_name_not_its_pieces() {
    let pieces = |ui: &mut Ui, named: bool| {
        frame(ui, idle(), NavInput::default(), |ui| {
            let mut spans = Vec::new();
            let label = ui.block(".label", &[], |ui| {
                spans.push(ui.span(".hl", "ga", &[]).key);
                spans.push(ui.span(".tx", "mma", &[]).key);
            });
            if named {
                ui.set_accessible_label(label.key, "gamma");
            }
            (label.key, spans)
        })
    };
    let mut ui = Ui::new();

    let (label, spans) = pieces(&mut ui, true);
    let update = ui.accessibility_update();
    let named = node(&update, label);
    assert_eq!(named.role(), Role::Label);
    assert_eq!(reader_name(named), Some("gamma"));
    assert!(named.children().is_empty(), "the pieces are not read again");
    for span in spans {
        assert!(
            update.nodes.iter().all(|(id, _)| *id != NodeId(span.0)),
            "{span:?} is one of the name's pieces"
        );
    }

    let (label, spans) = pieces(&mut ui, false);
    let update = ui.accessibility_update();
    assert_eq!(
        node(&update, label).children(),
        spans.iter().map(|key| NodeId(key.0)).collect::<Vec<_>>(),
        "unnamed, a reader hears each piece"
    );
}

/// **The selected row of a selectable list is selected to a reader, and no
/// other row says anything either way**, nor does any row of a plain list.
#[test]
fn a_selectable_lists_selected_row_reads_as_selected() {
    let selections = |ui: &mut Ui, selected: Option<usize>| {
        let list = frame(ui, idle(), NavInput::default(), |ui| {
            ui.selectable_list("#rows", 3, 20.0, selected, |ui, index| {
                ui.span("", format!("row {index}").as_str(), &[]);
            })
        });
        let update = ui.accessibility_update();
        let content = node(&update, list.key).children()[0];
        let rows = update
            .nodes
            .iter()
            .find(|(id, _)| *id == content)
            .expect("the list's content")
            .1
            .children()
            .to_vec();
        rows.iter()
            .map(|row| {
                let (_, row) = update
                    .nodes
                    .iter()
                    .find(|(id, _)| id == row)
                    .expect("a row");
                assert_eq!(row.role(), Role::ListItem);
                row.is_selected()
            })
            .collect::<Vec<_>>()
    };
    let mut ui = Ui::new();
    // Tall enough for every row from the first frame, which builds only what
    // a list of unknown height would show.
    ui.add_stylesheet("rows.css", "#rows { height: 100px; }");
    assert_eq!(selections(&mut ui, Some(1)), [None, Some(true), None]);
    assert_eq!(selections(&mut ui, None), [None, None, None]);
}

/// The text run `field`'s node holds, and its id.
fn text_run<'a>(update: &'a TreeUpdate, field: &Node) -> (NodeId, &'a Node) {
    let [run] = field.children() else {
        panic!("a text field holds one run, not {:?}", field.children());
    };
    let node = update
        .nodes
        .iter()
        .find(|(id, _)| id == run)
        .map(|(_, node)| node)
        .expect("the run is in the tree");
    assert_eq!(node.role(), Role::TextRun);
    (*run, node)
}

/// **A block a caller drew as a text field reads as one, with the value,
/// caret and selection it was given and not its spans**: "héllo" drawn as a
/// typed run and a pre-edit run is one `TextInput` whose run counts "é" as
/// one character, and a byte offset becomes the character it falls before.
#[test]
fn a_block_marked_as_a_text_input_reads_as_its_value_and_caret() {
    let field = |ui: &mut Ui, selection: std::ops::Range<usize>| {
        frame(ui, idle(), NavInput::default(), |ui| {
            let mut spans = Vec::new();
            let query = ui.block(".query", &[], |ui| {
                spans.push(ui.span(".typed", "hé", &[]).key);
                spans.push(ui.span(".preedit", "llo", &[]).key);
            });
            ui.set_accessible_text_input(query.key, "héllo", selection);
            ui.set_accessible_label(query.key, "Search");
            (query.key, spans)
        })
    };
    let mut ui = Ui::new();

    let (query, spans) = field(&mut ui, 3..3);
    let update = ui.accessibility_update();
    let input = node(&update, query);
    assert_eq!(input.role(), Role::TextInput);
    assert_eq!(input.value(), Some("héllo"));
    assert_eq!(input.label(), Some("Search"), "the label names it");
    for span in spans {
        assert!(
            update.nodes.iter().all(|(id, _)| *id != NodeId(span.0)),
            "{span:?} is drawn, not read"
        );
    }
    let (run_id, run) = text_run(&update, input);
    assert_eq!(run.value(), Some("héllo"));
    assert_eq!(run.character_lengths(), [1, 2, 1, 1, 1]);
    assert_eq!(run.bounds(), input.bounds());
    let caret = TextPosition {
        node: run_id,
        character_index: 2,
    };
    assert_eq!(
        input.text_selection(),
        Some(&TextSelection {
            anchor: caret,
            focus: caret
        }),
        "byte 3 is after \"hé\", two characters in"
    );

    let (query, _) = field(&mut ui, 0..6);
    let update = ui.accessibility_update();
    let input = node(&update, query);
    let (again, _) = text_run(&update, input);
    assert_eq!(again, run_id, "the run keeps its id across frames");
    let selection = input.text_selection().expect("a selection");
    assert_eq!(
        (
            selection.anchor.character_index,
            selection.focus.character_index
        ),
        (0, 5),
        "all of it selected, the caret at the end"
    );
}

/// **A byte inside a character is refused loudly**, as slicing at it would be.
#[test]
#[should_panic(expected = "is not between characters")]
fn a_text_inputs_caret_inside_a_character_panics() {
    let mut ui = Ui::new();
    frame(&mut ui, idle(), NavInput::default(), |ui| {
        let query = ui.block(".query", &[], |_| {});
        ui.set_accessible_text_input(query.key, "é", 1..1);
    });
}

/// **`text_input`'s own field carries its caret and selection too**, from
/// its line: the run holds what the field shows — dots for a password — and
/// the selection is the line's anchor and caret.
#[test]
fn a_built_text_input_reads_its_caret_and_selection() {
    let mut ui = Ui::new();
    let (mut agree, mut name) = (true, String::from("Ada"));
    let widgets = form(&mut ui, idle(), &mut agree, &mut name);
    let update = ui.accessibility_update();
    let input = node(&update, widgets.name.key);
    let (run_id, run) = text_run(&update, input);
    assert_eq!(run.value(), Some("Ada"));
    assert_eq!(run.character_lengths(), [1, 1, 1]);
    let line = ui.edits[&widgets.name.key].line();
    let selection = input.text_selection().expect("a selection");
    assert_eq!(selection.anchor.node, run_id);
    assert_eq!(
        (
            selection.anchor.character_index,
            selection.focus.character_index
        ),
        (line.anchor(), line.caret())
    );

    let password = frame(&mut ui, idle(), NavInput::default(), |ui| {
        let options = TextInputOptions {
            masked: true,
            ..TextInputOptions::default()
        };
        ui.text_input_with("#secret", &mut String::from("pw"), options)
    });
    let update = ui.accessibility_update();
    let input = node(&update, password.key);
    let (_, run) = text_run(&update, input);
    assert_eq!(
        run.value(),
        input.value(),
        "the run shows what the field does"
    );
    assert_ne!(run.value(), Some("pw"), "a password is not read out");
}
