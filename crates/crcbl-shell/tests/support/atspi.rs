//! A screen reader's side of AT-SPI, for the Wayland and X11 suites: switch
//! the accessibility bus on, find a window and its button by name the way Orca
//! walks the registry, invoke the button, and read a text field's text and
//! caret. Through `busctl`, which every
//! systemd system has, rather than a D-Bus crate every test build would compile.
//!
//! Switching AT-SPI on (`org.a11y.Status.IsEnabled`) changes every program on
//! that session bus, so [`private_bus`] refuses to go on unless the harness
//! started one with `dbus-run-session` and said so.

use accesskit::{
    Action, Node, NodeId, Role, TextPosition, TextSelection, TreeId, TreeInfo, TreeUpdate,
};
use crcbl_shell::{ShellEvent, WindowId};

/// The tree's root, the window itself.
const ROOT: NodeId = NodeId(1);
/// The one button under it.
const BUTTON: NodeId = NodeId(2);
/// What the root is called, and what the reader finds the window by.
pub const WINDOW_LABEL: &str = "crcbl accessibility";
/// What the button is called, and what the reader finds it by.
const BUTTON_LABEL: &str = "Press me";
/// A text field beside the button, and the run holding its text.
const FIELD: NodeId = NodeId(3);
const FIELD_RUN: NodeId = NodeId(4);
/// What the field is called, and what the reader finds it by.
const FIELD_LABEL: &str = "Query";
/// What the field holds: a two-byte character, so a caret counted in bytes
/// and one counted in characters disagree.
pub const FIELD_TEXT: &str = "héllo";
/// Where its caret is, in characters: after "hé".
pub const FIELD_CARET: i32 = 2;

/// A window labelled [`WINDOW_LABEL`] holding one clickable button and one
/// text field, shaped as `crcbl-ui` builds a text field: a `TextInput` with
/// one `TextRun` child and the caret as a selection on that run.
pub fn tree() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label(WINDOW_LABEL);
    root.set_children(vec![BUTTON, FIELD]);
    let mut button = Node::new(Role::Button);
    button.set_label(BUTTON_LABEL);
    button.add_action(Action::Click);
    let mut field = Node::new(Role::TextInput);
    field.set_label(FIELD_LABEL);
    field.set_value(FIELD_TEXT);
    field.set_children(vec![FIELD_RUN]);
    let caret = TextPosition {
        node: FIELD_RUN,
        character_index: usize::try_from(FIELD_CARET).expect("a small caret"),
    };
    field.set_text_selection(TextSelection {
        anchor: caret,
        focus: caret,
    });
    let mut run = Node::new(Role::TextRun);
    run.set_value(FIELD_TEXT);
    let lengths: Vec<u8> = FIELD_TEXT
        .chars()
        .map(|c| u8::try_from(c.len_utf8()).expect("at most four bytes"))
        .collect();
    run.set_character_lengths(lengths);
    TreeUpdate {
        nodes: vec![
            (ROOT, root),
            (BUTTON, button),
            (FIELD, field),
            (FIELD_RUN, run),
        ],
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    }
}

/// Whether `event` is `window`'s activation.
pub fn is_activation(event: &ShellEvent, window: WindowId) -> bool {
    matches!(event, ShellEvent::AccessibilityActivated { window: w } if *w == window)
}

/// Whether `event` is the reader's press of the button in `window`.
pub fn is_press(event: &ShellEvent, window: WindowId) -> bool {
    matches!(event, ShellEvent::AccessibilityAction { window: w, request }
        if *w == window && request.action == Action::Click && request.target_node == BUTTON)
}

/// Proof the session bus is the harness's own, or a failure saying why not.
pub struct PrivateBus(());

/// See the [module docs](self).
pub fn private_bus() -> PrivateBus {
    assert_eq!(
        std::env::var("CRCBL_E2E_PRIVATE_BUS").as_deref(),
        Ok("1"),
        "this test turns AT-SPI on for its whole session bus; run it under the \
         harness's dbus-run-session, which sets CRCBL_E2E_PRIVATE_BUS=1"
    );
    PrivateBus(())
}

/// Switches AT-SPI on and answers the accessibility bus's address.
pub fn enable(_bus: &PrivateBus) -> String {
    busctl(&[
        "--user",
        "set-property",
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Status",
        "IsEnabled",
        "b",
        "true",
    ])
    .expect("the session bus starts the accessibility bus");
    busctl(&[
        "--user",
        "call",
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Bus",
        "GetAddress",
    ])
    .ok()
    .and_then(|reply| quoted(&reply).into_iter().next())
    .expect("the accessibility bus has an address")
}

