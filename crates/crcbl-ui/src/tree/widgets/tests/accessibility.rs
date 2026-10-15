//! The tree as a screen reader reads it, and the actions a reader asks for.

use accesskit::{Action, ActionRequest, Node, NodeId, Rect, Role, Toggled, TreeId, TreeUpdate};

use super::*;
use crate::tree::Response;

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