/// `busctl` with `args`, its stdout on success.
fn busctl(args: &[&str]) -> Result<String, String> {
    let output = crcbl_spawn::command("busctl")
        .args(args)
        .output()
        .map_err(|error| format!("busctl: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "busctl {args:?}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// The double-quoted strings in a `busctl` reply, in order: `s "x"` is
/// `["x"]`, and `a(so) 1 ":1.2" "/path"` is `[":1.2", "/path"]`. AT-SPI's
/// names and paths here carry no quotes or backslashes to unescape.
fn quoted(reply: &str) -> Vec<String> {
    reply
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// A string as `busctl` printed it, unescaped: systemd's `cescape` writes
/// `\a \b \f \n \r \t \v \\ \" \'` for those characters and every other
/// byte outside printable ASCII as `\` and three octal digits, so "é" comes
/// out as `\303\251`.
fn unescaped(printed: &str) -> String {
    let mut bytes = Vec::with_capacity(printed.len());
    let mut rest = printed.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let octal = rest
            .get(..3)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        if let Some(value) = octal {
            bytes.push(value);
            rest = &rest[3..];
            continue;
        }
        let Some((&escape, after)) = rest.split_first() else {
            bytes.push(byte);
            break;
        };
        rest = after;
        bytes.push(match escape {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            other => other,
        });
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `unescaped` against what `busctl` prints for known strings.
#[test]
fn busctl_escapes_are_undone() {
    assert_eq!(unescaped("h\\303\\251llo"), "héllo");
    assert_eq!(unescaped("a\\tb\\\\c\\\"d"), "a\tb\\c\"d");
    assert_eq!(unescaped("plain"), "plain");
}

/// An accessible's children, as `(bus name, object path)`.
fn children(bus: &str, name: &str, path: &str) -> Result<Vec<(String, String)>, String> {
    let address = format!("--address={bus}");
    let reply = busctl(&[
        &address,
        "call",
        name,
        path,
        "org.a11y.atspi.Accessible",
        "GetChildren",
    ])?;
    Ok(quoted(&reply)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[name, path]| (name.clone(), path.clone()))
        .collect())
}

/// An accessible's `Name`.
fn label(bus: &str, name: &str, path: &str) -> Option<String> {
    let address = format!("--address={bus}");
    busctl(&[
        &address,
        "get-property",
        name,
        path,
        "org.a11y.atspi.Accessible",
        "Name",
    ])
    .ok()
    .and_then(|reply| quoted(&reply).into_iter().next())
}

/// Finds the accessible called `wanted` under the window by name, under any
/// application the registry lists, as `(bus name, object path)`. An `Err` is
/// "not there yet", and the caller asks again.
fn find(bus: &str, wanted: &str) -> Result<(String, String), String> {
    let apps = children(
        bus,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
    )?;
    for (app, app_path) in apps {
        for (name, window) in children(bus, &app, &app_path)? {
            if label(bus, &name, &window).as_deref() != Some(WINDOW_LABEL) {
                continue;
            }
            for (name, node) in children(bus, &name, &window)? {
                if label(bus, &name, &node).as_deref() == Some(wanted) {
                    return Ok((name, node));
                }
            }
        }
    }
    Err(format!("no window labelled with {wanted:?} under it yet"))
}

/// The reader: finds the button and invokes its first action.
pub fn press_the_button(bus: &str) -> Result<(), String> {
    let (name, node) = find(bus, BUTTON_LABEL)?;
    let address = format!("--address={bus}");
    let reply = busctl(&[
        &address,
        "call",
        &name,
        &node,
        "org.a11y.atspi.Action",
        "DoAction",
        "i",
        "0",
    ])?;
    if reply.trim() == "b true" {
        Ok(())
    } else {
        Err(format!("DoAction answered {reply:?}"))
    }
}

/// The reader: finds the text field and reads its whole text and its caret
/// offset, in characters, through AT-SPI's `Text` interface.
pub fn read_the_field(bus: &str) -> Result<(String, i32), String> {
    let (name, node) = find(bus, FIELD_LABEL)?;
    let address = format!("--address={bus}");
    let text = busctl(&[
        &address,
        "call",
        &name,
        &node,
        "org.a11y.atspi.Text",
        "GetText",
        "ii",
        // `--`, or `busctl` reads the end offset as an option.
        "--",
        "0",
        "-1",
    ])?;
    let text = quoted(&text)
        .into_iter()
        .next()
        .map(|escaped| unescaped(&escaped))
        .ok_or_else(|| format!("GetText answered {text:?}"))?;
    let caret = busctl(&[
        &address,
        "get-property",
        &name,
        &node,
        "org.a11y.atspi.Text",
        "CaretOffset",
    ])?;
    let caret = caret
        .trim()
        .strip_prefix("i ")
        .and_then(|offset| offset.parse().ok())
        .ok_or_else(|| format!("CaretOffset answered {caret:?}"))?;
    Ok((text, caret))
}
